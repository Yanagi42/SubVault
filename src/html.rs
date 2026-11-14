use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::params;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::body::{self, StatedLine};
use crate::ids::{self, canonical_object_relpath, hex_encode, is_command_id, is_display_name};
use crate::journal::{self, Journal};
use crate::model::{
    db_fail, honesty, io_fail, unseen_db, Fail, Report, EXIT_NOT_PRESERVED, EXIT_OK, EXIT_RECOVERY,
    EXIT_REJECTED, EXIT_USAGE, EXIT_VERSION, NOT_PRESERVED, PRESERVED,
};
use crate::store::{self, CommandRow, ObjectRow, OccurrenceRow, SourceRow};

pub const EXTRACTOR: &str = "subvault-html";
pub const EXTRACTOR_VERSION: &str = "1";
pub const NORMALIZATION: &str = "html-static-text-1";
pub const JOIN: &str = "html-block-1";
pub const RULES_VERSION: &str = "stated-html-blocks-1";
pub const WHITESPACE: &str = "html-collapse-1";
pub const MEDIA_TYPE: &str = "text/html;charset=utf-8";
pub const ROLE: &str = "user_provided_html";
pub const OFFSET_UNIT: &str = "unicode_scalar_in_original_html_utf8";
pub const MAX_HTML_BYTES: u64 = 65_536;
const PARSER: &str = "tl";
const PARSER_VERSION: &str = "0.7.8";
const ENTITY_DECODER: &str = "html-escape";
const ENTITY_DECODER_VERSION: &str = "0.2.13";

pub struct HtmlRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub source: PathBuf,
    pub batch_id: Option<String>,
    pub hooks: super::Hooks,
}

pub fn submit_html(req: HtmlRequest) -> Report {
    match submit_inner(req) {
        Ok(body) => as_html(Report {
            code: EXIT_OK,
            body,
        }),
        Err(fail) => as_html(super::note_uncommitted(fail).into_report()),
    }
}

fn as_html(mut report: Report) -> Report {
    if let Some(map) = report.body.as_object_mut() {
        map.insert("slice".into(), json!("T4-html"));
    }
    report
}

fn submit_inner(req: HtmlRequest) -> Result<Value, Fail> {
    if req.hooks.stop_at.is_some() || req.hooks.inject_point.is_some() {
        return Err(
            Fail::new(EXIT_USAGE, "Usage", "html_has_no_hook").extra("new_effect", json!(false))
        );
    }
    if !is_command_id(&req.command_id) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_command_id"));
    }
    if let Some(batch) = &req.batch_id {
        if !is_command_id(batch) {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_batch_id"));
        }
    }
    match super::accept_wal_format(&req.vault.join("vault.db"))? {
        4 => {}
        version => {
            return Err(
                Fail::new(EXIT_VERSION, "VersionUnsupported", "html_requires_format_4")
                    .extra("schema_version_found", json!(version))
                    .extra("vault_format", json!(version))
                    .extra("new_effect", json!(false))
                    .extra("created_new_vault", json!(false)),
            );
        }
    }
    let session = super::open_session(&req.vault)?;
    if let Some(existing) = store::command(&session.conn, &req.command_id)
        .map_err(|err| unseen_db(err, "command_lookup"))?
    {
        return replay(&session, &req, &existing);
    }
    let bytes = read_html(&req.source)?;
    let text = std::str::from_utf8(&bytes).expect("read_html accepted utf-8");
    if let Some(source) = store::source_by_hash(&session.conn, &sha256_bytes(&bytes))
        .map_err(|err| db_fail(err, "source_lookup"))?
    {
        confirm_object(&session.root, &source)?;
        let mut body = honesty("AlreadyPreserved");
        body.insert("new_effect".into(), json!(false));
        body.insert("preservation".into(), json!(PRESERVED));
        body.insert("object_intact".into(), json!(true));
        body.insert("source_id".into(), json!(source.source_id));
        body.insert("content_sha256".into(), json!(source.content_sha256));
        body.insert("media_type".into(), json!(source.media_type));
        return Ok(Value::Object(body));
    }
    let sources = store::list_sources(&session.conn).map_err(|err| db_fail(err, "list_sources"))?;
    if !sources.is_empty() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "one_html_document")
            .extra("new_effect", json!(false))
            .extra("preservation", json!(PRESERVED)));
    }
    let mut journal = start_journal(&session.root, &req)?;
    let hash = sha256_bytes(&bytes);
    let key = logical_key(&hash);
    journal.content_sha256 = Some(hash.clone());
    journal.byte_length = Some(bytes.len() as i64);
    journal.payload_fingerprint = Some(key.clone());
    journal.media_type = Some(MEDIA_TYPE.into());
    journal::write(&session.root, &journal)?;
    let object = match place_object(&session.root, &session.conn, &bytes, &hash, &mut journal) {
        Ok(object) => object,
        Err(fail) => {
            abort_journal(&session.root, &mut journal, "object_not_placed")?;
            return Err(fail.extra("preservation", json!(NOT_PRESERVED)));
        }
    };
    journal.state = "object_ready".into();
    journal::write(&session.root, &journal)?;
    let prepared = extract(text);
    let committed = match commit(
        &session,
        &req,
        &mut journal,
        &object,
        &hash,
        &key,
        &prepared,
    ) {
        Ok(body) => body,
        Err(fail) => {
            let _ = abort_journal(&session.root, &mut journal, "commit_failed");
            return Err(fail.extra("preservation", json!(NOT_PRESERVED)));
        }
    };
    journal.state = "committed".into();
    journal.reason = None;
    match journal::write(&session.root, &journal) {
        Ok(()) => Ok(committed),
        Err(fail) => {
            let mut body = committed;
            if let Some(map) = body.as_object_mut() {
                map.insert("post_commit_io".into(), json!(true));
                map.insert("journal_label_update".into(), json!("failed"));
                map.insert("new_effect".into(), json!(true));
                map.insert("preservation".into(), json!(PRESERVED));
                if let Some(during) = fail.extra.get("during") {
                    map.insert("during".into(), during.clone());
                }
            }
            Ok(body)
        }
    }
}

