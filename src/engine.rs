use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::fsutil::{
    self, atomic_write, filesystem_magic, network_filesystem, sha256_file, MAX_OBJECT_BYTES,
};
use crate::ids::{
    self, canonical_object_relpath, hex_encode, is_command_id, is_display_name, is_lower_hex,
    is_media_type, is_prefixed_id,
};
use crate::lockfile::{self, sigstop, VaultLock, LOCK_TEXT};
use crate::model::{
    capabilities, honesty, platform_limits, Fail, Report, EXIT_DAMAGED, EXIT_FAULT_MISSED,
    EXIT_INTEGRITY, EXIT_LOCKED, EXIT_NOT_PRESERVED, EXIT_NOT_SYNTHETIC, EXIT_NOT_VAULT, EXIT_OK,
    EXIT_RECOVERY, EXIT_REJECTED, EXIT_STOP_MISSED, EXIT_UNSUPPORTED_FS, EXIT_USAGE, EXIT_VERSION,
    VAULT_BANNER,
};
use crate::store::{self, CommandRow, ObjectRow, OccurrenceRow, SourceRow};

const IMPORT_STOPS: &[&str] = &[
    "before_copy",
    "during_copy",
    "after_copy_before_publish",
    "object_ready",
    "before_db_commit",
    "after_db_commit",
    "before_receipt",
];
const IMPORT_INJECTS: &[&str] = &["after_staging_fsync", "object_fsync", "db_commit"];
const MARKER_STOPS: &[&str] = &["before_db_commit", "after_db_commit", "before_receipt"];
const MARKER_INJECTS: &[&str] = &["db_commit"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InjectAction {
    Enospc,
    IoError,
}

#[derive(Clone)]
pub struct Hooks {
    pub stop_at: Option<String>,
    pub checkpoint: Option<PathBuf>,
    pub inject_action: Option<InjectAction>,
    pub inject_point: Option<String>,
    pub witness: Option<PathBuf>,
}

struct HookRun {
    hooks: Hooks,
    stop_reached: bool,
    inject_reached: bool,
}

impl HookRun {
    fn new(hooks: Hooks) -> Self {
        Self {
            hooks,
            stop_reached: false,
            inject_reached: false,
        }
    }

    fn stop(&mut self, point: &str, mut details: Map<String, Value>) -> Result<(), Fail> {
        if self.hooks.stop_at.as_deref() != Some(point) {
            return Ok(());
        }
        let path = self.hooks.checkpoint.clone().ok_or_else(|| {
            Fail::new(EXIT_USAGE, "Usage", "checkpoint_required").extra("point", json!(point))
        })?;
        details.insert("point".into(), json!(point));
        let bytes = serde_json::to_vec_pretty(&Value::Object(details))
            .map_err(|_| Fail::new(EXIT_USAGE, "Usage", "checkpoint_encode"))?;
        atomic_write(&path, &bytes).map_err(|err| io_fail(err, "checkpoint"))?;
        self.stop_reached = true;
        sigstop();
        Ok(())
    }

    fn inject(&mut self, point: &str, details: Map<String, Value>) -> Result<(), Fail> {
        if self.hooks.inject_point.as_deref() != Some(point) {
            return Ok(());
        }
        let action = self
            .hooks
            .inject_action
            .ok_or_else(|| Fail::new(EXIT_USAGE, "Usage", "inject_action_required"))?;
        let path = self.hooks.witness.clone().ok_or_else(|| {
            Fail::new(EXIT_USAGE, "Usage", "witness_required").extra("point", json!(point))
        })?;
        let mut body = details;
        body.insert("reached".into(), Value::Bool(true));
        body.insert("point".into(), json!(point));
        body.insert(
            "action".into(),
            json!(match action {
                InjectAction::Enospc => "enospc",
                InjectAction::IoError => "io_error",
            }),
        );
        let bytes = serde_json::to_vec_pretty(&Value::Object(body))
            .map_err(|_| Fail::new(EXIT_USAGE, "Usage", "witness_encode"))?;
        atomic_write(&path, &bytes).map_err(|err| io_fail(err, "witness"))?;
        self.inject_reached = true;
        let reason = match action {
            InjectAction::Enospc => "injected_enospc",
            InjectAction::IoError => "injected_io_error",
        };
        Err(Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", reason)
            .extra("injected", Value::Bool(true))
            .extra("inject_point", json!(point))
            .extra(
                "inject_action",
                json!(match action {
                    InjectAction::Enospc => "enospc",
                    InjectAction::IoError => "io_error",
                }),
            )
            .extra(
                "note",
                json!("injected fault, not a full device and not a failed disk"),
            )
            .extra("preservation", json!("not_preserved")))
    }
}

struct Session {
    root: PathBuf,
    _lock: VaultLock,
    conn: Connection,
    magic: i64,
}

#[derive(Clone)]
struct Journal {
    command_id: String,
    kind: String,
    generation: i64,
    state: String,
    batch_id: Option<String>,
    display_name: Option<String>,
    external_path: Option<String>,
    media_type: Option<String>,
    occurrence_id: Option<String>,
    object_id: Option<String>,
    content_sha256: Option<String>,
    byte_length: Option<i64>,
    payload_fingerprint: Option<String>,
    storage_reused: Option<bool>,
    reason: Option<String>,
    marker_value: Option<String>,
    source_id: Option<String>,
}

struct Phase {
    published: bool,
}

pub fn help_report() -> Report {
    let mut body = honesty("Help");
    body.insert(
        "note".into(),
        json!("T1 preserves original bytes in a synthetic vault. It does not arrange, search, extract, or accept real data. Security is not complete. This is not a user-ready product."),
    );
    body.insert(
        "commands".into(),
        json!([
            "help",
            "version",
            "init --vault PATH",
            "open --vault PATH",
            "verify --vault PATH",
            "import --vault PATH --command-id ID --media-type TYPE --source PATH [--batch-id ID] [--display-name NAME]",
            "read --vault PATH --source-id ID --out PATH",
            "marker --vault PATH --command-id ID --source-id ID --value TEXT",
            "cancel --vault PATH --command-id ID --generation N [--kind preservation|extraction]",
            "recover --vault PATH",
            "hold --vault PATH --stop-at lock_held --checkpoint PATH"
        ]),
    );
    body.insert(
        "test_hooks".into(),
        json!("--stop-at and --inject are explicit test hooks. A missed hook does not report success. --inject never means the disk or device really failed."),
    );
    Report {
        code: EXIT_OK,
        body: Value::Object(body),
    }
}

pub fn version_report() -> Report {
    let mut body = honesty("Version");
    body.insert("git_sha".into(), json!(env!("SUBVAULT_GIT_SHA")));
    body.insert("vault_format".into(), json!(1));
    body.insert("capabilities".into(), capabilities());
    body.insert("platform_limits".into(), platform_limits(None));
    Report {
        code: EXIT_OK,
        body: Value::Object(body),
    }
}

pub fn init_vault(path: &Path) -> Report {
    match init_inner(path) {
        Ok(report) => report,
        Err(fail) => fail.into_report(),
    }
}

fn init_inner(path: &Path) -> Result<Report, Fail> {
    if path.exists() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "vault_path_exists")
            .extra("created_new_vault", json!(false)));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        return Err(Fail::new(EXIT_NOT_VAULT, "NotAVault", "parent_missing")
            .extra("created_new_vault", json!(false)));
    }
    let magic = filesystem_magic(parent).map_err(|err| io_fail(err, "statfs"))?;
    if network_filesystem(magic) {
        return Err(unsupported_fs(magic));
    }
    let scratch = parent.join(format!(
        ".subvault-init-{}",
        ids::random_id("tmp").map_err(|err| io_fail(err, "random"))?
    ));
    fs::create_dir(&scratch).map_err(|err| io_fail(err, "init_dir"))?;
    let built = (|| -> Result<String, Fail> {
        fs::write(scratch.join("SYNTHETIC_ONLY"), VAULT_BANNER)
            .map_err(|err| io_fail(err, "banner"))?;
        for dir in ["objects", "staging", "journal", "control"] {
            fs::create_dir(scratch.join(dir)).map_err(|err| io_fail(err, "init_dir"))?;
        }
        fs::write(scratch.join("writer.lock"), LOCK_TEXT)
            .map_err(|err| io_fail(err, "lock_file"))?;
        let conn =
            Connection::open(scratch.join("vault.db")).map_err(|err| db_fail(err, "init_open"))?;
        store::configure(&conn).map_err(|err| db_fail(err, "init_pragma"))?;
        let vault_id = ids::random_id("vlt_").map_err(|err| io_fail(err, "random"))?;
        store::init_schema(&conn, &vault_id).map_err(|err| db_fail(err, "init_schema"))?;
        store::checkpoint(&conn).map_err(|err| db_fail(err, "init_checkpoint"))?;
        Ok(vault_id)
    })();
    let vault_id = match built {
        Ok(id) => id,
        Err(fail) => {
            let _ = fs::remove_dir_all(&scratch);
            return Err(fail.extra("created_new_vault", json!(false)));
        }
    };
    if let Err(err) = fs::rename(&scratch, path) {
        let _ = fs::remove_dir_all(&scratch);
        return Err(io_fail(err, "init_rename").extra("created_new_vault", json!(false)));
    }
    let _ = fsutil::fsync_dir(parent);
    let mut body = honesty("Initialized");
    body.insert("vault_id".into(), json!(vault_id));
    body.insert("created_new_vault".into(), json!(true));
    body.insert("protection".into(), json!("unanswered_synthetic_plaintext"));
    body.insert("primary_input".into(), json!("unanswered"));
    body.insert("capabilities".into(), capabilities());
    body.insert("platform_limits".into(), platform_limits(Some(magic)));
    body.insert("new_effect".into(), json!(true));
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

