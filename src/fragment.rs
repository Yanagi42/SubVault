use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::body::{self, Draft, ExtractError, InputError};
use crate::ids::{
    self, canonical_object_relpath, hex_encode, is_command_id, is_display_name, is_prefixed_id,
};
use crate::journal::{self, Journal};
use crate::model::{
    db_fail, honesty, io_fail, unseen_db, Fail, Report, EXIT_FAULT_MISSED, EXIT_INTEGRITY,
    EXIT_NOT_PRESERVED, EXIT_OK, EXIT_RECOVERY, EXIT_REJECTED, EXIT_USAGE, NOT_PRESERVED,
    PRESERVED,
};
use crate::store::{self, CommandRow, ObjectRow, OccurrenceRow, SourceRow};

use super::{
    already_committed, checkpoint_map, confirm_source_bytes, find_orphan_with_hash,
    finish_after_commit, hosted_facts_ok, now_ms, open_session, read_committed,
    require_hosted_object, take_cancel, validate_hook_names, write_control, write_journal,
    CancelRequest, HookRun, Hooks, RECOGNIZED_INJECTS,
};

const STOPS: &[&str] = &["before_copy", "before_publish"];
const INJECTS: &[&str] = &[
    "body_schema",
    "extract_status",
    "publish_tx",
    "result_encode",
    "replay_tx",
    "db_commit",
];

pub struct FragmentRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub fragment: PathBuf,
    pub batch_id: Option<String>,
    pub hooks: Hooks,
}

pub fn submit_fragment(req: FragmentRequest) -> Report {
    match submit_inner(req) {
        Ok(body) => Report {
            code: EXIT_OK,
            body,
        },
        Err(fail) => super::note_uncommitted(fail).into_report(),
    }
}

pub fn query_candidates(vault: &Path, source_id: Option<String>) -> Report {
    match query_inner(vault, source_id.as_deref()) {
        Ok(body) => Report {
            code: body.0,
            body: body.1,
        },
        Err(fail) => super::note_uncommitted(fail).into_report(),
    }
}

pub fn cancel_fragment(req: CancelRequest) -> Result<Report, Fail> {
    if !is_command_id(&req.command_id) || req.generation < 1 {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_cancel"));
    }
    let Some(lock) = crate::lockfile::VaultLock::try_acquire(&req.vault.join("writer.lock"))
        .map_err(|err| super::unseen_io(err, "lock"))?
    else {
        return cancel_while_busy(&req);
    };
    let _lock = lock;
    super::guard_vault(&req.vault)?;
    let conn = super::open_writer(&req.vault)?;
    if let Some(existing) =
        store::command(&conn, &req.command_id).map_err(|err| unseen_db(err, "command_lookup"))?
    {
        let mut body = already_committed(&existing)?;
        if let Some(map) = body.as_object_mut() {
            map.insert("cancel_applied".into(), json!(false));
            map.insert("candidates_published".into(), json!(true));
        }
        return Ok(Report {
            code: EXIT_OK,
            body,
        });
    }
    let path = journal::path(&req.vault, &req.command_id);
    if !path.exists() {
        return Err(refused("unknown_command").extra("cancel_applied", json!(false)));
    }
    let mut journal = journal::read_file(&path).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("cancel_applied", json!(false))
    })?;
    if journal.kind != "fragment" {
        return Err(refused("command_kind_conflict").extra("cancel_applied", json!(false)));
    }
    if journal.generation != req.generation {
        return Err(refused("generation_mismatch").extra("cancel_applied", json!(false)));
    }
    let preserved = source_is_preserved(&conn, journal.source_id.as_deref())?;
    if let Some(attempt) = running_attempt(&conn, &req.command_id)? {
        set_attempt(&conn, &attempt, "cancelled", 0, "cancelled")?;
    }
    journal.state = "aborted".into();
    journal.reason = Some("cancelled".into());
    write_journal(&req.vault, &journal)?;
    let _ = fs::remove_file(req.vault.join("control").join(&req.command_id));
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(cancel_body(
            "Cancelled",
            &req.command_id,
            req.generation,
            true,
            preserved,
        )),
    })
}

fn submit_inner(req: FragmentRequest) -> Result<Value, Fail> {
    let mut recognized: Vec<&str> = RECOGNIZED_INJECTS.to_vec();
    recognized.extend_from_slice(INJECTS);
    validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        STOPS,
        &recognized,
        INJECTS,
    )?;
    if !is_command_id(&req.command_id) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_command_id"));
    }
    if let Some(batch) = &req.batch_id {
        if !is_command_id(batch) {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_batch_id"));
        }
    }
    let stat_len = stat_fragment(&req.fragment)?;
    let session = open_session(&req.vault)?;
    let mut hooks = HookRun::new(req.hooks.clone());
    if let Some(existing) = store::command(&session.conn, &req.command_id)
        .map_err(|err| unseen_db(err, "command_lookup"))?
    {
        if req.hooks.stop_at.is_some() || req.hooks.inject_point.is_some() {
            return Err(missed("command_already_committed"));
        }
        return replay_command(&session, &req, &existing);
    }
    let mut journal = load_journal(&session.root, &req)?;
    hooks.stop(
        "before_copy",
        checkpoint_map(&req.command_id, 0, Some(stat_len), None, false, false),
    )?;
    if take_cancel(
        &session.root,
        &req.command_id,
        journal.generation,
        "fragment",
    )? {
        return finish_cancel(&session, &mut journal, None);
    }
    let bytes = read_capped(&req.fragment, stat_len)?;
    let text = match body::prepare(&bytes) {
        Ok(text) => text.to_string(),
        Err(InputError::InvalidUtf8 { bytes_read }) => {
            abort_journal(&session.root, &mut journal, "invalid_utf8")?;
            return Err(refused("invalid_utf8")
                .extra("bytes_read", json!(bytes_read))
                .extra("preservation", json!(NOT_PRESERVED)));
        }
        Err(InputError::Empty) => {
            abort_journal(&session.root, &mut journal, "empty_fragment")?;
            return Err(refused("empty_fragment").extra("bytes_read", json!(0)));
        }
        Err(InputError::OverBudget { stat_size }) => {
            abort_journal(&session.root, &mut journal, "over_input_budget")?;
            return Err(refused("over_input_budget")
                .extra("bytes_read", json!(0))
                .extra("stat_size", json!(stat_size)));
        }
    };
    ensure_schema(&session.conn, &mut hooks)?;
    let hash = sha256_bytes(&bytes);
    let key = body::logical_key(&hash);
    journal.content_sha256 = Some(hash.clone());
    journal.byte_length = Some(bytes.len() as i64);
    journal.payload_fingerprint = Some(key.clone());
    journal.media_type = Some(body::MEDIA_TYPE.into());
    write_journal(&session.root, &journal)?;

    if let Some(source) =
        store::source_by_hash(&session.conn, &hash).map_err(|err| db_fail(err, "source_lookup"))?
    {
        confirm_source_bytes(&session, &source, &req.command_id)?;
        if source.media_type != body::MEDIA_TYPE
            || version_for(&session.conn, &source.source_id)?.is_none()
        {
            abort_journal(&session.root, &mut journal, "not_a_submitted_fragment")?;
            return Err(refused("not_a_submitted_fragment")
                .extra("preservation", json!(PRESERVED))
                .extra("object_intact", json!(true))
                .extra("source_id", json!(source.source_id))
                .extra("content_sha256", json!(hash))
                .extra("media_type", json!(source.media_type)));
        }
    }

    let object = place_object(&session.root, &session.conn, &bytes, &hash, &mut journal)?;
    let opened = open_attempt(&session.conn, &req, &journal, &object, &hash, &key)?;
    journal.source_id = Some(opened.source_id.clone());
    journal.object_id = Some(opened.object_id.clone());
    journal.state = "object_ready".into();
    write_journal(&session.root, &journal)?;
    // The source row is committed. Later failures of this call must not fall
    // through note_uncommitted's default NotPreserved. Pre-commit failures,
    // including body_schema, return above this line and keep that default.
    let source_id = opened.source_id.clone();
    let kept_hash = hash.clone();
    continue_after_source(
        &session,
        &req,
        &mut journal,
        &opened,
        &hash,
        &text,
        &mut hooks,
    )
    .map_err(|fail| after_source_committed(fail, &source_id, &kept_hash))
}