fn read_html(path: &Path) -> Result<Vec<u8>, Fail> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) => {
            return Err(io_fail(err, "html_stat").extra("preservation", json!(NOT_PRESERVED)))
        }
    };
    if !meta.is_file() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "source_not_a_file")
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    if meta.len() == 0 {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "empty_html")
            .extra("bytes_read", json!(0))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    if meta.len() > MAX_HTML_BYTES {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "over_input_budget")
            .extra("bytes_read", json!(0))
            .extra("stat_size", json!(meta.len()))
            .extra("limit", json!(MAX_HTML_BYTES))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    let mut file = fs::File::open(path)
        .map_err(|err| io_fail(err, "html_read").extra("preservation", json!(NOT_PRESERVED)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| io_fail(err, "html_read").extra("preservation", json!(NOT_PRESERVED)))?;
    if std::str::from_utf8(&bytes).is_err() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "invalid_utf8")
            .extra("bytes_read", json!(bytes.len()))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    Ok(bytes)
}

fn start_journal(root: &Path, req: &HtmlRequest) -> Result<Journal, Fail> {
    let display = req
        .source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source")
        .to_string();
    if !is_display_name(&display) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_display_name"));
    }
    let journal = Journal {
        command_id: req.command_id.clone(),
        kind: "html".into(),
        generation: 1,
        state: "started".into(),
        batch_id: Some(match &req.batch_id {
            Some(id) => id.clone(),
            None => ids::random_id("b_").map_err(|err| io_fail(err, "random"))?,
        }),
        display_name: Some(display),
        external_path: Some(req.source.display().to_string()),
        media_type: Some(MEDIA_TYPE.into()),
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
    journal::write(root, &journal)?;
    Ok(journal)
}

fn abort_journal(root: &Path, journal: &mut Journal, reason: &str) -> Result<(), Fail> {
    journal.state = "aborted".into();
    journal.reason = Some(reason.into());
    journal::write(root, journal)
}

fn place_object(
    root: &Path,
    conn: &rusqlite::Connection,
    bytes: &[u8],
    hash: &str,
    journal: &mut Journal,
) -> Result<ObjectRow, Fail> {
    if let Some(object) =
        store::object_by_hash(conn, hash).map_err(|err| db_fail(err, "object_lookup"))?
    {
        confirm_hosted(root, &object, hash, bytes.len() as i64)?;
        journal.storage_reused = Some(true);
        journal.object_id = Some(object.object_id.clone());
        return Ok(object);
    }
    let object_id = ids::random_id("obj_").map_err(|err| io_fail(err, "random"))?;
    let staging = journal::staging_path(root, &journal.command_id);
    crate::fsutil::atomic_write(&staging, bytes).map_err(|err| io_fail(err, "write_staging"))?;
    let dest = root.join("objects").join(&object_id);
    if let Err(err) = fs::hard_link(&staging, &dest) {
        return Err(io_fail(err, "publish_object"));
    }
    File::open(&dest)
        .and_then(|file| file.sync_all())
        .map_err(|err| io_fail(err, "object_fsync"))?;
    crate::fsutil::fsync_dir(dest.parent().unwrap_or(root))
        .map_err(|err| io_fail(err, "object_dir_fsync"))?;
    let _ = fs::remove_file(&staging);
    journal.storage_reused = Some(false);
    journal.object_id = Some(object_id.clone());
    let relpath = canonical_object_relpath(&object_id).ok_or_else(|| {
        Fail::new(EXIT_REJECTED, "Rejected", "bad_object_id").extra("new_effect", json!(false))
    })?;
    Ok(ObjectRow {
        object_id,
        content_sha256: hash.into(),
        byte_length: bytes.len() as i64,
        relpath,
    })
}