pub fn open_vault(path: &Path) -> Report {
    reopen(path, true)
}

pub fn verify_vault(path: &Path) -> Report {
    reopen(path, true)
}

pub fn recover_vault(path: &Path) -> Report {
    reopen(path, true)
}

fn reopen(path: &Path, clean: bool) -> Report {
    match reopen_inner(path, clean) {
        Ok(report) => report,
        Err(fail) => fail.into_report(),
    }
}

fn reopen_inner(path: &Path, clean: bool) -> Result<Report, Fail> {
    let session = open_session(path)?;
    if clean {
        clean_temps(&session.root)?;
    }
    reconcile(&session)?;
    let inspection = inspect(&session)?;
    store::checkpoint(&session.conn).map_err(|err| db_fail(err, "checkpoint"))?;
    Ok(inspection)
}

pub fn hold_vault(path: &Path, hooks: Hooks) -> Report {
    match hold_inner(path, hooks) {
        Ok(report) => report,
        Err(fail) => fail.into_report(),
    }
}

fn hold_inner(path: &Path, hooks: Hooks) -> Result<Report, Fail> {
    if hooks.stop_at.as_deref() != Some("lock_held") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "hold_requires_lock_held"));
    }
    let session = open_session(path)?;
    let mut hooks = HookRun::new(hooks);
    hooks.stop(
        "lock_held",
        map_of(&[
            ("published", json!(false)),
            ("db_committed", json!(false)),
            ("bytes_written", json!(0)),
        ]),
    )?;
    let mut body = honesty("Held");
    body.insert("new_effect".into(), json!(false));
    let _session = session;
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

pub struct ImportRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub batch_id: Option<String>,
    pub media_type: String,
    pub source: PathBuf,
    pub display_name: Option<String>,
    pub hooks: Hooks,
}

pub fn import_file(req: ImportRequest) -> Report {
    match import_inner(req) {
        Ok(body) => Report {
            code: EXIT_OK,
            body,
        },
        Err(fail) => fail.into_report(),
    }
}

fn import_inner(req: ImportRequest) -> Result<Value, Fail> {
    validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        IMPORT_STOPS,
        IMPORT_INJECTS,
    )?;
    if !is_command_id(&req.command_id) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_command_id"));
    }
    if !is_media_type(&req.media_type) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_media_type"));
    }
    if let Some(batch) = &req.batch_id {
        if !is_command_id(batch) {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_batch_id"));
        }
    }
    let session = open_session(&req.vault)?;
    reconcile(&session)?;
    let meta = fs::symlink_metadata(&req.source).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "source_missing")
                .extra("preservation", json!("not_preserved"))
                .extra("injected", json!(false))
        } else {
            io_fail(err, "source_metadata")
        }
    })?;
    if !meta.file_type().is_file() {
        return Err(
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "source_not_a_file")
                .extra("preservation", json!("not_preserved"))
                .extra("injected", json!(false)),
        );
    }
    if meta.len() > MAX_OBJECT_BYTES {
        return Err(
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "object_too_large")
                .extra("preservation", json!("not_preserved"))
                .extra("byte_length", json!(meta.len()))
                .extra("limit", json!(MAX_OBJECT_BYTES))
                .extra("injected", json!(false)),
        );
    }
    let display_default = req
        .source
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("source")
        .to_string();
    let display_name = req.display_name.clone().unwrap_or(display_default);
    if !is_display_name(&display_name) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_display_name"));
    }

    if let Some(existing) = store::command(&session.conn, &req.command_id)
        .map_err(|err| db_fail(err, "command_lookup"))?
    {
        if req.hooks.inject_point.is_some() || req.hooks.stop_at.is_some() {
            return Err(Fail::new(
                EXIT_FAULT_MISSED,
                "FaultNotReached",
                "command_already_committed",
            )
            .extra("injected", json!(req.hooks.inject_point.is_some()))
            .extra("preservation", json!("not_preserved")));
        }
        let (hash, _len) = sha256_file(&req.source).map_err(|err| io_fail(err, "hash_source"))?;
        let fingerprint = fingerprint(&["import", &hash, &req.media_type]);
        if fingerprint != existing.payload_fingerprint {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "payload_conflict")
                .extra("command_id", json!(req.command_id))
                .extra("preservation", json!("not_preserved"))
                .extra("new_effect", json!(false)));
        }
        return already_committed(&existing);
    }

    let mut hooks = HookRun::new(req.hooks.clone());
    let mut journal = load_or_new_journal(&session.root, &req, &display_name)?;
    if journal.kind != "import" {
        return Err(Fail::new(
            EXIT_REJECTED,
            "Rejected",
            "command_kind_conflict",
        ));
    }
    if let Some(media) = &journal.media_type {
        if media != &req.media_type {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "payload_conflict")
                .extra("command_id", json!(req.command_id))
                .extra("preservation", json!("not_preserved")));
        }
    }
    journal.media_type = Some(req.media_type.clone());
    journal.external_path = Some(req.source.display().to_string());
    if journal.display_name.is_none() {
        journal.display_name = Some(display_name.clone());
    }
    if journal.batch_id.is_none() {
        journal.batch_id = Some(match &req.batch_id {
            Some(id) => id.clone(),
            None => ids::random_id("b_").map_err(|err| io_fail(err, "random"))?,
        });
    }
    if journal.occurrence_id.is_none() {
        journal.occurrence_id = Some(ids::random_id("occ_").map_err(|err| io_fail(err, "random"))?);
    }
    journal.generation = 1;
    journal.state = "started".into();
    journal.reason = None;
    write_journal(&session.root, &journal)?;

    let mut phase = Phase { published: false };
    let result = import_attempt(
        &session,
        &req,
        &mut journal,
        &mut hooks,
        &mut phase,
        meta.len(),
    );
    match result {
        Ok(value) => Ok(value),
        Err(fail) => Err(unwind(&session.root, &mut journal, &phase, fail)?),
    }
}

