use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde_json::{json, Map, Value};

use crate::body;
use crate::fsutil::atomic_write;
use crate::ids::{self, is_command_id, is_prefixed_id};
use crate::journal::{self, Journal, LabelMiss};
use crate::model::{
    db_fail, honesty, unseen_db, Fail, Report, EXIT_NOT_PRESERVED, EXIT_OK, EXIT_RECOVERY,
    EXIT_REJECTED, EXIT_USAGE, EXIT_VERSION,
};
use crate::store;

pub(super) const PAYLOAD_VERSION: &str = "t3-payload-1";
pub(super) const RULE_ACTOR_ID: &str = "act_00000000000000000000000000000001";

pub(super) struct StatedDraft<'a> {
    pub candidate_id: &'a str,
    pub kind: &'a str,
    pub value: &'a Map<String, Value>,
}

pub(super) struct Adopted {
    pub fact_id: String,
    pub decision_id: String,
    pub candidate_id: String,
    pub fact_type: String,
}

pub(super) fn fragment_canonical(hash: &str, logical_key: &str, adoption: &str) -> String {
    format!(
        "{PAYLOAD_VERSION}\nfragment\ncontent_sha256={hash}\nmedia_type={}\nrules_version={}\nlogical_key={logical_key}\nadoption={adoption}\n",
        body::MEDIA_TYPE,
        body::RULES_VERSION
    )
}

pub(super) fn require_fact_tables(conn: &Connection) -> Result<(), Fail> {
    for name in crate::store::FACT_TABLES {
        if !crate::store::table_exists(conn, name).map_err(|err| db_fail(err, "fact_schema"))? {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_schema_mismatch")
                    .extra("new_effect", json!(false)),
            );
        }
    }
    Ok(())
}

pub(super) fn adopt_statements(
    tx: &Transaction<'_>,
    command_id: &str,
    source_id: &str,
    source_version_id: &str,
    run_id: &str,
    rule_version: &str,
    drafts: &[StatedDraft<'_>],
) -> Result<Vec<Adopted>, Fail> {
    let recorded_at = super::now_ms();
    let mut adopted = Vec::new();
    for draft in drafts {
        let value = stated_value(draft)?;
        let fact_id =
            ids::random_id("fact_").map_err(|err| crate::model::io_fail(err, "random"))?;
        let decision_id =
            ids::random_id("dec_").map_err(|err| crate::model::io_fail(err, "random"))?;
        let value_json = Value::Object(value).to_string();
        tx.execute(
            "INSERT INTO facts(fact_id, source_id, source_version_id, run_id, candidate_id, fact_type, value_json, revision) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            params![
                fact_id,
                source_id,
                source_version_id,
                run_id,
                draft.candidate_id,
                draft.kind,
                value_json
            ],
        )
        .map_err(|err| db_fail(err, "insert_fact"))?;
        tx.execute(
            "INSERT INTO decisions(decision_id, fact_id, command_id, candidate_id, slot, kind, actor_kind, actor_id, recorded_at, rule_version, prior_value_json, new_value_json, target_decision_id) VALUES (?1, ?2, ?3, ?4, ?5, 'rule_adopt', 'rule', ?6, ?7, ?8, NULL, ?9, NULL)",
            params![
                decision_id,
                fact_id,
                command_id,
                draft.candidate_id,
                draft.kind,
                RULE_ACTOR_ID,
                recorded_at,
                rule_version,
                value_json
            ],
        )
        .map_err(|err| db_fail(err, "insert_decision"))?;
        adopted.push(Adopted {
            fact_id,
            decision_id,
            candidate_id: draft.candidate_id.to_string(),
            fact_type: draft.kind.to_string(),
        });
    }
    Ok(adopted)
}

pub(super) fn insert_receipt(
    tx: &Transaction<'_>,
    command_id: &str,
    canonical: &str,
    result_json: &str,
) -> Result<(), Fail> {
    tx.execute(
        "INSERT INTO t3_receipts(command_id, payload_version, canonical_payload, result_json, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            command_id,
            PAYLOAD_VERSION,
            canonical,
            result_json,
            super::now_ms()
        ],
    )
    .map_err(|err| db_fail(err, "insert_t3_receipt"))?;
    Ok(())
}

fn stated_value(draft: &StatedDraft<'_>) -> Result<Map<String, Value>, Fail> {
    let mut value = Map::new();
    value.insert("type".into(), json!(draft.kind));
    value.insert("statement".into(), json!("source_stated"));
    value.insert("payment_verified".into(), json!(false));
    value.insert("purchase_id".into(), Value::Null);
    value.insert("account_id".into(), Value::Null);
    value.insert("observed_at".into(), Value::Null);
    value.insert("time_status".into(), json!("unknown"));
    value.insert("current_status".into(), json!("unknown"));
    match draft.kind {
        "stated_amount_paid" => {
            let amount = draft
                .value
                .get("amount_text")
                .and_then(|item| item.as_str())
                .filter(|text| body::usd_amount_text(text))
                .ok_or_else(|| {
                    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "amount_text_unreadable")
                        .extra("new_effect", json!(false))
                })?;
            let currency = draft
                .value
                .get("currency")
                .and_then(|item| item.as_str())
                .ok_or_else(|| {
                    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "currency_unreadable")
                        .extra("new_effect", json!(false))
                })?;
            if currency != "USD" {
                return Err(
                    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "currency_unreadable")
                        .extra("new_effect", json!(false)),
                );
            }
            value.insert("currency".into(), json!("USD"));
            value.insert("amount_text".into(), json!(amount));
        }
        "stated_service_period" => {
            let start = required_date(draft, "period_start")?;
            let end = required_date(draft, "period_end")?;
            value.insert("period_start".into(), json!(start));
            value.insert("period_end".into(), json!(end));
        }
        _ => {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_type_unreadable")
                    .extra("new_effect", json!(false)),
            );
        }
    }
    Ok(value)
}

fn required_date(draft: &StatedDraft<'_>, key: &str) -> Result<String, Fail> {
    let text = draft
        .value
        .get(key)
        .and_then(|item| item.as_str())
        .filter(|text| date_text(text))
        .ok_or_else(|| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "period_unreadable")
                .extra("new_effect", json!(false))
        })?;
    Ok(text.to_string())
}

