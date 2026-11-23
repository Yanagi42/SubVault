use std::fs::{self, File};
use std::io::Read;
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
use crate::pdf_parse::{
    self, PdfExtraction, EXTRACTOR, EXTRACTOR_VERSION, JOIN, MAX_PDF_BYTES, NORMALIZATION,
    OFFSET_UNIT, RULES_VERSION,
};
use crate::store::{self, CommandRow, ObjectRow, OccurrenceRow, SourceRow};

pub const MEDIA_TYPE: &str = "application/pdf";
pub const ROLE: &str = "user_provided_pdf";

pub struct PdfRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub source: PathBuf,
    pub batch_id: Option<String>,
    pub auth: AuthInput,
    pub hooks: super::Hooks,
}

pub fn submit_pdf(req: PdfRequest) -> Report {
    match submit_inner(req) {
        Ok(body) => as_pdf(Report {
            code: EXIT_OK,
            body,
        }),
        Err(fail) => as_pdf(super::note_uncommitted(fail).into_report()),
    }
}

fn as_pdf(mut report: Report) -> Report {
    if let Some(map) = report.body.as_object_mut() {
        if map.get("slice").and_then(Value::as_str) != Some("M4-real") {
            map.insert("slice".into(), json!("T5-pdf"));
        }
    }
    report
}

fn submit_inner(req: PdfRequest) -> Result<Value, Fail> {
    if req.hooks.stop_at.is_some() || req.hooks.inject_point.is_some() {
        return Err(
            Fail::new(EXIT_USAGE, "Usage", "pdf_has_no_hook").extra("new_effect", json!(false))
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
    let session = super::open_pdf_session(&req.vault, &req.auth)?;
    if let Some(existing) =
        store::command_with_cipher(&session.conn, &req.command_id, &session.cipher)
            .map_err(|err| unseen_db(err, "command_lookup"))?
    {
        return replay(&session, &req, &existing);
    }
    super::decide::reject_if_t3_command(&session.conn, &req.command_id, &session.cipher)?;
    let bytes = read_pdf(&req.source)?;
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

    let (extracted, hints) = extract_for_session(&bytes, session.vault_format);
    let committed = match commit(
        &session,
        &req,
        &mut journal,
        &object,
        &hash,
        &key,
        &extracted,
        &hints,
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

fn read_pdf(path: &Path) -> Result<Vec<u8>, Fail> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) => {
            return Err(io_fail(err, "pdf_stat").extra("preservation", json!(NOT_PRESERVED)))
        }
    };
    if !meta.is_file() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "source_not_a_file")
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    if meta.len() == 0 {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "empty_pdf")
            .extra("bytes_read", json!(0))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    if meta.len() > MAX_PDF_BYTES {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "over_input_budget")
            .extra("bytes_read", json!(0))
            .extra("stat_size", json!(meta.len()))
            .extra("limit", json!(MAX_PDF_BYTES))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    let file = File::open(path)
        .map_err(|err| io_fail(err, "pdf_read").extra("preservation", json!(NOT_PRESERVED)))?;
    let mut bytes = Vec::new();
    file.take(MAX_PDF_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| io_fail(err, "pdf_read").extra("preservation", json!(NOT_PRESERVED)))?;
    if bytes.len() as u64 > MAX_PDF_BYTES {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "over_input_budget")
            .extra("bytes_read", json!(bytes.len()))
            .extra("limit", json!(MAX_PDF_BYTES))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    if !pdf_parse::has_valid_pdf_envelope(&bytes) {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "invalid_pdf_header")
            .extra("bytes_read", json!(bytes.len()))
            .extra("preservation", json!(NOT_PRESERVED))
            .extra("new_effect", json!(false)));
    }
    Ok(bytes)
}