fn import_attempt(
    session: &Session,
    req: &ImportRequest,
    journal: &mut Journal,
    hooks: &mut HookRun,
    phase: &mut Phase,
    expected_len: u64,
) -> Result<Value, Fail> {
    let command_id = req.command_id.clone();
    hooks.stop(
        "before_copy",
        checkpoint_map(&command_id, 0, Some(expected_len), None, false, false),
    )?;
    if take_cancel(&session.root, &command_id, journal.generation)? {
        return Err(cancelled());
    }
    journal.state = "copying".into();
    write_journal(&session.root, journal)?;
    let staging = staging_path(&session.root, &command_id);
    let _ = fs::remove_file(&staging);
    copy_source(
        &req.source,
        &staging,
        hooks,
        &session.root,
        &command_id,
        journal.generation,
        expected_len,
    )?;
    if take_cancel(&session.root, &command_id, journal.generation)? {
        return Err(cancelled());
    }
    let (hash, len) = sha256_file(&staging).map_err(|err| io_fail(err, "hash_staging"))?;
    journal.content_sha256 = Some(hash.clone());
    journal.byte_length = Some(len as i64);
    write_journal(&session.root, journal)?;
    hooks.inject(
        "after_staging_fsync",
        checkpoint_map(&command_id, len, Some(len), Some(&hash), false, false),
    )?;
    hooks.stop(
        "after_copy_before_publish",
        checkpoint_map(&command_id, len, Some(len), Some(&hash), false, false),
    )?;
    let fingerprint = fingerprint(&["import", &hash, &req.media_type]);
    if let Some(previous) = &journal.payload_fingerprint {
        if previous != &fingerprint {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "payload_conflict")
                .extra("command_id", json!(command_id))
                .extra("preservation", json!("not_preserved")));
        }
    }
    journal.payload_fingerprint = Some(fingerprint.clone());
    journal.content_sha256 = Some(hash.clone());
    journal.byte_length = Some(len as i64);

    let existing =
        store::object_by_hash(&session.conn, &hash).map_err(|err| db_fail(err, "object_lookup"))?;
    let (object_id, storage_reused) = if let Some(object) = existing {
        verify_referenced_file(&session.root, &object).map_err(|alert| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", alert)
                .extra("preservation", json!("not_preserved"))
                .extra("content_sha256", json!(hash))
                .extra("injected", json!(false))
        })?;
        (object.object_id, true)
    } else if let Some(orphan) = find_orphan_with_hash(&session.root, &session.conn, &hash)? {
        (orphan, true)
    } else {
        let mut object_id = journal
            .object_id
            .clone()
            .filter(|id| is_prefixed_id(id, "obj_"))
            .unwrap_or_default();
        if object_id.is_empty() || session.root.join("objects").join(&object_id).exists() {
            object_id = ids::random_id("obj_").map_err(|err| io_fail(err, "random"))?;
        }
        let dest = session.root.join("objects").join(&object_id);
        fs::hard_link(&staging, &dest).map_err(|err| io_fail(err, "publish_object"))?;
        phase.published = true;
        hooks.inject(
            "object_fsync",
            checkpoint_map(&command_id, len, Some(len), Some(&hash), true, false),
        )?;
        File::open(&dest)
            .and_then(|file| file.sync_all())
            .map_err(|err| io_fail(err, "object_fsync"))?;
        fsutil::fsync_dir(dest.parent().unwrap_or(&session.root))
            .map_err(|err| io_fail(err, "object_dir_fsync"))?;
        (object_id, false)
    };

    journal.object_id = Some(object_id.clone());
    journal.storage_reused = Some(storage_reused);
    journal.state = "object_ready".into();
    write_journal(&session.root, journal)?;
    let _ = fs::remove_file(&staging);

    hooks.stop(
        "object_ready",
        checkpoint_map(&command_id, len, Some(len), Some(&hash), true, false),
    )?;
    hooks.stop(
        "before_db_commit",
        checkpoint_map(&command_id, len, Some(len), Some(&hash), true, false),
    )?;
    ensure_precommit(hooks)?;
    if take_cancel(&session.root, &command_id, journal.generation)? {
        return Err(cancelled());
    }

    let result = {
        let tx = session
            .conn
            .unchecked_transaction()
            .map_err(|err| db_fail(err, "begin"))?;
        if store::command(&tx, &command_id)
            .map_err(|err| db_fail(err, "command_recheck"))?
            .is_some()
        {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "command_race"));
        }
        if store::object_by_hash(&tx, &hash)
            .map_err(|err| db_fail(err, "object_recheck"))?
            .is_none()
        {
            let relpath = canonical_object_relpath(&object_id)
                .ok_or_else(|| Fail::new(EXIT_REJECTED, "Rejected", "bad_object_id"))?;
            store::insert_object(
                &tx,
                &ObjectRow {
                    object_id: object_id.clone(),
                    content_sha256: hash.clone(),
                    byte_length: len as i64,
                    relpath,
                },
            )
            .map_err(|err| db_fail(err, "insert_object"))?;
        }
        let source = if let Some(source) =
            store::source_by_hash(&tx, &hash).map_err(|err| db_fail(err, "source_lookup"))?
        {
            source
        } else {
            let source_id = ids::random_id("src_").map_err(|err| io_fail(err, "random"))?;
            let source = SourceRow {
                source_id,
                object_id: object_id.clone(),
                content_sha256: hash.clone(),
                byte_length: len as i64,
                media_type: req.media_type.clone(),
                user_marker: None,
            };
            store::insert_source(&tx, &source).map_err(|err| db_fail(err, "insert_source"))?;
            source
        };
        let occurrence_id = journal.occurrence_id.clone().ok_or_else(|| {
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "missing_occurrence_id")
        })?;
        let recorded_at = now_ms();
        store::insert_occurrence(
            &tx,
            &OccurrenceRow {
                occurrence_id: occurrence_id.clone(),
                source_id: source.source_id.clone(),
                command_id: command_id.clone(),
                batch_id: journal.batch_id.clone().unwrap_or_default(),
                display_name: journal
                    .display_name
                    .clone()
                    .unwrap_or(display_name_fallback()),
                external_path: journal.external_path.clone(),
                storage_reused,
                recorded_at,
            },
        )
        .map_err(|err| db_fail(err, "insert_occurrence"))?;
        let result = import_result(
            &command_id,
            &source,
            &occurrence_id,
            storage_reused,
            journal,
        );
        let encoded = serde_json::to_string(&result)
            .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
        store::insert_command(
            &tx,
            &CommandRow {
                command_id: command_id.clone(),
                kind: "import".into(),
                payload_fingerprint: fingerprint,
                result_json: encoded,
                outcome: "Committed".into(),
            },
            recorded_at,
        )
        .map_err(|err| db_fail(err, "insert_command"))?;
        hooks.inject(
            "db_commit",
            checkpoint_map(&command_id, len, Some(len), Some(&hash), true, false),
        )?;
        tx.commit().map_err(|err| db_fail(err, "db_commit"))?;
        result
    };

    hooks.stop(
        "after_db_commit",
        checkpoint_map(&command_id, len, Some(len), Some(&hash), true, true),
    )?;
    journal.state = "committed".into();
    journal.reason = None;
    write_journal(&session.root, journal)?;
    hooks.stop(
        "before_receipt",
        checkpoint_map(&command_id, len, Some(len), Some(&hash), true, true),
    )?;
    if hooks.hooks.inject_point.is_some() && !hooks.inject_reached {
        return Err(
            Fail::new(EXIT_FAULT_MISSED, "FaultNotReached", "inject_after_commit")
                .extra("injected", json!(true))
                .extra("preservation", json!("preserved")),
        );
    }
    Ok(result)
}

pub struct MarkerRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub source_id: String,
    pub value: String,
    pub hooks: Hooks,
}

pub fn set_marker(req: MarkerRequest) -> Report {
    match marker_inner(req) {
        Ok(body) => Report {
            code: EXIT_OK,
            body,
        },
        Err(fail) => fail.into_report(),
    }
}