fn continue_after_source(
    session: &super::Session,
    req: &FragmentRequest,
    journal: &mut Journal,
    opened: &Opened,
    hash: &str,
    text: &str,
    hooks: &mut HookRun,
) -> Result<Value, Fail> {
    if opened.already_succeeded {
        match req.hooks.inject_point.as_deref() {
            Some("replay_tx") | Some("result_encode") => {}
            Some(_) => return Err(missed("run_already_succeeded")),
            None => {}
        }
        if req.hooks.stop_at.is_some() {
            return Err(missed("run_already_succeeded"));
        }
        let receipt = publish_replay(&session.conn, req, journal, opened, hash, hooks)?;
        hook_reached(hooks)?;
        return finish_journal(&session.root, journal, receipt);
    }

    let drafts = match body::extract(text) {
        Ok(drafts) => drafts,
        Err(error) => {
            if req.hooks.inject_point.as_deref() == Some("extract_status") {
                hooks.inject(
                    "extract_status",
                    checkpoint_map(
                        &req.command_id,
                        0,
                        journal.byte_length.map(|n| n as u64),
                        Some(hash),
                        true,
                        false,
                    ),
                )?;
                return Err(missed("extract_status"));
            }
            let reason = match error {
                ExtractError::OverLine => "over_line",
                ExtractError::TooMany => "too_many_candidates",
            };
            set_attempt(&session.conn, &opened.attempt_id, "failed", 0, reason)?;
            set_run_status(&session.conn, &opened.run_id, "failed")?;
            journal.state = "aborted".into();
            journal.reason = Some(reason.into());
            write_journal(&session.root, journal)?;
            return Err(refused(reason)
                .extra("preservation", json!(PRESERVED))
                .extra("object_intact", json!(true))
                .extra("candidates_published", json!(false))
                .extra("source_id", json!(opened.source_id))
                .extra("content_sha256", json!(hash))
                .extra("attempt_id", json!(opened.attempt_id))
                .extra("run_id", json!(opened.run_id)));
        }
    };
    match req.hooks.inject_point.as_deref() {
        Some(point @ ("extract_status" | "replay_tx")) => return Err(missed(point)),
        _ => {}
    }

    let byte_len = journal.byte_length.unwrap_or(0) as u64;
    hooks.stop(
        "before_publish",
        checkpoint_map(
            &req.command_id,
            byte_len,
            Some(byte_len),
            Some(hash),
            true,
            false,
        ),
    )?;
    if take_cancel(
        &session.root,
        &req.command_id,
        journal.generation,
        "fragment",
    )? {
        set_attempt(
            &session.conn,
            &opened.attempt_id,
            "cancelled",
            0,
            "cancelled",
        )?;
        set_run_status(&session.conn, &opened.run_id, "cancelled")?;
        return finish_cancel(session, journal, Some(opened));
    }

    let receipt = match publish(session, req, journal, opened, hash, &drafts, hooks) {
        Ok(receipt) => receipt,
        Err(fail) => return Err(note_status_writes(session, journal, opened, fail)),
    };
    hook_reached(hooks)?;
    finish_journal(&session.root, journal, receipt)
}

/// The source row survived. This call's error does not relabel it.
fn after_source_committed(mut fail: Fail, source_id: &str, hash: &str) -> Fail {
    // The constructors still say NotPreserved. This call did not publish, and
    // the source row is already stored. Exit codes stay with the constructor:
    // db_commit remains exit 3, a database error remains exit 15.
    if fail.outcome == "NotPreserved" {
        fail.outcome = "NotPublished";
    }
    fail.extra.insert("preservation".into(), json!(PRESERVED));
    if !fail.extra.contains_key("candidates_published") {
        fail.extra
            .insert("candidates_published".into(), json!(false));
    }
    if !fail.extra.contains_key("source_id") {
        fail.extra.insert("source_id".into(), json!(source_id));
    }
    if !fail.extra.contains_key("content_sha256") {
        fail.extra.insert("content_sha256".into(), json!(hash));
    }
    if !fail.extra.contains_key("new_effect") {
        fail.extra.insert("new_effect".into(), json!(false));
    }
    fail
}

fn note_status_writes(
    session: &super::Session,
    journal: &mut Journal,
    opened: &Opened,
    mut fail: Fail,
) -> Fail {
    let attempt = set_attempt(
        &session.conn,
        &opened.attempt_id,
        "failed",
        0,
        fail.reason.as_str(),
    );
    let run = set_run_status(&session.conn, &opened.run_id, "failed");
    if attempt.is_err() {
        fail = fail.extra("attempt_update", json!("failed"));
    }
    if run.is_err() {
        fail = fail.extra("run_update", json!("failed"));
    }
    if attempt.is_ok() && run.is_ok() {
        journal.state = "aborted".into();
        journal.reason = Some(fail.reason.clone());
        if write_journal(&session.root, journal).is_err() {
            fail = fail.extra("journal_update", json!("failed"));
        }
    }
    fail
}

fn hook_reached(hooks: &HookRun) -> Result<(), Fail> {
    if let Some(point) = hooks.hooks.inject_point.as_deref() {
        if !hooks.inject_reached {
            return Err(missed(point));
        }
    }
    Ok(())
}

struct Opened {
    source_id: String,
    source_version_id: String,
    object_id: String,
    run_id: String,
    attempt_id: String,
    attempt_no: i64,
    already_succeeded: bool,
    candidate_ids: Vec<String>,
}