fn date_text(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

pub fn query_facts(vault: &Path, source_id: Option<String>, fact_id: Option<String>) -> Report {
    match query_inner(vault, source_id.as_deref(), fact_id.as_deref()) {
        Ok((code, body)) => Report { code, body },
        Err(fail) => tag(fail.into_report()),
    }
}

fn tag(mut report: Report) -> Report {
    if let Some(map) = report.body.as_object_mut() {
        if map.get("slice").and_then(|value| value.as_str()) == Some("T1") {
            map.insert("slice".into(), json!("T3-facts"));
        }
    }
    report
}

fn query_inner(
    vault: &Path,
    source_id: Option<&str>,
    fact_id: Option<&str>,
) -> Result<(i32, Value), Fail> {
    if let Some(id) = source_id {
        if !is_prefixed_id(id, "src_") {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
        }
    }
    if let Some(id) = fact_id {
        if !is_prefixed_id(id, "fact_") {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
        }
    }
    super::guard_vault(vault)?;
    let version = super::inspect_version_readonly(vault)?;
    refuse_without_facts(version)?;
    let uri = super::wal_visible_uri(&vault.join("vault.db"))?;
    let conn = Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| unseen_db(err, "facts_open"))?;
    super::accept_connection_format(&vault.join("vault.db"), &conn)?;
    require_fact_tables(&conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT fact_id, source_id, source_version_id, run_id, candidate_id, fact_type, value_json, revision FROM facts WHERE (?1 IS NULL OR source_id = ?1) AND (?2 IS NULL OR fact_id = ?2) ORDER BY source_id, fact_id",
        )
        .map_err(|err| unseen_db(err, "facts"))?;
    let rows = stmt
        .query_map(params![source_id, fact_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(|err| unseen_db(err, "facts"))?;
    let mut facts = Vec::new();
    for row in rows {
        let row = row.map_err(|err| unseen_db(err, "facts"))?;
        let value: Value = serde_json::from_str(&row.6).map_err(|_| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_value_unreadable")
                .extra("fact_id", json!(row.0))
                .extra("new_effect", json!(false))
        })?;
        if !value.is_object() {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_value_unreadable")
                    .extra("fact_id", json!(row.0))
                    .extra("new_effect", json!(false)),
            );
        }
        let locator = candidate_anchor(&conn, &row.4, &row.1)?;
        let decisions = decision_history(&conn, &row.0)?;
        facts.push(json!({
            "fact_id": row.0,
            "source_id": row.1,
            "source_version_id": row.2,
            "run_id": row.3,
            "candidate_id": row.4,
            "fact_type": row.5,
            "revision": row.7,
            "value": value,
            "observed_at": Value::Null,
            "time_status": "unknown",
            "current_status": "unknown",
            "purchase_id": Value::Null,
            "account_id": Value::Null,
            "payment_verified": false,
            "locator": locator,
            "decisions": decisions
        }));
    }
    let mut body = honesty("Facts");
    body.insert(
        "slice".into(),
        json!(if version == 4 { "T4-html" } else { "T3-facts" }),
    );
    body.insert("new_effect".into(), json!(false));
    body.insert("read_only".into(), json!(true));
    body.insert("facts".into(), Value::Array(facts));
    Ok((EXIT_OK, Value::Object(body)))
}

fn candidate_anchor(conn: &Connection, candidate_id: &str, source_id: &str) -> Result<Value, Fail> {
    let found = conn
        .query_row(
            "SELECT c.excerpt, c.locator_json, c.kind, sv.source_id FROM candidates c JOIN source_versions sv ON sv.source_version_id = c.source_version_id WHERE c.candidate_id = ?1",
            [candidate_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|err| unseen_db(err, "fact_anchor"))?;
    let Some((excerpt, locator_json, kind, anchored_source)) = found else {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_anchor_missing")
                .extra("candidate_id", json!(candidate_id))
                .extra("new_effect", json!(false)),
        );
    };
    if anchored_source != source_id {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_anchor_mismatch")
                .extra("candidate_id", json!(candidate_id))
                .extra("new_effect", json!(false)),
        );
    }
    let locator: Value = serde_json::from_str(&locator_json).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "locator_unreadable")
            .extra("candidate_id", json!(candidate_id))
            .extra("new_effect", json!(false))
    })?;
    let mut anchor = json!({
        "candidate_id": candidate_id,
        "kind": kind,
        "excerpt": excerpt,
        "locator": locator
    });
    if let Some(display) = anchor
        .get("locator")
        .and_then(|item| item.get("display_excerpt"))
        .cloned()
    {
        if let Some(map) = anchor.as_object_mut() {
            map.insert("display_excerpt".into(), display);
        }
    }
    Ok(anchor)
}

fn decision_history(conn: &Connection, fact_id: &str) -> Result<Vec<Value>, Fail> {
    let mut stmt = conn
        .prepare(
            "SELECT decision_id, command_id, candidate_id, slot, kind, actor_kind, actor_id, recorded_at, rule_version, prior_value_json, new_value_json, target_decision_id FROM decisions WHERE fact_id = ?1 ORDER BY rowid",
        )
        .map_err(|err| unseen_db(err, "decisions"))?;
    let rows = stmt
        .query_map([fact_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
            ))
        })
        .map_err(|err| unseen_db(err, "decisions"))?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|err| unseen_db(err, "decisions"))?;
        let prior = parse_optional_object(row.9.as_deref(), "prior_value_unreadable", &row.0)?;
        let new_value = parse_object(&row.10, "new_value_unreadable", &row.0)?;
        out.push(json!({
            "decision_id": row.0,
            "command_id": row.1,
            "candidate_id": row.2,
            "slot": row.3,
            "kind": row.4,
            "actor_kind": row.5,
            "actor_id": row.6,
            "recorded_at": row.7,
            "rule_version": row.8,
            "prior_value": prior,
            "new_value": new_value,
            "target_decision_id": row.11
        }));
    }
    Ok(out)
}