fn marker_inner(req: MarkerRequest) -> Result<Value, Fail> {
    validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        MARKER_STOPS,
        MARKER_INJECTS,
    )?;
    if !is_command_id(&req.command_id) || !is_prefixed_id(&req.source_id, "src_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
    }
    if req.value.is_empty() || req.value.len() > 500 || req.value.contains(['\0', '\n', '\r']) {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_marker"));
    }
    let session = open_session(&req.vault)?;
    let fingerprint = fingerprint(&["marker", &req.source_id, &req.value]);
    if let Some(existing) = store::command(&session.conn, &req.command_id)
        .map_err(|err| db_fail(err, "command_lookup"))?
    {
        if req.hooks.inject_point.is_some() || req.hooks.stop_at.is_some() {
            return Err(Fail::new(
                EXIT_FAULT_MISSED,
                "FaultNotReached",
                "command_already_committed",
            )
            .extra("injected", json!(req.hooks.inject_point.is_some())));
        }
        if existing.payload_fingerprint != fingerprint {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "payload_conflict")
                .extra("command_id", json!(req.command_id))
                .extra("new_effect", json!(false)));
        }
        return already_committed(&existing);
    }
    let source = store::source_by_id(&session.conn, &req.source_id)
        .map_err(|err| db_fail(err, "source_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_REJECTED, "Rejected", "unknown_source")
                .extra("source_id", json!(req.source_id))
        })?;
    let mut journal = Journal {
        command_id: req.command_id.clone(),
        kind: "marker".into(),
        generation: 1,
        state: "started".into(),
        batch_id: None,
        display_name: None,
        external_path: None,
        media_type: None,
        occurrence_id: None,
        object_id: Some(source.object_id.clone()),
        content_sha256: Some(source.content_sha256.clone()),
        byte_length: Some(source.byte_length),
        payload_fingerprint: Some(fingerprint.clone()),
        storage_reused: None,
        reason: None,
        marker_value: Some(req.value.clone()),
        source_id: Some(req.source_id.clone()),
    };
    write_journal(&session.root, &journal)?;
    let mut hooks = HookRun::new(req.hooks.clone());
    hooks.stop(
        "before_db_commit",
        checkpoint_map(&req.command_id, 0, None, None, false, false),
    )?;
    ensure_precommit(&hooks)?;
    let recorded_at = now_ms();
    let result = {
        let tx = session
            .conn
            .unchecked_transaction()
            .map_err(|err| db_fail(err, "begin"))?;
        let updated = store::update_marker(&tx, &req.source_id, &req.value)
            .map_err(|err| db_fail(err, "update_marker"))?;
        if updated != 1 {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "unknown_source"));
        }
        let result = marker_result(&req, &source, recorded_at);
        store::insert_command(
            &tx,
            &CommandRow {
                command_id: req.command_id.clone(),
                kind: "marker".into(),
                payload_fingerprint: fingerprint,
                result_json: serde_json::to_string(&result)
                    .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?,
                outcome: "Committed".into(),
            },
            recorded_at,
        )
        .map_err(|err| db_fail(err, "insert_command"))?;
        hooks.inject(
            "db_commit",
            checkpoint_map(&req.command_id, 0, None, None, false, false),
        )?;
        tx.commit().map_err(|err| db_fail(err, "db_commit"))?;
        result
    };
    hooks.stop(
        "after_db_commit",
        checkpoint_map(&req.command_id, 0, None, None, false, true),
    )?;
    journal.state = "committed".into();
    write_journal(&session.root, &journal)?;
    hooks.stop(
        "before_receipt",
        checkpoint_map(&req.command_id, 0, None, None, false, true),
    )?;
    Ok(result)
}

pub struct ReadRequest {
    pub vault: PathBuf,
    pub source_id: String,
    pub out: PathBuf,
}

pub fn read_source(req: ReadRequest) -> Report {
    match read_inner(req) {
        Ok(body) => Report {
            code: EXIT_OK,
            body,
        },
        Err(fail) => fail.into_report(),
    }
}

fn read_inner(req: ReadRequest) -> Result<Value, Fail> {
    if !is_prefixed_id(&req.source_id, "src_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_source_id"));
    }
    let session = open_session(&req.vault)?;
    let source = store::source_by_id(&session.conn, &req.source_id)
        .map_err(|err| db_fail(err, "source_lookup"))?
        .ok_or_else(|| Fail::new(EXIT_REJECTED, "Rejected", "unknown_source"))?;
    let object = lookup_object(&session, &source.object_id)?;
    let relpath = canonical_object_relpath(&source.object_id).ok_or_else(|| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_id_rejected")
            .extra("source_id", json!(source.source_id))
    })?;
    let path_rejected = object.relpath != relpath;
    let file_path = session.root.join(&relpath);
    if !file_path.is_file() {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_missing")
                .extra("source_id", json!(source.source_id))
                .extra("object_id", json!(source.object_id)),
        );
    }
    let (hash, len) = sha256_file(&file_path).map_err(|err| io_fail(err, "hash_object"))?;
    if hash != source.content_sha256 || len as i64 != source.byte_length {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "hash_mismatch")
                .extra("source_id", json!(source.source_id))
                .extra("object_id", json!(source.object_id))
                .extra("expected_sha256", json!(source.content_sha256))
                .extra("actual_sha256", json!(hash)),
        );
    }
    if req.out.exists() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_exists"));
    }
    if let Some(parent) = req.out.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(Fail::new(
                EXIT_REJECTED,
                "Rejected",
                "output_parent_missing",
            ));
        }
    }
    fs::copy(&file_path, &req.out).map_err(|err| io_fail(err, "read_copy"))?;
    let mut body = honesty("Read");
    body.insert("source_id".into(), json!(source.source_id));
    body.insert("object_id".into(), json!(source.object_id));
    body.insert("content_sha256".into(), json!(hash));
    body.insert("hash_alg".into(), json!("sha256"));
    body.insert("byte_length".into(), json!(len));
    body.insert("preservation".into(), json!("preserved"));
    body.insert("arrangement".into(), json!("not_arranged"));
    body.insert("searchability".into(), json!("not_searchable"));
    body.insert("object_path_rejected".into(), json!(path_rejected));
    body.insert("new_effect".into(), json!(false));
    Ok(Value::Object(body))
}

pub struct CancelRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub generation: i64,
    pub kind: String,
}

pub fn cancel_command(req: CancelRequest) -> Report {
    match cancel_inner(req) {
        Ok(report) => report,
        Err(fail) => fail.into_report(),
    }
}

fn cancel_inner(req: CancelRequest) -> Result<Report, Fail> {
    if !is_command_id(&req.command_id) || req.generation < 1 {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_cancel"));
    }
    if req.kind == "extraction" {
        let session = open_session(&req.vault)?;
        let _session = session;
        return Err(
            Fail::new(EXIT_REJECTED, "Rejected", "extraction_cancel_not_available")
                .extra("preservation_unchanged", json!(true))
                .extra("cancel_applied", json!(false))
                .extra("kind", json!("extraction")),
        );
    }
    if req.kind != "preservation" {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_cancel_kind"));
    }
    require_vault_shape(&req.vault)?;
    let lock = match lockfile::VaultLock::try_acquire(&req.vault.join("writer.lock"))
        .map_err(|err| io_fail(err, "lock"))?
    {
        Some(lock) => lock,
        None => return cancel_without_lock(&req),
    };
    let _lock = lock;
    if !banner_exact(&req.vault)? {
        return Err(Fail::new(
            EXIT_NOT_SYNTHETIC,
            "NotSynthetic",
            "banner_mismatch",
        ));
    }
    let conn = open_writer(&req.vault)?;
    if let Some(existing) =
        store::command(&conn, &req.command_id).map_err(|err| db_fail(err, "command_lookup"))?
    {
        let generation = result_generation(&existing.result_json);
        if generation != req.generation {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "generation_mismatch")
                .extra("cancel_applied", json!(false)));
        }
        let mut body = already_committed(&existing)?;
        if let Some(map) = body.as_object_mut() {
            map.insert("cancel_applied".into(), json!(false));
            map.insert("outcome".into(), json!("AlreadyCommitted"));
        }
        return Ok(Report {
            code: EXIT_OK,
            body,
        });
    }
    let path = journal_path(&req.vault, &req.command_id);
    if !path.exists() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "unknown_command")
            .extra("cancel_applied", json!(false))
            .extra("preservation", json!("not_preserved")));
    }
    let mut journal = read_journal_file(&path).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("cancel_applied", json!(false))
    })?;
    if journal.generation != req.generation {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "generation_mismatch")
            .extra("cancel_applied", json!(false)));
    }
    journal.state = "aborted".into();
    journal.reason = Some("cancelled".into());
    write_journal(&req.vault, &journal)?;
    let _ = fs::remove_file(staging_path(&req.vault, &req.command_id));
    let _ = fs::remove_file(req.vault.join("control").join(&req.command_id));
    let mut body = honesty("Cancelled");
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("generation".into(), json!(req.generation));
    body.insert("cancel_applied".into(), json!(true));
    body.insert("preservation".into(), json!("not_preserved"));
    body.insert("arrangement".into(), json!("not_arranged"));
    body.insert("searchability".into(), json!("not_searchable"));
    body.insert("new_effect".into(), json!(false));
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

