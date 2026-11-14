use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const BILL: &str = concat!(
    "<!DOCTYPE html>\n<html>\n<body>\n",
    "<p>Amount paid USD <b>15&#46;99</b><!--\u{202E}\n你修改（原文仍写0）\n合成开发审阅单\n已撤销的决定仍留在记录里。--></p>\n",
    "<p>Service period: <span>2026-09-01</span> through <span>2026-09-30</span></p>\n",
    "<table><tr><th>Amount paid</th><td>USD 10.00</td><td>USD 12.00</td></tr></table>\n",
    "<p>Nothing owed</p>\n",
    "</body>\n</html>\n"
);

const PLAIN: &str = "Amount paid USD 8.00\nTotal USD 40.00\nNot a stored line\n";

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-review-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("kept {}", self.path.display());
        } else {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(1), body)
}

fn sha256(bytes: &[u8]) -> String {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn init(tmp: &Tmp, name: &str, format: &str) -> PathBuf {
    let vault = tmp.path.join(name);
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        format,
    ]);
    assert_eq!(code, 0, "{body}");
    vault
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

fn sidecars(tree: &BTreeMap<String, Vec<u8>>) -> Vec<(String, usize)> {
    tree.iter()
        .filter(|(name, _)| name.ends_with("vault.db-wal") || name.ends_with("vault.db-shm"))
        .map(|(name, bytes)| (name.clone(), bytes.len()))
        .collect()
}

fn same_except_sqlite_sidecars(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) {
    eprintln!(
        "sqlite sidecars before {:?} after {:?}",
        sidecars(before),
        sidecars(after)
    );
    let mut names = BTreeMap::new();
    for name in before.keys().chain(after.keys()) {
        names.insert(name.clone(), ());
    }
    for name in names.keys() {
        if name.ends_with("vault.db-wal") || name.ends_with("vault.db-shm") {
            continue;
        }
        assert_eq!(before.get(name), after.get(name), "{name}");
    }
}

fn csv_table(bytes: &[u8]) -> Vec<Vec<String>> {
    let text = std::str::from_utf8(bytes).unwrap();
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
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut cell)),
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {}
            other => cell.push(other),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('u') => {
                assert_eq!(chars.next(), Some('{'));
                let mut hex = String::new();
                loop {
                    let next = chars.next().unwrap();
                    if next == '}' {
                        break;
                    }
                    hex.push(next);
                }
                let code = u32::from_str_radix(&hex, 16).unwrap();
                out.push(char::from_u32(code).unwrap());
            }
            other => panic!("bad escape {other:?} in {text}"),
        }
    }
    out
}

fn blocks(text: &str) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("记录 ") {
            if rest.chars().all(|ch| ch.is_ascii_digit()) {
                if !current.is_empty() {
                    blocks.push(current);
                }
                current = vec![line.to_string()];
                continue;
            }
        }
        if !current.is_empty() {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn current(block: &[String], key: &str) -> String {
    let start = block
        .iter()
        .position(|line| line == "当前值")
        .unwrap_or_else(|| panic!("no 当前值 in {block:?}"));
    let prefix = format!("{key}=");
    let line = block[start + 1..]
        .iter()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("missing {key}"));
    unescape(&line[prefix.len()..])
}

fn token<'a>(line: &'a str, key: &str) -> &'a str {
    let pattern = format!("{key}=");
    let start = line.find(&pattern).unwrap() + pattern.len();
    line[start..].split_whitespace().next().unwrap()
}

fn line_value<'a>(block: &'a [String], prefix: &str) -> &'a str {
    let line = block
        .iter()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing {prefix}"));
    &line[prefix.len()..]
}

fn assert_span(block: &[String], source: &str) {
    let excerpt = unescape(line_value(block, "原文="));
    let range = block.iter().find(|line| line.starts_with("字节 ")).unwrap();
    let start: usize = token(range, "byte_start").parse().unwrap();
    let end: usize = token(range, "byte_end").parse().unwrap();
    assert_eq!(excerpt.as_bytes(), &source.as_bytes()[start..end]);
    assert_eq!(
        token(range, "unicode_scalar_start")
            .parse::<usize>()
            .unwrap(),
        source[..start].chars().count()
    );
    assert_eq!(
        token(range, "unicode_scalar_end").parse::<usize>().unwrap(),
        source[..end].chars().count()
    );
}