fn open_attempt(
    conn: &Connection,
    req: &FragmentRequest,
    journal: &Journal,
    object: &ObjectRow,
    hash: &str,
    key: &str,
) -> Result<Opened, Fail> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    let source = ensure_source(&tx, journal, object, hash)?;
    let version = ensure_version(&tx, &source, hash)?;
    let existing = run_by_key(&tx, key)?;
    let opened = if let Some(run) = existing {
        if run.status == "succeeded" {
            let ids = candidate_ids(&tx, &run.run_id)?;
            Opened {
                source_id: source.source_id,
                source_version_id: version,
                object_id: source.object_id,
                run_id: run.run_id,
                attempt_id: String::new(),
                attempt_no: 0,
                already_succeeded: true,
                candidate_ids: ids,
            }
        } else {
            insert_occurrence_if_absent(&tx, req, journal, &source)?;
            tx.execute(
                "UPDATE extraction_attempts SET status = 'abandoned', published = 0 WHERE run_id = ?1 AND status = 'running'",
                [&run.run_id],
            )
            .map_err(|err| db_fail(err, "abandon_attempt"))?;
            let attempt_no = next_attempt_no(&tx, &run.run_id)?;
            let attempt_id = ids::random_id("att_").map_err(|err| io_fail(err, "random"))?;
            insert_attempt(
                &tx,
                &attempt_id,
                &run.run_id,
                attempt_no,
                &req.command_id,
                "running",
            )?;
            tx.execute(
                "UPDATE extraction_runs SET status = 'open' WHERE run_id = ?1",
                [&run.run_id],
            )
            .map_err(|err| db_fail(err, "reopen_run"))?;
            Opened {
                source_id: source.source_id,
                source_version_id: version,
                object_id: source.object_id,
                run_id: run.run_id,
                attempt_id,
                attempt_no,
                already_succeeded: false,
                candidate_ids: Vec::new(),
            }
        }
    } else {
        insert_occurrence_if_absent(&tx, req, journal, &source)?;
        let run_id = ids::random_id("run_").map_err(|err| io_fail(err, "random"))?;
        let attempt_id = ids::random_id("att_").map_err(|err| io_fail(err, "random"))?;
        tx.execute(
            "INSERT INTO extraction_runs(run_id, logical_key, source_version_id, source_id, extractor, extractor_version, normalization, join_version, rules_version, config_json, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'open')",
            params![
                run_id,
                key,
                version,
                source.source_id,
                body::EXTRACTOR,
                body::EXTRACTOR_VERSION,
                body::NORMALIZATION,
                body::JOIN,
                body::RULES_VERSION,
                body::config_json().to_string()
            ],
        )
        .map_err(|err| db_fail(err, "insert_run"))?;
        insert_attempt(&tx, &attempt_id, &run_id, 1, &req.command_id, "running")?;
        Opened {
            source_id: source.source_id,
            source_version_id: version,
            object_id: source.object_id,
            run_id,
            attempt_id,
            attempt_no: 1,
            already_succeeded: false,
            candidate_ids: Vec::new(),
        }
    };
    tx.commit().map_err(|err| db_fail(err, "attempt_commit"))?;
    Ok(opened)
}

fn publish(
    session: &super::Session,
    req: &FragmentRequest,
    journal: &Journal,
    opened: &Opened,
    hash: &str,
    drafts: &[Draft],
    hooks: &mut HookRun,
) -> Result<Value, Fail> {
    let source = store::source_by_id(&session.conn, &opened.source_id)
        .map_err(|err| db_fail(err, "source_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "source_row_missing")
                .extra("new_effect", json!(false))
        })?;
    confirm_source_bytes(session, &source, &req.command_id)?;
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    hooks.inject(
        "publish_tx",
        checkpoint_map(
            &req.command_id,
            hash.len() as u64,
            Some(hash.len() as u64),
            Some(hash),
            true,
            false,
        ),
    )?;
    let attempt_status = attempt_status(&tx, &opened.attempt_id)?;
    if attempt_status != "running" {
        return Err(refused("attempt_not_running")
            .extra("preservation", json!(PRESERVED))
            .extra("candidates_published", json!(false))
            .extra("late", json!(true))
            .extra("source_id", json!(opened.source_id)));
    }
    if store::command(&tx, &req.command_id)
        .map_err(|err| db_fail(err, "command_recheck"))?
        .is_some()
    {
        return Err(refused("command_race"));
    }
    let mut ids = Vec::new();
    for (ordinal, draft) in drafts.iter().enumerate() {
        let candidate_id = ids::random_id("cand_").map_err(|err| io_fail(err, "random"))?;
        let locator = locator_json(&opened.source_version_id, hash, draft);
        tx.execute(
            "INSERT INTO candidates(candidate_id, run_id, attempt_id, source_version_id, ordinal, kind, usable, excerpt, value_json, locator_json, adopted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
            params![
                candidate_id,
                opened.run_id,
                opened.attempt_id,
                opened.source_version_id,
                ordinal as i64,
                draft.kind,
                if draft.usable { 1 } else { 0 },
                draft.excerpt,
                Value::Object(draft.value.clone()).to_string(),
                locator.to_string()
            ],
        )
        .map_err(|err| db_fail(err, "insert_candidate"))?;
        ids.push(candidate_id);
    }
    tx.execute(
        "UPDATE extraction_attempts SET status = 'succeeded', published = 1, detail_json = ?2 WHERE attempt_id = ?1 AND status = 'running'",
        params![opened.attempt_id, json!({"reason": "published"}).to_string()],
    )
    .map_err(|err| db_fail(err, "finish_attempt"))?;
    tx.execute(
        "UPDATE extraction_runs SET status = 'succeeded' WHERE run_id = ?1",
        [&opened.run_id],
    )
    .map_err(|err| db_fail(err, "finish_run"))?;
    let receipt = success_receipt(req, journal, opened, hash, &ids, true);
    hooks.inject(
        "result_encode",
        checkpoint_map(
            &req.command_id,
            hash.len() as u64,
            Some(hash.len() as u64),
            Some(hash),
            true,
            false,
        ),
    )?;
    let encoded = serde_json::to_string(&receipt)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    store::insert_command(
        &tx,
        &CommandRow {
            command_id: req.command_id.clone(),
            kind: "fragment".into(),
            payload_fingerprint: journal.payload_fingerprint.clone().unwrap_or_default(),
            result_json: encoded,
            outcome: "Committed".into(),
        },
        now_ms(),
    )
    .map_err(|err| db_fail(err, "insert_command"))?;
    if let Err(fail) = hooks.inject(
        "db_commit",
        checkpoint_map(
            &req.command_id,
            hash.len() as u64,
            Some(hash.len() as u64),
            Some(hash),
            true,
            false,
        ),
    ) {
        return Err(not_published(fail, &opened.source_id, hash));
    }
    match tx.commit() {
        Ok(()) => Ok(receipt),
        Err(err) => Err(not_published(
            db_fail(err, "db_commit"),
            &opened.source_id,
            hash,
        )),
    }
}