fn open_session(path: &Path) -> Result<Session, Fail> {
    require_vault_shape(path)?;
    if !banner_exact(path)? {
        return Err(
            Fail::new(EXIT_NOT_SYNTHETIC, "NotSynthetic", "banner_mismatch")
                .extra("created_new_vault", json!(false)),
        );
    }
    if !sqlite_header_ok(&path.join("vault.db"))? {
        return Err(Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "sqlite_header")
            .extra("created_new_vault", json!(false)));
    }
    let magic = filesystem_magic(path).map_err(|err| io_fail(err, "statfs"))?;
    if network_filesystem(magic) {
        return Err(unsupported_fs(magic));
    }
    inspect_version_readonly(path)?;
    let lock = lockfile::VaultLock::try_acquire(&path.join("writer.lock"))
        .map_err(|err| io_fail(err, "lock"))?
        .ok_or_else(|| {
            Fail::new(EXIT_LOCKED, "Locked", "writer_busy").extra("created_new_vault", json!(false))
        })?;
    let conn = open_writer(path)?;
    Ok(Session {
        root: path.to_path_buf(),
        _lock: lock,
        conn,
        magic,
    })
}

fn require_vault_shape(path: &Path) -> Result<(), Fail> {
    if !path.exists() {
        return Err(Fail::new(EXIT_NOT_VAULT, "NotAVault", "missing_path")
            .extra("created_new_vault", json!(false)));
    }
    if !path.is_dir() {
        return Err(Fail::new(EXIT_NOT_VAULT, "NotAVault", "not_a_directory")
            .extra("created_new_vault", json!(false)));
    }
    if !path.join("SYNTHETIC_ONLY").is_file() || !path.join("vault.db").is_file() {
        return Err(
            Fail::new(EXIT_NOT_VAULT, "NotAVault", "missing_vault_files")
                .extra("created_new_vault", json!(false)),
        );
    }
    Ok(())
}

fn banner_exact(path: &Path) -> Result<bool, Fail> {
    let bytes = fs::read(path.join("SYNTHETIC_ONLY")).map_err(|err| io_fail(err, "banner"))?;
    Ok(bytes == VAULT_BANNER.as_bytes())
}

fn sqlite_header_ok(path: &Path) -> Result<bool, Fail> {
    let mut file = File::open(path).map_err(|err| io_fail(err, "sqlite_header"))?;
    let mut buf = [0u8; 16];
    let n = file
        .read(&mut buf)
        .map_err(|err| io_fail(err, "sqlite_header"))?;
    Ok(n == 16 && &buf == b"SQLite format 3\0")
}

fn inspect_version_readonly(path: &Path) -> Result<(), Fail> {
    let uri = readonly_uri(&path.join("vault.db"))?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "sqlite_unreadable")
            .extra("created_new_vault", json!(false))
    })?;
    let version = store::user_version(&conn).map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "user_version_unreadable")
            .extra("created_new_vault", json!(false))
    })?;
    if version > 1 {
        return Err(
            Fail::new(EXIT_VERSION, "VersionUnsupported", "newer_schema")
                .extra("schema_version_found", json!(version))
                .extra("created_new_vault", json!(false)),
        );
    }
    if version != 1 {
        return Err(
            Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "unexpected_user_version")
                .extra("schema_version_found", json!(version))
                .extra("created_new_vault", json!(false)),
        );
    }
    let schema = store::meta(&conn, "schema_version").map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "meta_unreadable")
            .extra("created_new_vault", json!(false))
    })?;
    if schema.as_deref() != Some("1") {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "schema_mismatch")
                .extra("created_new_vault", json!(false)),
        );
    }
    Ok(())
}

fn readonly_uri(path: &Path) -> Result<String, Fail> {
    let abs = path
        .canonicalize()
        .map_err(|err| io_fail(err, "canonicalize"))?;
    let text = abs.to_string_lossy();
    if text.contains('?') || text.contains('#') {
        return Err(Fail::new(
            EXIT_REJECTED,
            "Rejected",
            "vault_path_unsupported",
        ));
    }
    Ok(format!("file:{text}?mode=ro&immutable=1"))
}

fn open_writer(path: &Path) -> Result<Connection, Fail> {
    let conn = Connection::open(path.join("vault.db")).map_err(|err| db_fail(err, "open"))?;
    store::configure(&conn).map_err(|err| db_fail(err, "pragma"))?;
    Ok(conn)
}