fn start_journal(root: &Path, req: &PdfRequest, cipher: &VaultCipher) -> Result<Journal, Fail> {
    let display = req
        .source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source.pdf")
        .to_string();
    if !is_display_name(&display) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_display_name"));
    }
    let journal = Journal {
        command_id: req.command_id.clone(),
        kind: "pdf".into(),
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
        journal.storage_reused = Some(true);
        journal.object_id = Some(object.object_id.clone());
        return Ok(object);
    }
    let object_id = ids::random_id("obj_").map_err(|err| io_fail(err, "random"))?;
    let staging = journal::staging_path(&session.root, &journal.command_id);
    let disk_bytes = session
        .cipher
        .encrypt_object_bytes(&object_id, bytes)
        .map_err(|err| cipher_tamper_fail(err, "encrypt_object"))?;
    crate::fsutil::atomic_write(&staging, &disk_bytes)
        .map_err(|err| io_fail(err, "write_staging"))?;
    let dest = session.root.join("objects").join(&object_id);
    if let Err(err) = fs::hard_link(&staging, &dest) {
        return Err(io_fail(err, "publish_object"));
    }
    crate::platform::sync_file(&dest).map_err(|err| io_fail(err, "object_fsync"))?;
    crate::fsutil::fsync_dir(dest.parent().unwrap_or(&session.root))
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

fn confirm_object(root: &Path, source: &SourceRow, cipher: &VaultCipher) -> Result<(), Fail> {
    let object = ObjectRow {
        object_id: source.object_id.clone(),
        content_sha256: source.content_sha256.clone(),
        byte_length: source.byte_length,
        relpath: canonical_object_relpath(&source.object_id).unwrap_or_default(),
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
    hash: &str,
    len: i64,
    cipher: &VaultCipher,
) -> Result<(), Fail> {
    let path = root.join("objects").join(&object.object_id);
    let (actual_hash, actual_len) = match cipher.verify_object_file(&object.object_id, &path) {
        Ok(pair) => pair,
        Err(CryptoError::Io(err)) => {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_unreadable")
                    .extra("during", json!("hash_object"))
                    .extra("os_error", json!(err.to_string()))
                    .extra("new_effect", json!(false)),
            );
        }
        Err(err) => return Err(cipher_tamper_fail(err, "hash_object")),
    };
    if actual_hash != hash || actual_len as i64 != len {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_mismatch")
                .extra("new_effect", json!(false)),
        );
    }
    Ok(())
}