fn publish_replay(
    conn: &Connection,
    req: &FragmentRequest,
    journal: &Journal,
    opened: &Opened,
    hash: &str,
    hooks: &mut HookRun,
) -> Result<Value, Fail> {
    hooks.inject(
        "replay_tx",
        checkpoint_map(
            &req.command_id,
            hash.len() as u64,
            Some(hash.len() as u64),
            Some(hash),
            true,
            true,
        ),
    )?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    let receipt = success_receipt(req, journal, opened, hash, &opened.candidate_ids, false);
    let mut replay = receipt.clone();
    if let Some(map) = replay.as_object_mut() {
        map.insert("outcome".into(), json!("AlreadyPublished"));
        map.insert("new_effect".into(), json!(false));
        map.insert("attempt_created".into(), json!(false));
    }
    hooks.inject(
        "result_encode",
        checkpoint_map(
            &req.command_id,
            hash.len() as u64,
            Some(hash.len() as u64),
            Some(hash),
            true,
            true,
        ),
    )?;
    let encoded = serde_json::to_string(&replay)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    store::insert_command(
        &tx,
        &CommandRow {
            command_id: req.command_id.clone(),
            kind: "fragment".into(),
            payload_fingerprint: journal.payload_fingerprint.clone().unwrap_or_default(),
            result_json: encoded,
            outcome: "AlreadyPublished".into(),
        },
        now_ms(),
    )
    .map_err(|err| db_fail(err, "insert_command"))?;
    tx.commit().map_err(|err| db_fail(err, "replay_commit"))?;
    Ok(replay)
}

fn success_receipt(
    req: &FragmentRequest,
    journal: &Journal,
    opened: &Opened,
    hash: &str,
    ids: &[String],
    created_attempt: bool,
) -> Value {
    let mut body = body_outcome(if created_attempt {
        "Committed"
    } else {
        "AlreadyPublished"
    });
    body.insert("new_effect".into(), json!(created_attempt));
    body.insert("preservation".into(), json!(PRESERVED));
    body.insert("arrangement".into(), json!("not_arranged"));
    body.insert("searchability".into(), json!("not_searchable"));
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("generation".into(), json!(1));
    body.insert("batch_id".into(), json!(journal.batch_id));
    body.insert("source_id".into(), json!(opened.source_id));
    body.insert("source_version_id".into(), json!(opened.source_version_id));
    body.insert("object_id".into(), json!(opened.object_id));
    body.insert("content_sha256".into(), json!(hash));
    body.insert("hash_alg".into(), json!("sha256"));
    body.insert("byte_length".into(), json!(journal.byte_length));
    body.insert("media_type".into(), json!(body::MEDIA_TYPE));
    body.insert("role".into(), json!(body::ROLE));
    body.insert("complete_message".into(), json!(false));
    body.insert("mailbox".into(), json!(false));
    body.insert("clipboard".into(), json!(false));
    body.insert("logical_key".into(), json!(journal.payload_fingerprint));
    body.insert("run_id".into(), json!(opened.run_id));
    body.insert(
        "attempt_id".into(),
        if created_attempt {
            json!(opened.attempt_id)
        } else {
            Value::Null
        },
    );
    body.insert("attempt_no".into(), json!(opened.attempt_no));
    body.insert("attempt_created".into(), json!(created_attempt));
    body.insert("candidates_published".into(), json!(true));
    body.insert("candidate_ids".into(), json!(ids));
    body.insert("candidate_count".into(), json!(ids.len()));
    body.insert("config".into(), body::config_json());
    body.insert("injected".into(), json!(false));
    Value::Object(body)
}

fn query_inner(vault: &Path, source_id: Option<&str>) -> Result<(i32, Value), Fail> {
    if let Some(id) = source_id {
        if !is_prefixed_id(id, "src_") {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
        }
    }
    super::guard_vault(vault)?;
    let uri = super::wal_visible_uri(&vault.join("vault.db"))?;
    let conn = Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| unseen_db(err, "candidates_open"))?;
    let schema = store::meta(&conn, "body_schema").map_err(|err| unseen_db(err, "body_schema"))?;
    if schema.is_none() {
        let mut body = body_outcome("Candidates");
        body.insert("new_effect".into(), json!(false));
        body.insert("body_schema".into(), Value::Null);
        body.insert("candidates".into(), json!([]));
        body.insert("runs".into(), json!([]));
        body.insert("read_only".into(), json!(true));
        return Ok((EXIT_OK, Value::Object(body)));
    }
    if schema.as_deref() != Some(store::BODY_SCHEMA) {
        return Err(refused("body_schema_unsupported").extra("new_effect", json!(false)));
    }
    for name in store::BODY_TABLES {
        if !store::table_exists(&conn, name).map_err(|err| unseen_db(err, "body_schema"))? {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "body_schema_mismatch")
                    .extra("new_effect", json!(false)),
            );
        }
    }
    let mut stmt = conn
        .prepare(
            "SELECT c.candidate_id, c.run_id, c.attempt_id, c.ordinal, c.kind, c.usable, c.excerpt, c.value_json, c.locator_json, c.adopted, sv.source_id, sv.object_id, sv.content_sha256, sv.byte_length, sv.role, sv.eml, r.status FROM candidates c JOIN source_versions sv ON sv.source_version_id = c.source_version_id JOIN extraction_runs r ON r.run_id = c.run_id WHERE (?1 IS NULL OR sv.source_id = ?1) ORDER BY sv.source_id, c.ordinal",
        )
        .map_err(|err| unseen_db(err, "candidates"))?;
    let rows = stmt
        .query_map(params![source_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, i64>(13)?,
                row.get::<_, String>(14)?,
                row.get::<_, i64>(15)?,
                row.get::<_, String>(16)?,
            ))
        })
        .map_err(|err| unseen_db(err, "candidates"))?;
    let mut candidates = Vec::new();
    let mut alerts = Vec::new();
    let mut damaged = false;
    let mut seen = std::collections::BTreeMap::<String, bool>::new();
    for row in rows {
        let row = row.map_err(|err| unseen_db(err, "candidates"))?;
        let object_id = row.11.clone();
        let intact = if let Some(flag) = seen.get(&object_id) {
            *flag
        } else {
            let facts = crate::facts::object_facts(
                vault,
                &object_id,
                Some(&format!("objects/{object_id}")),
                &row.12,
                row.13,
            );
            let ok = hosted_facts_ok(&facts).is_ok();
            if !ok {
                damaged = facts.missing
                    || !facts.hash_matches
                    || !facts.length_matches
                    || facts.id_rejected;
                alerts.push(json!({
                    "code": if facts.io_error.is_some() { "object_unreadable" } else if facts.missing { "object_missing" } else { "hash_mismatch" },
                    "source_id": row.10,
                    "object_id": object_id,
                }));
            }
            seen.insert(object_id.clone(), ok);
            ok
        };
        if !intact {
            continue;
        }
        let value = match serde_json::from_str::<Value>(&row.7) {
            Ok(value) => Some(value),
            Err(_) => {
                damaged = true;
                alerts.push(json!({
                    "code": "stored_record_unreadable",
                    "during": "value_json",
                    "candidate_id": row.0,
                }));
                None
            }
        };
        let locator = match serde_json::from_str::<Value>(&row.8) {
            Ok(value) => Some(value),
            Err(_) => {
                damaged = true;
                alerts.push(json!({
                    "code": "stored_record_unreadable",
                    "during": "locator_json",
                    "candidate_id": row.0,
                }));
                None
            }
        };
        let (Some(value), Some(locator)) = (value, locator) else {
            continue;
        };
        candidates.push(json!({
            "candidate_id": row.0,
            "run_id": row.1,
            "attempt_id": row.2,
            "ordinal": row.3,
            "kind": row.4,
            "usable": row.5 == 1,
            "excerpt": row.6,
            "value": value,
            "locator": locator,
            "adopted": row.9 == 1,
            "source_id": row.10,
            "object_id": row.11,
            "content_sha256": row.12,
            "role": row.14,
            "eml": if row.15 == 0 { Value::Null } else { json!(true) },
            "run_status": row.16,
            "effective": false,
            "purchase_id": null,
            "account_id": null,
            "relationship": null
        }));
    }
    drop(stmt);
    let runs = list_runs(&conn, source_id, &mut alerts, &mut damaged)?;
    let mut body = body_outcome("Candidates");
    body.insert("new_effect".into(), json!(false));
    body.insert("read_only".into(), json!(true));
    body.insert("body_schema".into(), json!(store::BODY_SCHEMA));
    body.insert("candidates".into(), Value::Array(candidates));
    body.insert("runs".into(), runs);
    body.insert("alerts".into(), Value::Array(alerts.clone()));
    if damaged {
        body.insert("outcome".into(), json!("RecoveryRequired"));
        return Ok((EXIT_RECOVERY, Value::Object(body)));
    }
    if !alerts.is_empty() {
        body.insert("outcome".into(), json!("IntegrityAlert"));
        return Ok((EXIT_INTEGRITY, Value::Object(body)));
    }
    Ok((EXIT_OK, Value::Object(body)))
}

