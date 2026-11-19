//! Read-only UTF-8 review sheet for one synthetic vault.
//!
//! One `writer.lock` and one deferred read transaction cover the vault
//! version, sources, facts, candidates, and each fact's decision history.
//! A new commit lands entirely before that read or entirely after it.
//! The sheet does not extract, migrate, publish, or change a fact, an
//! original, or a decision.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::ids::hex_encode;
use crate::model::{
    honesty, unseen_db, Fail, Report, EXIT_INTEGRITY, EXIT_OK, EXIT_RECOVERY, EXIT_REJECTED,
};

pub fn review_sheet(vault: &Path, out: &Path, package: Option<&Path>) -> Report {
    let (facts, candidates, sources) = match read_snapshot(vault) {
        Ok(rows) => rows,
        Err(report) => return report,
    };
    let link = export_link(package, &sources, &facts);
    let (text, fact_count, rest) = render(&facts, &candidates, &sources, &link);
    match super::place_output(vault, out) {
        super::Place::Inside => return reject("output_inside_vault"),
        super::Place::Unresolved(message) => return reject_message("output_unresolved", message),
        super::Place::MissingParent => return reject("output_parent_missing"),
        super::Place::Exists => return reject("output_exists"),
        super::Place::Create => {}
    }
    if let Err(report) = write_new(out, text.as_bytes()) {
        return report;
    }
    reviewed(
        &facts,
        out,
        fact_count,
        rest,
        sources.len(),
        link.associated(),
        link.verified(),
    )
}

fn read_snapshot(vault: &Path) -> Result<(Value, Value, Vec<SourceLine>), Report> {
    let (read, version) = super::open_review_read(vault).map_err(|fail| fail.into_report())?;
    let facts = match super::decide::read_facts(read.connection(), version, None, None) {
        Ok((code, body)) => {
            if code != EXIT_OK {
                return Err(Report { code, body });
            }
            body
        }
        Err(fail) => return Err(tag_facts(fail.into_report())),
    };
    let candidates = match super::fragment::read_candidates(vault, read.connection(), None) {
        Ok((code, body)) => {
            if code != EXIT_OK {
                return Err(Report { code, body });
            }
            body
        }
        Err(fail) => return Err(fail.into_report()),
    };
    if !facts.is_object() || !candidates.is_object() {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "review_unreadable")
                .extra("read_only", json!(true))
                .extra("new_effect", json!(false))
                .extra("output_written", json!(false))
                .into_report(),
        );
    }
    let sources = sources_on(read.connection()).map_err(|fail| fail.into_report())?;
    Ok((facts, candidates, sources))
}

fn tag_facts(mut report: Report) -> Report {
    if let Some(map) = report.body.as_object_mut() {
        if map.get("slice").and_then(|value| value.as_str()) == Some("T1") {
            map.insert("slice".into(), json!("T3-facts"));
        }
    }
    report
}

struct SourceLine {
    source_id: String,
    object_id: String,
    content_sha256: String,
    byte_length: i64,
    media_type: String,
    roles: Vec<String>,
}

