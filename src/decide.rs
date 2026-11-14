use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Map, Value};

use crate::body;
use crate::ids::{self, is_prefixed_id};
use crate::model::{
    db_fail, honesty, unseen_db, Fail, Report, EXIT_OK, EXIT_RECOVERY, EXIT_USAGE, EXIT_VERSION,
};

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
                body::RULES_VERSION,
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
        map.insert("slice".into(), json!("T3-facts"));
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
    body.insert("slice".into(), json!("T3-facts"));
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
    Ok(json!({
        "candidate_id": candidate_id,
        "kind": kind,
        "excerpt": excerpt,
        "locator": locator
    }))
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