fn inspect(session: &Session) -> Result<Report, Fail> {
    let sources = store::list_sources(&session.conn).map_err(|err| db_fail(err, "list_sources"))?;
    let objects = store::list_objects(&session.conn).map_err(|err| db_fail(err, "list_objects"))?;
    let occurrences =
        store::list_occurrences(&session.conn).map_err(|err| db_fail(err, "list_occurrences"))?;
    let commands =
        store::list_commands(&session.conn).map_err(|err| db_fail(err, "list_commands"))?;
    let journals = scan_journals(&session.root)?;
    let mut alerts = Vec::new();
    let mut dangling = Vec::new();
    for name in &journals.unexpected {
        alerts.push(json!({"code": "unexpected_journal_name", "name": name}));
    }
    for name in &journals.invalid {
        alerts.push(json!({"code": "journal_invalid", "name": name}));
    }
    let db_object_ids: std::collections::BTreeSet<&str> =
        objects.iter().map(|o| o.object_id.as_str()).collect();
    let mut unreferenced = Vec::new();
    let object_dir = session.root.join("objects");
    if object_dir.is_dir() {
        let mut names = Vec::new();
        for entry in fs::read_dir(&object_dir).map_err(|err| io_fail(err, "read_objects"))? {
            let entry = entry.map_err(|err| io_fail(err, "read_objects"))?;
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        names.sort();
        for name in names {
            if !is_prefixed_id(&name, "obj_") {
                alerts.push(json!({"code": "unexpected_object_name", "name": name}));
                continue;
            }
            if !db_object_ids.contains(name.as_str()) {
                let path = object_dir.join(&name);
                let (hash, len) = sha256_file(&path).map_err(|err| io_fail(err, "hash_orphan"))?;
                unreferenced.push(json!({
                    "object_id": name,
                    "content_sha256": hash,
                    "hash_alg": "sha256",
                    "byte_length": len,
                    "object_relpath": format!("objects/{name}")
                }));
            }
        }
    }
    let mut source_json = Vec::new();
    for source in &sources {
        let mut intact = false;
        let mut relpath = String::new();
        if !is_prefixed_id(&source.object_id, "obj_") {
            alerts.push(json!({"code": "object_id_rejected", "source_id": source.source_id, "object_id": source.object_id}));
            dangling.push(json!({"source_id": source.source_id, "object_id": source.object_id, "reason": "object_id_rejected"}));
        } else {
            relpath = format!("objects/{}", source.object_id);
            if let Some(object) = objects.iter().find(|o| o.object_id == source.object_id) {
                if object.relpath != relpath {
                    alerts.push(json!({"code": "object_path_rejected", "source_id": source.source_id, "object_id": source.object_id}));
                }
            }
            let path = session.root.join(&relpath);
            if !path.is_file() {
                alerts.push(json!({"code": "object_missing", "source_id": source.source_id, "object_id": source.object_id}));
                dangling.push(json!({"source_id": source.source_id, "object_id": source.object_id, "reason": "object_missing"}));
            } else {
                let (hash, len) = sha256_file(&path).map_err(|err| io_fail(err, "hash_object"))?;
                if hash != source.content_sha256 || len as i64 != source.byte_length {
                    alerts.push(json!({
                        "code": "hash_mismatch",
                        "source_id": source.source_id,
                        "object_id": source.object_id,
                        "expected_sha256": source.content_sha256,
                        "actual_sha256": hash
                    }));
                    dangling.push(json!({"source_id": source.source_id, "object_id": source.object_id, "reason": "hash_mismatch"}));
                } else {
                    intact = true;
                }
            }
        }
        source_json.push(json!({
            "source_id": source.source_id,
            "object_id": source.object_id,
            "content_sha256": source.content_sha256,
            "hash_alg": "sha256",
            "byte_length": source.byte_length,
            "media_type": source.media_type,
            "user_marker": source.user_marker,
            "preservation": "preserved",
            "arrangement": "not_arranged",
            "searchability": "not_searchable",
            "purchase_id": null,
            "entitlement_id": null,
            "object_relpath": relpath,
            "object_intact": intact
        }));
    }
    let command_ids: std::collections::BTreeSet<&str> =
        commands.iter().map(|c| c.command_id.as_str()).collect();
    let mut incomplete = Vec::new();
    for journal in &journals.valid {
        if command_ids.contains(journal.command_id.as_str()) {
            continue;
        }
        incomplete.push(json!({
            "command_id": journal.command_id,
            "kind": journal.kind,
            "state": journal.state,
            "reason": journal.reason,
            "content_sha256": journal.content_sha256,
            "byte_length": journal.byte_length,
            "preservation": "not_preserved",
            "source_id": null,
            "purchase_id": null
        }));
    }
    let invalid_journal = !journals.invalid.is_empty() || !journals.unexpected.is_empty();
    let (code, outcome) = if !dangling.is_empty() || invalid_journal {
        (EXIT_RECOVERY, "RecoveryRequired")
    } else if !alerts.is_empty() {
        (EXIT_INTEGRITY, "IntegrityAlert")
    } else {
        (EXIT_OK, "Ready")
    };
    let vault_id = store::meta(&session.conn, "vault_id")
        .map_err(|err| db_fail(err, "meta"))?
        .unwrap_or_default();
    let mut body = honesty(outcome);
    body.insert("vault_id".into(), json!(vault_id));
    body.insert("disposable".into(), json!(true));
    body.insert("protection".into(), json!("unanswered_synthetic_plaintext"));
    body.insert("primary_input".into(), json!("unanswered"));
    body.insert("schema_version".into(), json!(1));
    body.insert("capabilities".into(), capabilities());
    body.insert(
        "platform_limits".into(),
        platform_limits(Some(session.magic)),
    );
    body.insert("has_incomplete".into(), json!(!incomplete.is_empty()));
    body.insert("sources".into(), Value::Array(source_json));
    body.insert(
        "occurrences".into(),
        Value::Array(
            occurrences
                .iter()
                .map(|row| {
                    json!({
                        "occurrence_id": row.occurrence_id,
                        "source_id": row.source_id,
                        "command_id": row.command_id,
                        "batch_id": row.batch_id,
                        "display_name": row.display_name,
                        "external_path": row.external_path,
                        "storage_reused": row.storage_reused,
                        "recorded_at": row.recorded_at
                    })
                })
                .collect(),
        ),
    );
    body.insert(
        "commands".into(),
        Value::Array(
            commands
                .iter()
                .map(|row| json!({"command_id": row.command_id, "kind": row.kind, "outcome": row.outcome}))
                .collect(),
        ),
    );
    body.insert("incomplete_commands".into(), Value::Array(incomplete));
    body.insert("unreferenced_objects".into(), Value::Array(unreferenced));
    body.insert("dangling_references".into(), Value::Array(dangling));
    body.insert("alerts".into(), Value::Array(alerts));
    body.insert(
        "temporary_file_count".into(),
        json!(count_staging(&session.root)),
    );
    body.insert("created_new_vault".into(), json!(false));
    Ok(Report {
        code,
        body: Value::Object(body),
    })
}

struct JournalScan {
    valid: Vec<Journal>,
    invalid: Vec<String>,
    unexpected: Vec<String>,
}

fn scan_journals(root: &Path) -> Result<JournalScan, Fail> {
    let mut scan = JournalScan {
        valid: Vec::new(),
        invalid: Vec::new(),
        unexpected: Vec::new(),
    };
    let dir = root.join("journal");
    if !dir.exists() {
        return Ok(scan);
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| io_fail(err, "read_journal"))? {
        let entry = entry.map_err(|err| io_fail(err, "read_journal"))?;
        names.push(entry.file_name().to_string_lossy().to_string());
    }
    names.sort();
    for name in names {
        if name.ends_with(".json.tmp") || name.starts_with('.') {
            continue;
        }
        if !name.ends_with(".json") {
            scan.unexpected.push(name);
            continue;
        }
        let command_id = &name[..name.len() - 5];
        if !is_command_id(command_id) {
            scan.unexpected.push(name);
            continue;
        }
        let path = dir.join(&name);
        match read_journal_file(&path) {
            Ok(journal) if journal.command_id == command_id => scan.valid.push(journal),
            _ => scan.invalid.push(name),
        }
    }
    Ok(scan)
}

fn reconcile(session: &Session) -> Result<(), Fail> {
    let scan = scan_journals(&session.root)?;
    for journal in scan.valid {
        if journal.state == "committed" {
            continue;
        }
        if store::command(&session.conn, &journal.command_id)
            .map_err(|err| db_fail(err, "reconcile"))?
            .is_some()
        {
            let mut journal = journal;
            journal.state = "committed".into();
            journal.reason = None;
            write_journal(&session.root, &journal)?;
        }
    }
    Ok(())
}

fn clean_temps(root: &Path) -> Result<(), Fail> {
    let staging = root.join("staging");
    if staging.is_dir() {
        for entry in fs::read_dir(&staging).map_err(|err| io_fail(err, "read_staging"))? {
            let entry = entry.map_err(|err| io_fail(err, "read_staging"))?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|err| io_fail(err, "staging_meta"))?;
            if meta.is_file() || meta.file_type().is_symlink() {
                fs::remove_file(&path).map_err(|err| io_fail(err, "remove_staging"))?;
            }
        }
    }
    let journal = root.join("journal");
    if journal.is_dir() {
        for entry in fs::read_dir(&journal).map_err(|err| io_fail(err, "read_journal"))? {
            let entry = entry.map_err(|err| io_fail(err, "read_journal"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json.tmp") || (name.starts_with('.') && name.ends_with(".tmp")) {
                let meta =
                    fs::symlink_metadata(entry.path()).map_err(|err| io_fail(err, "tmp_meta"))?;
                if meta.is_file() || meta.file_type().is_symlink() {
                    fs::remove_file(entry.path()).map_err(|err| io_fail(err, "remove_tmp"))?;
                }
            }
        }
    }
    Ok(())
}

fn count_staging(root: &Path) -> i64 {
    let staging = root.join("staging");
    let Ok(rd) = fs::read_dir(staging) else {
        return 0;
    };
    rd.filter_map(|e| e.ok()).count() as i64
}

fn load_or_new_journal(
    root: &Path,
    req: &ImportRequest,
    display_name: &str,
) -> Result<Journal, Fail> {
    let path = journal_path(root, &req.command_id);
    if path.exists() {
        let journal = read_journal_file(&path).map_err(|_| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
                .extra("command_id", json!(req.command_id))
        })?;
        if journal.command_id != req.command_id {
            return Err(Fail::new(
                EXIT_RECOVERY,
                "RecoveryRequired",
                "journal_invalid",
            ));
        }
        return Ok(journal);
    }
    Ok(Journal {
        command_id: req.command_id.clone(),
        kind: "import".into(),
        generation: 1,
        state: "started".into(),
        batch_id: req.batch_id.clone(),
        display_name: Some(display_name.to_string()),
        external_path: Some(req.source.display().to_string()),
        media_type: Some(req.media_type.clone()),
        occurrence_id: None,
        object_id: None,
        content_sha256: None,
        byte_length: None,
        payload_fingerprint: None,
        storage_reused: None,
        reason: None,
        marker_value: None,
        source_id: None,
    })
}

fn write_journal(root: &Path, journal: &Journal) -> Result<(), Fail> {
    let path = journal_path(root, &journal.command_id);
    let value = json!({
        "command_id": journal.command_id,
        "kind": journal.kind,
        "generation": journal.generation,
        "state": journal.state,
        "batch_id": journal.batch_id,
        "display_name": journal.display_name,
        "external_path": journal.external_path,
        "media_type": journal.media_type,
        "occurrence_id": journal.occurrence_id,
        "object_id": journal.object_id,
        "content_sha256": journal.content_sha256,
        "byte_length": journal.byte_length,
        "payload_fingerprint": journal.payload_fingerprint,
        "storage_reused": journal.storage_reused,
        "reason": journal.reason,
        "marker_value": journal.marker_value,
        "source_id": journal.source_id
    });
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "journal_encode"))?;
    atomic_write(&path, &bytes).map_err(|err| io_fail(err, "journal_write"))
}