fn sources_on(conn: &Connection) -> Result<Vec<SourceLine>, Fail> {
    let mut stmt = conn
        .prepare(
            "SELECT s.source_id, s.object_id, s.content_sha256, s.byte_length, s.media_type, IFNULL(v.role, '') FROM sources s LEFT JOIN source_versions v ON v.source_id = s.source_id ORDER BY s.source_id, v.source_version_id",
        )
        .map_err(|err| unseen_db(err, "review_sources"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|err| unseen_db(err, "review_sources"))?;
    let mut sources = Vec::new();
    for row in rows {
        let row = row.map_err(|err| unseen_db(err, "review_sources"))?;
        if let Some(last) = sources
            .iter_mut()
            .rev()
            .find(|item: &&mut SourceLine| item.source_id == row.0)
        {
            if !row.5.is_empty() {
                last.roles.push(row.5);
            }
            continue;
        }
        sources.push(SourceLine {
            source_id: row.0,
            object_id: row.1,
            content_sha256: row.2,
            byte_length: row.3,
            media_type: row.4,
            roles: if row.5.is_empty() {
                Vec::new()
            } else {
                vec![row.5]
            },
        });
    }
    Ok(sources)
}

enum ExportLink {
    Unset,
    NotAssociated,
    Paths(Vec<(String, String)>),
}

impl ExportLink {
    fn associated(&self) -> bool {
        matches!(self, ExportLink::Paths(_))
    }

    fn verified(&self) -> usize {
        match self {
            ExportLink::Paths(paths) => paths.len(),
            ExportLink::Unset | ExportLink::NotAssociated => 0,
        }
    }
}

fn export_link(package: Option<&Path>, sources: &[SourceLine], facts: &Value) -> ExportLink {
    let Some(package) = package else {
        return ExportLink::Unset;
    };
    if !same_package_range(package, sources, facts) {
        return ExportLink::NotAssociated;
    }
    let paths = verified_originals(package, sources);
    let every_source = sources.iter().all(|source| {
        paths
            .iter()
            .any(|(source_id, _)| source_id == &source.source_id)
    });
    if paths.len() != sources.len() || !every_source {
        return ExportLink::NotAssociated;
    }
    ExportLink::Paths(paths)
}

fn same_package_range(package: &Path, sources: &[SourceLine], facts: &Value) -> bool {
    let records = match fs::read(package.join("records.json")) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => value,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    let csv = match fs::read_to_string(package.join("facts.csv")) {
        Ok(text) => text,
        Err(_) => return false,
    };
    let Some(packed_sources) = packed_source_keys(records.get("sources")) else {
        return false;
    };
    let Some(packed_facts) = packed_fact_keys(records.get("facts")) else {
        return false;
    };
    let Some(csv_facts) = csv_fact_keys(&csv) else {
        return false;
    };
    let live_sources = sources.iter().map(source_key).collect::<BTreeSet<_>>();
    let Some(live_facts) = live_fact_keys(facts) else {
        return false;
    };
    packed_sources == live_sources && packed_facts == live_facts && csv_facts == live_facts
}

fn source_key(source: &SourceLine) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        source.source_id, source.object_id, source.content_sha256, source.byte_length
    )
}

fn packed_source_keys(sources: Option<&Value>) -> Option<BTreeSet<String>> {
    let rows = sources?.as_array()?;
    let mut keys = BTreeSet::new();
    for row in rows {
        keys.insert(format!(
            "{}\t{}\t{}\t{}",
            row.get("source_id")?.as_str()?,
            row.get("object_id")?.as_str()?,
            row.get("content_sha256")?.as_str()?,
            row.get("byte_length")?.as_i64()?
        ));
    }
    Some(keys)
}

fn live_fact_keys(facts: &Value) -> Option<BTreeSet<String>> {
    let rows = facts.get("facts")?.as_array()?;
    let mut keys = BTreeSet::new();
    for row in rows {
        let decisions = row.get("decisions")?.as_array()?;
        let latest = decisions
            .last()
            .and_then(|item| item.get("decision_id"))
            .and_then(|item| item.as_str())
            .unwrap_or("");
        keys.insert(fact_key(
            row.get("fact_id")?.as_str()?,
            row.get("source_id")?.as_str()?,
            row.get("candidate_id")?.as_str()?,
            row.get("fact_type")?.as_str()?,
            row.get("revision")?.as_i64()?,
            field(row.get("value")?, "amount_text"),
            field(row.get("value")?, "period_start"),
            field(row.get("value")?, "period_end"),
            latest,
        ));
    }
    Some(keys)
}

fn packed_fact_keys(facts: Option<&Value>) -> Option<BTreeSet<String>> {
    let rows = facts?.as_array()?;
    let mut keys = BTreeSet::new();
    for row in rows {
        let decisions = row.get("decision_ids")?.as_array()?;
        let latest = decisions
            .last()
            .and_then(|item| item.as_str())
            .unwrap_or("");
        keys.insert(fact_key(
            row.get("fact_id")?.as_str()?,
            row.get("source_id")?.as_str()?,
            row.get("candidate_id")?.as_str()?,
            row.get("fact_type")?.as_str()?,
            row.get("revision")?.as_i64()?,
            field(row.get("value")?, "amount_text"),
            field(row.get("value")?, "period_start"),
            field(row.get("value")?, "period_end"),
            latest,
        ));
    }
    Some(keys)
}