fn persist_reuse(
    session: &super::Session,
    req: &PdfRequest,
    source: &SourceRow,
    hash: &str,
) -> Result<Value, Fail> {
    let key = logical_key(hash);
    let mut journal = start_journal(&session.root, req, &session.cipher)?;
    journal.content_sha256 = Some(hash.to_string());
    journal.byte_length = Some(source.byte_length);
    journal.payload_fingerprint = Some(key.clone());
    journal.object_id = Some(source.object_id.clone());
    journal.source_id = Some(source.source_id.clone());
    journal.storage_reused = Some(true);
    journal.media_type = Some(source.media_type.clone());
    journal::write_with_cipher(&session.root, &journal, &session.cipher)?;

    let result = (|| -> Result<Value, Fail> {
        let tx = session
            .conn
            .unchecked_transaction()
            .map_err(|err| db_fail(err, "begin"))?;
        if store::command_with_cipher(&tx, &req.command_id, &session.cipher)
            .map_err(|err| db_fail(err, "command_recheck"))?
            .is_some()
        {
            return Err(
                Fail::new(EXIT_REJECTED, "Rejected", "command_kind_conflict")
                    .extra("new_effect", json!(false))
                    .extra("facts_created", json!(false)),
            );
        }
        super::decide::reject_if_t3_command(&tx, &req.command_id, &session.cipher)?;
        let current =
            store::source_by_hash_with_cipher(&tx, session.vault_format, hash, &session.cipher)
                .map_err(|err| db_fail(err, "source_recheck"))?;
        let Some(current) = current else {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "source_missing")
                    .extra("new_effect", json!(false))
                    .extra("facts_created", json!(false)),
            );
        };
        if current.source_id != source.source_id || current.object_id != source.object_id {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "source_missing")
                    .extra("new_effect", json!(false))
                    .extra("facts_created", json!(false)),
            );
        }
        let occurrence_id = journal.occurrence_id.clone().ok_or_else(|| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "occurrence_missing")
                .extra("new_effect", json!(false))
                .extra("facts_created", json!(false))
        })?;
        store::insert_occurrence_with_cipher(
            &tx,
            &OccurrenceRow {
                occurrence_id,
                source_id: source.source_id.clone(),
                command_id: req.command_id.clone(),
                batch_id: journal.batch_id.clone().unwrap_or_default(),
                display_name: journal
                    .display_name
                    .clone()
                    .unwrap_or_else(|| "source.pdf".into()),
                external_path: journal.external_path.clone(),
                storage_reused: true,
                recorded_at: super::now_ms(),
            },
            &session.cipher,
        )
        .map_err(|err| db_fail(err, "insert_occurrence"))?;

        let mut body = reuse_body(session, req, source, hash, &key);
        let encoded = serde_json::to_string(&body)
            .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
        store::insert_command_with_cipher(
            &tx,
            &CommandRow {
                command_id: req.command_id.clone(),
                kind: "pdf".into(),
                payload_fingerprint: key.clone(),
                result_json: encoded.clone(),
                outcome: "AlreadyPreserved".into(),
            },
            super::now_ms(),
            &session.cipher,
        )
        .map_err(|err| db_fail(err, "insert_command"))?;
        let canonical = pdf_canonical(hash, &key, "reused");
        super::decide::insert_receipt(&tx, &req.command_id, &canonical, &encoded, &session.cipher)?;
        tx.commit().map_err(|err| db_fail(err, "commit"))?;
        if let Some(map) = body.as_object_mut() {
            map.insert("db_committed".into(), json!(true));
        }
        Ok(body)
    })();

    match result {
        Ok(body) => {
            journal.state = "committed".into();
            journal.reason = None;
            match journal::write_with_cipher(&session.root, &journal, &session.cipher) {
                Ok(()) => Ok(body),
                Err(fail) => {
                    let mut body = body;
                    if let Some(map) = body.as_object_mut() {
                        map.insert("post_commit_io".into(), json!(true));
                        map.insert("journal_label_update".into(), json!("failed"));
                        map.insert("new_effect".into(), json!(false));
                        map.insert("facts_created".into(), json!(false));
                        map.insert("preservation".into(), json!(PRESERVED));
                        if let Some(during) = fail.extra.get("during") {
                            map.insert("during".into(), during.clone());
                        }
                    }
                    Ok(body)
                }
            }
        }
        Err(fail) => {
            let _ = abort_journal(
                &session.root,
                &mut journal,
                "reuse_not_committed",
                &session.cipher,
            );
            Err(fail.extra("facts_created", json!(false)))
        }
    }
}

fn reuse_body(
    session: &super::Session,
    req: &PdfRequest,
    source: &SourceRow,
    hash: &str,
    key: &str,
) -> Value {
    let mut body = honesty("AlreadyPreserved");
    body.insert("new_effect".into(), json!(false));
    body.insert("facts_created".into(), json!(false));
    body.insert("preservation".into(), json!(PRESERVED));
    body.insert("object_intact".into(), json!(true));
    body.insert("content_identity".into(), json!("same_bytes"));
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("source_id".into(), json!(source.source_id));
    body.insert("object_id".into(), json!(source.object_id));
    body.insert("content_sha256".into(), json!(hash));
    body.insert("byte_length".into(), json!(source.byte_length));
    body.insert("media_type".into(), json!(source.media_type));
    body.insert("logical_key".into(), json!(key));
    body.insert("ocr".into(), json!("not_available"));
    if session.vault_format >= 6 {
        body.insert("slice".into(), json!("M4-real"));
        apply_format_honesty(&mut body, session.vault_format, session.protection);
    }
    Value::Object(body)
}