fn confirm_object(root: &Path, source: &SourceRow) -> Result<(), Fail> {
    let object = ObjectRow {
        object_id: source.object_id.clone(),
        content_sha256: source.content_sha256.clone(),
        byte_length: source.byte_length,
        relpath: canonical_object_relpath(&source.object_id).unwrap_or_default(),
    };
    confirm_hosted(root, &object, &source.content_sha256, source.byte_length)
}

fn confirm_hosted(root: &Path, object: &ObjectRow, hash: &str, len: i64) -> Result<(), Fail> {
    let path = root.join("objects").join(&object.object_id);
    let bytes = fs::read(&path).map_err(|err| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_unreadable")
            .extra("during", json!("hash_object"))
            .extra("os_error", json!(err.to_string()))
            .extra("new_effect", json!(false))
    })?;
    if sha256_bytes(&bytes) != hash || bytes.len() as i64 != len {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_mismatch")
                .extra("new_effect", json!(false)),
        );
    }
    Ok(())
}

fn replay(
    session: &super::Session,
    req: &HtmlRequest,
    existing: &CommandRow,
) -> Result<Value, Fail> {
    if existing.kind != "html" {
        return Err(
            Fail::new(EXIT_REJECTED, "Rejected", "command_kind_conflict")
                .extra("new_effect", json!(false)),
        );
    }
    let mut body: Value = serde_json::from_str(&existing.result_json).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "result_unreadable")
            .extra("new_effect", json!(false))
    })?;
    let hash = body
        .get("content_sha256")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let object_id = body
        .get("object_id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if !hash.is_empty() {
        let object = ObjectRow {
            object_id: object_id.clone(),
            content_sha256: hash.clone(),
            byte_length: body
                .get("byte_length")
                .and_then(|value| value.as_i64())
                .unwrap_or(0),
            relpath: String::new(),
        };
        confirm_hosted(&session.root, &object, &hash, object.byte_length)?;
    }
    let Some(map) = body.as_object_mut() else {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "result_unreadable")
                .extra("new_effect", json!(false)),
        );
    };
    map.insert("new_effect".into(), json!(false));
    map.insert("command_id".into(), json!(req.command_id));
    match fs::read(&req.source) {
        Ok(bytes) if sha256_bytes(&bytes) == hash => {
            map.insert("payload_matches".into(), json!(true));
            map.insert("external_reread".into(), json!(true));
        }
        Ok(_) => {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "payload_conflict")
                .extra("new_effect", json!(false))
                .extra("preservation", json!(PRESERVED))
                .extra("payload_matches", json!(false))
                .extra("external_reread", json!(true))
                .extra("content_sha256", json!(hash)));
        }
        Err(_) => {
            map.insert("payload_matches".into(), Value::Null);
            map.insert("external_reread".into(), json!(false));
        }
    }
    Ok(body)
}