fn list_runs(
    conn: &Connection,
    source_id: Option<&str>,
    alerts: &mut Vec<Value>,
    damaged: &mut bool,
) -> Result<Value, Fail> {
    let mut stmt = conn
        .prepare(
            "SELECT run_id, logical_key, source_id, source_version_id, extractor, extractor_version, normalization, join_version, rules_version, config_json, status FROM extraction_runs WHERE (?1 IS NULL OR source_id = ?1) ORDER BY run_id",
        )
        .map_err(|err| unseen_db(err, "runs"))?;
    let rows = stmt
        .query_map(params![source_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
            ))
        })
        .map_err(|err| unseen_db(err, "runs"))?;
    let mut runs = Vec::new();
    for row in rows {
        let row = row.map_err(|err| unseen_db(err, "runs"))?;
        let attempts = list_attempts(conn, &row.0, alerts, damaged)?;
        let mut run = json!({
            "run_id": row.0,
            "logical_key": row.1,
            "source_id": row.2,
            "source_version_id": row.3,
            "extractor": row.4,
            "extractor_version": row.5,
            "normalization": row.6,
            "join": row.7,
            "rules_version": row.8,
            "status": row.10,
            "attempts": attempts
        });
        match serde_json::from_str::<Value>(&row.9) {
            Ok(config) => {
                run.as_object_mut()
                    .expect("run object")
                    .insert("config".into(), config);
            }
            Err(_) => {
                *damaged = true;
                alerts.push(json!({
                    "code": "stored_record_unreadable",
                    "during": "config_json",
                    "run_id": row.0,
                }));
            }
        }
        runs.push(run);
    }
    Ok(Value::Array(runs))
}

fn list_attempts(
    conn: &Connection,
    run_id: &str,
    alerts: &mut Vec<Value>,
    damaged: &mut bool,
) -> Result<Value, Fail> {
    let mut stmt = conn
        .prepare(
            "SELECT attempt_id, attempt_no, command_id, status, published, detail_json FROM extraction_attempts WHERE run_id = ?1 ORDER BY attempt_no",
        )
        .map_err(|err| unseen_db(err, "attempts"))?;
    let rows = stmt
        .query_map([run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|err| unseen_db(err, "attempts"))?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|err| unseen_db(err, "attempts"))?;
        let mut attempt = json!({
            "attempt_id": row.0,
            "attempt_no": row.1,
            "command_id": row.2,
            "status": row.3,
            "published": row.4 == 1
        });
        match serde_json::from_str::<Value>(&row.5) {
            Ok(detail) => {
                attempt
                    .as_object_mut()
                    .expect("attempt object")
                    .insert("detail".into(), detail);
            }
            Err(_) => {
                *damaged = true;
                alerts.push(json!({
                    "code": "stored_record_unreadable",
                    "during": "detail_json",
                    "attempt_id": row.0,
                }));
            }
        }
        out.push(attempt);
    }
    Ok(Value::Array(out))
}

fn ensure_schema(conn: &Connection, hooks: &mut HookRun) -> Result<(), Fail> {
    let current = store::meta(conn, "body_schema").map_err(|err| db_fail(err, "body_schema"))?;
    match current.as_deref() {
        Some(store::BODY_SCHEMA) => {
            if hooks.hooks.inject_point.as_deref() == Some("body_schema") {
                return Err(missed("body_schema"));
            }
            for name in store::BODY_TABLES {
                if !store::table_exists(conn, name).map_err(|err| db_fail(err, "body_schema"))? {
                    return Err(Fail::new(
                        EXIT_RECOVERY,
                        "RecoveryRequired",
                        "body_schema_mismatch",
                    )
                    .extra("new_effect", json!(false)));
                }
            }
            Ok(())
        }
        Some(_) => Err(refused("body_schema_unsupported").extra("vault_unchanged", json!(true))),
        None => {
            for name in store::BODY_TABLES {
                if store::table_exists(conn, name).map_err(|err| db_fail(err, "body_schema"))? {
                    return Err(Fail::new(
                        EXIT_RECOVERY,
                        "RecoveryRequired",
                        "body_schema_mismatch",
                    )
                    .extra("new_effect", json!(false)));
                }
            }
            let tx = conn
                .unchecked_transaction()
                .map_err(|err| db_fail(err, "begin"))?;
            tx.execute_batch(store::BODY_DDL)
                .map_err(|err| db_fail(err, "body_schema"))?;
            tx.execute(
                "INSERT INTO meta(key, value) VALUES ('body_schema', ?1)",
                [store::BODY_SCHEMA],
            )
            .map_err(|err| db_fail(err, "body_schema"))?;
            hooks.inject(
                "body_schema",
                checkpoint_map("body_schema", 0, None, None, false, false),
            )?;
            tx.commit().map_err(|err| db_fail(err, "body_schema"))?;
            Ok(())
        }
    }
}