fn assert_csv(block: &[String], row: &[String]) {
    let keys = [
        "fact_id",
        "source_id",
        "candidate_id",
        "fact_type",
        "revision",
        "statement",
        "currency",
        "amount_text",
        "period_start",
        "period_end",
        "latest_decision_id",
    ];
    assert_eq!(row.len(), keys.len(), "{row:?}");
    for (key, cell) in keys.iter().zip(row) {
        assert_eq!(current(block, key), *cell, "{key}");
    }
}

#[test]
fn review_sheet_lists_html_facts_history_and_the_stored_non_fact() {
    let tmp = Tmp::new("html");
    let vault = init(&tmp, "vault", "4");
    let source = tmp.path.join("bill.html");
    fs::write(&source, BILL.as_bytes()).unwrap();
    let (code, imported) = run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "bill",
        "--source",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    assert_eq!(imported["extraction"], "conflict");
    assert_eq!(imported["extraction_reason"], "ambiguous_association");
    assert_eq!(imported["preservation"], "preserved");
    assert_eq!(imported["network"], false);
    assert_eq!(imported["rendered"], false);
    assert_eq!(imported["payment_verified"], false);
    let object_id = imported["object_id"].as_str().unwrap();
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        BILL.as_bytes()
    );

    let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let amount = facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap();
    let fact_id = amount["fact_id"].as_str().unwrap();
    assert_eq!(amount["value"]["amount_text"], "15.99");
    let (code, corrected) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "fix",
        "--fact-id",
        fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "10.00",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{corrected}");
    assert_eq!(corrected["amount_text"], "10.00");
    let decision_id = corrected["decision_id"].as_str().unwrap();
    let (code, undone) = run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "undo-bill",
        "--decision-id",
        decision_id,
        "--expected-revision",
        "2",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["amount_text"], "15.99");
    assert_eq!(undone["revision"], 3);
    let (code, kept) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "fix-kept",
        "--fact-id",
        fact_id,
        "--expected-revision",
        "3",
        "--amount-text",
        "12.00",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{kept}");
    assert_eq!(kept["amount_text"], "12.00");
    assert_eq!(kept["revision"], 4);

    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");

    let package = tmp.path.join("package-1");
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    assert!(exported.get("published").is_none(), "{exported}");
    let package_2 = tmp.path.join("package-2");
    let (code, second) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package_2.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{second}");
    let packed_csv = fs::read(package.join("facts.csv")).unwrap();
    let packed_csv_2 = fs::read(package_2.join("facts.csv")).unwrap();
    assert_eq!(packed_csv, packed_csv_2);
    let package_before = files(&package);
    let (code, clash) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{clash}");
    assert_eq!(clash["reason"], "output_exists");
    assert_eq!(files(&package), package_before);
    assert_eq!(
        fs::read(package.join("originals").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    assert_eq!(
        fs::read(package_2.join("originals").join(object_id)).unwrap(),
        BILL.as_bytes()
    );

    let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let (code, candidates) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{candidates}");
    let before = files(&vault);
    let sheet = tmp.path.join("review.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        sheet.to_str().unwrap(),
        "--package",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["outcome"], "Reviewed");
    assert_eq!(reviewed["read_only"], true);
    assert_eq!(reviewed["new_effect"], false);
    assert_eq!(reviewed["output_written"], true);
    assert_eq!(reviewed["user_ready"], false);
    assert_eq!(reviewed["synthetic_only"], true);
    assert_eq!(reviewed["fact_count"], 2);
    assert_eq!(reviewed["candidate_not_fact_count"], 1);
    assert_eq!(reviewed["source_count"], 1);
    assert_eq!(reviewed["slice"], "T4-html");
    assert_eq!(reviewed["package_associated"], true);
    assert_eq!(reviewed["originals_verified"], 1);
    assert!(reviewed.get("published").is_none(), "{reviewed}");
    assert!(reviewed["filesystem_note"]
        .as_str()
        .unwrap()
        .contains("-shm"));
    let after = files(&vault);
    same_except_sqlite_sidecars(&before, &after);
    let (code, facts_after) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts_after}");
    assert_eq!(facts_after["facts"], facts["facts"]);
    let (code, candidates_after) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{candidates_after}");
    assert_eq!(candidates_after["candidates"], candidates["candidates"]);
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    assert_eq!(fs::read(&source).unwrap(), BILL.as_bytes());

    let raw = fs::read(&sheet).unwrap();
    let text = String::from_utf8(raw.clone()).unwrap();
    assert!(!raw.windows(3).any(|item| item == [0xE2, 0x80, 0xAE]));
    assert!(text.contains("\\u{202E}"));
    assert!(text.contains("\\u{000A}"));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "合成开发审阅单");
    assert_eq!(lines[1], "这不是 Windows 界面。");
    assert_eq!(lines[2], "当前结论");
    let head = text.split("\n详单\n").next().unwrap();
    assert!(head.contains("当前记录金额 USD 12.00 是人工修改后的值，原件仍写 15.99。"));
    assert!(head.contains("服务期 2026-09-01 至 2026-09-30。"));
    assert!(head.find("当前记录金额").unwrap() < head.find("服务期").unwrap());
    assert!(head.contains("同一来源上的多条事实不代表已核同一笔支付。"));
    assert!(head.contains("付款未核。不能证明订阅仍有效。"));
    assert!(head.contains("摘录不是全文。"));
    assert!(head.contains(&format!("原件相对位置 originals/{object_id}")));
    assert!(!head.contains("10.00"));
    assert!(!head.contains("已撤销"));
    assert!(!head.contains("ambiguous"));
    assert!(!text.contains("/tmp"));
    assert!(!text.contains("bill.html"));
    assert_eq!(
        fs::read(package.join("originals").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| **line == "合成开发审阅单")
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| **line == "已撤销的决定仍留在记录里。")
            .count(),
        1
    );
    assert!(lines.iter().all(|line| !line.starts_with("你修改")));
    assert!(lines
        .iter()
        .filter(|line| line.contains("原文仍写0"))
        .all(|line| line.starts_with("原文=")));
    assert!(text.contains("这些信息不代表什么：凭据写明、你修改（原文仍写X）、已撤销。"));
    assert!(text.contains("格式 4 不收纯文本片段。这份单没有把纯文本塞进这个库。"));
    assert!(text.contains("这份只读打开可能让 SQLite 碰到 -shm。"));
    assert!(text.contains("没有另存的未匹配文字。"));
    assert!(!text.contains("Nothing owed"));
    assert!(BILL.contains("Nothing owed"));
    assert!(!text.contains("http://"));
    assert!(!text.contains("https://"));
    assert_eq!(sha256(&raw), sha256(fs::read(&sheet).unwrap().as_slice()));

    let found = blocks(&text);
    assert_eq!(found.len(), 3, "{text}");
    let facts_blocks: Vec<_> = found
        .iter()
        .filter(|block| block.iter().any(|line| line == "种类 事实"))
        .cloned()
        .collect();
    let other: Vec<_> = found
        .iter()
        .filter(|block| block.iter().any(|line| line == "种类 候选"))
        .cloned()
        .collect();
    assert_eq!(facts_blocks.len(), 2);
    assert_eq!(other.len(), 1);
    let table = csv_table(&packed_csv);
    assert_eq!(table[0][0], "fact_id");
    assert_eq!(table.len(), 3, "{table:?}");
    for (block, row) in facts_blocks.iter().zip(table.iter().skip(1)) {
        assert_csv(block, row);
        assert_span(block, BILL);
        assert!(block
            .iter()
            .any(|line| line == "media_type=text/html;charset=utf-8"));
        assert!(block.iter().any(|line| line == "role=user_provided_html"));
        assert!(block
            .iter()
            .any(|line| line == &format!("object_id={object_id}")));
        assert!(block
            .iter()
            .any(|line| line == &format!("content_sha256={}", sha256(BILL.as_bytes()))));
        assert!(block.iter().any(|line| line == "payment_verified=false"));
        assert!(block.iter().any(|line| line == "observed_at=null"));
        assert!(block.iter().any(|line| line == "current_status=unknown"));
        assert!(block.iter().any(|line| line == "time_status=unknown"));
    }
    let amount_block = facts_blocks
        .iter()
        .find(|block| current(block, "fact_type") == "stated_amount_paid")
        .unwrap();
    let period_block = facts_blocks
        .iter()
        .find(|block| current(block, "fact_type") == "stated_service_period")
        .unwrap();
    assert_eq!(current(amount_block, "amount_text"), "12.00");
    assert_eq!(current(amount_block, "revision"), "4");
    assert_eq!(current(amount_block, "statement"), "source_stated");
    assert_eq!(current(amount_block, "currency"), "USD");
    assert_eq!(current(period_block, "period_start"), "2026-09-01");
    assert_eq!(current(period_block, "period_end"), "2026-09-30");
    assert_eq!(current(period_block, "amount_text"), "");
    assert_eq!(current(period_block, "revision"), "1");
    assert!(amount_block
        .iter()
        .any(|line| line == "凭据写明 amount_text=15.99"));
    assert_eq!(line_value(amount_block, "显示="), "Amount paid USD 15.99");
    assert_eq!(line_value(amount_block, "显示版本="), "html-collapse-1");
    assert!(line_value(amount_block, "原文=").contains("15&#46;99"));
    assert!(line_value(amount_block, "原文=").contains("<b>"));
    assert_ne!(
        line_value(amount_block, "原文="),
        line_value(amount_block, "显示=")
    );
    assert_eq!(
        line_value(period_block, "显示="),
        "Service period: 2026-09-01 through 2026-09-30"
    );
    let amount_json = facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap();
    let decision_lines: Vec<_> = amount_block
        .iter()
        .filter(|line| line.contains("kind="))
        .collect();
    let stored = amount_json["decisions"].as_array().unwrap();
    assert_eq!(decision_lines.len(), stored.len());
    assert_eq!(stored.len(), 4);
    for (line, decision) in decision_lines.iter().zip(stored) {
        assert_eq!(token(line, "kind"), decision["kind"].as_str().unwrap());
        assert_eq!(
            token(line, "decision_id"),
            decision["decision_id"].as_str().unwrap()
        );
        assert_eq!(
            token(line, "recorded_at"),
            decision["recorded_at"].as_i64().unwrap().to_string()
        );
    }
    assert!(decision_lines[0].contains("凭据写明 amount_text=15.99"));
    assert!(decision_lines[1].contains("你修改（原文仍写15.99）当前 10.00"));
    assert!(decision_lines[2].contains("已撤销 当前 15.99"));
    assert!(decision_lines[3].contains("你修改（原文仍写15.99）当前 12.00"));
    assert!(period_block
        .iter()
        .all(|line| !line.contains("你修改（原文仍写")));
    assert!(period_block
        .iter()
        .any(|line| line.contains("凭据写明 period_start=2026-09-01 period_end=2026-09-30")));

    let candidate = &other[0];
    assert!(candidate.iter().any(|line| line == "不是事实"));
    assert!(candidate.iter().any(|line| line == "当前值 无"));
    assert!(candidate.iter().any(|line| line == "凭据写明 没有事实"));
    assert!(candidate
        .iter()
        .any(|line| line == "未知原因 reason=ambiguous_association"));
    assert!(candidate.iter().any(|line| line == "usable=false"));
    assert_eq!(line_value(candidate, "显示="), "");
    assert_eq!(line_value(candidate, "显示版本="), "");
    assert!(candidate
        .iter()
        .all(|line| !line.starts_with("amount_text=")));
    assert_span(candidate, BILL);
    let stored_candidate = candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["value"]["reason"] == "ambiguous_association")
        .unwrap();
    assert_eq!(
        unescape(line_value(candidate, "原文=")),
        stored_candidate["excerpt"].as_str().unwrap()
    );
    assert!(text.contains("\"extraction\":\"conflict\""));
    assert!(text.contains("\"reason\":\"ambiguous_association\""));

    let sheet_bytes = fs::read(&sheet).unwrap();
    let (code, again) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        sheet.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{again}");
    assert_eq!(again["reason"], "output_exists");
    assert_eq!(again["output_written"], false);
    assert_eq!(again["read_only"], true);
    assert_eq!(again["new_effect"], false);
    assert!(again.get("published").is_none(), "{again}");
    assert_eq!(fs::read(&sheet).unwrap(), sheet_bytes);

    let inside = vault.join("inside.txt");
    let (code, inside_body) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        inside.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{inside_body}");
    assert_eq!(inside_body["reason"], "output_inside_vault");
    assert!(!inside.exists());
    let taken = tmp.path.join("taken.txt");
    fs::write(&taken, b"").unwrap();
    let (code, taken_body) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        taken.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{taken_body}");
    assert_eq!(taken_body["reason"], "output_exists");
    assert_eq!(fs::read(&taken).unwrap(), b"");
    let missing = tmp.path.join("missing").join("review.txt");
    let (code, missing_body) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{missing_body}");
    assert_eq!(missing_body["reason"], "output_parent_missing");
    assert!(!missing.exists());

    let (code, refused) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "plain-no",
        "--fragment",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["reason"], "format_4_refuses_fragment");
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    let (code, reopened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{reopened}");
    assert_eq!(reopened["outcome"], "Ready");
    let (code, facts_ready) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts_ready}");
    assert_eq!(facts_ready["facts"], facts["facts"]);
}

