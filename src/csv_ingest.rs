//! Real-world CSV statement ingestion (`subvault csv`, Format 6 only).
//!
//! Preserves original CSV bytes verbatim (including UTF-8 BOM or GB18030/GBK
//! Alipay/WeChat exports) in the content-addressed object store with SHA-256
//! deduplication, records parsed rows as `csv_transaction` candidates, and
//! adopts unambiguous single-row expense CSVs directly into `facts`.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::crypto::{AuthInput, CryptoError, VaultCipher};
use crate::ids::{self, canonical_object_relpath, hex_encode, is_command_id, is_display_name};
use crate::journal::{self, Journal};
use crate::model::{
    apply_format_honesty, cipher_tamper_fail, db_fail, honesty, io_fail, unseen_db, Fail, Report,
    EXIT_NOT_PRESERVED, EXIT_OK, EXIT_RECOVERY, EXIT_REJECTED, EXIT_USAGE, NOT_PRESERVED,
    PRESERVED,
};
use crate::real_extract::{self, CsvExtraction, MAX_CSV_BYTES, REAL_CSV_RULES_VERSION};
use crate::store::{self, CommandRow, ObjectRow, OccurrenceRow, SourceRow};

pub const MEDIA_TYPE: &str = "text/csv;charset=utf-8";
pub const ROLE: &str = "user_provided_csv";
const EXTRACTOR: &str = "subvault-csv";
const EXTRACTOR_VERSION: &str = "1";
const NORMALIZATION: &str = "csv-bilingual-1";
const JOIN: &str = "csv-rows-1";

pub struct CsvRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub source: PathBuf,
    pub batch_id: Option<String>,
    pub auth: AuthInput,
    pub hooks: super::Hooks,
}

pub fn submit_csv(req: CsvRequest) -> Report {
    match submit_inner(req) {
        Ok(body) => Report {
            code: EXIT_OK,
            body,
        },
        Err(fail) => super::note_uncommitted(fail).into_report(),
    }
}

fn submit_inner(req: CsvRequest) -> Result<Value, Fail> {
    if req.hooks.stop_at.is_some() || req.hooks.inject_point.is_some() {
        return Err(
            Fail::new(EXIT_USAGE, "Usage", "csv_has_no_hook").extra("new_effect", json!(false))
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
    let session = super::open_real_session(&req.vault, &req.auth)?;
    if let Some(existing) =
        store::command_with_cipher(&session.conn, &req.command_id, &session.cipher)
            .map_err(|err| unseen_db(err, "command_lookup"))?
    {
        return replay(&session, &req, &existing);
    }
    super::decide::reject_if_t3_command(&session.conn, &req.command_id, &session.cipher)?;
    let bytes = read_csv(&req.source)?;
    let hash = sha256_bytes(&bytes);
    if let Some(source) = store::source_by_hash_with_cipher(
        &session.conn,
        session.vault_format,
        &hash,
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "source_lookup"))?
    {
        confirm_object(&session.root, &source, &session.cipher)?;
        return persist_reuse(&session, &req, &source, &hash);
    }

    let mut journal = start_journal(&session.root, &req, &session.cipher)?;
    let key = logical_key(&hash);
    journal.content_sha256 = Some(hash.clone());
    journal.byte_length = Some(bytes.len() as i64);
    journal.payload_fingerprint = Some(key.clone());
    journal.media_type = Some(MEDIA_TYPE.into());
    journal::write_with_cipher(&session.root, &journal, &session.cipher)?;

    let object = match place_object(&session, &bytes, &hash, &mut journal) {
        Ok(object) => object,
        Err(fail) => {
            abort_journal(
                &session.root,
                &mut journal,
                "object_not_placed",
                &session.cipher,
            )?;
            return Err(fail.extra("preservation", json!(NOT_PRESERVED)));
        }
    };
    journal.state = "object_ready".into();
    journal::write_with_cipher(&session.root, &journal, &session.cipher)?;

    let extracted = real_extract::parse_csv_statement(&bytes);
    let committed = match commit(
        &session,
        &req,
        &mut journal,
        &object,
        &hash,
        &key,
        &extracted,
    ) {
        Ok(body) => body,
        Err(fail) => {
            let _ = abort_journal(
                &session.root,
                &mut journal,
                "commit_failed",
                &session.cipher,
            );
            return Err(fail.extra("preservation", json!(NOT_PRESERVED)));
        }
    };

    journal.state = "committed".into();
    journal.reason = None;
    match journal::write_with_cipher(&session.root, &journal, &session.cipher) {
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

fn read_csv(path: &Path) -> Result<Vec<u8>, Fail> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) => {
            return Err(io_fail(err, "csv_stat").extra("preservation", json!(NOT_PRESERVED)))
        }
    };
    if !meta.is_file() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "source_not_a_file")
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    if meta.len() == 0 {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "empty_csv")
            .extra("bytes_read", json!(0))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    if meta.len() > MAX_CSV_BYTES {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "over_input_budget")
            .extra("bytes_read", json!(0))
            .extra("stat_size", json!(meta.len()))
            .extra("limit", json!(MAX_CSV_BYTES))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    let mut file = File::open(path)
        .map_err(|err| io_fail(err, "csv_read").extra("preservation", json!(NOT_PRESERVED)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| io_fail(err, "csv_read").extra("preservation", json!(NOT_PRESERVED)))?;
    Ok(bytes)
}