fn parse_object(text: &str, reason: &'static str, decision_id: &str) -> Result<Value, Fail> {
    let value: Value = serde_json::from_str(text).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", reason)
            .extra("decision_id", json!(decision_id))
            .extra("new_effect", json!(false))
    })?;
    if !value.is_object() {
        return Err(Fail::new(EXIT_RECOVERY, "RecoveryRequired", reason)
            .extra("decision_id", json!(decision_id))
            .extra("new_effect", json!(false)));
    }
    Ok(value)
}

fn parse_optional_object(
    text: Option<&str>,
    reason: &'static str,
    decision_id: &str,
) -> Result<Value, Fail> {
    match text {
        None => Ok(Value::Null),
        Some(text) => parse_object(text, reason, decision_id),
    }
}

fn refuse_without_facts(version: i64) -> Result<(), Fail> {
    if version == 1 {
        return Err(Fail::new(EXIT_VERSION, "NeedsUpgrade", "needs_upgrade")
            .extra("slice", json!("T3-facts"))
            .extra("new_effect", json!(false))
            .extra("vault_format", json!(1))
            .extra("created_new_vault", json!(false)));
    }
    if version == 2 {
        return Err(
            Fail::new(EXIT_VERSION, "VersionUnsupported", "facts_require_format_3")
                .extra("slice", json!("T3-facts"))
                .extra("new_effect", json!(false))
                .extra("vault_format", json!(2))
                .extra("created_new_vault", json!(false)),
        );
    }
    Ok(())
}

const DECISION_STOPS: &[&str] = &["before_decision_commit"];
const DECISION_INJECTS: &[&str] = &[
    "receipt_read",
    "decision_tx",
    "db_commit",
    "commit_error_visible",
    "commit_error_absent",
    "commit_error_unseen",
    "commit_error_unreadable",
];
pub(super) const SIMULATED_COMMIT_ERROR: &str = "simulated commit() error";

pub struct CorrectRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub fact_id: String,
    pub expected_revision: String,
    pub amount_text: String,
    pub actor_id: String,
    pub hooks: super::Hooks,
}

pub struct UndoRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub decision_id: String,
    pub expected_revision: String,
    pub actor_id: String,
    pub hooks: super::Hooks,
}

pub fn correct_fact(req: CorrectRequest) -> Report {
    match correct_inner(req) {
        Ok(body) => tag(Report {
            code: EXIT_OK,
            body,
        }),
        Err(fail) => tag(fail.into_report()),
    }
}

pub fn undo_decision(req: UndoRequest) -> Report {
    match undo_inner(req) {
        Ok(body) => tag(Report {
            code: EXIT_OK,
            body,
        }),
        Err(fail) => tag(fail.into_report()),
    }
}

fn correct_inner(req: CorrectRequest) -> Result<Value, Fail> {
    super::validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        DECISION_STOPS,
        DECISION_INJECTS,
        DECISION_INJECTS,
    )?;
    if !is_command_id(&req.command_id) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_command_id"));
    }
    if !is_prefixed_id(&req.fact_id, "fact_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
    }
    let revision = parse_revision(&req.expected_revision)?;
    if !body::usd_amount_text(&req.amount_text) {
        return Err(rejected("amount_text_rejected"));
    }
    let actor_id = human_actor(&req.actor_id)?;
    let canonical = correct_canonical(
        &req.fact_id,
        &req.expected_revision,
        &req.amount_text,
        &actor_id,
    );
    require_decision_vault(&req.vault)?;
    let session = super::open_session(&req.vault)?;
    require_fact_tables(&session.conn)?;
    let mut hooks = super::HookRun::new(req.hooks);
    injected_receipt_read(&hooks)?;
    if let Some(body) = take_committed(&session, &req.command_id, &canonical)? {
        return Ok(body);
    }
    let hash = super::fingerprint(&[&canonical]);
    let mut journal = prepare_journal(&session, &req.command_id, &hash)?;
    let fact = load_fact(&session.conn, &req.fact_id)?.ok_or_else(|| rejected("unknown_fact"))?;
    if fact.fact_type != "stated_amount_paid" {
        return Err(rejected("not_amount_fact"));
    }
    if fact.revision != revision {
        return Err(rejected("revision_conflict"));
    }
    journal.state = "started".into();
    journal.payload_fingerprint = Some(hash);
    journal.reason = None;
    super::write_journal(&session.root, &journal)?;
    hooks.stop(
        "before_decision_commit",
        super::checkpoint_map(&req.command_id, 0, None, None, false, false),
    )?;
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    hooks
        .inject(
            "decision_tx",
            super::checkpoint_map(&req.command_id, 0, None, None, false, false),
        )
        .map_err(decision_inject)?;
    let fact = load_fact(&tx, &req.fact_id)?.ok_or_else(|| rejected("unknown_fact"))?;
    if fact.revision != revision {
        return Err(rejected("revision_conflict"));
    }
    if read_receipt(&tx, &req.command_id)?.is_ready() {
        return Err(rejected("command_race"));
    }
    let mut value = fact.value.clone();
    value.insert("amount_text".into(), json!(req.amount_text));
    let new_json = Value::Object(value).to_string();
    let decision_id = ids::random_id("dec_").map_err(|err| crate::model::io_fail(err, "random"))?;
    let recorded_at = super::now_ms();
    insert_human(
        &tx,
        &decision_id,
        &fact,
        &req.command_id,
        "human_correct",
        &actor_id,
        recorded_at,
        Some(&fact.value_json),
        &new_json,
        None,
    )?;
    let updated = tx
        .execute(
            "UPDATE facts SET value_json = ?2, revision = ?3 WHERE fact_id = ?1 AND revision = ?4",
            params![fact.fact_id, new_json, fact.revision + 1, fact.revision],
        )
        .map_err(|err| db_fail(err, "update_fact"))?;
    if updated != 1 {
        return Err(rejected("revision_conflict"));
    }
    let mut body = decision_body("Committed", &req.command_id, &fact, fact.revision + 1);
    body.insert("decision_id".into(), json!(decision_id));
    body.insert("amount_text".into(), json!(req.amount_text));
    body.insert("currency".into(), json!("USD"));
    body.insert("actor_kind".into(), json!("human"));
    body.insert("actor_id".into(), json!(actor_id));
    body.insert("target_decision_id".into(), Value::Null);
    body.insert("new_effect".into(), json!(true));
    let result = Value::Object(body);
    let encoded = serde_json::to_string(&result)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    insert_receipt(&tx, &req.command_id, &canonical, &encoded)?;
    hooks
        .inject(
            "db_commit",
            super::checkpoint_map(&req.command_id, 0, None, None, false, false),
        )
        .map_err(decision_inject)?;
    conclude_commit(
        &session.root,
        &session.root.join("vault.db"),
        &mut journal,
        &req.command_id,
        &canonical,
        result,
        tx,
        &hooks,
    )
}