#[test]
fn review_sheet_reads_a_format3_fragment_without_a_contract_change() {
    let tmp = Tmp::new("plain");
    let vault = init(&tmp, "vault", "3");
    let source = tmp.path.join("note.txt");
    fs::write(&source, PLAIN.as_bytes()).unwrap();
    let (code, imported) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "plain",
        "--fragment",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    let object_id = imported["object_id"].as_str().unwrap();
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        PLAIN.as_bytes()
    );
    let package = tmp.path.join("package");
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let before = files(&vault);
    let sheet = tmp.path.join("review.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        sheet.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["slice"], "T3-facts");
    assert_eq!(reviewed["package_associated"], false);
    assert_eq!(reviewed["originals_verified"], 0);
    assert_eq!(reviewed["fact_count"], 1);
    assert_eq!(reviewed["candidate_not_fact_count"], 1);
    assert_eq!(reviewed["read_only"], true);
    assert_eq!(reviewed["new_effect"], false);
    assert!(reviewed.get("published").is_none(), "{reviewed}");
    same_except_sqlite_sidecars(&before, &files(&vault));
    let text = fs::read_to_string(&sheet).unwrap();
    assert!(!text.contains("Not a stored line"));
    assert!(PLAIN.contains("Not a stored line"));
    assert!(!text.contains("格式 4 不收纯文本片段。"));
    assert_eq!(text.lines().next(), Some("合成开发审阅单"));
    let head = text.split("\n详单\n").next().unwrap();
    assert!(head.contains("当前记录金额 USD 8.00，与原件所写相同。"));
    assert!(!head.contains("人工修改后的值"));
    assert!(!head.contains("服务期"));
    assert!(!head.contains("同一来源上的多条事实"));
    assert!(head.contains("付款未核。不能证明订阅仍有效。"));
    assert!(head.contains("单独生成，未关联导出目录。"));
    assert!(!head.contains("originals/"));
    let found = blocks(&text);
    let fact = found
        .iter()
        .find(|block| block.iter().any(|line| line == "种类 事实"))
        .unwrap();
    let candidate = found
        .iter()
        .find(|block| block.iter().any(|line| line == "种类 候选"))
        .unwrap();
    let table = csv_table(&fs::read(package.join("facts.csv")).unwrap());
    assert_eq!(table.len(), 2);
    assert_csv(fact, &table[1]);
    assert_eq!(current(fact, "amount_text"), "8.00");
    assert_eq!(current(fact, "revision"), "1");
    assert!(fact.iter().any(|line| line == "凭据写明 amount_text=8.00"));
    assert!(fact.iter().all(|line| !line.contains("你修改（原文仍写")));
    assert!(fact
        .iter()
        .any(|line| line == "media_type=text/plain;charset=utf-8"));
    assert!(fact
        .iter()
        .any(|line| line == "role=user_provided_fragment"));
    assert_span(fact, PLAIN);
    assert!(candidate
        .iter()
        .any(|line| line == "未知原因 reason=payment_nature_not_stated"));
    assert!(candidate.iter().any(|line| line == "当前值 无"));
    assert!(candidate
        .iter()
        .all(|line| !line.starts_with("amount_text=")));
    assert_span(candidate, PLAIN);
    assert_eq!(unescape(line_value(candidate, "原文=")), "Total USD 40.00");
    let (code, facts_after) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts_after}");
    assert_eq!(facts_after["facts"], facts["facts"]);
    let (code, again) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        sheet.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{again}");
    assert_eq!(again["reason"], "output_exists");
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        PLAIN.as_bytes()
    );
}