fn start_journal(root: &Path, req: &CsvRequest, cipher: &VaultCipher) -> Result<Journal, Fail> {
    let display = req
        .source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("statement.csv")
        .to_string();
    if !is_display_name(&display) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_display_name"));
    }
    let journal = Journal {
        command_id: req.command_id.clone(),
        kind: "csv".into(),
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
    journal::write_with_cipher(root, &journal, cipher)?;
    Ok(journal)
}

fn abort_journal(
    root: &Path,
    journal: &mut Journal,
    reason: &str,
    cipher: &VaultCipher,
) -> Result<(), Fail> {
    journal.state = "aborted".into();
    journal.reason = Some(reason.into());
    journal::write_with_cipher(root, journal, cipher)
}

fn place_object(
    session: &super::Session,
    bytes: &[u8],
    hash: &str,
    journal: &mut Journal,
) -> Result<ObjectRow, Fail> {
    if let Some(object) = store::object_by_hash_with_cipher(
        &session.conn,
        session.vault_format,
        hash,
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "object_lookup"))?
    {
        confirm_hosted(
            &session.root,
            &object,
            hash,
            bytes.len() as i64,
            &session.cipher,
        )?;
        journal.object_id = Some(object.object_id.clone());
        journal.storage_reused = Some(true);
        return Ok(object);
    }
    let object_id = ids::random_id("obj_").map_err(|err| io_fail(err, "random"))?;
    let relpath = canonical_object_relpath(&object_id)
        .ok_or_else(|| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_id_rejected"))?;
    let staging = session
        .root
        .join("staging")
        .join(format!("{object_id}.partial"));
    let final_path = session.root.join(&relpath);
    let stored_bytes = session
        .cipher
        .encrypt_object_bytes(&object_id, bytes)
        .map_err(|err| cipher_tamper_fail(err, "encrypt_object"))?;
    let mut file =
        crate::platform::open_new_nofollow(&staging).map_err(|err| io_fail(err, "stage_csv"))?;
    file.write_all(&stored_bytes)
        .and_then(|_| file.sync_all())
        .map_err(|err| io_fail(err, "stage_csv"))?;
    drop(file);
    fs::rename(&staging, &final_path).map_err(|err| io_fail(err, "publish_object"))?;
    crate::fsutil::fsync_dir(&session.root.join("objects"))
        .map_err(|err| io_fail(err, "sync_objects"))?;
    journal.object_id = Some(object_id.clone());
    journal.storage_reused = Some(false);
    Ok(ObjectRow {
        object_id,
        content_sha256: hash.to_string(),
        byte_length: bytes.len() as i64,
        relpath,
    })
}

fn confirm_object(root: &Path, source: &SourceRow, cipher: &VaultCipher) -> Result<(), Fail> {
    let object = ObjectRow {
        object_id: source.object_id.clone(),
        content_sha256: source.content_sha256.clone(),
        byte_length: source.byte_length,
        relpath: String::new(),
    };
    confirm_hosted(
        root,
        &object,
        &source.content_sha256,
        source.byte_length,
        cipher,
    )
}