fn read_journal_file(path: &Path) -> Result<Journal, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let command_id = value
        .get("command_id")
        .and_then(|v| v.as_str())
        .ok_or(())?
        .to_string();
    if !is_command_id(&command_id) {
        return Err(());
    }
    let kind = value
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or(())?
        .to_string();
    if kind != "import" && kind != "marker" {
        return Err(());
    }
    let generation = value.get("generation").and_then(|v| v.as_i64()).ok_or(())?;
    if generation < 1 {
        return Err(());
    }
    let state = value
        .get("state")
        .and_then(|v| v.as_str())
        .ok_or(())?
        .to_string();
    if !matches!(
        state.as_str(),
        "started" | "copying" | "object_ready" | "aborted" | "committed"
    ) {
        return Err(());
    }
    let object_id = optional_string(&value, "object_id")?;
    if let Some(id) = &object_id {
        if !is_prefixed_id(id, "obj_") {
            return Err(());
        }
    }
    let occurrence_id = optional_string(&value, "occurrence_id")?;
    if let Some(id) = &occurrence_id {
        if !is_prefixed_id(id, "occ_") {
            return Err(());
        }
    }
    let source_id = optional_string(&value, "source_id")?;
    if let Some(id) = &source_id {
        if !is_prefixed_id(id, "src_") {
            return Err(());
        }
    }
    let content_sha256 = optional_string(&value, "content_sha256")?;
    if let Some(hash) = &content_sha256 {
        if !is_lower_hex(hash, 64) {
            return Err(());
        }
    }
    Ok(Journal {
        command_id,
        kind,
        generation,
        state,
        batch_id: optional_string(&value, "batch_id")?,
        display_name: optional_string(&value, "display_name")?,
        external_path: optional_string(&value, "external_path")?,
        media_type: optional_string(&value, "media_type")?,
        occurrence_id,
        object_id,
        content_sha256,
        byte_length: match value.get("byte_length") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => Some(n.as_i64().ok_or(())?),
            _ => return Err(()),
        },
        payload_fingerprint: optional_string(&value, "payload_fingerprint")?,
        storage_reused: match value.get("storage_reused") {
            None | Some(Value::Null) => None,
            Some(Value::Bool(v)) => Some(*v),
            _ => return Err(()),
        },
        reason: optional_string(&value, "reason")?,
        marker_value: optional_string(&value, "marker_value")?,
        source_id,
    })
}

fn optional_string(value: &Value, key: &str) -> Result<Option<String>, ()> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(()),
    }
}

fn copy_source(
    src: &Path,
    dest: &Path,
    hooks: &mut HookRun,
    root: &Path,
    command_id: &str,
    generation: i64,
    expected_len: u64,
) -> Result<(), Fail> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| io_fail(err, "staging_dir"))?;
    }
    let mut input = File::open(src).map_err(|err| io_fail(err, "read_source"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|err| io_fail(err, "create_staging"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    let mut paused = hooks.hooks.stop_at.as_deref() != Some("during_copy");
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|err| io_fail(err, "read_source"))?;
        if n == 0 {
            break;
        }
        if !paused && n > 1 {
            output
                .write_all(&buf[..n - 1])
                .map_err(|err| io_fail(err, "write_staging"))?;
            output
                .flush()
                .map_err(|err| io_fail(err, "flush_staging"))?;
            written += (n as u64) - 1;
            hooks.stop(
                "during_copy",
                checkpoint_map(command_id, written, Some(expected_len), None, false, false),
            )?;
            if take_cancel(root, command_id, generation)? {
                return Err(cancelled());
            }
            output
                .write_all(&buf[n - 1..n])
                .map_err(|err| io_fail(err, "write_staging"))?;
            written += 1;
            paused = true;
            continue;
        }
        output
            .write_all(&buf[..n])
            .map_err(|err| io_fail(err, "write_staging"))?;
        written += n as u64;
        paused = true;
    }
    if !paused {
        hooks.stop(
            "during_copy",
            checkpoint_map(command_id, written, Some(expected_len), None, false, false),
        )?;
    }
    output
        .sync_all()
        .map_err(|err| io_fail(err, "staging_fsync"))?;
    if let Some(parent) = dest.parent() {
        fsutil::fsync_dir(parent).map_err(|err| io_fail(err, "staging_dir_fsync"))?;
    }
    Ok(())
}

fn find_orphan_with_hash(
    root: &Path,
    conn: &Connection,
    hash: &str,
) -> Result<Option<String>, Fail> {
    let objects = store::list_objects(conn).map_err(|err| db_fail(err, "list_objects"))?;
    let known: std::collections::BTreeSet<String> =
        objects.into_iter().map(|o| o.object_id).collect();
    let dir = root.join("objects");
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| io_fail(err, "read_objects"))? {
        let entry = entry.map_err(|err| io_fail(err, "read_objects"))?;
        names.push(entry.file_name().to_string_lossy().to_string());
    }
    names.sort();
    for name in names {
        if !is_prefixed_id(&name, "obj_") || known.contains(&name) {
            continue;
        }
        let (file_hash, _) =
            sha256_file(&dir.join(&name)).map_err(|err| io_fail(err, "hash_orphan"))?;
        if file_hash == hash {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn verify_referenced_file(root: &Path, object: &ObjectRow) -> Result<(), &'static str> {
    let Some(relpath) = canonical_object_relpath(&object.object_id) else {
        return Err("object_id_rejected");
    };
    let path = root.join(relpath);
    if !path.is_file() {
        return Err("object_missing");
    }
    let (hash, len) = sha256_file(&path).map_err(|_| "hash_mismatch")?;
    if hash != object.content_sha256 || len as i64 != object.byte_length {
        return Err("hash_mismatch");
    }
    Ok(())
}

fn lookup_object(session: &Session, object_id: &str) -> Result<ObjectRow, Fail> {
    let objects = store::list_objects(&session.conn).map_err(|err| db_fail(err, "list_objects"))?;
    objects
        .into_iter()
        .find(|o| o.object_id == object_id)
        .ok_or_else(|| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_row_missing"))
}

fn unwind(root: &Path, journal: &mut Journal, phase: &Phase, fail: Fail) -> Result<Fail, Fail> {
    // Injected failures leave the copied temp so a test can see the bytes that were
    // written before the fault. open/recover removes staging. Cancel still removes it.
    if !fail.reason.starts_with("injected_") {
        let _ = fs::remove_file(staging_path(root, &journal.command_id));
    }
    if journal.state != "committed" {
        if fail.outcome == "Cancelled" || !phase.published {
            journal.state = "aborted".into();
        } else {
            journal.state = "object_ready".into();
        }
        journal.reason = Some(fail.reason.clone());
        write_journal(root, journal)?;
    }
    Ok(fail)
}

fn take_cancel(root: &Path, command_id: &str, generation: i64) -> Result<bool, Fail> {
    let path = root.join("control").join(command_id);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(io_fail(err, "control_read")),
    };
    let mut cancel = false;
    let mut seen_generation = None;
    let mut kind = None;
    for line in text.lines() {
        if line == "cancel" {
            cancel = true;
        } else if let Some(value) = line.strip_prefix("generation=") {
            seen_generation = value.parse::<i64>().ok();
        } else if let Some(value) = line.strip_prefix("kind=") {
            kind = Some(value.to_string());
        }
    }
    if !cancel
        || seen_generation != Some(generation)
        || kind.as_deref().unwrap_or("preservation") != "preservation"
    {
        return Ok(false);
    }
    let _ = fs::remove_file(&path);
    Ok(true)
}

fn write_control(root: &Path, command_id: &str, generation: i64) -> Result<(), Fail> {
    fs::create_dir_all(root.join("control")).map_err(|err| io_fail(err, "control_dir"))?;
    let body = format!("cancel\ngeneration={generation}\nkind=preservation\n");
    atomic_write(&root.join("control").join(command_id), body.as_bytes())
        .map_err(|err| io_fail(err, "control_write"))
}

fn result_generation(json_text: &str) -> i64 {
    serde_json::from_str::<Value>(json_text)
        .ok()
        .and_then(|v| v.get("generation").and_then(|g| g.as_i64()))
        .unwrap_or(1)
}

fn already_committed(row: &CommandRow) -> Result<Value, Fail> {
    let mut value: Value = serde_json::from_str(&row.result_json).map_err(|_| {
        Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "command_result_unreadable",
        )
    })?;
    if let Some(map) = value.as_object_mut() {
        map.insert("outcome".into(), json!("AlreadyCommitted"));
        map.insert("new_effect".into(), json!(false));
        map.insert("user_ready".into(), json!(false));
        map.insert("security_complete".into(), json!(false));
        map.insert("purchase_id".into(), Value::Null);
        map.insert("entitlement_id".into(), Value::Null);
    }
    Ok(value)
}