fn commit(
    session: &super::Session,
    req: &HtmlRequest,
    journal: &mut Journal,
    object: &ObjectRow,
    hash: &str,
    key: &str,
    prepared: &Prepared,
) -> Result<Value, Fail> {
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    if store::object_by_hash(&tx, hash)
        .map_err(|err| db_fail(err, "object_recheck"))?
        .is_none()
    {
        store::insert_object(&tx, object).map_err(|err| db_fail(err, "insert_object"))?;
    }
    let source_id = ids::random_id("src_").map_err(|err| io_fail(err, "random"))?;
    store::insert_source(
        &tx,
        &SourceRow {
            source_id: source_id.clone(),
            object_id: object.object_id.clone(),
            content_sha256: hash.into(),
            byte_length: object.byte_length,
            media_type: MEDIA_TYPE.into(),
            user_marker: None,
        },
    )
    .map_err(|err| db_fail(err, "insert_source"))?;
    let occurrence_id = journal
        .occurrence_id
        .clone()
        .ok_or_else(|| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "occurrence_missing"))?;
    store::insert_occurrence(
        &tx,
        &OccurrenceRow {
            occurrence_id,
            source_id: source_id.clone(),
            command_id: req.command_id.clone(),
            batch_id: journal.batch_id.clone().unwrap_or_default(),
            display_name: journal
                .display_name
                .clone()
                .unwrap_or_else(|| "source".into()),
            external_path: journal.external_path.clone(),
            storage_reused: journal.storage_reused.unwrap_or(false),
            recorded_at: super::now_ms(),
        },
    )
    .map_err(|err| db_fail(err, "insert_occurrence"))?;
    let source_version_id = ids::random_id("sv_").map_err(|err| io_fail(err, "random"))?;
    tx.execute(
        "INSERT INTO source_versions(source_version_id, source_id, object_id, content_sha256, byte_length, media_type, role, eml) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
        params![
            source_version_id,
            source_id,
            object.object_id,
            hash,
            object.byte_length,
            MEDIA_TYPE,
            ROLE
        ],
    )
    .map_err(|err| db_fail(err, "insert_version"))?;
    let run_id = ids::random_id("run_").map_err(|err| io_fail(err, "random"))?;
    let attempt_id = ids::random_id("att_").map_err(|err| io_fail(err, "random"))?;
    let run_status = if prepared.status == "unparsed" {
        "failed"
    } else {
        "succeeded"
    };
    tx.execute(
        "INSERT INTO extraction_runs(run_id, logical_key, source_version_id, source_id, extractor, extractor_version, normalization, join_version, rules_version, config_json, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            run_id,
            key,
            source_version_id,
            source_id,
            EXTRACTOR,
            EXTRACTOR_VERSION,
            NORMALIZATION,
            JOIN,
            RULES_VERSION,
            config_json().to_string(),
            run_status
        ],
    )
    .map_err(|err| db_fail(err, "insert_run"))?;
    let published = prepared.status != "unparsed";
    tx.execute(
        "INSERT INTO extraction_attempts(attempt_id, run_id, attempt_no, command_id, status, published, detail_json) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)",
        params![
            attempt_id,
            run_id,
            req.command_id,
            run_status,
            if published { 1 } else { 0 },
            json!({"extraction": prepared.status, "reason": prepared.reason}).to_string()
        ],
    )
    .map_err(|err| db_fail(err, "insert_attempt"))?;
    let mut candidate_ids = Vec::new();
    for (ordinal, hit) in prepared.hits.iter().enumerate() {
        let candidate_id = ids::random_id("cand_").map_err(|err| io_fail(err, "random"))?;
        let value = hit_value(hit);
        let locator = hit_locator(&source_version_id, hash, hit);
        tx.execute(
            "INSERT INTO candidates(candidate_id, run_id, attempt_id, source_version_id, ordinal, kind, usable, excerpt, value_json, locator_json, adopted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
            params![
                candidate_id,
                run_id,
                attempt_id,
                source_version_id,
                ordinal as i64,
                hit.kind,
                if hit.usable { 1 } else { 0 },
                hit.excerpt,
                Value::Object(value).to_string(),
                locator.to_string()
            ],
        )
        .map_err(|err| db_fail(err, "insert_candidate"))?;
        candidate_ids.push(candidate_id);
    }
    let mut owned_values = Vec::new();
    let mut draft_index = Vec::new();
    for (ordinal, hit) in prepared.hits.iter().enumerate() {
        if !hit.usable {
            continue;
        }
        owned_values.push(hit_value(hit));
        draft_index.push(ordinal);
    }
    let drafts: Vec<super::decide::StatedDraft<'_>> = draft_index
        .iter()
        .enumerate()
        .map(|(pos, ordinal)| super::decide::StatedDraft {
            candidate_id: &candidate_ids[*ordinal],
            kind: prepared.hits[*ordinal].kind,
            value: &owned_values[pos],
        })
        .collect();
    let adopted = super::decide::adopt_statements(
        &tx,
        &req.command_id,
        &source_id,
        &source_version_id,
        &run_id,
        RULES_VERSION,
        &drafts,
    )?;
    let mut body = receipt(
        req,
        journal,
        &source_id,
        &source_version_id,
        &object.object_id,
        hash,
        key,
        &run_id,
        prepared,
        &candidate_ids,
        &adopted,
    );
    let encoded = serde_json::to_string(&body)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    store::insert_command(
        &tx,
        &CommandRow {
            command_id: req.command_id.clone(),
            kind: "html".into(),
            payload_fingerprint: key.into(),
            result_json: encoded.clone(),
            outcome: "Committed".into(),
        },
        super::now_ms(),
    )
    .map_err(|err| db_fail(err, "insert_command"))?;
    let canonical = html_canonical(hash, key, prepared.status);
    super::decide::insert_receipt(&tx, &req.command_id, &canonical, &encoded)?;
    tx.commit().map_err(|err| db_fail(err, "commit"))?;
    body.as_object_mut()
        .map(|map| map.insert("db_committed".into(), json!(true)));
    Ok(body)
}