fn confirm_hosted(
    root: &Path,
    object: &ObjectRow,
    expected_hash: &str,
    expected_len: i64,
    cipher: &VaultCipher,
) -> Result<(), Fail> {
    let Some(relpath) = canonical_object_relpath(&object.object_id) else {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_id_rejected")
                .extra("new_effect", json!(false)),
        );
    };
    let path = root.join(&relpath);
    match cipher.verify_object_file(&object.object_id, &path) {
        Ok((actual_hash, actual_len))
            if actual_hash == expected_hash && actual_len as i64 == expected_len =>
        {
            Ok(())
        }
        Ok(_) => Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "hash_mismatch")
                .extra("new_effect", json!(false)),
        ),
        Err(CryptoError::Io(err)) => {
            Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_missing")
                    .extra("os_error", json!(err.to_string()))
                    .extra("new_effect", json!(false)),
            )
        }
        Err(err) => Err(cipher_tamper_fail(err, "hash_object")),
    }
}

fn persist_reuse(
    session: &super::Session,
    req: &CsvRequest,
    source: &SourceRow,
    hash: &str,
) -> Result<Value, Fail> {
    let display = req
        .source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("statement.csv")
        .to_string();
    if !is_display_name(&display) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_display_name"));
    }
    let batch_id = match &req.batch_id {
        Some(id) => id.clone(),
        None => ids::random_id("b_").map_err(|err| io_fail(err, "random"))?,
    };
    let occurrence_id = ids::random_id("occ_").map_err(|err| io_fail(err, "random"))?;
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    store::insert_occurrence_with_cipher(
        &tx,
        &OccurrenceRow {
            occurrence_id: occurrence_id.clone(),
            source_id: source.source_id.clone(),
            command_id: req.command_id.clone(),
            batch_id: batch_id.clone(),
            display_name: display,
            external_path: Some(req.source.display().to_string()),
            storage_reused: true,
            recorded_at: super::now_ms(),
        },
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "insert_occurrence"))?;

    let mut body = honesty("Committed");
    body.insert("slice".into(), json!("M4-real"));
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("preservation".into(), json!(PRESERVED));
    body.insert("new_effect".into(), json!(true));
    body.insert("storage_reused".into(), json!(true));
    body.insert("source_id".into(), json!(source.source_id));
    body.insert("object_id".into(), json!(source.object_id));
    body.insert("occurrence_id".into(), json!(occurrence_id));
    body.insert("batch_id".into(), json!(batch_id));
    body.insert("content_sha256".into(), json!(hash));
    body.insert("byte_length".into(), json!(source.byte_length));
    body.insert("media_type".into(), json!(source.media_type));
    body.insert("extraction".into(), json!("reused"));
    apply_format_honesty(&mut body, session.vault_format, session.protection);
    let result = Value::Object(body);
    let encoded = serde_json::to_string(&result)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    let key = logical_key(hash);
    store::insert_command_with_cipher(
        &tx,
        &CommandRow {
            command_id: req.command_id.clone(),
            kind: "csv".into(),
            payload_fingerprint: key,
            result_json: encoded,
            outcome: "Committed".into(),
        },
        super::now_ms(),
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "insert_command"))?;
    tx.commit().map_err(|err| db_fail(err, "commit"))?;
    let _ = store::checkpoint(&session.conn);
    Ok(result)
}