fn undo_inner(req: UndoRequest) -> Result<Value, Fail> {
    super::validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        DECISION_STOPS,
        DECISION_INJECTS,
        DECISION_INJECTS,
    )?;
    if !is_command_id(&req.command_id) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_command_id"));
    }
    if !is_prefixed_id(&req.decision_id, "dec_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
    }
    let revision = parse_revision(&req.expected_revision)?;
    let actor_id = human_actor(&req.actor_id)?;
    let canonical = undo_canonical(&req.decision_id, &req.expected_revision, &actor_id);
    require_decision_vault(&req.vault)?;
    let session = super::open_session(&req.vault)?;
    require_fact_tables(&session.conn)?;
    let mut hooks = super::HookRun::new(req.hooks);
    injected_receipt_read(&hooks)?;
    if let Some(body) = take_committed(&session, &req.command_id, &canonical)? {
        return Ok(body);
    }
    let hash = super::fingerprint(&[&canonical]);
    let mut journal = prepare_journal(&session, &req.command_id, &hash)?;
    let target = load_decision(&session.conn, &req.decision_id)?
        .ok_or_else(|| rejected("unknown_decision"))?;
    let fact = load_fact(&session.conn, &target.fact_id)?.ok_or_else(|| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_missing")
            .extra("new_effect", json!(false))
    })?;
    if fact.revision != revision {
        return Err(rejected("revision_conflict"));
    }
    if successor_count(&session.conn, &fact.fact_id, &target.decision_id)? > 0
        || fact.value_json != target.new_value_json
    {
        return Err(rejected("successor_conflict"));
    }
    journal.state = "started".into();
    journal.payload_fingerprint = Some(hash);
    journal.reason = None;
    super::write_journal(&session.root, &journal)?;
    hooks.stop(
        "before_decision_commit",
        super::checkpoint_map(&req.command_id, 0, None, None, false, false),
    )?;
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    hooks
        .inject(
            "decision_tx",
            super::checkpoint_map(&req.command_id, 0, None, None, false, false),
        )
        .map_err(decision_inject)?;
    let target =
        load_decision(&tx, &req.decision_id)?.ok_or_else(|| rejected("unknown_decision"))?;
    let fact = load_fact(&tx, &target.fact_id)?.ok_or_else(|| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_missing")
            .extra("new_effect", json!(false))
    })?;
    if fact.revision != revision {
        return Err(rejected("revision_conflict"));
    }
    if successor_count(&tx, &fact.fact_id, &target.decision_id)? > 0
        || fact.value_json != target.new_value_json
    {
        return Err(rejected("successor_conflict"));
    }
    if read_receipt(&tx, &req.command_id)?.is_ready() {
        return Err(rejected("command_race"));
    }
    let restored = match &target.prior_value_json {
        Some(prior) => prior.clone(),
        None => unknown_value(&fact).to_string(),
    };
    let decision_id = ids::random_id("dec_").map_err(|err| crate::model::io_fail(err, "random"))?;
    let recorded_at = super::now_ms();
    insert_human(
        &tx,
        &decision_id,
        &fact,
        &req.command_id,
        "undo",
        &actor_id,
        recorded_at,
        Some(&fact.value_json),
        &restored,
        Some(&target.decision_id),
    )?;
    let updated = tx
        .execute(
            "UPDATE facts SET value_json = ?2, revision = ?3 WHERE fact_id = ?1 AND revision = ?4",
            params![fact.fact_id, restored, fact.revision + 1, fact.revision],
        )
        .map_err(|err| db_fail(err, "update_fact"))?;
    if updated != 1 {
        return Err(rejected("revision_conflict"));
    }
    let mut body = decision_body("Committed", &req.command_id, &fact, fact.revision + 1);
    body.insert("decision_id".into(), json!(decision_id));
    body.insert("target_decision_id".into(), json!(target.decision_id));
    body.insert("actor_kind".into(), json!("human"));
    body.insert("actor_id".into(), json!(actor_id));
    body.insert(
        "amount_text".into(),
        serde_json::from_str::<Value>(&restored)
            .ok()
            .and_then(|value| value.get("amount_text").cloned())
            .unwrap_or(Value::Null),
    );
    body.insert("new_effect".into(), json!(true));
    let result = Value::Object(body);
    let encoded = serde_json::to_string(&result)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    insert_receipt(&tx, &req.command_id, &canonical, &encoded)?;
    hooks
        .inject(
            "db_commit",
            super::checkpoint_map(&req.command_id, 0, None, None, false, false),
        )
        .map_err(decision_inject)?;
    conclude_commit(
        &session.root,
        &session.root.join("vault.db"),
        &mut journal,
        &req.command_id,
        &canonical,
        result,
        tx,
        &hooks,
    )
}

fn correct_canonical(fact_id: &str, revision: &str, amount: &str, actor_id: &str) -> String {
    format!(
        "{PAYLOAD_VERSION}\ncorrect\nfact_id={fact_id}\nexpected_revision={revision}\ncurrency=USD\namount_text={amount}\nactor_id={actor_id}\n"
    )
}

fn undo_canonical(decision_id: &str, revision: &str, actor_id: &str) -> String {
    format!(
        "{PAYLOAD_VERSION}\nundo\ndecision_id={decision_id}\nexpected_revision={revision}\nactor_id={actor_id}\n"
    )
}

fn parse_revision(text: &str) -> Result<i64, Fail> {
    if text.is_empty()
        || text.starts_with('+')
        || text.starts_with('-')
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_revision"));
    }
    let revision = text
        .parse::<i64>()
        .map_err(|_| Fail::new(EXIT_USAGE, "Usage", "bad_revision"))?;
    if revision < 1 {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_revision"));
    }
    Ok(revision)
}