fn csv_fact_keys(text: &str) -> Option<BTreeSet<String>> {
    let rows = csv_rows(text)?;
    let header = rows.first()?;
    let index = |name: &str| header.iter().position(|cell| cell == name);
    let fact_id = index("fact_id")?;
    let source_id = index("source_id")?;
    let candidate_id = index("candidate_id")?;
    let fact_type = index("fact_type")?;
    let revision = index("revision")?;
    let amount = index("amount_text")?;
    let start = index("period_start")?;
    let end = index("period_end")?;
    let latest = index("latest_decision_id")?;
    let width = header.len();
    let mut keys = BTreeSet::new();
    for row in rows.iter().skip(1) {
        if row.iter().all(|cell| cell.is_empty()) {
            continue;
        }
        if row.len() != width {
            return None;
        }
        keys.insert(fact_key(
            &row[fact_id],
            &row[source_id],
            &row[candidate_id],
            &row[fact_type],
            row[revision].parse().ok()?,
            row[amount].clone(),
            row[start].clone(),
            row[end].clone(),
            &row[latest],
        ));
    }
    Some(keys)
}

#[allow(clippy::too_many_arguments)]
fn fact_key(
    fact_id: &str,
    source_id: &str,
    candidate_id: &str,
    fact_type: &str,
    revision: i64,
    amount: String,
    start: String,
    end: String,
    latest: &str,
) -> String {
    format!(
        "{fact_id}\t{source_id}\t{candidate_id}\t{fact_type}\t{revision}\t{amount}\t{start}\t{end}\t{latest}"
    )
}

fn field(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn csv_rows(text: &str) -> Option<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(ch);
            }
            continue;
        }
        match ch {
            '"' if cell.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut cell)),
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {}
            _ => cell.push(ch),
        }
    }
    if quoted {
        return None;
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Some(rows)
}

fn verified_originals(package: &Path, sources: &[SourceLine]) -> Vec<(String, String)> {
    let mut paths = Vec::new();
    for source in sources {
        if !safe_object_id(&source.object_id) {
            continue;
        }
        let relative = format!("originals/{}", source.object_id);
        let path = package.join("originals").join(&source.object_id);
        if original_matches(&path, &source.content_sha256, source.byte_length) {
            paths.push((source.source_id.clone(), relative));
        }
    }
    paths
}

fn safe_object_id(object_id: &str) -> bool {
    !object_id.is_empty()
        && object_id != "."
        && object_id != ".."
        && !object_id.contains(['/', '\\'])
}

fn original_matches(path: &Path, hash: &str, byte_length: i64) -> bool {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return false,
    };
    if !meta.file_type().is_file() {
        return false;
    }
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return false;
    }
    bytes.len() as i64 == byte_length && sha256_bytes(&bytes) == hash
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn push_conclusion(
    text: &mut String,
    facts: Option<&Vec<Value>>,
    sources: &[SourceLine],
    link: &ExportLink,
) {
    text.push_str("当前结论\n");
    let rows: &[Value] = facts.map(Vec::as_slice).unwrap_or(&[]);
    let source_ids = conclusion_sources(rows, sources);
    if source_ids.is_empty() {
        text.push_str("当前没有事实。没有可写的当前金额或服务期。\n");
    } else {
        for source_id in &source_ids {
            text.push_str(&format!("来源分组 source_id={}\n", show(source_id)));
            let group = rows
                .iter()
                .filter(|fact| fact["source_id"].as_str() == Some(source_id))
                .collect::<Vec<_>>();
            if group.is_empty() {
                text.push_str("这条来源没有当前金额或服务期。\n");
            } else {
                for fact in conclusion_order(&group) {
                    text.push_str(&conclusion_line(fact));
                    text.push('\n');
                }
                if group.len() > 1 {
                    text.push_str("同一来源上的多条事实不代表已核同一笔支付。\n");
                }
            }
            match link {
                ExportLink::Paths(paths) => {
                    if let Some((_, path)) = paths.iter().find(|(id, _)| id == source_id) {
                        text.push_str(&format!("原件相对位置 {}\n", show(path)));
                    }
                }
                ExportLink::Unset | ExportLink::NotAssociated => {}
            }
        }
        if source_ids.len() > 1 {
            text.push_str("不同来源不是同一笔支付。\n");
        }
    }
    let payment_unverified =
        rows.is_empty() || rows.iter().all(|fact| fact["payment_verified"] == false);
    let status_unknown = rows.is_empty()
        || rows.iter().any(|fact| {
            matches!(fact["current_status"].as_str(), None | Some("unknown"))
                || matches!(fact["time_status"].as_str(), None | Some("unknown"))
        });
    if payment_unverified && status_unknown {
        text.push_str("付款未核。不能证明订阅仍有效。\n");
    } else if payment_unverified {
        text.push_str("付款未核。\n");
    } else if status_unknown {
        text.push_str("不能证明订阅仍有效。\n");
    }
    text.push_str("摘录不是全文。\n");
    match link {
        ExportLink::Unset => text.push_str("单独生成，未关联导出目录。\n"),
        ExportLink::NotAssociated => {
            text.push_str("给出的导出和这次审阅不是同一范围，本单不关联该导出。\n");
        }
        ExportLink::Paths(_) => {
            text.push_str(
                "上面写出的相对位置在关联的导出目录内，已核对文件存在且内容哈希与库中的收录标识一致。\n",
            );
        }
    }
    text.push_str("请按源码查看，程序不打开、不渲染 HTML。\n");
}