fn receipt(
    req: &HtmlRequest,
    journal: &Journal,
    source_id: &str,
    source_version_id: &str,
    object_id: &str,
    hash: &str,
    key: &str,
    run_id: &str,
    prepared: &Prepared,
    candidate_ids: &[String],
    adopted: &[super::decide::Adopted],
) -> Value {
    let mut body = honesty("Committed");
    body.insert("new_effect".into(), json!(true));
    body.insert("preservation".into(), json!(PRESERVED));
    body.insert("arrangement".into(), json!("not_arranged"));
    body.insert("searchability".into(), json!("not_searchable"));
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("batch_id".into(), json!(journal.batch_id));
    body.insert("source_id".into(), json!(source_id));
    body.insert("source_version_id".into(), json!(source_version_id));
    body.insert("object_id".into(), json!(object_id));
    body.insert("content_sha256".into(), json!(hash));
    body.insert("hash_alg".into(), json!("sha256"));
    body.insert("byte_length".into(), json!(journal.byte_length));
    body.insert("media_type".into(), json!(MEDIA_TYPE));
    body.insert("role".into(), json!(ROLE));
    body.insert("logical_key".into(), json!(key));
    body.insert("run_id".into(), json!(run_id));
    body.insert("extraction".into(), json!(prepared.status));
    body.insert(
        "extraction_reason".into(),
        match &prepared.reason {
            Some(reason) => json!(reason),
            None => Value::Null,
        },
    );
    body.insert(
        "candidates_published".into(),
        json!(prepared.status != "unparsed"),
    );
    body.insert("mailbox".into(), json!(false));
    body.insert("clipboard".into(), json!(false));
    body.insert("network".into(), json!(false));
    body.insert("rendered".into(), json!(false));
    body.insert("external_references".into(), json!("not_loaded"));
    body.insert("payment_verified".into(), json!(false));
    body.insert("pdf_extraction".into(), json!("not_available"));
    body.insert("ocr".into(), json!("not_available"));
    body.insert("candidate_ids".into(), json!(candidate_ids));
    body.insert(
        "facts".into(),
        json!(adopted
            .iter()
            .map(|row| json!({
                "fact_id": row.fact_id,
                "decision_id": row.decision_id,
                "candidate_id": row.candidate_id,
                "fact_type": row.fact_type,
                "revision": 1
            }))
            .collect::<Vec<_>>()),
    );
    Value::Object(body)
}

fn hit_value(hit: &Hit) -> Map<String, Value> {
    let mut value = Map::new();
    value.insert("kind".into(), json!(hit.kind));
    value.insert("display_excerpt".into(), json!(hit.display));
    value.insert("usable".into(), json!(hit.usable));
    value.insert(
        "reason".into(),
        match hit.reason {
            Some(reason) => json!(reason),
            None => Value::Null,
        },
    );
    value.insert("element".into(), json!(hit.element));
    if let Some(amount) = &hit.amount_text {
        value.insert("amount_text".into(), json!(amount));
        value.insert("currency".into(), json!("USD"));
    }
    if let Some(start) = &hit.period_start {
        value.insert("period_start".into(), json!(start));
        value.insert("period_end".into(), json!(hit.period_end));
    }
    value
}

fn hit_locator(version_id: &str, hash: &str, hit: &Hit) -> Value {
    json!({
        "source_version_id": version_id,
        "content_sha256": hash,
        "normalization": NORMALIZATION,
        "join": JOIN,
        "rules_version": RULES_VERSION,
        "whitespace": WHITESPACE,
        "offset_unit": OFFSET_UNIT,
        "element": hit.element,
        "unicode_scalar_start": hit.scalar_start,
        "unicode_scalar_end": hit.scalar_end,
        "byte_start": hit.byte_start,
        "byte_end": hit.byte_end,
        "excerpt": hit.excerpt,
        "display_excerpt": hit.display,
        "eml": null,
        "pdf_page": null,
        "coordinates": null
    })
}