fn human_actor(actor_id: &str) -> Result<String, Fail> {
    if !is_prefixed_id(actor_id, "act_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_actor"));
    }
    if actor_id == RULE_ACTOR_ID {
        return Err(rejected("actor_not_human"));
    }
    Ok(actor_id.to_string())
}

fn require_decision_vault(vault: &Path) -> Result<(), Fail> {
    super::guard_vault(vault)?;
    let version = super::inspect_version_readonly(vault)?;
    refuse_without_facts(version)?;
    Ok(())
}

fn rejected(reason: &str) -> Fail {
    Fail::new(EXIT_REJECTED, "Rejected", reason)
        .extra("new_effect", json!(false))
        .extra("decision_committed", json!(false))
        .extra("facts_changed", json!(false))
}

fn decision_inject(mut fail: Fail) -> Fail {
    fail.extra.insert("decision_committed".into(), json!(false));
    fail.extra.insert("facts_changed".into(), json!(false));
    fail.extra.insert(
        "note".into(),
        json!("injected fault before commit; not a COMMIT visibility result"),
    );
    fail
}

fn injected_receipt_read(hooks: &super::HookRun) -> Result<(), Fail> {
    if hooks.hooks.inject_point.as_deref() != Some("receipt_read") {
        return Ok(());
    }
    let path = hooks.hooks.witness.clone().ok_or_else(|| {
        Fail::new(EXIT_USAGE, "Usage", "witness_required").extra("point", json!("receipt_read"))
    })?;
    atomic_write(
        &path,
        br#"{"reached":true,"point":"receipt_read","action":"unseen"}"#,
    )
    .map_err(|err| crate::model::io_fail(err, "witness"))?;
    Err(
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unseen")
            .extra("injected", json!(true))
            .extra("inject_point", json!("receipt_read"))
            .extra(
                "note",
                json!("injected fault, not a failed read and not a commit result"),
            )
            .extra("new_effect", json!(false))
            .extra("decision_committed", json!(false)),
    )
}

pub(super) enum ReceiptRead {
    Absent,
    Ready {
        canonical: String,
        result_json: String,
    },
    Unreadable,
}

impl ReceiptRead {
    fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

fn read_receipt(conn: &Connection, command_id: &str) -> Result<ReceiptRead, Fail> {
    let row = conn
        .query_row(
            "SELECT payload_version, canonical_payload, result_json FROM t3_receipts WHERE command_id = ?1",
            [command_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unseen")
                .extra("during", json!("t3_receipt"))
                .extra("new_effect", json!(false))
                .extra("decision_committed", json!(false))
        })?;
    let Some((version, canonical, result_json)) = row else {
        return Ok(ReceiptRead::Absent);
    };
    if version != PAYLOAD_VERSION {
        return Ok(ReceiptRead::Unreadable);
    }
    match serde_json::from_str::<Value>(&result_json) {
        Ok(Value::Object(_)) => Ok(ReceiptRead::Ready {
            canonical,
            result_json,
        }),
        _ => Ok(ReceiptRead::Unreadable),
    }
}

fn take_committed(
    session: &super::Session,
    command_id: &str,
    canonical: &str,
) -> Result<Option<Value>, Fail> {
    match read_receipt(&session.conn, command_id)? {
        ReceiptRead::Unreadable => {
            Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unreadable")
                    .extra("new_effect", json!(false))
                    .extra("decision_committed", json!(false)),
            )
        }
        ReceiptRead::Absent => {
            if store::command(&session.conn, command_id)
                .map_err(|err| unseen_db(err, "command_lookup"))?
                .is_some()
            {
                Err(rejected("command_kind_conflict"))
            } else {
                Ok(None)
            }
        }
        ReceiptRead::Ready {
            canonical: stored,
            result_json,
        } => {
            if stored != canonical {
                return Err(rejected("payload_conflict"));
            }
            let mut value: Value = serde_json::from_str(&result_json).map_err(|_| {
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unreadable")
                    .extra("new_effect", json!(false))
            })?;
            let Some(map) = value.as_object_mut() else {
                return Err(
                    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unreadable")
                        .extra("new_effect", json!(false)),
                );
            };
            map.insert("outcome".into(), json!("AlreadyCommitted"));
            map.insert("new_effect".into(), json!(false));
            if let Err(fail) = label_committed_if_present(&session.root, command_id) {
                if fail.extra.get("during").and_then(Value::as_str) != Some("journal_write") {
                    return Err(fail);
                }
                map.insert("post_commit_io".into(), json!(true));
                map.insert("journal_label_update".into(), json!("failed"));
            }
            Ok(Some(value))
        }
    }
}

fn label_committed_if_present(root: &Path, command_id: &str) -> Result<(), Fail> {
    let path = journal::path(root, command_id);
    if !path.exists() {
        return Ok(());
    }
    let mut journal = journal::read_file(&path).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("new_effect", json!(false))
    })?;
    if journal.kind != "decision" {
        return Err(rejected("command_kind_conflict"));
    }
    if journal.state == "committed" {
        return Ok(());
    }
    journal.state = "committed".into();
    journal.reason = None;
    super::write_journal(root, &journal)?;
    Ok(())
}

fn prepare_journal(
    session: &super::Session,
    command_id: &str,
    hash: &str,
) -> Result<Journal, Fail> {
    require_fact_tables(&session.conn)?;
    let path = journal::path(&session.root, command_id);
    if !path.exists() {
        return Ok(fresh_journal(command_id));
    }
    let journal = journal::read_file(&path).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("new_effect", json!(false))
    })?;
    if journal.kind != "decision" {
        return Err(rejected("command_kind_conflict"));
    }
    if let Some(previous) = &journal.payload_fingerprint {
        if previous != hash {
            return Err(rejected("payload_conflict"));
        }
    }
    match journal.state.as_str() {
        "started" | "aborted" => Ok(journal),
        "committed" => Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_missing")
                .extra("new_effect", json!(false))
                .extra("decision_committed", json!(false)),
        ),
        _ => Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "decision_journal_state")
                .extra("new_effect", json!(false))
                .extra("decision_committed", json!(false)),
        ),
    }
}