fn stat_fragment(path: &Path) -> Result<u64, Fail> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(refused("source_missing").extra("bytes_read", json!(0)));
        }
        Err(err) => return Err(io_fail(err, "fragment_metadata")),
    };
    if !meta.file_type().is_file() {
        return Err(refused("source_not_a_file").extra("bytes_read", json!(0)));
    }
    if meta.len() == 0 {
        return Err(refused("empty_fragment")
            .extra("bytes_read", json!(0))
            .extra("stat_size", json!(0)));
    }
    if meta.len() > body::MAX_FRAGMENT_BYTES {
        return Err(refused("over_input_budget")
            .extra("bytes_read", json!(0))
            .extra("stat_size", json!(meta.len()))
            .extra("limit", json!(body::MAX_FRAGMENT_BYTES)));
    }
    Ok(meta.len())
}

fn read_capped(path: &Path, stat_len: u64) -> Result<Vec<u8>, Fail> {
    let mut file = File::open(path).map_err(|err| io_fail(err, "read_fragment"))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = file
            .read(&mut tmp)
            .map_err(|err| io_fail(err, "read_fragment"))?;
        if n == 0 {
            break;
        }
        if buf.len() as u64 + n as u64 > body::MAX_FRAGMENT_BYTES {
            return Err(refused("over_input_budget")
                .extra("bytes_read", json!(body::MAX_FRAGMENT_BYTES + 1))
                .extra("stat_size", json!(stat_len))
                .extra("limit", json!(body::MAX_FRAGMENT_BYTES)));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    Ok(buf)
}

fn place_object(
    root: &Path,
    conn: &Connection,
    bytes: &[u8],
    hash: &str,
    journal: &mut Journal,
) -> Result<ObjectRow, Fail> {
    if let Some(object) =
        store::object_by_hash(conn, hash).map_err(|err| db_fail(err, "object_lookup"))?
    {
        require_hosted_object(root, &object, hash, bytes.len() as i64)?;
        journal.storage_reused = Some(true);
        journal.object_id = Some(object.object_id.clone());
        return Ok(object);
    }
    if let Some(orphan) = find_orphan_with_hash(root, conn, hash)? {
        journal.storage_reused = Some(true);
        journal.object_id = Some(orphan.clone());
        return Ok(ObjectRow {
            object_id: orphan,
            content_sha256: hash.into(),
            byte_length: bytes.len() as i64,
            relpath: String::new(),
        });
    }
    let mut object_id = journal
        .object_id
        .clone()
        .filter(|id| is_prefixed_id(id, "obj_"))
        .unwrap_or_default();
    if object_id.is_empty() || root.join("objects").join(&object_id).exists() {
        object_id = ids::random_id("obj_").map_err(|err| io_fail(err, "random"))?;
    }
    let staging = journal::staging_path(root, &journal.command_id);
    crate::fsutil::atomic_write(&staging, bytes).map_err(|err| io_fail(err, "write_staging"))?;
    let dest = root.join("objects").join(&object_id);
    fs::hard_link(&staging, &dest).map_err(|err| io_fail(err, "publish_object"))?;
    File::open(&dest)
        .and_then(|file| file.sync_all())
        .map_err(|err| io_fail(err, "object_fsync"))?;
    crate::fsutil::fsync_dir(dest.parent().unwrap_or(root))
        .map_err(|err| io_fail(err, "object_dir_fsync"))?;
    let _ = fs::remove_file(&staging);
    journal.storage_reused = Some(false);
    journal.object_id = Some(object_id.clone());
    let relpath = canonical_object_relpath(&object_id).ok_or_else(|| refused("bad_object_id"))?;
    Ok(ObjectRow {
        object_id,
        content_sha256: hash.into(),
        byte_length: bytes.len() as i64,
        relpath,
    })
}

fn ensure_source(
    tx: &rusqlite::Transaction<'_>,
    journal: &Journal,
    object: &ObjectRow,
    hash: &str,
) -> Result<SourceRow, Fail> {
    if let Some(source) =
        store::source_by_hash(tx, hash).map_err(|err| db_fail(err, "source_lookup"))?
    {
        return Ok(source);
    }
    if store::object_by_hash(tx, hash)
        .map_err(|err| db_fail(err, "object_recheck"))?
        .is_none()
    {
        let relpath =
            canonical_object_relpath(&object.object_id).ok_or_else(|| refused("bad_object_id"))?;
        store::insert_object(
            tx,
            &ObjectRow {
                object_id: object.object_id.clone(),
                content_sha256: hash.into(),
                byte_length: object.byte_length,
                relpath,
            },
        )
        .map_err(|err| db_fail(err, "insert_object"))?;
    }
    let source_id = journal
        .source_id
        .clone()
        .filter(|id| is_prefixed_id(id, "src_"))
        .unwrap_or_default();
    let source_id = if source_id.is_empty() {
        ids::random_id("src_").map_err(|err| io_fail(err, "random"))?
    } else {
        source_id
    };
    let source = SourceRow {
        source_id,
        object_id: object.object_id.clone(),
        content_sha256: hash.into(),
        byte_length: object.byte_length,
        media_type: body::MEDIA_TYPE.into(),
        user_marker: None,
    };
    store::insert_source(tx, &source).map_err(|err| db_fail(err, "insert_source"))?;
    Ok(source)
}

fn ensure_version(
    tx: &rusqlite::Transaction<'_>,
    source: &SourceRow,
    hash: &str,
) -> Result<String, Fail> {
    if let Some(id) = version_for(tx, &source.source_id)? {
        return Ok(id);
    }
    let id = ids::random_id("sv_").map_err(|err| io_fail(err, "random"))?;
    tx.execute(
        "INSERT INTO source_versions(source_version_id, source_id, object_id, content_sha256, byte_length, media_type, role, eml) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
        params![id, source.source_id, source.object_id, hash, source.byte_length, body::MEDIA_TYPE, body::ROLE],
    )
    .map_err(|err| db_fail(err, "insert_source_version"))?;
    Ok(id)
}

fn version_for(conn: &Connection, source_id: &str) -> Result<Option<String>, Fail> {
    conn.query_row(
        "SELECT source_version_id FROM source_versions WHERE source_id = ?1",
        [source_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| db_fail(err, "source_version"))
}

fn insert_occurrence_if_absent(
    tx: &rusqlite::Transaction<'_>,
    req: &FragmentRequest,
    journal: &Journal,
    source: &SourceRow,
) -> Result<(), Fail> {
    if occurrence_exists(tx, &req.command_id)? {
        return Ok(());
    }
    let occurrence_id = journal
        .occurrence_id
        .clone()
        .ok_or_else(|| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "missing_occurrence_id"))?;
    store::insert_occurrence(
        tx,
        &OccurrenceRow {
            occurrence_id,
            source_id: source.source_id.clone(),
            command_id: req.command_id.clone(),
            batch_id: journal.batch_id.clone().unwrap_or_default(),
            display_name: journal
                .display_name
                .clone()
                .unwrap_or_else(|| "fragment".into()),
            external_path: journal.external_path.clone(),
            storage_reused: journal.storage_reused.unwrap_or(false),
            recorded_at: now_ms(),
        },
    )
    .map_err(|err| db_fail(err, "insert_occurrence"))?;
    Ok(())
}

fn occurrence_exists(conn: &Connection, command_id: &str) -> Result<bool, Fail> {
    conn.query_row(
        "SELECT 1 FROM occurrences WHERE command_id = ?1",
        [command_id],
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
    .map_err(|err| db_fail(err, "occurrence_lookup"))
}

struct RunRow {
    run_id: String,
    status: String,
}

fn run_by_key(conn: &Connection, key: &str) -> Result<Option<RunRow>, Fail> {
    conn.query_row(
        "SELECT run_id, status FROM extraction_runs WHERE logical_key = ?1",
        [key],
        |row| {
            Ok(RunRow {
                run_id: row.get(0)?,
                status: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|err| db_fail(err, "run_lookup"))
}

fn next_attempt_no(conn: &Connection, run_id: &str) -> Result<i64, Fail> {
    let max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(attempt_no), 0) FROM extraction_attempts WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .map_err(|err| db_fail(err, "attempt_no"))?;
    Ok(max + 1)
}

fn insert_attempt(
    conn: &Connection,
    attempt_id: &str,
    run_id: &str,
    attempt_no: i64,
    command_id: &str,
    status: &str,
) -> Result<(), Fail> {
    conn.execute(
        "INSERT INTO extraction_attempts(attempt_id, run_id, attempt_no, command_id, status, published, detail_json) VALUES (?1, ?2, ?3, ?4, ?5, 0, '{}')",
        params![attempt_id, run_id, attempt_no, command_id, status],
    )
    .map_err(|err| db_fail(err, "insert_attempt"))?;
    Ok(())
}

fn attempt_status(conn: &Connection, attempt_id: &str) -> Result<String, Fail> {
    conn.query_row(
        "SELECT status FROM extraction_attempts WHERE attempt_id = ?1",
        [attempt_id],
        |row| row.get(0),
    )
    .map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "attempt_missing")
            .extra("new_effect", json!(false))
    })
}

fn candidate_ids(conn: &Connection, run_id: &str) -> Result<Vec<String>, Fail> {
    let mut stmt = conn
        .prepare("SELECT candidate_id FROM candidates WHERE run_id = ?1 ORDER BY ordinal")
        .map_err(|err| db_fail(err, "candidates"))?;
    let rows = stmt
        .query_map([run_id], |row| row.get(0))
        .map_err(|err| db_fail(err, "candidates"))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|err| db_fail(err, "candidates"))?);
    }
    Ok(ids)
}