fn html_canonical(hash: &str, key: &str, status: &str) -> String {
    format!(
        "{}\nhtml\ncontent_sha256={hash}\nmedia_type={MEDIA_TYPE}\nrules_version={RULES_VERSION}\nlogical_key={key}\nextraction={status}\n",
        super::decide::PAYLOAD_VERSION
    )
}

fn config_json() -> Value {
    json!({
        "extractor": EXTRACTOR,
        "extractor_version": EXTRACTOR_VERSION,
        "normalization": NORMALIZATION,
        "join": JOIN,
        "rules_version": RULES_VERSION,
        "whitespace": WHITESPACE,
        "offset_unit": OFFSET_UNIT,
        "media_type": MEDIA_TYPE,
        "role": ROLE,
        "max_html_bytes": MAX_HTML_BYTES,
        "blocks": "p,div,li,h1,h2,h3,h4,h5,h6,blockquote",
        "inlines": "b,strong,i,em,span,br",
        "containers": "html,body,ul,ol",
        "excluded": "head,script,comment,hidden",
        "parser": PARSER,
        "parser_version": PARSER_VERSION,
        "entity_decoder": ENTITY_DECODER,
        "entity_decoder_version": ENTITY_DECODER_VERSION
    })
}

fn logical_key(content_sha256: &str) -> String {
    super::fingerprint(&[
        EXTRACTOR,
        EXTRACTOR_VERSION,
        NORMALIZATION,
        JOIN,
        RULES_VERSION,
        WHITESPACE,
        OFFSET_UNIT,
        MEDIA_TYPE,
        ROLE,
        "p,div,li,h1,h2,h3,h4,h5,h6,blockquote",
        "b,strong,i,em,span,br",
        &MAX_HTML_BYTES.to_string(),
        content_sha256,
    ])
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

struct Hit {
    kind: &'static str,
    display: String,
    usable: bool,
    reason: Option<&'static str>,
    amount_text: Option<String>,
    period_start: Option<String>,
    period_end: Option<String>,
    byte_start: usize,
    byte_end: usize,
    scalar_start: usize,
    scalar_end: usize,
    excerpt: String,
    element: String,
}

struct Prepared {
    status: &'static str,
    reason: Option<String>,
    hits: Vec<Hit>,
}

fn prepared(status: &'static str, reason: Option<String>, hits: Vec<Hit>) -> Prepared {
    Prepared {
        status,
        reason,
        hits,
    }
}

fn extract(source: &str) -> Prepared {
    let dom = match tl::parse(source, tl::ParserOptions::default()) {
        Ok(dom) => dom,
        Err(_) => return prepared("unparsed", Some("html_unparsed".into()), Vec::new()),
    };
    let parser = dom.parser();
    if let Some(reason) = find_style(dom.children(), parser) {
        return prepared("unsupported", Some(reason.into()), Vec::new());
    }
    let mut scan = Scan::default();
    walk(dom.children(), parser, source, &mut scan);
    if scan.span_broken {
        return prepared("unparsed", Some("span_not_verifiable".into()), Vec::new());
    }
    if let Some(element) = scan.unknown {
        return prepared(
            "out_of_scope",
            Some(format!("unsupported_element:{element}")),
            Vec::new(),
        );
    }
    if scan.outside {
        return prepared(
            "out_of_scope",
            Some("text_outside_block".into()),
            Vec::new(),
        );
    }
    settle(scan.hits)
}

#[derive(Default)]
struct Scan {
    unknown: Option<String>,
    outside: bool,
    span_broken: bool,
    hits: Vec<Hit>,
}

fn find_style(handles: &[tl::NodeHandle], parser: &tl::Parser) -> Option<&'static str> {
    let mut found = None;
    fn visit(handles: &[tl::NodeHandle], parser: &tl::Parser, found: &mut Option<&'static str>) {
        for handle in handles {
            let Some(tl::Node::Tag(tag)) = handle.get(parser) else {
                continue;
            };
            note_style(tag, found);
            let children: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
            visit(&children, parser, found);
        }
    }
    visit(handles, parser, &mut found);
    found
}

fn note_style(tag: &tl::HTMLTag, found: &mut Option<&'static str>) {
    if found.is_some() {
        return;
    }
    if has_attr(tag, "style") {
        *found = Some("style_attribute");
        return;
    }
    let name = tag_name(tag);
    if name == "style" {
        *found = Some("style_element");
        return;
    }
    if name == "link" {
        if let Some(rel) = attr_value(tag, "rel") {
            if rel
                .split_whitespace()
                .any(|token| token.eq_ignore_ascii_case("stylesheet"))
            {
                *found = Some("stylesheet_link");
            }
        }
    }
}