fn fresh_journal(command_id: &str) -> Journal {
    Journal {
        command_id: command_id.to_string(),
        kind: "decision".into(),
        generation: 1,
        state: "started".into(),
        batch_id: None,
        display_name: None,
        external_path: None,
        media_type: None,
        occurrence_id: None,
        object_id: None,
        content_sha256: None,
        byte_length: None,
        payload_fingerprint: None,
        storage_reused: None,
        reason: None,
        marker_value: None,
        source_id: None,
    }
}

struct FactHead {
    fact_id: String,
    fact_type: String,
    value_json: String,
    value: Map<String, Value>,
    revision: i64,
    candidate_id: String,
    source_id: String,
}

fn load_fact(conn: &Connection, fact_id: &str) -> Result<Option<FactHead>, Fail> {
    let row = conn
        .query_row(
            "SELECT fact_id, fact_type, value_json, revision, candidate_id, source_id FROM facts WHERE fact_id = ?1",
            [fact_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|err| unseen_db(err, "fact_lookup"))?;
    let Some((fact_id, fact_type, value_json, revision, candidate_id, source_id)) = row else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&value_json).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_value_unreadable")
            .extra("fact_id", json!(fact_id))
            .extra("new_effect", json!(false))
    })?;
    let Value::Object(value) = value else {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "fact_value_unreadable")
                .extra("fact_id", json!(fact_id))
                .extra("new_effect", json!(false)),
        );
    };
    Ok(Some(FactHead {
        fact_id,
        fact_type,
        value_json,
        value,
        revision,
        candidate_id,
        source_id,
    }))
}

struct DecisionHead {
    decision_id: String,
    fact_id: String,
    new_value_json: String,
    prior_value_json: Option<String>,
}

fn load_decision(conn: &Connection, decision_id: &str) -> Result<Option<DecisionHead>, Fail> {
    let row = conn
        .query_row(
            "SELECT decision_id, fact_id, new_value_json, prior_value_json FROM decisions WHERE decision_id = ?1",
            [decision_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|err| unseen_db(err, "decision_lookup"))?;
    Ok(row.map(|row| DecisionHead {
        decision_id: row.0,
        fact_id: row.1,
        new_value_json: row.2,
        prior_value_json: row.3,
    }))
}

fn successor_count(conn: &Connection, fact_id: &str, decision_id: &str) -> Result<i64, Fail> {
    conn.query_row(
        "SELECT count(*) FROM decisions WHERE fact_id = ?1 AND rowid > (SELECT rowid FROM decisions WHERE decision_id = ?2)",
        params![fact_id, decision_id],
        |row| row.get(0),
    )
    .map_err(|err| unseen_db(err, "successor_lookup"))
}

fn insert_human(
    tx: &Transaction<'_>,
    decision_id: &str,
    fact: &FactHead,
    command_id: &str,
    kind: &str,
    actor_id: &str,
    recorded_at: i64,
    prior: Option<&str>,
    new_value: &str,
    target: Option<&str>,
) -> Result<(), Fail> {
    tx.execute(
        "INSERT INTO decisions(decision_id, fact_id, command_id, candidate_id, slot, kind, actor_kind, actor_id, recorded_at, rule_version, prior_value_json, new_value_json, target_decision_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'human', ?7, ?8, NULL, ?9, ?10, ?11)",
        params![
            decision_id,
            fact.fact_id,
            command_id,
            fact.candidate_id,
            fact.fact_type,
            kind,
            actor_id,
            recorded_at,
            prior,
            new_value,
            target
        ],
    )
    .map_err(|err| db_fail(err, "insert_decision"))?;
    Ok(())
}

fn unknown_value(fact: &FactHead) -> Value {
    let mut value = Map::new();
    value.insert("type".into(), json!(fact.fact_type));
    value.insert("statement".into(), json!("unknown"));
    value.insert("payment_verified".into(), json!(false));
    value.insert("purchase_id".into(), Value::Null);
    value.insert("account_id".into(), Value::Null);
    value.insert("observed_at".into(), Value::Null);
    value.insert("time_status".into(), json!("unknown"));
    value.insert("current_status".into(), json!("unknown"));
    if fact.fact_type == "stated_amount_paid" {
        value.insert("currency".into(), json!("USD"));
        value.insert("amount_text".into(), Value::Null);
    } else {
        value.insert("period_start".into(), Value::Null);
        value.insert("period_end".into(), Value::Null);
    }
    Value::Object(value)
}

fn decision_body(
    outcome: &str,
    command_id: &str,
    fact: &FactHead,
    revision: i64,
) -> Map<String, Value> {
    let mut body = honesty(outcome);
    body.insert("slice".into(), json!("T3-facts"));
    body.insert("command_id".into(), json!(command_id));
    body.insert("fact_id".into(), json!(fact.fact_id));
    body.insert("source_id".into(), json!(fact.source_id));
    body.insert("candidate_id".into(), json!(fact.candidate_id));
    body.insert("revision".into(), json!(revision));
    body.insert("observed_at".into(), Value::Null);
    body.insert("time_status".into(), json!("unknown"));
    body.insert("current_status".into(), json!("unknown"));
    body.insert("payment_verified".into(), json!(false));
    body.insert("account_id".into(), Value::Null);
    body.insert("payload_version".into(), json!(PAYLOAD_VERSION));
    body.insert("rule_version".into(), Value::Null);
    body
}

enum CommitProbe {
    None,
    Visible,
    Absent,
    Unseen,
    Unreadable,
}

/// `correct` and `undo`. Fragment publish settles the same T3 receipt and does not read `commands`.
fn conclude_commit(
    root: &Path,
    db: &Path,
    journal: &mut Journal,
    command_id: &str,
    canonical: &str,
    result: Value,
    tx: Transaction<'_>,
    hooks: &super::HookRun,
) -> Result<Value, Fail> {
    match commit_probe(hooks)? {
        CommitProbe::None => match tx.commit() {
            Ok(()) => {
                journal.state = "committed".into();
                journal.reason = None;
                finish_decision(root, journal, result)
            }
            Err(err) => settle_commit_error(
                root,
                db,
                journal,
                command_id,
                canonical,
                &err.to_string(),
                false,
            ),
        },
        CommitProbe::Absent => {
            drop(tx);
            settle_commit_error(
                root,
                db,
                journal,
                command_id,
                canonical,
                SIMULATED_COMMIT_ERROR,
                true,
            )
        }
        CommitProbe::Visible => match tx.commit() {
            Ok(()) => settle_commit_error(
                root,
                db,
                journal,
                command_id,
                canonical,
                SIMULATED_COMMIT_ERROR,
                true,
            ),
            Err(err) => settle_commit_error(
                root,
                db,
                journal,
                command_id,
                canonical,
                &err.to_string(),
                false,
            ),
        },
        CommitProbe::Unreadable => match tx.commit() {
            Ok(()) => {
                corrupt_receipt_result(db, command_id)?;
                settle_commit_error(
                    root,
                    db,
                    journal,
                    command_id,
                    canonical,
                    SIMULATED_COMMIT_ERROR,
                    true,
                )
            }
            Err(err) => settle_commit_error(
                root,
                db,
                journal,
                command_id,
                canonical,
                &err.to_string(),
                false,
            ),
        },
        CommitProbe::Unseen => match tx.commit() {
            Ok(()) => settle_commit_error(
                root,
                &root.join("not-a-vault.db"),
                journal,
                command_id,
                canonical,
                SIMULATED_COMMIT_ERROR,
                true,
            ),
            Err(err) => settle_commit_error(
                root,
                db,
                journal,
                command_id,
                canonical,
                &err.to_string(),
                false,
            ),
        },
    }
}

fn commit_probe(hooks: &super::HookRun) -> Result<CommitProbe, Fail> {
    let Some(point) = hooks.hooks.inject_point.as_deref() else {
        return Ok(CommitProbe::None);
    };
    let probe = match point {
        "commit_error_visible" => CommitProbe::Visible,
        "commit_error_absent" => CommitProbe::Absent,
        "commit_error_unseen" => CommitProbe::Unseen,
        "commit_error_unreadable" => CommitProbe::Unreadable,
        _ => return Ok(CommitProbe::None),
    };
    let path = hooks.hooks.witness.clone().ok_or_else(|| {
        Fail::new(EXIT_USAGE, "Usage", "witness_required").extra("point", json!(point))
    })?;
    let action = match hooks.hooks.inject_action {
        Some(super::InjectAction::Enospc) => "enospc",
        Some(super::InjectAction::IoError) => "io_error",
        None => "simulated_commit_error",
    };
    let body = json!({
        "reached": true,
        "point": point,
        "action": action,
        "note": "simulated commit() error, not a disk COMMIT fault"
    });
    let bytes =
        serde_json::to_vec(&body).map_err(|_| Fail::new(EXIT_USAGE, "Usage", "witness_encode"))?;
    atomic_write(&path, &bytes).map_err(|err| crate::model::io_fail(err, "witness"))?;
    Ok(probe)
}

fn settle_commit_error(
    root: &Path,
    db: &Path,
    journal: &mut Journal,
    command_id: &str,
    canonical: &str,
    commit_error: &str,
    injected: bool,
) -> Result<Value, Fail> {
    let read = match read_t3_receipt_wal(db, command_id) {
        Ok(read) => read,
        Err(_) => return Err(receipt_unseen(commit_error, injected)),
    };
    match read {
        ReceiptRead::Absent => Err(receipt_absent(commit_error, injected)),
        ReceiptRead::Unreadable => Err(receipt_unseen(commit_error, injected)),
        ReceiptRead::Ready {
            canonical: stored,
            result_json,
        } => {
            let Some(mut value) =
                accepted_stored_result(command_id, canonical, &stored, &result_json)
            else {
                return Err(receipt_unseen(commit_error, injected));
            };
            if let Some(map) = value.as_object_mut() {
                map.insert("visible_despite_commit_error".into(), json!(true));
                map.insert("commit_error".into(), json!(commit_error));
                map.insert("injected".into(), json!(injected));
                map.insert("note".into(), json!(visible_note(injected)));
            }
            journal.state = "committed".into();
            journal.reason = None;
            finish_decision(root, journal, value)
        }
    }
}

/// Independent mode=ro connection. It can see a committed WAL frame and reads `t3_receipts` only.
pub(super) fn read_t3_receipt_wal(db: &Path, command_id: &str) -> Result<ReceiptRead, Fail> {
    if !super::sqlite_header_ok(db).unwrap_or(false) {
        return Err(Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "t3_receipt_unseen",
        ));
    }
    let uri = super::wal_visible_uri(db)
        .map_err(|_| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unseen"))?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unseen"))?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(0));
    read_receipt(&conn, command_id)
}