fn conclusion_sources(facts: &[Value], sources: &[SourceLine]) -> Vec<String> {
    let mut ids = sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<Vec<_>>();
    for fact in facts {
        let Some(id) = fact["source_id"].as_str() else {
            continue;
        };
        if !ids.iter().any(|kept| kept == id) {
            ids.push(id.to_string());
        }
    }
    ids
}

fn conclusion_line(fact: &Value) -> String {
    match fact["fact_type"].as_str().unwrap_or("") {
        "stated_amount_paid" => amount_conclusion(fact, &fact["value"]),
        "stated_service_period" => period_conclusion(&fact["value"]),
        other => format!(
            "有一条 {} 事实。当前值在详单里，这里不另起一个结论。",
            show(other)
        ),
    }
}

fn amount_conclusion(fact: &Value, value: &Value) -> String {
    let current = value["amount_text"].as_str().unwrap_or("");
    let currency = value["currency"].as_str().unwrap_or("");
    let money = if currency.is_empty() {
        show(current)
    } else {
        format!("{} {}", show(currency), show(current))
    };
    match stated_amount(fact["decisions"].as_array()) {
        Some(original) if original != current => format!(
            "当前记录金额 {money} 是人工修改后的值，原件仍写 {}。",
            show(&original)
        ),
        Some(_) => format!("当前记录金额 {money}，与原件所写相同。"),
        None if current.is_empty() => "这条金额事实没有当前金额文本。".to_string(),
        None => format!("当前记录金额 {money}。库里没有凭据金额。"),
    }
}

fn period_conclusion(value: &Value) -> String {
    let start = value["period_start"].as_str().unwrap_or("");
    let end = value["period_end"].as_str().unwrap_or("");
    if start.is_empty() && end.is_empty() {
        "这条服务期事实没有起止。".to_string()
    } else {
        format!("服务期 {} 至 {}。", show(start), show(end))
    }
}

fn conclusion_order<'a>(facts: &[&'a Value]) -> Vec<&'a Value> {
    let mut amounts = Vec::new();
    let mut periods = Vec::new();
    let mut rest = Vec::new();
    for fact in facts {
        match fact["fact_type"].as_str() {
            Some("stated_amount_paid") => amounts.push(*fact),
            Some("stated_service_period") => periods.push(*fact),
            _ => rest.push(*fact),
        }
    }
    amounts.extend(periods);
    amounts.extend(rest);
    amounts
}