fn import_result(
    command_id: &str,
    source: &SourceRow,
    occurrence_id: &str,
    storage_reused: bool,
    journal: &Journal,
) -> Value {
    let mut body = honesty("Committed");
    body.insert("new_effect".into(), json!(true));
    body.insert("preservation".into(), json!("preserved"));
    body.insert("arrangement".into(), json!("not_arranged"));
    body.insert("searchability".into(), json!("not_searchable"));
    body.insert("command_id".into(), json!(command_id));
    body.insert("generation".into(), json!(1));
    body.insert("batch_id".into(), json!(journal.batch_id));
    body.insert("source_id".into(), json!(source.source_id));
    body.insert("object_id".into(), json!(source.object_id));
    body.insert("occurrence_id".into(), json!(occurrence_id));
    body.insert("content_sha256".into(), json!(source.content_sha256));
    body.insert("hash_alg".into(), json!("sha256"));
    body.insert("byte_length".into(), json!(source.byte_length));
    body.insert("storage_reused".into(), json!(storage_reused));
    body.insert("media_type".into(), json!(source.media_type));
    body.insert("display_name".into(), json!(journal.display_name));
    body.insert("user_marker".into(), json!(source.user_marker));
    body.insert("injected".into(), json!(false));
    Value::Object(body)
}

fn marker_result(req: &MarkerRequest, source: &SourceRow, recorded_at: i64) -> Value {
    let mut body = honesty("Committed");
    body.insert("new_effect".into(), json!(true));
    body.insert("kind".into(), json!("marker"));
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("generation".into(), json!(1));
    body.insert("source_id".into(), json!(req.source_id));
    body.insert("user_marker".into(), json!(req.value));
    body.insert("previous_user_marker".into(), json!(source.user_marker));
    body.insert("content_sha256".into(), json!(source.content_sha256));
    body.insert("byte_length".into(), json!(source.byte_length));
    body.insert("preservation".into(), json!("preserved"));
    body.insert("arrangement".into(), json!("not_arranged"));
    body.insert("searchability".into(), json!("not_searchable"));
    body.insert("recorded_at".into(), json!(recorded_at));
    body.insert(
        "marker_role".into(),
        json!("committed metadata observation point, not a subscription fact"),
    );
    Value::Object(body)
}

fn cancelled() -> Fail {
    Fail::new(EXIT_OK, "Cancelled", "cancelled")
        .extra("preservation", json!("not_preserved"))
        .extra("arrangement", json!("not_arranged"))
        .extra("searchability", json!("not_searchable"))
        .extra("cancel_applied", json!(true))
        .extra("new_effect", json!(false))
        .extra("purchase_id", Value::Null)
}

fn ensure_precommit(hooks: &HookRun) -> Result<(), Fail> {
    if let Some(point) = &hooks.hooks.stop_at {
        if IMPORT_STOPS.contains(&point.as_str())
            && point != "after_db_commit"
            && point != "before_receipt"
            && !hooks.stop_reached
        {
            return Err(
                Fail::new(EXIT_STOP_MISSED, "StopPointNotReached", point.clone())
                    .extra("preservation", json!("not_preserved"))
                    .extra("new_effect", json!(false)),
            );
        }
    }
    if let Some(point) = &hooks.hooks.inject_point {
        if (point == "after_staging_fsync" || point == "object_fsync") && !hooks.inject_reached {
            return Err(
                Fail::new(EXIT_FAULT_MISSED, "FaultNotReached", point.clone())
                    .extra("injected", json!(true))
                    .extra("preservation", json!("not_preserved"))
                    .extra("new_effect", json!(false)),
            );
        }
    }
    Ok(())
}

fn validate_hook_names(
    stop: Option<&str>,
    inject: Option<&str>,
    stops: &[&str],
    injects: &[&str],
) -> Result<(), Fail> {
    if let Some(point) = stop {
        if !stops.contains(&point) {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "unknown_stop_point").extra("point", json!(point))
            );
        }
    }
    if let Some(point) = inject {
        if !injects.contains(&point)
            && !IMPORT_INJECTS.contains(&point)
            && !MARKER_INJECTS.contains(&point)
        {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "unknown_inject_point").extra("point", json!(point))
            );
        }
        if !injects.contains(&point) {
            return Err(Fail::new(EXIT_FAULT_MISSED, "FaultNotReached", point)
                .extra("injected", json!(true))
                .extra("preservation", json!("not_preserved"))
                .extra("new_effect", json!(false)));
        }
    }
    Ok(())
}

fn checkpoint_map(
    command_id: &str,
    bytes_written: u64,
    byte_length: Option<u64>,
    hash: Option<&str>,
    published: bool,
    db_committed: bool,
) -> Map<String, Value> {
    map_of(&[
        ("command_id", json!(command_id)),
        ("bytes_written", json!(bytes_written)),
        ("byte_length", json!(byte_length)),
        ("content_sha256", json!(hash)),
        ("published", json!(published)),
        ("db_committed", json!(db_committed)),
    ])
}

fn map_of(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([0xff]);
        }
        hasher.update(part.as_bytes());
    }
    hex_encode(&hasher.finalize())
}

fn journal_path(root: &Path, command_id: &str) -> PathBuf {
    root.join("journal").join(format!("{command_id}.json"))
}

fn staging_path(root: &Path, command_id: &str) -> PathBuf {
    root.join("staging").join(format!("{command_id}.partial"))
}

fn display_name_fallback() -> String {
    "source".into()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn io_fail(err: io::Error, during: &str) -> Fail {
    let enospc = err.raw_os_error() == Some(libc::ENOSPC);
    Fail::new(
        EXIT_NOT_PRESERVED,
        "NotPreserved",
        if enospc { "enospc" } else { "io_error" },
    )
    .extra("injected", json!(false))
    .extra("during", json!(during))
    .extra("os_error", json!(err.to_string()))
    .extra("preservation", json!("not_preserved"))
    .extra("new_effect", json!(false))
}

fn db_fail(err: rusqlite::Error, during: &str) -> Fail {
    let full = matches!(
        &err,
        rusqlite::Error::SqliteFailure(ffi, _) if ffi.code == rusqlite::ErrorCode::DiskFull
    );
    Fail::new(
        EXIT_NOT_PRESERVED,
        "NotPreserved",
        if full { "enospc" } else { "db_error" },
    )
    .extra("injected", json!(false))
    .extra("during", json!(during))
    .extra("os_error", json!(err.to_string()))
    .extra("preservation", json!("not_preserved"))
    .extra("new_effect", json!(false))
}

fn cancel_without_lock(req: &CancelRequest) -> Result<Report, Fail> {
    if let Some(row) = read_committed(&req.vault, &req.command_id)? {
        let generation = result_generation(&row.result_json);
        if generation != req.generation {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "generation_mismatch")
                .extra("cancel_applied", json!(false)));
        }
        let mut body = already_committed(&row)?;
        if let Some(map) = body.as_object_mut() {
            map.insert("cancel_applied".into(), json!(false));
        }
        return Ok(Report {
            code: EXIT_OK,
            body,
        });
    }
    let path = journal_path(&req.vault, &req.command_id);
    if !path.exists() {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "unknown_command")
            .extra("cancel_applied", json!(false))
            .extra("preservation", json!("not_preserved")));
    }
    let journal = read_journal_file(&path).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("cancel_applied", json!(false))
    })?;
    if journal.generation != req.generation {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "generation_mismatch")
            .extra("cancel_applied", json!(false)));
    }
    write_control(&req.vault, &req.command_id, req.generation)?;
    let mut body = honesty("CancelPending");
    body.insert("command_id".into(), json!(req.command_id));
    body.insert("generation".into(), json!(req.generation));
    body.insert("cancel_applied".into(), json!(false));
    body.insert("preservation".into(), json!("not_preserved"));
    body.insert("new_effect".into(), json!(false));
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

fn read_committed(root: &Path, command_id: &str) -> Result<Option<CommandRow>, Fail> {
    if !root.join("vault.db").is_file() || !sqlite_header_ok(&root.join("vault.db"))? {
        return Ok(None);
    }
    let uri = readonly_uri(&root.join("vault.db"))?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "sqlite_unreadable"))?;
    store::command(&conn, command_id).map_err(|err| db_fail(err, "command_lookup"))
}

fn unsupported_fs(magic: i64) -> Fail {
    Fail::new(
        EXIT_UNSUPPORTED_FS,
        "UnsupportedFilesystem",
        "network_filesystem",
    )
    .extra("filesystem_magic", json!(format!("0x{magic:x}")))
    .extra("created_new_vault", json!(false))
}