fn walk(handles: &[tl::NodeHandle], parser: &tl::Parser, source: &str, scan: &mut Scan) {
    for handle in handles {
        let Some(node) = handle.get(parser) else {
            scan.span_broken = true;
            return;
        };
        match node {
            tl::Node::Comment(_) => {}
            tl::Node::Raw(raw) => {
                if !fold(&decode_text(raw)).is_empty() {
                    scan.outside = true;
                }
            }
            tl::Node::Tag(tag) => walk_tag(tag, parser, source, scan),
        }
    }
}

fn walk_tag(tag: &tl::HTMLTag, parser: &tl::Parser, source: &str, scan: &mut Scan) {
    let name = tag_name(tag);
    let children: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
    if matches!(name.as_str(), "head" | "script") || has_attr(tag, "hidden") {
        return;
    }
    match name.as_str() {
        "html" | "body" | "ul" | "ol" => walk(&children, parser, source, scan),
        "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" => {
            match element_span(tag, parser, source) {
                Some(span) => record_block(tag, parser, &name, span, scan),
                None => scan.span_broken = true,
            }
            for handle in &children {
                let Some(tl::Node::Tag(child)) = handle.get(parser) else {
                    continue;
                };
                let child_name = tag_name(child);
                if matches!(
                    child_name.as_str(),
                    "p" | "div"
                        | "li"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "blockquote"
                        | "html"
                        | "body"
                        | "ul"
                        | "ol"
                        | "head"
                        | "script"
                ) {
                    walk_tag(child, parser, source, scan);
                }
            }
        }
        "b" | "strong" | "i" | "em" | "span" | "br" => {
            if !fold(&collect_inline(tag, parser, scan)).is_empty() {
                scan.outside = true;
            }
        }
        _ => {
            if scan.unknown.is_none() {
                scan.unknown = Some(name);
            }
        }
    }
}

struct OwnedSpan {
    byte_start: usize,
    byte_end: usize,
    scalar_start: usize,
    scalar_end: usize,
    excerpt: String,
}

fn element_span(tag: &tl::HTMLTag, parser: &tl::Parser, source: &str) -> Option<OwnedSpan> {
    let raw = tag.raw().as_bytes();
    if raw.is_empty() {
        return None;
    }
    let (start, inclusive) = tag.boundaries(parser);
    let end = inclusive.checked_add(1)?;
    if end > source.len() || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return None;
    }
    let excerpt = &source[start..end];
    if excerpt.as_bytes() != raw {
        return None;
    }
    let scalar_start = source[..start].chars().count();
    let scalar_end = scalar_start + excerpt.chars().count();
    Some(OwnedSpan {
        byte_start: start,
        byte_end: end,
        scalar_start,
        scalar_end,
        excerpt: excerpt.to_string(),
    })
}

fn record_block(
    tag: &tl::HTMLTag,
    parser: &tl::Parser,
    name: &str,
    span: OwnedSpan,
    scan: &mut Scan,
) {
    let text = collect_inline(tag, parser, scan);
    for line in fold(&text) {
        let (kind, amount, start, end) = match body::stated_line(&line) {
            StatedLine::Amount(amount) => ("stated_amount_paid", Some(amount), None, None),
            StatedLine::Period(start, end) => {
                ("stated_service_period", None, Some(start), Some(end))
            }
            StatedLine::Other => continue,
        };
        scan.hits.push(Hit {
            kind,
            display: line,
            usable: true,
            reason: None,
            amount_text: amount,
            period_start: start,
            period_end: end,
            byte_start: span.byte_start,
            byte_end: span.byte_end,
            scalar_start: span.scalar_start,
            scalar_end: span.scalar_end,
            excerpt: span.excerpt.clone(),
            element: name.to_string(),
        });
    }
}

fn collect_inline(tag: &tl::HTMLTag, parser: &tl::Parser, scan: &mut Scan) -> String {
    let mut text = String::new();
    for handle in tag.children().top().iter().copied() {
        let Some(node) = handle.get(parser) else {
            scan.span_broken = true;
            continue;
        };
        match node {
            tl::Node::Comment(_) => {}
            tl::Node::Raw(raw) => text.push_str(&decode_text(raw)),
            tl::Node::Tag(child) => {
                let name = tag_name(child);
                match name.as_str() {
                    "br" => text.push('\n'),
                    "b" | "strong" | "i" | "em" | "span" => {
                        text.push_str(&collect_inline(child, parser, scan));
                    }
                    "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote"
                    | "html" | "body" | "ul" | "ol" => {}
                    _ => {
                        if scan.unknown.is_none() {
                            scan.unknown = Some(name);
                        }
                    }
                }
            }
        }
    }
    text
}