fn render(
    facts_body: &Value,
    candidates_body: &Value,
    sources: &[SourceLine],
    link: &ExportLink,
) -> (String, usize, usize) {
    let facts = facts_body["facts"].as_array();
    let candidates = candidates_body["candidates"].as_array();
    let mut cited = Vec::new();
    if let Some(facts) = facts {
        for fact in facts {
            if let Some(id) = fact["candidate_id"].as_str() {
                cited.push(id.to_string());
            }
        }
    }
    let mut text = String::from(
        "\
合成开发审阅单
这不是 Windows 界面。
",
    );
    push_conclusion(&mut text, facts, sources, link);
    text.push_str(
        "\
详单
这是合成开发库的只读文字。它不是完成的产品。
原件里的 HTML 只按文字抄出，不渲染，也不打开链接，也不取远程资源。
控制字符、换行和双向控制写成 \\u{XXXX}。反斜杠写成 \\\\。
这些信息不代表什么
这些信息不代表什么：凭据写明、你修改（原文仍写X）、已撤销。
凭据写明的金额不是已核到账。
修改后的当前值不改原件。原文仍在库里。
已撤销的决定仍留在记录里。
候选不是事实。
显示文本不是原件切片。
提取尝试的 attempt_published 只是库里的尝试标记。这份单没有发布库。
这份只读打开可能让 SQLite 碰到 -shm。它不改已提交的逻辑数据、原件字节和决定记录。它不证明目录里没有新文件。
记录顺序 每个来源先写自己的事实（source_id、fact_id），再写该来源未成为事实的候选（ordinal）。决定按库内 rowid。recorded_at 是库里的整数，不是这份单编的时刻。
",
    );
    if facts_body["slice"].as_str() == Some("T4-html") {
        text.push_str("格式 4 不收纯文本片段。这份单没有把纯文本塞进这个库。\n");
    }
    text.push_str("来源\n");
    if sources.is_empty() {
        text.push_str("来源 无\n");
    }
    for source in sources {
        push_source(&mut text, source);
    }
    text.push_str("提取\n");
    let runs = candidates_body["runs"].as_array();
    if runs.map(|rows| rows.is_empty()).unwrap_or(true) {
        text.push_str("提取 无\n");
    }
    if let Some(runs) = runs {
        for run in runs {
            text.push_str(&format!(
                "提取 run_id={} source_id={} status={}\n",
                shown(run, "run_id"),
                shown(run, "source_id"),
                shown(run, "status")
            ));
            let Some(attempts) = run["attempts"].as_array() else {
                continue;
            };
            for attempt in attempts {
                text.push_str(&format!(
                    "提取尝试 attempt_no={} status={} attempt_published={} detail={}\n",
                    shown(attempt, "attempt_no"),
                    shown(attempt, "status"),
                    shown(attempt, "published"),
                    show(&compact(&attempt["detail"]))
                ));
            }
        }
    }
    let fact_count = facts.map(|rows| rows.len()).unwrap_or(0);
    let rest = candidates
        .map(|rows| {
            rows.iter()
                .filter(|row| {
                    row["candidate_id"]
                        .as_str()
                        .map(|id| !cited.iter().any(|kept| kept == id))
                        .unwrap_or(true)
                })
                .count()
        })
        .unwrap_or(0);
    text.push_str(&format!("来源数 {}\n", sources.len()));
    text.push_str(&format!("事实数 {fact_count}\n"));
    text.push_str(&format!("未成为事实的候选数 {rest}\n"));
    let mut record = 1usize;
    let fact_rows = facts.cloned().unwrap_or_default();
    let candidate_rows = candidates.cloned().unwrap_or_default();
    let mut seen_sources = Vec::new();
    for source in sources {
        seen_sources.push(source.source_id.clone());
    }
    for fact in &fact_rows {
        if let Some(id) = fact["source_id"].as_str() {
            if !seen_sources.iter().any(|kept| kept == id) {
                seen_sources.push(id.to_string());
            }
        }
    }
    for source_id in &seen_sources {
        let source_facts = fact_rows
            .iter()
            .filter(|fact| fact["source_id"].as_str() == Some(source_id))
            .collect::<Vec<_>>();
        let source_candidates = candidate_rows
            .iter()
            .filter(|row| row["source_id"].as_str() == Some(source_id.as_str()))
            .filter(|row| {
                row["candidate_id"]
                    .as_str()
                    .map(|id| !cited.iter().any(|kept| kept == id))
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        if source_facts.is_empty() && source_candidates.is_empty() {
            continue;
        }
        text.push_str(&format!("来源分组 source_id={}\n", show(source_id)));
        for fact in source_facts {
            push_fact(&mut text, record, fact, sources);
            record += 1;
        }
        for candidate in source_candidates {
            push_candidate(&mut text, record, candidate, sources);
            record += 1;
        }
    }
    for candidate in &candidate_rows {
        let scoped = candidate["source_id"]
            .as_str()
            .map(|id| seen_sources.iter().any(|kept| kept == id))
            .unwrap_or(false);
        if scoped {
            continue;
        }
        if candidate["candidate_id"]
            .as_str()
            .map(|id| cited.iter().any(|kept| kept == id))
            .unwrap_or(false)
        {
            continue;
        }
        push_candidate(&mut text, record, candidate, sources);
        record += 1;
    }
    text.push_str(
        "没有另存的未匹配文字。这份单不从原件再猜，也不把没写成候选的文字补成一条记录。\n",
    );
    (text, fact_count, rest)
}

fn push_source(text: &mut String, source: &SourceLine) {
    text.push_str("来源类型\n");
    text.push_str(&format!("media_type={}\n", show(&source.media_type)));
    if source.roles.is_empty() {
        text.push_str("role=\n");
    }
    for role in &source.roles {
        text.push_str(&format!("role={}\n", show(role)));
    }
    text.push_str("收录标识\n");
    text.push_str(&format!("source_id={}\n", show(&source.source_id)));
    text.push_str(&format!("object_id={}\n", show(&source.object_id)));
    text.push_str(&format!(
        "content_sha256={}\n",
        show(&source.content_sha256)
    ));
    text.push_str(&format!("byte_length={}\n", source.byte_length));
}

fn push_fact(text: &mut String, record: usize, fact: &Value, sources: &[SourceLine]) {
    text.push_str(&format!("记录 {record}\n"));
    text.push_str("种类 事实\n");
    let source_id = fact["source_id"].as_str().unwrap_or("");
    if let Some(source) = sources.iter().find(|item| item.source_id == source_id) {
        push_source(text, source);
    } else {
        text.push_str("来源类型\nmedia_type=\nrole=\n收录标识\nsource_id=\nobject_id=\ncontent_sha256=\nbyte_length=\n");
    }
    let stated = stated_amount(fact["decisions"].as_array());
    text.push_str("凭据写明 ");
    text.push_str(&stated_phrase(&stated, fact));
    text.push('\n');
    text.push_str("当前值\n");
    let value = &fact["value"];
    let latest = fact["decisions"]
        .as_array()
        .and_then(|rows| rows.last())
        .and_then(|row| row["decision_id"].as_str())
        .unwrap_or("");
    for (key, cell) in [
        ("fact_id", shown(fact, "fact_id")),
        ("source_id", shown(fact, "source_id")),
        ("candidate_id", shown(fact, "candidate_id")),
        ("fact_type", shown(fact, "fact_type")),
        ("revision", shown(fact, "revision")),
        ("statement", csv_cell(value, "statement")),
        ("currency", csv_cell(value, "currency")),
        ("amount_text", csv_cell(value, "amount_text")),
        ("period_start", csv_cell(value, "period_start")),
        ("period_end", csv_cell(value, "period_end")),
        ("latest_decision_id", show(latest)),
    ] {
        text.push_str(&format!("{key}={cell}\n"));
    }
    text.push_str(&format!(
        "payment_verified={}\nobserved_at={}\ncurrent_status={}\ntime_status={}\n",
        shown(fact, "payment_verified"),
        shown(fact, "observed_at"),
        shown(fact, "current_status"),
        shown(fact, "time_status")
    ));
    push_span(
        text,
        &fact["locator"]["locator"],
        fact["locator"]["excerpt"].as_str(),
    );
    text.push_str("决定\n");
    let Some(decisions) = fact["decisions"].as_array() else {
        text.push_str("决定 无\n");
        return;
    };
    if decisions.is_empty() {
        text.push_str("决定 无\n");
    }
    for (index, decision) in decisions.iter().enumerate() {
        text.push_str(&format!(
            "决定 {} kind={} decision_id={} recorded_at={} actor_kind={} actor_id={} target_decision_id={} {}\n",
            index + 1,
            shown(decision, "kind"),
            shown(decision, "decision_id"),
            shown(decision, "recorded_at"),
            shown(decision, "actor_kind"),
            shown(decision, "actor_id"),
            shown(decision, "target_decision_id"),
            decision_words(decision, stated.as_deref())
        ));
    }
}

fn push_candidate(text: &mut String, record: usize, candidate: &Value, sources: &[SourceLine]) {
    text.push_str(&format!("记录 {record}\n"));
    text.push_str("种类 候选\n");
    text.push_str("不是事实\n");
    let source_id = candidate["source_id"].as_str().unwrap_or("");
    if let Some(source) = sources.iter().find(|item| item.source_id == source_id) {
        push_source(text, source);
    } else {
        text.push_str("来源类型\nmedia_type=\nrole=\n收录标识\nsource_id=\nobject_id=\ncontent_sha256=\nbyte_length=\n");
    }
    text.push_str(&format!(
        "candidate_id={}\nkind={}\nusable={}\nrun_status={}\n",
        shown(candidate, "candidate_id"),
        shown(candidate, "kind"),
        shown(candidate, "usable"),
        shown(candidate, "run_status")
    ));
    text.push_str("未知原因 ");
    text.push_str(&reason_words(&candidate["value"]["reason"]));
    text.push('\n');
    text.push_str("凭据写明 没有事实\n");
    text.push_str("当前值 无\n");
    push_span(text, &candidate["locator"], candidate["excerpt"].as_str());
}

fn push_span(text: &mut String, locator: &Value, fallback_excerpt: Option<&str>) {
    let excerpt = locator["excerpt"]
        .as_str()
        .or(fallback_excerpt)
        .unwrap_or("");
    text.push_str(&format!("原文={}\n", show(excerpt)));
    let display = locator["display_excerpt"].as_str().unwrap_or("");
    text.push_str(&format!("显示={}\n", show(display)));
    let version = locator["display_version"].as_str().unwrap_or("");
    text.push_str(&format!("显示版本={}\n", show(version)));
    text.push_str(&format!("offset_unit={}\n", shown(locator, "offset_unit")));
    text.push_str(&format!(
        "字节 byte_start={} byte_end={} unicode_scalar_start={} unicode_scalar_end={}\n",
        shown(locator, "byte_start"),
        shown(locator, "byte_end"),
        shown(locator, "unicode_scalar_start"),
        shown(locator, "unicode_scalar_end")
    ));
}

fn stated_amount(decisions: Option<&Vec<Value>>) -> Option<String> {
    let decisions = decisions?;
    for decision in decisions {
        if decision["kind"].as_str() != Some("rule_adopt") {
            continue;
        }
        if let Some(amount) = decision["new_value"]["amount_text"].as_str() {
            return Some(amount.to_string());
        }
    }
    None
}

fn stated_phrase(amount: &Option<String>, fact: &Value) -> String {
    if let Some(amount) = amount {
        return format!("amount_text={}", show(amount));
    }
    let Some(decisions) = fact["decisions"].as_array() else {
        return "库里没有rule_adopt".to_string();
    };
    for decision in decisions {
        if decision["kind"].as_str() != Some("rule_adopt") {
            continue;
        }
        let start = decision["new_value"]["period_start"].as_str().unwrap_or("");
        let end = decision["new_value"]["period_end"].as_str().unwrap_or("");
        if !start.is_empty() || !end.is_empty() {
            return format!("period_start={} period_end={}", show(start), show(end));
        }
    }
    "库里没有rule_adopt".to_string()
}

fn decision_words(decision: &Value, stated_amount: Option<&str>) -> String {
    let kind = decision["kind"].as_str().unwrap_or("");
    let new_value = &decision["new_value"];
    match kind {
        "rule_adopt" => {
            if let Some(amount) = new_value["amount_text"].as_str() {
                format!("凭据写明 amount_text={}", show(amount))
            } else {
                let start = new_value["period_start"].as_str().unwrap_or("");
                let end = new_value["period_end"].as_str().unwrap_or("");
                if start.is_empty() && end.is_empty() {
                    "凭据写明".to_string()
                } else {
                    format!(
                        "凭据写明 period_start={} period_end={}",
                        show(start),
                        show(end)
                    )
                }
            }
        }
        "human_correct" => {
            let stated = stated_amount.unwrap_or("库里没有凭据金额");
            let current = new_value["amount_text"].as_str().unwrap_or("");
            format!("你修改（原文仍写{}）当前 {}", show(stated), show(current))
        }
        "undo" => {
            let current = new_value["amount_text"].as_str().unwrap_or("");
            format!("已撤销 当前 {}", show(current))
        }
        other => show(other),
    }
}

fn reason_words(reason: &Value) -> String {
    match reason {
        Value::Null => "reason=null".to_string(),
        Value::String(text) => format!("reason={}", show(text)),
        other => format!("reason={}", show(&compact(other))),
    }
}

fn shown(value: &Value, key: &str) -> String {
    match value.get(key) {
        None | Some(Value::Null) => "null".to_string(),
        Some(Value::String(text)) => show(text),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(other) => show(&compact(other)),
    }
}

fn csv_cell(value: &Value, key: &str) -> String {
    match value.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => show(text),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(other) => show(&compact(other)),
    }
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn show(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch == '\\' {
            out.push_str("\\\\");
        } else if ch.is_control() || disguised(ch) {
            out.push_str(&format!("\\u{{{:04X}}}", u32::from(ch)));
        } else {
            out.push(ch);
        }
    }
    out
}

fn disguised(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{2029}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{206A}'..='\u{206F}'
            | '\u{FEFF}'
    )
}