fn set_attempt(
    conn: &Connection,
    attempt_id: &str,
    status: &str,
    published: i64,
    reason: &str,
) -> Result<(), Fail> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    tx.execute(
        "UPDATE extraction_attempts SET status = ?2, published = ?3, detail_json = ?4 WHERE attempt_id = ?1",
        params![attempt_id, status, published, json!({"reason": reason}).to_string()],
    )
    .map_err(|err| db_fail(err, "update_attempt"))?;
    tx.commit().map_err(|err| db_fail(err, "update_attempt"))?;
    Ok(())
}

fn set_run_status(conn: &Connection, run_id: &str, status: &str) -> Result<(), Fail> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    tx.execute(
        "UPDATE extraction_runs SET status = ?2 WHERE run_id = ?1 AND status != 'succeeded'",
        params![run_id, status],
    )
    .map_err(|err| db_fail(err, "update_run"))?;
    tx.commit().map_err(|err| db_fail(err, "update_run"))?;
    Ok(())
}

fn running_attempt(conn: &Connection, command_id: &str) -> Result<Option<String>, Fail> {
    conn.query_row(
        "SELECT attempt_id FROM extraction_attempts WHERE command_id = ?1 AND status = 'running' ORDER BY attempt_no DESC LIMIT 1",
        [command_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| db_fail(err, "attempt_lookup"))
}

fn source_is_preserved(conn: &Connection, source_id: Option<&str>) -> Result<bool, Fail> {
    let Some(source_id) = source_id else {
        return Ok(false);
    };
    let found =
        store::source_by_id(conn, source_id).map_err(|err| unseen_db(err, "source_lookup"))?;
    Ok(found.is_some())
}

fn load_journal(root: &Path, req: &FragmentRequest) -> Result<Journal, Fail> {
    let path = journal::path(root, &req.command_id);
    if path.exists() {
        let journal = journal::read_file(&path).map_err(|_| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
                .extra("command_id", json!(req.command_id))
        })?;
        if journal.kind != "fragment" {
            return Err(refused("command_kind_conflict"));
        }
        return Ok(journal);
    }
    let display_default = req
        .fragment
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("fragment")
        .to_string();
    if !is_display_name(&display_default) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_display_name"));
    }
    let journal = Journal {
        command_id: req.command_id.clone(),
        kind: "fragment".into(),
        generation: 1,
        state: "started".into(),
        batch_id: Some(match &req.batch_id {
            Some(id) => id.clone(),
            None => ids::random_id("b_").map_err(|err| io_fail(err, "random"))?,
        }),
        display_name: Some(display_default),
        external_path: Some(req.fragment.display().to_string()),
        media_type: Some(body::MEDIA_TYPE.into()),
        occurrence_id: Some(ids::random_id("occ_").map_err(|err| io_fail(err, "random"))?),
        object_id: None,
        content_sha256: None,
        byte_length: None,
        payload_fingerprint: None,
        storage_reused: None,
        reason: None,
        marker_value: None,
        source_id: None,
    };
    write_journal(root, &journal)?;
    Ok(journal)
}