#[test]
fn review_refuses_old_vaults_hooks_and_a_missing_path() {
    let tmp = Tmp::new("refuse");
    let older = init(&tmp, "v1", "1");
    let out = tmp.path.join("v1.txt");
    let (code, body) = run(&[
        "review",
        "--vault",
        older.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "needs_upgrade");
    assert!(!out.exists());
    let format2 = init(&tmp, "v2", "2");
    let out2 = tmp.path.join("v2.txt");
    let (code, body) = run(&[
        "review",
        "--vault",
        format2.to_str().unwrap(),
        "--out",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "facts_require_format_3");
    assert!(!out2.exists());
    let (code, body) = run(&["review", "--vault", older.to_str().unwrap()]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "review_required_flags");
    let missing = tmp.path.join("nowhere.txt");
    let (code, body) = run(&[
        "review",
        "--vault",
        older.to_str().unwrap(),
        "--out",
        missing.to_str().unwrap(),
        "--stop-at",
        "before_commit",
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "review_has_no_hook");
    assert!(!missing.exists());
    let (code, body) = run(&[
        "review",
        "--vault",
        older.to_str().unwrap(),
        "--out",
        missing.to_str().unwrap(),
        "--inject",
        "io_error:commit",
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "review_has_no_hook");
    assert!(!missing.exists());
    let (code, help) = run(&["help"]);
    assert_eq!(code, 0, "{help}");
    assert!(help["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command == "review --vault PATH --out PATH [--package PATH]"));
    let note = help["note"].as_str().unwrap();
    assert!(note.contains("not a user-ready product"));
    assert!(note.contains("not a Windows screen"));
}

#[test]
fn review_conclusion_follows_stored_fields_and_stays_empty_without_facts() {
    let tmp = Tmp::new("other-amount");
    let vault = init(&tmp, "vault", "3");
    let source = tmp.path.join("note.txt");
    let note = "Amount paid USD 4.00\nService period: 2024-01-02 through 2024-03-04\n";
    fs::write(&source, note.as_bytes()).unwrap();
    let (code, imported) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "other",
        "--fragment",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let amount = facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap();
    let (code, corrected) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "other-fix",
        "--fact-id",
        amount["fact_id"].as_str().unwrap(),
        "--expected-revision",
        "1",
        "--amount-text",
        "6.50",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{corrected}");
    let sheet = tmp.path.join("review.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        sheet.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    let text = fs::read_to_string(&sheet).unwrap();
    let head = text.split("\n详单\n").next().unwrap();
    assert!(head.contains("当前记录金额 USD 6.50 是人工修改后的值，原件仍写 4.00。"));
    assert!(head.contains("服务期 2024-01-02 至 2024-03-04。"));
    assert!(head.find("当前记录金额").unwrap() < head.find("服务期").unwrap());
    assert!(head.contains("同一来源上的多条事实不代表已核同一笔支付。"));
    assert!(!head.contains("12.00"));
    assert!(!head.contains("15.99"));
    assert!(!head.contains("2026-09"));
    assert!(!head.contains("10.00"));
    assert!(!head.contains("已撤销"));
    let detail = text.split("\n详单\n").nth(1).unwrap();
    assert!(detail.contains("你修改（原文仍写4.00）当前 6.50"));

    let empty = init(&tmp, "empty", "4");
    let blank = tmp.path.join("blank.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        empty.to_str().unwrap(),
        "--out",
        blank.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["fact_count"], 0);
    let text = fs::read_to_string(&blank).unwrap();
    let head = text.split("\n详单\n").next().unwrap();
    assert!(head.contains("当前没有事实。没有可写的当前金额或服务期。"));
    assert!(head.contains("付款未核。不能证明订阅仍有效。"));
    assert!(!head.contains("USD"));
    assert!(!head.contains("服务期 "));
    assert!(!head.contains("同一来源上的多条事实"));
}

#[test]
fn review_names_a_package_original_only_after_the_bytes_match() {
    let tmp = Tmp::new("locate");
    let vault = init(&tmp, "vault", "3");
    let source = tmp.path.join("note.txt");
    fs::write(&source, PLAIN.as_bytes()).unwrap();
    let (code, imported) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "locate",
        "--fragment",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    let object_id = imported["object_id"].as_str().unwrap();
    let package = tmp.path.join("package");
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    let original = package.join("originals").join(object_id);
    let packed = fs::read(&original).unwrap();
    assert_eq!(packed, PLAIN.as_bytes());
    assert_eq!(sha256(&packed), sha256(PLAIN.as_bytes()));

    let alone = tmp.path.join("alone.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        alone.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["package_associated"], false);
    let text = fs::read_to_string(&alone).unwrap();
    assert!(text.contains("单独生成，未关联导出目录。"));
    assert!(text.contains("摘录不是全文。"));
    assert!(!text.contains("originals/"));
    assert!(!text.contains(package.to_str().unwrap()));

    let empty = tmp.path.join("empty-export");
    fs::create_dir(&empty).unwrap();
    let missed = tmp.path.join("missed.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--package",
        empty.to_str().unwrap(),
        "--out",
        missed.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["package_associated"], true);
    assert_eq!(reviewed["originals_verified"], 0);
    let text = fs::read_to_string(&missed).unwrap();
    assert!(text.contains("给出的导出目录里没有对上的原件，本单不写路径。"));
    assert!(!text.contains("originals/"));

    let truncated = tmp.path.join("truncated");
    fs::create_dir_all(truncated.join("originals")).unwrap();
    fs::write(truncated.join("originals").join(object_id), b"nope").unwrap();
    let short = tmp.path.join("short.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--package",
        truncated.to_str().unwrap(),
        "--out",
        short.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["originals_verified"], 0);
    assert!(!fs::read_to_string(&short).unwrap().contains("originals/"));

    let linked = tmp.path.join("linked");
    fs::create_dir_all(linked.join("originals")).unwrap();
    std::os::unix::fs::symlink(&original, linked.join("originals").join(object_id)).unwrap();
    let link_sheet = tmp.path.join("link.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--package",
        linked.to_str().unwrap(),
        "--out",
        link_sheet.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["originals_verified"], 0);
    assert!(!fs::read_to_string(&link_sheet)
        .unwrap()
        .contains("originals/"));

    let named = tmp.path.join("named.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--package",
        package.to_str().unwrap(),
        "--out",
        named.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    assert_eq!(reviewed["originals_verified"], 1);
    let text = fs::read_to_string(&named).unwrap();
    let head = text.split("\n详单\n").next().unwrap();
    assert!(head.contains(&format!("原件相对位置 originals/{object_id}")));
    assert_eq!(head.matches("原件相对位置").count(), 1);
    assert!(!text.contains(package.to_str().unwrap()));
    assert!(!text.contains(source.to_str().unwrap()));
    assert_eq!(fs::read(&original).unwrap(), PLAIN.as_bytes());
    assert_eq!(
        sha256(&fs::read(&original).unwrap()),
        sha256(PLAIN.as_bytes())
    );

    let (code, refused) = run(&[
        "facts",
        "--vault",
        vault.to_str().unwrap(),
        "--package",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{refused}");
    assert_eq!(refused["reason"], "package_only_on_review");
}