fn replay(
    session: &super::Session,
    req: &CsvRequest,
    existing: &CommandRow,
) -> Result<Value, Fail> {
    if existing.kind != "csv" {
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
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let object_id = body
        .get("object_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !hash.is_empty() && !object_id.is_empty() {
        let object = ObjectRow {
            object_id,
            content_sha256: hash.clone(),
            byte_length: body.get("byte_length").and_then(Value::as_i64).unwrap_or(0),
            relpath: String::new(),
        };
        confirm_hosted(
            &session.root,
            &object,
            &hash,
            object.byte_length,
            &session.cipher,
        )?;
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
    req: &CsvRequest,
    journal: &mut Journal,
    object: &ObjectRow,
    hash: &str,
    key: &str,
    extracted: &CsvExtraction,
) -> Result<Value, Fail> {
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "begin"))?;
    if store::object_by_hash_with_cipher(&tx, session.vault_format, hash, &session.cipher)
        .map_err(|err| db_fail(err, "object_recheck"))?
        .is_none()
    {
        store::insert_object_with_cipher(&tx, session.vault_format, object, &session.cipher)
            .map_err(|err| db_fail(err, "insert_object"))?;
    }
    let source_id = ids::random_id("src_").map_err(|err| io_fail(err, "random"))?;
    store::insert_source_with_cipher(
        &tx,
        session.vault_format,
        &SourceRow {
            source_id: source_id.clone(),
            object_id: object.object_id.clone(),
            content_sha256: hash.into(),
            byte_length: object.byte_length,
            media_type: MEDIA_TYPE.into(),
            user_marker: None,
        },
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "insert_source"))?;
    let occurrence_id = journal
        .occurrence_id
        .clone()
        .ok_or_else(|| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "occurrence_missing"))?;
    store::insert_occurrence_with_cipher(
        &tx,
        &OccurrenceRow {
            occurrence_id: occurrence_id.clone(),
            source_id: source_id.clone(),
            command_id: req.command_id.clone(),
            batch_id: journal.batch_id.clone().unwrap_or_default(),
            display_name: journal
                .display_name
                .clone()
                .unwrap_or_else(|| "statement.csv".into()),
            external_path: journal.external_path.clone(),
            storage_reused: journal.storage_reused.unwrap_or(false),
            recorded_at: super::now_ms(),
        },
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "insert_occurrence"))?;

    let source_version_id = ids::random_id("sv_").map_err(|err| io_fail(err, "random"))?;
    let stored_sv_hash = session
        .cipher
        .encrypt_field(
            "source_versions",
            &source_version_id,
            "content_sha256",
            hash,
        )
        .map_err(|err| cipher_tamper_fail(err, "encrypt_version"))?;
    tx.execute(
        "INSERT INTO source_versions(source_version_id, source_id, object_id, content_sha256, byte_length, media_type, role, eml) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
        params![
            source_version_id,
            source_id,
            object.object_id,
            stored_sv_hash,
            object.byte_length,
            MEDIA_TYPE,
            ROLE,
        ],
    )
    .map_err(|err| db_fail(err, "insert_version"))?;

    let run_id = ids::random_id("run_").map_err(|err| io_fail(err, "random"))?;
    let attempt_id = ids::random_id("att_").map_err(|err| io_fail(err, "random"))?;
    let stored_logical_key = session.cipher.dedup_key("extraction_runs.logical_key", key);
    tx.execute(
        "INSERT INTO extraction_runs(run_id, logical_key, source_version_id, source_id, extractor, extractor_version, normalization, join_version, rules_version, config_json, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'succeeded')",
        params![
            run_id,
            stored_logical_key,
            source_version_id,
            source_id,
            EXTRACTOR,
            EXTRACTOR_VERSION,
            NORMALIZATION,
            JOIN,
            REAL_CSV_RULES_VERSION,
            json!({
                "extractor": EXTRACTOR,
                "rules_version": REAL_CSV_RULES_VERSION,
                "encoding": extracted.encoding,
                "provider": extracted.provider,
            })
            .to_string(),
        ],
    )
    .map_err(|err| db_fail(err, "insert_run"))?;
    let detail_plain = json!({
        "extraction": extracted.status,
        "reason": extracted.reason,
        "encoding": extracted.encoding,
        "provider": extracted.provider,
        "total_rows": extracted.total_rows,
        "truncated": extracted.truncated,
    })
    .to_string();
    let detail_stored = session
        .cipher
        .encrypt_field(
            "extraction_attempts",
            &attempt_id,
            "detail_json",
            &detail_plain,
        )
        .map_err(|err| cipher_tamper_fail(err, "encrypt_attempt"))?;
    tx.execute(
        "INSERT INTO extraction_attempts(attempt_id, run_id, attempt_no, command_id, status, published, detail_json) VALUES (?1, ?2, 1, ?3, 'succeeded', 1, ?4)",
        params![attempt_id, run_id, req.command_id, detail_stored],
    )
    .map_err(|err| db_fail(err, "insert_attempt"))?;

    let mut candidate_ids = Vec::new();
    for (ordinal, cand) in extracted.candidates.iter().enumerate() {
        let candidate_id = ids::random_id("cand_").map_err(|err| io_fail(err, "random"))?;
        let mut locator = cand.locator.clone();
        if let Some(map) = locator.as_object_mut() {
            map.insert("source_version_id".into(), json!(source_version_id));
            map.insert("content_sha256".into(), json!(hash));
        }
        let enc_excerpt = session
            .cipher
            .encrypt_field("candidates", &candidate_id, "excerpt", &cand.excerpt)
            .map_err(|err| cipher_tamper_fail(err, "encrypt_candidate"))?;
        let enc_value = session
            .cipher
            .encrypt_field(
                "candidates",
                &candidate_id,
                "value_json",
                &Value::Object(cand.value.clone()).to_string(),
            )
            .map_err(|err| cipher_tamper_fail(err, "encrypt_candidate"))?;
        let enc_locator = session
            .cipher
            .encrypt_field(
                "candidates",
                &candidate_id,
                "locator_json",
                &locator.to_string(),
            )
            .map_err(|err| cipher_tamper_fail(err, "encrypt_candidate"))?;
        tx.execute(
            "INSERT INTO candidates(candidate_id, run_id, attempt_id, source_version_id, ordinal, kind, usable, excerpt, value_json, locator_json, adopted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
            params![
                candidate_id,
                run_id,
                attempt_id,
                source_version_id,
                ordinal as i64,
                cand.kind,
                if cand.usable { 1 } else { 0 },
                enc_excerpt,
                enc_value,
                enc_locator,
            ],
        )
        .map_err(|err| db_fail(err, "insert_candidate"))?;
        candidate_ids.push(candidate_id);
    }

    let mut drafts = Vec::new();
    if let Some(idx) = extracted.adopt_amount_idx {
        drafts.push(super::decide::StatedDraft {
            candidate_id: &candidate_ids[idx],
            kind: &extracted.candidates[idx].kind,
            value: &extracted.candidates[idx].value,
        });
    }
    let adopted = super::decide::adopt_statements(
        &tx,
        &req.command_id,
        &source_id,
        &source_version_id,
        &run_id,
        REAL_CSV_RULES_VERSION,
        &drafts,
        &session.cipher,
    )?;

    store::seed_annotation_hints_with_cipher(
        &tx,
        &source_id,
        &extracted.hints,
        super::now_ms(),
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "seed_annotation"))?;

    let mut body = honesty("Committed");
    body.insert("slice".into(), json!("M4-real"));
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("preservation".into(), json!(PRESERVED));
    body.insert("new_effect".into(), json!(true));
    body.insert(
        "storage_reused".into(),
        json!(journal.storage_reused.unwrap_or(false)),
    );
    body.insert("source_id".into(), json!(source_id));
    body.insert("source_version_id".into(), json!(source_version_id));
    body.insert("object_id".into(), json!(object.object_id));
    body.insert("occurrence_id".into(), json!(occurrence_id));
    body.insert("content_sha256".into(), json!(hash));
    body.insert("byte_length".into(), json!(object.byte_length));
    body.insert("media_type".into(), json!(MEDIA_TYPE));
    body.insert("role".into(), json!(ROLE));
    body.insert("run_id".into(), json!(run_id));
    body.insert("extraction".into(), json!(extracted.status));
    body.insert("encoding".into(), json!(extracted.encoding));
    body.insert("provider".into(), json!(extracted.provider));
    body.insert("total_rows".into(), json!(extracted.total_rows));
    body.insert("truncated".into(), json!(extracted.truncated));
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
    apply_format_honesty(&mut body, session.vault_format, session.protection);
    let result = Value::Object(body);
    let encoded = serde_json::to_string(&result)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    store::insert_command_with_cipher(
        &tx,
        &CommandRow {
            command_id: req.command_id.clone(),
            kind: "csv".into(),
            payload_fingerprint: key.to_string(),
            result_json: encoded,
            outcome: "Committed".into(),
        },
        super::now_ms(),
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "insert_command"))?;
    tx.commit().map_err(|err| db_fail(err, "commit"))?;
    let _ = store::checkpoint(&session.conn);
    journal.source_id = Some(source_id);
    Ok(result)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex_encode(&h.finalize())
}

fn logical_key(hash: &str) -> String {
    format!("csv:{hash}:{EXTRACTOR}:{EXTRACTOR_VERSION}:{REAL_CSV_RULES_VERSION}")
}