fn replay_command(
    session: &super::Session,
    req: &FragmentRequest,
    existing: &CommandRow,
) -> Result<Value, Fail> {
    let receipt: Value = serde_json::from_str(&existing.result_json).map_err(|_| {
        Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "command_result_unreadable",
        )
        .extra("new_effect", json!(false))
    })?;
    let hash = receipt
        .get("content_sha256")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source_id = receipt
        .get("source_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some(source) = store::source_by_id(&session.conn, source_id)
        .map_err(|err| unseen_db(err, "source_lookup"))?
    {
        confirm_source_bytes(session, &source, &req.command_id)?;
        if source.content_sha256 != hash {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "hash_mismatch")
                    .extra("new_effect", json!(false))
                    .extra("preservation", json!(PRESERVED)),
            );
        }
    }
    match classify_external(&req.fragment) {
        External::Missing => {}
        External::Unreadable(message) => {
            return Err(
                Fail::new(EXIT_INTEGRITY, "ExternalUnverified", "source_unreadable")
                    .extra("os_error", json!(message))
                    .extra("new_effect", json!(false))
                    .extra("preservation", json!(PRESERVED))
                    .extra("object_intact", json!(true))
                    .extra("payload_verified", json!(false)),
            );
        }
        External::Hash(found) if found != hash => {
            return Err(refused("payload_conflict")
                .extra("preservation", json!(PRESERVED))
                .extra("object_intact", json!(true))
                .extra("payload_matches", json!(false))
                .extra("new_effect", json!(false))
                .extra("content_sha256", json!(hash)));
        }
        External::Hash(_) => {}
    }
    let mut body = already_committed(existing)?;
    if let Some(map) = body.as_object_mut() {
        map.insert("candidates_published".into(), json!(true));
    }
    Ok(body)
}

enum External {
    Missing,
    Unreadable(String),
    Hash(String),
}

fn classify_external(path: &Path) -> External {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => External::Missing,
        Err(err) => External::Unreadable(err.to_string()),
        Ok(meta) if !meta.file_type().is_file() => External::Unreadable("not a file".into()),
        Ok(_) => match fs::read(path) {
            Ok(bytes) => External::Hash(sha256_bytes(&bytes)),
            Err(err) => External::Unreadable(err.to_string()),
        },
    }
}

fn finish_cancel(
    session: &super::Session,
    journal: &mut Journal,
    opened: Option<&Opened>,
) -> Result<Value, Fail> {
    journal.state = "aborted".into();
    journal.reason = Some("cancelled".into());
    write_journal(&session.root, journal)?;
    let preserved =
        opened.is_some() || source_is_preserved(&session.conn, journal.source_id.as_deref())?;
    let mut body = cancel_body(
        "Late",
        &journal.command_id,
        journal.generation,
        true,
        preserved,
    );
    body.insert("late".into(), json!(true));
    body.insert("candidates_published".into(), json!(false));
    if let Some(opened) = opened {
        body.insert("source_id".into(), json!(opened.source_id));
        body.insert("attempt_id".into(), json!(opened.attempt_id));
        body.insert("run_id".into(), json!(opened.run_id));
    }
    Ok(Value::Object(body))
}

fn abort_journal(root: &Path, journal: &mut Journal, reason: &str) -> Result<(), Fail> {
    journal.state = "aborted".into();
    journal.reason = Some(reason.into());
    write_journal(root, journal)
}

fn finish_journal(root: &Path, journal: &mut Journal, receipt: Value) -> Result<Value, Fail> {
    finish_after_commit(receipt, || {
        journal.state = "committed".into();
        journal.reason = None;
        write_journal(root, journal)
    })
}

fn cancel_while_busy(req: &CancelRequest) -> Result<Report, Fail> {
    if let Some(row) = read_committed(&req.vault, &req.command_id)? {
        let mut body = already_committed(&row)?;
        if let Some(map) = body.as_object_mut() {
            map.insert("cancel_applied".into(), json!(false));
        }
        return Ok(Report {
            code: EXIT_OK,
            body,
        });
    }
    let path = journal::path(&req.vault, &req.command_id);
    if !path.exists() {
        return Err(refused("unknown_command").extra("cancel_applied", json!(false)));
    }
    let journal = journal::read_file(&path).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("cancel_applied", json!(false))
    })?;
    if journal.generation != req.generation || journal.kind != "fragment" {
        return Err(refused("generation_mismatch").extra("cancel_applied", json!(false)));
    }
    write_control(&req.vault, &req.command_id, req.generation, "fragment")?;
    let mut body = body_outcome("CancelPending");
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("generation".into(), json!(req.generation));
    body.insert("cancel_applied".into(), json!(false));
    body.insert("candidates_published".into(), json!(false));
    body.insert("new_effect".into(), json!(false));
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

fn cancel_body(
    outcome: &str,
    command_id: &str,
    generation: i64,
    applied: bool,
    preserved: bool,
) -> Map<String, Value> {
    let mut body = body_outcome(outcome);
    body.insert("command_id".into(), json!(command_id));
    body.insert("generation".into(), json!(generation));
    body.insert("cancel_applied".into(), json!(applied));
    body.insert("candidates_published".into(), json!(false));
    body.insert("new_effect".into(), json!(false));
    body.insert(
        "preservation".into(),
        if preserved {
            json!(PRESERVED)
        } else {
            json!(NOT_PRESERVED)
        },
    );
    body
}

fn body_outcome(outcome: &str) -> Map<String, Value> {
    let mut body = honesty(outcome);
    body.insert("slice".into(), json!("T2a-body"));
    body.insert("eml".into(), Value::Null);
    body.insert("pdf_extraction".into(), json!("not_available"));
    body.insert("ocr".into(), json!("not_available"));
    body.insert("adopted".into(), json!(false));
    body
}

fn locator_json(version_id: &str, hash: &str, draft: &Draft) -> Value {
    json!({
        "source_version_id": version_id,
        "content_sha256": hash,
        "normalization": body::NORMALIZATION,
        "join": body::JOIN,
        "rules_version": body::RULES_VERSION,
        "offset_unit": body::OFFSET_UNIT,
        "unicode_scalar_start": draft.scalar_start,
        "unicode_scalar_end": draft.scalar_end,
        "byte_start": draft.byte_start,
        "byte_end": draft.byte_end,
        "excerpt": draft.excerpt,
        "eml": null,
        "pdf_page": null,
        "coordinates": null
    })
}

fn not_published(mut fail: Fail, source_id: &str, hash: &str) -> Fail {
    fail.code = EXIT_INTEGRITY;
    fail.outcome = "NotPublished";
    fail.extra.insert("preservation".into(), json!(PRESERVED));
    fail.extra.insert("object_intact".into(), json!(true));
    fail.extra
        .insert("candidates_published".into(), json!(false));
    fail.extra.insert("new_effect".into(), json!(false));
    fail.extra.insert("source_id".into(), json!(source_id));
    fail.extra.insert("content_sha256".into(), json!(hash));
    fail
}

fn refused(reason: &str) -> Fail {
    Fail::new(EXIT_REJECTED, "Rejected", reason)
        .extra("new_effect", json!(false))
        .extra("candidates_published", json!(false))
        .extra("eml", Value::Null)
        .extra("external_unchanged", json!(true))
        .extra("role", json!(body::ROLE))
}

fn missed(reason: &str) -> Fail {
    Fail::new(EXIT_FAULT_MISSED, "FaultNotReached", reason)
        .extra("injected", json!(true))
        .extra("new_effect", json!(false))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}