fn accepted_stored_result(
    command_id: &str,
    canonical: &str,
    stored: &str,
    result_json: &str,
) -> Option<Value> {
    if stored != canonical {
        return None;
    }
    let Value::Object(map) = serde_json::from_str(result_json).ok()? else {
        return None;
    };
    if map.get("outcome").and_then(Value::as_str) != Some("Committed") {
        return None;
    }
    if map.get("command_id").and_then(Value::as_str) != Some(command_id) {
        return None;
    }
    let decision = map.get("decision_id").and_then(Value::as_str)?;
    if decision.is_empty() {
        return None;
    }
    if map.get("revision").and_then(Value::as_i64)? < 1 {
        return None;
    }
    Some(Value::Object(map))
}

pub(super) fn corrupt_receipt_result(db: &Path, command_id: &str) -> Result<(), Fail> {
    let conn = Connection::open(db).map_err(|_| receipt_unseen(SIMULATED_COMMIT_ERROR, true))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(2));
    let changed = conn
        .execute(
            "UPDATE t3_receipts SET result_json = '{' WHERE command_id = ?1",
            [command_id],
        )
        .map_err(|_| receipt_unseen(SIMULATED_COMMIT_ERROR, true))?;
    if changed != 1 {
        return Err(receipt_unseen(SIMULATED_COMMIT_ERROR, true));
    }
    Ok(())
}

fn receipt_absent(commit_error: &str, injected: bool) -> Fail {
    Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "db_error")
        .extra("injected", json!(injected))
        .extra("during", json!("db_commit"))
        .extra("os_error", json!(commit_error))
        .extra("new_effect", json!(false))
        .extra("decision_committed", json!(false))
        .extra("facts_changed", json!(false))
        .extra(
            "note",
            json!(if injected {
                "simulated commit() error; independent read found no t3 receipt. Not a disk COMMIT fault."
            } else {
                "commit() returned an error; independent read found no t3 receipt."
            }),
        )
}