fn replay(
    session: &super::Session,
    req: &PdfRequest,
    existing: &CommandRow,
) -> Result<Value, Fail> {
    if existing.kind != "pdf" {
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
    if !hash.is_empty() {
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
    match crate::fsutil::sha256_file(&req.source) {
        Ok((actual_hash, _)) if actual_hash == hash => {
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

fn extract_for_session(
    bytes: &[u8],
    vault_format: i64,
) -> (PdfExtraction, crate::real_extract::ExtractedHints) {
    if vault_format < 6 {
        return (
            pdf_parse::extract(bytes),
            crate::real_extract::ExtractedHints::default(),
        );
    }
    let (mut strict, pages_res) = pdf_parse::extract_strict_and_pages(bytes);
    let real = match pages_res {
        Ok(pages) => crate::real_extract::extract_real_pdf_from_pages(pages),
        Err(reason) => crate::real_extract::RealBillExtraction {
            status: "unsupported",
            reason: Some(reason),
            page_count: 0,
            candidates: Vec::new(),
            adopt_amount_idx: None,
            adopt_period_idx: None,
            hints: crate::real_extract::ExtractedHints::default(),
        },
    };
    let hints = real.hints;
    if strict.status == "conflict" {
        return (strict, hints);
    }
    let strict_has_amt = strict.adopt_amount_idx.is_some();
    let strict_has_per = strict.adopt_period_idx.is_some();
    if strict_has_amt || strict_has_per {
        if !strict_has_amt {
            if let Some(idx) = real.adopt_amount_idx {
                let c = &real.candidates[idx];
                strict.adopt_amount_idx = Some(strict.candidates.len());
                strict.candidates.push(pdf_parse::PdfCandidate {
                    kind: c.kind.clone(),
                    usable: true,
                    excerpt: c.excerpt.clone(),
                    value: c.value.clone(),
                    locator: c.locator.clone(),
                });
                strict.status = "matched";
                strict.reason = None;
            }
        }
        if !strict_has_per {
            if let Some(idx) = real.adopt_period_idx {
                let c = &real.candidates[idx];
                strict.adopt_period_idx = Some(strict.candidates.len());
                strict.candidates.push(pdf_parse::PdfCandidate {
                    kind: c.kind.clone(),
                    usable: true,
                    excerpt: c.excerpt.clone(),
                    value: c.value.clone(),
                    locator: c.locator.clone(),
                });
                strict.status = "matched";
                strict.reason = None;
            }
        }
        return (strict, hints);
    }
    if real.adopt_amount_idx.is_some()
        || real.adopt_period_idx.is_some()
        || real.status == "conflict"
        || !real.candidates.is_empty()
    {
        let converted = PdfExtraction {
            status: real.status,
            reason: real.reason,
            page_count: real.page_count,
            candidates: real
                .candidates
                .into_iter()
                .map(|c| pdf_parse::PdfCandidate {
                    kind: c.kind,
                    usable: c.usable,
                    excerpt: c.excerpt,
                    value: c.value,
                    locator: c.locator,
                })
                .collect(),
            adopt_amount_idx: real.adopt_amount_idx,
            adopt_period_idx: real.adopt_period_idx,
        };
        return (converted, hints);
    }
    (strict, hints)
}

#[allow(clippy::too_many_arguments)]
fn commit(
    session: &super::Session,
    req: &PdfRequest,
    journal: &mut Journal,
    object: &ObjectRow,
    hash: &str,
    key: &str,
    extracted: &PdfExtraction,
    hints: &crate::real_extract::ExtractedHints,
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
            occurrence_id,
            source_id: source_id.clone(),
            command_id: req.command_id.clone(),
            batch_id: journal.batch_id.clone().unwrap_or_default(),
            display_name: journal
                .display_name
                .clone()
                .unwrap_or_else(|| "source.pdf".into()),
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
            ROLE
        ],
    )
    .map_err(|err| db_fail(err, "insert_version"))?;

    let run_id = ids::random_id("run_").map_err(|err| io_fail(err, "random"))?;
    let attempt_id = ids::random_id("att_").map_err(|err| io_fail(err, "random"))?;
    let stored_logical_key = session.cipher.dedup_key("logical_key", key);
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
            RULES_VERSION,
            config_json().to_string(),
        ],
    )
    .map_err(|err| db_fail(err, "insert_run"))?;
    let detail_plain = json!({
        "extraction": extracted.status,
        "reason": extracted.reason,
        "page_count": extracted.page_count
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
                enc_locator
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
    if let Some(idx) = extracted.adopt_period_idx {
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
        RULES_VERSION,
        &drafts,
        &session.cipher,
    )?;

    if session.vault_format >= 6 {
        store::seed_annotation_hints_with_cipher(
            &tx,
            &source_id,
            hints,
            super::now_ms(),
            &session.cipher,
        )
        .map_err(|err| db_fail(err, "seed_annotation"))?;
    }

    let mut body = receipt(
        session,
        req,
        journal,
        &source_id,
        &source_version_id,
        &object.object_id,
        hash,
        key,
        &run_id,
        extracted,
        &candidate_ids,
        &adopted,
    );
    let encoded = serde_json::to_string(&body)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
    store::insert_command_with_cipher(
        &tx,
        &CommandRow {
            command_id: req.command_id.clone(),
            kind: "pdf".into(),
            payload_fingerprint: key.into(),
            result_json: encoded.clone(),
            outcome: "Committed".into(),
        },
        super::now_ms(),
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "insert_command"))?;
    let canonical = pdf_canonical(hash, key, extracted.status);
    super::decide::insert_receipt(&tx, &req.command_id, &canonical, &encoded, &session.cipher)?;
    tx.commit().map_err(|err| db_fail(err, "commit"))?;
    if let Some(map) = body.as_object_mut() {
        map.insert("db_committed".into(), json!(true));
    }
    Ok(body)
}

#[allow(clippy::too_many_arguments)]
fn receipt(
    session: &super::Session,
    req: &PdfRequest,
    journal: &Journal,
    source_id: &str,
    source_version_id: &str,
    object_id: &str,
    hash: &str,
    key: &str,
    run_id: &str,
    extracted: &PdfExtraction,
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
    body.insert("run_id".into(), json!(run_id));
    body.insert("logical_key".into(), json!(key));
    body.insert("extraction".into(), json!(extracted.status));
    body.insert("extraction_reason".into(), json!(extracted.reason));
    body.insert("page_count".into(), json!(extracted.page_count));
    body.insert("candidates_published".into(), json!(true));
    body.insert("candidate_ids".into(), json!(candidate_ids));
    body.insert(
        "facts".into(),
        Value::Array(
            adopted
                .iter()
                .map(|row| {
                    json!({
                        "fact_id": row.fact_id,
                        "decision_id": row.decision_id,
                        "candidate_id": row.candidate_id,
                        "fact_type": row.fact_type,
                        "revision": 1
                    })
                })
                .collect(),
        ),
    );
    body.insert("payment_verified".into(), json!(false));
    body.insert("ocr".into(), json!("not_available"));
    body.insert("network".into(), json!(false));
    body.insert("mailbox".into(), json!(false));
    body.insert("clipboard".into(), json!(false));
    if session.vault_format >= 6 {
        body.insert("slice".into(), json!("M4-real"));
        apply_format_honesty(&mut body, session.vault_format, session.protection);
    }
    Value::Object(body)
}

fn config_json() -> Value {
    json!({
        "extractor": EXTRACTOR,
        "extractor_version": EXTRACTOR_VERSION,
        "normalization": NORMALIZATION,
        "join": JOIN,
        "rules_version": RULES_VERSION,
        "media_type": MEDIA_TYPE,
        "role": ROLE,
        "offset_unit": OFFSET_UNIT,
        "max_pdf_bytes": MAX_PDF_BYTES,
        "flate_decoder": "miniz_oxide",
        "flate_decoder_version": "0.8.8",
        "ocr": "not_available"
    })
}

fn logical_key(content_sha256: &str) -> String {
    let parts = [
        EXTRACTOR,
        EXTRACTOR_VERSION,
        NORMALIZATION,
        JOIN,
        RULES_VERSION,
        MEDIA_TYPE,
        ROLE,
        OFFSET_UNIT,
        &MAX_PDF_BYTES.to_string(),
        content_sha256,
    ];
    let mut hasher = Sha256::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([0xff]);
        }
        hasher.update(part.as_bytes());
    }
    hex_encode(&hasher.finalize())
}

fn pdf_canonical(hash: &str, logical_key: &str, status: &str) -> String {
    format!(
        "{}\npdf\ncontent_sha256={hash}\nmedia_type={MEDIA_TYPE}\nrules_version={RULES_VERSION}\nlogical_key={logical_key}\nstatus={status}\n",
        super::decide::PAYLOAD_VERSION
    )
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}