fn reviewed(
    facts: &Value,
    out: &Path,
    fact_count: usize,
    rest: usize,
    source_count: usize,
    package_associated: bool,
    originals_verified: usize,
) -> Report {
    let mut body = honesty("Reviewed");
    if let Some(slice) = facts.get("slice") {
        body.insert("slice".into(), slice.clone());
    }
    body.insert("read_only".into(), json!(true));
    body.insert("new_effect".into(), json!(false));
    body.insert("output_written".into(), json!(true));
    body.insert("output".into(), json!(out.display().to_string()));
    body.insert("fact_count".into(), json!(fact_count));
    body.insert("candidate_not_fact_count".into(), json!(rest));
    body.insert("source_count".into(), json!(source_count));
    body.insert("package_associated".into(), json!(package_associated));
    body.insert("originals_verified".into(), json!(originals_verified));
    body.insert(
        "filesystem_note".into(),
        json!("只读打开可能让 SQLite 碰到 -shm。已提交的逻辑数据、原件字节和决定记录不因这次审阅改写。这不证明目录里没有新文件。"),
    );
    Report {
        code: EXIT_OK,
        body: Value::Object(body),
    }
}

fn reject(reason: &'static str) -> Report {
    Fail::new(EXIT_REJECTED, "Rejected", reason)
        .extra("read_only", json!(true))
        .extra("new_effect", json!(false))
        .extra("output_written", json!(false))
        .into_report()
}