fn receipt_unseen(commit_error: &str, injected: bool) -> Fail {
    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unseen")
        .extra("injected", json!(injected))
        .extra("during", json!("t3_receipt"))
        .extra("commit_error", json!(commit_error))
        .extra("new_effect", Value::Null)
        .extra("decision_committed", Value::Null)
        .extra("facts_changed", Value::Null)
        .extra(
            "note",
            json!(if injected {
                "simulated commit() error; the independent t3 receipt read is unusable. Not a disk COMMIT fault. The journal stays started."
            } else {
                "commit() returned an error and the t3 receipt could not be read. The journal stays started."
            }),
        )
}

fn visible_note(injected: bool) -> &'static str {
    if injected {
        "simulated commit() error; the t3 receipt is visible, so the stored result stands. Not a disk COMMIT fault."
    } else {
        "commit() returned an error; the t3 receipt is visible, so the stored result stands."
    }
}

fn finish_decision(root: &Path, journal: &Journal, mut result: Value) -> Result<Value, Fail> {
    match super::write_journal(root, journal) {
        Ok(()) => Ok(result),
        Err(fail) => {
            if let Some(map) = result.as_object_mut() {
                map.insert("post_commit_io".into(), json!(true));
                map.insert("journal_label_update".into(), json!("failed"));
                map.insert("new_effect".into(), json!(true));
                if let Some(during) = fail.extra.get("during") {
                    map.insert("during".into(), during.clone());
                }
            }
            Ok(result)
        }
    }
}

pub(super) fn reconcile_decision_journals(
    root: &Path,
    conn: &Connection,
) -> Result<Vec<LabelMiss>, Fail> {
    let found = journal::scan(root);
    if found.unlisted.is_some() {
        return Ok(Vec::new());
    }
    let pending = found.valid.iter().any(|journal| {
        journal.kind == "decision" && journal.state != "committed" && journal.state != "aborted"
    });
    if pending {
        require_fact_tables(conn)?;
    }
    for journal in &found.valid {
        if journal.kind != "decision" {
            continue;
        }
        if matches!(
            read_receipt(conn, &journal.command_id)?,
            ReceiptRead::Unreadable
        ) {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unreadable")
                    .extra("command_id", json!(journal.command_id))
                    .extra("new_effect", json!(false)),
            );
        }
    }
    let mut misses = Vec::new();
    for journal in found.valid {
        if journal.kind != "decision" || journal.state == "committed" || journal.state == "aborted"
        {
            continue;
        }
        if journal.state != "started" {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "decision_journal_state")
                    .extra("command_id", json!(journal.command_id))
                    .extra("new_effect", json!(false)),
            );
        }
        let mut journal = journal;
        match read_receipt(conn, &journal.command_id)? {
            ReceiptRead::Unreadable => {
                return Err(
                    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unreadable")
                        .extra("command_id", json!(journal.command_id))
                        .extra("new_effect", json!(false)),
                );
            }
            ReceiptRead::Ready { .. } => {
                journal.state = "committed".into();
                journal.reason = None;
            }
            ReceiptRead::Absent => {
                journal.state = "aborted".into();
                journal.reason = Some("uncommitted".into());
            }
        }
        if let Err(fail) = super::write_journal(root, &journal) {
            misses.push(LabelMiss {
                command_id: journal.command_id,
                during: fail
                    .extra
                    .get("during")
                    .and_then(|value| value.as_str())
                    .unwrap_or("journal_write")
                    .to_string(),
                os_error: fail
                    .extra
                    .get("os_error")
                    .and_then(|value| value.as_str())
                    .unwrap_or(fail.reason.as_str())
                    .to_string(),
            });
        }
    }
    Ok(misses)
}

/// Format 3 fragment journals follow the T3 receipt. A commands row is not that commit.
pub(super) fn reconcile_fragment_journals(
    root: &Path,
    conn: &Connection,
) -> Result<Vec<LabelMiss>, Fail> {
    if !store::table_exists(conn, "t3_receipts").map_err(|err| unseen_db(err, "t3_receipt"))? {
        return Ok(Vec::new());
    }
    let found = journal::scan(root);
    if found.unlisted.is_some() {
        return Ok(Vec::new());
    }
    for journal in &found.valid {
        if journal.kind != "fragment" {
            continue;
        }
        if matches!(
            read_receipt(conn, &journal.command_id)?,
            ReceiptRead::Unreadable
        ) {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unreadable")
                    .extra("command_id", json!(journal.command_id))
                    .extra("new_effect", json!(false)),
            );
        }
    }
    let mut misses = Vec::new();
    for journal in found.valid {
        if journal.kind != "fragment" || journal.state == "committed" {
            continue;
        }
        if !matches!(
            read_receipt(conn, &journal.command_id)?,
            ReceiptRead::Ready { .. }
        ) {
            continue;
        }
        let mut journal = journal;
        journal.state = "committed".into();
        journal.reason = None;
        if let Err(fail) = super::write_journal(root, &journal) {
            misses.push(LabelMiss {
                command_id: journal.command_id,
                during: fail
                    .extra
                    .get("during")
                    .and_then(|value| value.as_str())
                    .unwrap_or("journal_write")
                    .to_string(),
                os_error: fail
                    .extra
                    .get("os_error")
                    .and_then(|value| value.as_str())
                    .unwrap_or(fail.reason.as_str())
                    .to_string(),
            });
        }
    }
    Ok(misses)
}

pub(super) fn reject_if_t3_command(conn: &Connection, command_id: &str) -> Result<(), Fail> {
    if !store::table_exists(conn, "t3_receipts").map_err(|err| unseen_db(err, "t3_receipt"))? {
        return Ok(());
    }
    match read_receipt(conn, command_id)? {
        ReceiptRead::Absent => Ok(()),
        ReceiptRead::Unreadable => {
            Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "t3_receipt_unreadable")
                    .extra("new_effect", json!(false)),
            )
        }
        ReceiptRead::Ready { .. } => Err(rejected("command_kind_conflict")),
    }
}