fn settle(mut hits: Vec<Hit>) -> Prepared {
    mark(&mut hits, "stated_amount_paid", |hit| {
        hit.amount_text.clone()
    });
    mark(&mut hits, "stated_service_period", |hit| {
        Some(format!(
            "{}..{}",
            hit.period_start.as_deref().unwrap_or(""),
            hit.period_end.as_deref().unwrap_or("")
        ))
    });
    if hits.is_empty() {
        return prepared("no_match", None, hits);
    }
    let conflict = hits.iter().any(|hit| {
        matches!(
            hit.reason,
            Some("conflicting_amounts" | "conflicting_periods")
        )
    });
    if conflict {
        for hit in &mut hits {
            if hit.reason == Some("conflicting_amounts")
                || hit.reason == Some("conflicting_periods")
            {
                hit.usable = false;
            }
        }
        prepared("conflict", Some("conflicting_statements".into()), hits)
    } else {
        prepared("matched", None, hits)
    }
}

fn mark(hits: &mut [Hit], kind: &str, value: impl Fn(&Hit) -> Option<String>) {
    let mut distinct = Vec::new();
    for hit in hits.iter().filter(|hit| hit.kind == kind) {
        if let Some(text) = value(hit) {
            if !distinct.contains(&text) {
                distinct.push(text);
            }
        }
    }
    if distinct.len() > 1 {
        let reason = if kind == "stated_amount_paid" {
            "conflicting_amounts"
        } else {
            "conflicting_periods"
        };
        for hit in hits.iter_mut().filter(|hit| hit.kind == kind) {
            hit.usable = false;
            hit.reason = Some(reason);
        }
        return;
    }
    let mut kept = false;
    for hit in hits.iter_mut().filter(|hit| hit.kind == kind) {
        if !kept {
            kept = true;
            continue;
        }
        hit.usable = false;
        hit.reason = Some("duplicate_statement");
    }
}

fn fold(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for piece in text.split('\n') {
        let mut line = String::new();
        let mut gap = false;
        for ch in piece.chars() {
            if matches!(ch, ' ' | '\t' | '\u{000c}' | '\r') {
                gap = true;
                continue;
            }
            if gap && !line.is_empty() {
                line.push(' ');
            }
            gap = false;
            line.push(ch);
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn decode_text(raw: &tl::Bytes) -> String {
    html_escape::decode_html_entities(&raw.as_utf8_str()).into_owned()
}

fn tag_name(tag: &tl::HTMLTag) -> String {
    tag.name().as_utf8_str().to_ascii_lowercase()
}

fn has_attr(tag: &tl::HTMLTag, key: &str) -> bool {
    tag.attributes()
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(key))
}

fn attr_value(tag: &tl::HTMLTag, key: &str) -> Option<String> {
    tag.attributes().iter().find_map(|(name, value)| {
        if name.eq_ignore_ascii_case(key) {
            value.map(|text| text.into_owned())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_entity_span_is_the_original_element() {
        let source = concat!(
            "<div hidden><p>Amount paid USD 99.99</p></div>",
            "<p>Amount paid USD <b>15&#46;99</b></p>",
            "<p>Service period: <span>2026-09-01</span> through <span>2026-09-30</span></p>"
        );
        let prepared = extract(source);
        assert_eq!(
            prepared.status,
            "matched",
            "{}",
            prepared.reason.unwrap_or_default()
        );
        assert_eq!(prepared.hits.len(), 2);
        let amount = &prepared.hits[0];
        assert_eq!(amount.display, "Amount paid USD 15.99");
        assert_eq!(&source[amount.byte_start..amount.byte_end], amount.excerpt);
        assert!(amount.excerpt.contains("&#46;"));
        assert!(amount.excerpt.contains("<b>"));
        assert!(!amount.display.contains('&'));
        let period = &prepared.hits[1];
        assert_eq!(
            period.display,
            "Service period: 2026-09-01 through 2026-09-30"
        );
        assert_eq!(&source[period.byte_start..period.byte_end], period.excerpt);
        assert!(period.excerpt.contains("<span>"));
    }
}