fn reject_message(reason: &'static str, message: String) -> Report {
    Fail::new(EXIT_REJECTED, "Rejected", reason)
        .extra("os_error", json!(message))
        .extra("read_only", json!(true))
        .extra("new_effect", json!(false))
        .extra("output_written", json!(false))
        .into_report()
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Report> {
    let mut file = match crate::platform::open_new_nofollow(path) {
        Ok(file) => file,
        Err(err)
            if err.kind() == io::ErrorKind::AlreadyExists
                || crate::platform::exclusive_create_refused(&err) =>
        {
            return Err(reject("output_exists"));
        }
        Err(err) => return Err(write_io("not_created", &err, None)),
    };
    if let Err(err) = file.write_all(bytes).and_then(|_| file.flush()) {
        let (cleanup, cleanup_error) = match fs::remove_file(path) {
            Ok(()) => ("removed", None),
            Err(remove_err) => ("failed", Some(remove_err.to_string())),
        };
        return Err(write_io(cleanup, &err, cleanup_error));
    }
    Ok(())
}

fn write_io(cleanup: &str, err: &io::Error, cleanup_error: Option<String>) -> Report {
    let mut fail = Fail::new(EXIT_INTEGRITY, "IntegrityAlert", "io_error")
        .extra("during", json!("review_write"))
        .extra("os_error", json!(err.to_string()))
        .extra("output_written", json!(false))
        .extra("output_cleanup", json!(cleanup))
        .extra("read_only", json!(true))
        .extra("new_effect", json!(false))
        .extra("injected", json!(false));
    if let Some(message) = cleanup_error {
        fail = fail.extra("output_cleanup_error", json!(message));
    }
    fail.into_report()
}
