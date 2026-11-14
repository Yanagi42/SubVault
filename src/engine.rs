use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::facts::{self, ObjectFacts};
use crate::fsutil::{
    self, atomic_write, filesystem_magic, network_filesystem, sha256_file, MAX_OBJECT_BYTES,
};
use crate::ids::{
    self, canonical_object_relpath, hex_encode, is_command_id, is_display_name, is_media_type,
    is_prefixed_id,
};
use crate::journal::{
    self, path as journal_path, read_file as read_journal_file, staging_path,
    write as write_journal, Journal,
};
use crate::lockfile::{self, sigstop, VaultLock, LOCK_TEXT};
use crate::model::{
    capabilities, db_fail, honesty, io_fail, platform_limits, unseen_db, unseen_io, Fail, Report,
    EXIT_DAMAGED, EXIT_FAULT_MISSED, EXIT_INTEGRITY, EXIT_LOCKED, EXIT_NOT_PRESERVED,
    EXIT_NOT_SYNTHETIC, EXIT_NOT_VAULT, EXIT_OK, EXIT_RECOVERY, EXIT_REJECTED, EXIT_STOP_MISSED,
    EXIT_UNCERTAIN, EXIT_UNSUPPORTED_FS, EXIT_USAGE, EXIT_VERSION, NOT_ARRANGED, NOT_PRESERVED,
    NOT_SEARCHABLE, PRESERVED, VAULT_BANNER,
};
use crate::store::{self, CommandRow, ObjectRow, OccurrenceRow, SourceRow};

#[path = "decide.rs"]
mod decide;
#[path = "export.rs"]
mod export;
#[path = "fragment.rs"]
mod fragment;

pub use decide::{correct_fact, query_facts, undo_decision, CorrectRequest, UndoRequest};
pub use export::{export_copy, ExportRequest};
pub use fragment::{query_candidates, submit_fragment, FragmentRequest};

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
const RECOGNIZED_INJECTS: &[&str] = &["after_staging_fsync", "object_fsync", "db_commit"];
const INIT_STOPS: &[&str] = &[
    "version_set",
    "version_checkpoint",
    "before_publish",
    "after_publish",
];
const INIT_INJECTS: &[&str] = &["version_checkpoint", "version_verify"];

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
            .keep_staging()
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
            .extra("preservation", json!(NOT_PRESERVED)))
    }
}

struct Session {
    root: PathBuf,
    _lock: VaultLock,
    conn: Connection,
    magic: i64,
}

struct Phase {
    published: bool,
}

pub fn help_report() -> Report {
    let mut body = honesty("Help");
    body.insert(
        "note".into(),
        json!("T1 preserves original bytes in a synthetic vault. It does not arrange, search, or accept real data. fragment publishes source-stated candidates for one user-submitted text fragment. On a new format 3 vault, that same publish also records a stated paid amount and a stated service period as facts. Those facts are source statements, not verified payments, purchases, or account links. correct can replace the stated USD amount text, and undo appends a compensating decision. export writes one offline package outside the vault: originals, candidates, current facts, and the decision chain. It does not choose a protection mode. PDF extraction is not available. Security is not complete. This is not a user-ready product."),
    );
    body.insert(
        "commands".into(),
        json!([
            "help",
            "version",
            "init --vault PATH [--format 1|2|3]",
            "open --vault PATH",
            "verify --vault PATH",
            "import --vault PATH --command-id ID --media-type TYPE --source PATH [--batch-id ID] [--display-name NAME]",
            "read --vault PATH --source-id ID --out PATH",
            "marker --vault PATH --command-id ID --source-id ID --value TEXT",
            "fragment --vault PATH --command-id ID --fragment PATH [--batch-id ID]",
            "candidates --vault PATH [--source-id ID]",
            "facts --vault PATH [--source-id ID] [--fact-id ID]",
            "export --vault PATH --out PATH [--allow-partial yes]",
            "correct --vault PATH --command-id ID --fact-id ID --expected-revision N --amount-text TEXT --actor-id ID",
            "undo --vault PATH --command-id ID --decision-id ID --expected-revision N --actor-id ID",
            "cancel --vault PATH --command-id ID --generation N [--kind preservation|fragment|extraction]",
            "recover --vault PATH",
            "hold --vault PATH --stop-at lock_held --checkpoint PATH"
        ]),
    );
    body.insert(
        "test_hooks".into(),
        json!("--stop-at and --inject are explicit test hooks. A missed hook does not report success. --inject never means the disk or device really failed."),
    );
    body.insert(
        "exit_codes".into(),
        json!({
            "20": EXIT_STOP_MISSED,
            "21": EXIT_UNCERTAIN
        }),
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
    body.insert("supported_vault_formats".into(), json!([1, 2, 3]));
    body.insert("capabilities".into(), capabilities());
    body.insert("platform_limits".into(), platform_limits(None));
    Report {
        code: EXIT_OK,
        body: Value::Object(body),
    }
}

pub fn init_vault(path: &Path, format: u8, hooks: Hooks) -> Report {
    match init_inner(path, format, hooks) {
        Ok(report) => report,
        Err(fail) => note_uncommitted(fail).into_report(),
    }
}

fn init_inner(path: &Path, format: u8, hooks: Hooks) -> Result<Report, Fail> {
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
    if format == 1 {
        if hooks.stop_at.is_some() || hooks.inject_point.is_some() {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "format_1_has_no_version_hook")
                    .extra("created_new_vault", json!(false)),
            );
        }
    } else if format == 2 || format == 3 {
        validate_hook_names(
            hooks.stop_at.as_deref(),
            hooks.inject_point.as_deref(),
            INIT_STOPS,
            INIT_INJECTS,
            INIT_INJECTS,
        )?;
    } else {
        return Err(
            Fail::new(EXIT_USAGE, "Usage", "bad_format").extra("created_new_vault", json!(false))
        );
    }
    let scratch = parent.join(format!(
        ".subvault-init-{}",
        ids::random_id("tmp").map_err(|err| io_fail(err, "random"))?
    ));
    fs::create_dir(&scratch).map_err(|err| io_fail(err, "init_dir"))?;
    let mut hooks = HookRun::new(hooks);
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
        if format == 1 {
            store::checkpoint(&conn).map_err(|err| db_fail(err, "init_checkpoint"))?;
            return Ok(vault_id);
        }
        let format_text = format.to_string();
        conn.execute_batch(&format!("PRAGMA user_version = {format};"))
            .map_err(|err| db_fail(err, "init_version"))?;
        conn.execute(
            "INSERT INTO meta(key, value) VALUES ('vault_format', ?1)",
            [&format_text],
        )
        .map_err(|err| db_fail(err, "init_version"))?;
        if format == 3 {
            conn.execute_batch(store::BODY_DDL)
                .map_err(|err| db_fail(err, "init_schema"))?;
            conn.execute(
                "INSERT INTO meta(key, value) VALUES ('body_schema', ?1)",
                [store::BODY_SCHEMA],
            )
            .map_err(|err| db_fail(err, "init_schema"))?;
            conn.execute_batch(store::FACT_DDL)
                .map_err(|err| db_fail(err, "init_schema"))?;
        }
        hooks.stop("version_set", init_point(false, format))?;
        hooks.inject("version_checkpoint", init_point(false, format))?;
        publish_checkpoint(&conn)?;
        drop(conn);
        hooks.stop("version_checkpoint", init_point(false, format))?;
        let (seen, companion, schema) = visible_main_format(&scratch.join("vault.db"))?;
        if seen != format as i64
            || companion.as_deref() != Some(format_text.as_str())
            || schema.as_deref() != Some("1")
        {
            return Err(
                Fail::new(EXIT_VERSION, "VersionUnsupported", "version_not_in_main_db")
                    .extra("schema_version_found", json!(seen))
                    .extra("vault_format_seen", json!(companion))
                    .extra("created_new_vault", json!(false))
                    .extra("published", json!(false)),
            );
        }
        hooks.inject("version_verify", init_point(false, format))?;
        hooks.stop("before_publish", init_point(false, format))?;
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
    hooks.stop("after_publish", init_point(true, format))?;
    let mut body = honesty("Initialized");
    body.insert("vault_id".into(), json!(vault_id));
    body.insert("vault_format".into(), json!(format));
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

fn init_point(published: bool, format: u8) -> Map<String, Value> {
    map_of(&[
        ("published", json!(published)),
        ("vault_format", json!(format)),
    ])
}

fn publish_checkpoint(conn: &Connection) -> Result<(), Fail> {
    match store::checkpoint_for_publish(conn) {
        Ok(()) => Ok(()),
        Err(store::CheckpointFailure::Sqlite(err)) => Err(db_fail(err, "init_checkpoint")
            .extra("created_new_vault", json!(false))
            .extra("published", json!(false))),
        Err(store::CheckpointFailure::Busy(busy)) => {
            Err(
                Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "checkpoint_busy")
                    .extra("during", json!("init_checkpoint"))
                    .extra("checkpoint_busy", json!(busy))
                    .extra("injected", json!(false))
                    .extra("created_new_vault", json!(false))
                    .extra("published", json!(false)),
            )
        }
    }
}

pub fn open_vault(path: &Path) -> Report {
    reopen(path)
}

pub fn verify_vault(path: &Path) -> Report {
    reopen(path)
}

pub fn recover_vault(path: &Path) -> Report {
    reopen(path)
}

fn reopen(path: &Path) -> Report {
    match reopen_inner(path) {
        Ok(report) => report,
        Err(fail) => demote_unscoped(fail).into_report(),
    }
}

fn reopen_inner(path: &Path) -> Result<Report, Fail> {
    let session = open_session(path)?;
    let notices = journal::clean_temps(&session.root);
    let reconciled = journal::reconcile(&session.root, &session.conn)?;
    let decision_misses = decide::reconcile_decision_journals(&session.root, &session.conn)?;
    let fragment_misses = decide::reconcile_fragment_journals(&session.root, &session.conn)?;
    let mut inspection = crate::inspect::report(&session.root, &session.conn, session.magic)?;
    for notice in notices {
        attach_notice(
            &mut inspection,
            notice.code,
            notice.during,
            &notice.os_error,
        );
    }
    let mut label_misses = reconciled.misses;
    label_misses.extend(decision_misses);
    label_misses.extend(fragment_misses);
    apply_label_misses(&mut inspection, &label_misses);
    if let Err(err) = store::checkpoint(&session.conn) {
        attach_notice(
            &mut inspection,
            "checkpoint_failed",
            "checkpoint",
            &err.to_string(),
        );
    }
    Ok(inspection)
}

pub fn hold_vault(path: &Path, hooks: Hooks) -> Report {
    match hold_inner(path, hooks) {
        Ok(report) => report,
        Err(fail) => demote_unscoped(fail).into_report(),
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
        Err(fail) => note_uncommitted(fail).into_report(),
    }
}

fn import_inner(req: ImportRequest) -> Result<Value, Fail> {
    validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        IMPORT_STOPS,
        RECOGNIZED_INJECTS,
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
    let reconciled = journal::reconcile(&session.root, &session.conn)?;
    let journal_unlisted = reconciled.journal_unlisted.clone();
    let label_misses = reconciled.misses;
    if let Some(existing) = store::command(&session.conn, &req.command_id)
        .map_err(|err| unseen_db(err, "command_lookup"))?
    {
        return match replay_committed(&session, &req, &existing) {
            Ok(body) => Ok(stamp_journal_list(
                stamp_label_misses_value(body, &label_misses),
                journal_unlisted.as_deref(),
            )),
            Err(fail) => Err(stamp_journal_list_fail(
                stamp_label_misses_fail(fail, &label_misses),
                journal_unlisted.as_deref(),
            )),
        };
    }
    decide::reject_if_t3_command(&session.conn, &req.command_id)?;
    let meta = require_new_source(&req.source)?;
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
        Ok(value) => Ok(stamp_journal_list(value, journal_unlisted.as_deref())),
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
    if take_cancel(
        &session.root,
        &command_id,
        journal.generation,
        "preservation",
    )? {
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
    if take_cancel(
        &session.root,
        &command_id,
        journal.generation,
        "preservation",
    )? {
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
        refuse_unreached_object_fsync(hooks)?;
        require_hosted_object(&session.root, &object, &hash, object.byte_length)?;
        (object.object_id, true)
    } else if let Some(orphan) = find_orphan_with_hash(&session.root, &session.conn, &hash)? {
        refuse_unreached_object_fsync(hooks)?;
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
    if take_cancel(
        &session.root,
        &command_id,
        journal.generation,
        "preservation",
    )? {
        return Err(cancelled());
    }

    let committed = {
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
        match tx.commit() {
            Ok(()) => result,
            Err(err) => return settle_commit(&session.root, &command_id, err),
        }
    };
    finish_after_commit(committed, || {
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
        Ok(())
    })
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
        Err(fail) => note_uncommitted(fail).into_report(),
    }
}

fn marker_inner(req: MarkerRequest) -> Result<Value, Fail> {
    validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        MARKER_STOPS,
        RECOGNIZED_INJECTS,
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
        .map_err(|err| unseen_db(err, "command_lookup"))?
    {
        if req.hooks.inject_point.is_some() || req.hooks.stop_at.is_some() {
            return Err(Fail::new(
                EXIT_FAULT_MISSED,
                "FaultNotReached",
                "command_already_committed",
            )
            .extra("injected", json!(req.hooks.inject_point.is_some()))
            .extra("new_effect", json!(false))
            .extra("command_id", json!(req.command_id)));
        }
        let source = store::source_by_id(&session.conn, &req.source_id)
            .map_err(|err| unseen_db(err, "source_lookup"))?
            .ok_or_else(|| {
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "source_row_missing")
                    .extra("new_effect", json!(false))
            })?;
        confirm_source_bytes(&session, &source, &req.command_id)?;
        if existing.payload_fingerprint != fingerprint {
            return Err(checked_conflict(&req.command_id, &source));
        }
        return already_committed(&existing);
    }
    decide::reject_if_t3_command(&session.conn, &req.command_id)?;
    let source = store::source_by_id(&session.conn, &req.source_id)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_REJECTED, "Rejected", "unknown_source")
                .extra("source_id", json!(req.source_id))
        })?;
    let existing_journal = journal::path(&session.root, &req.command_id);
    if existing_journal.exists() {
        let existing = journal::read_file(&existing_journal).map_err(|_| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
                .extra("new_effect", json!(false))
        })?;
        if existing.kind != "marker" {
            return Err(
                Fail::new(EXIT_REJECTED, "Rejected", "command_kind_conflict")
                    .extra("new_effect", json!(false)),
            );
        }
    }
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
    let recorded_at = now_ms();
    let committed = {
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
        match tx.commit() {
            Ok(()) => result,
            Err(err) => return settle_commit(&session.root, &req.command_id, err),
        }
    };
    finish_after_commit(committed, || {
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
        Ok(())
    })
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
        Err(fail) => demote_unscoped(fail).into_report(),
    }
}

fn read_inner(req: ReadRequest) -> Result<Value, Fail> {
    if !is_prefixed_id(&req.source_id, "src_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_source_id"));
    }
    let session = open_session(&req.vault)?;
    let source = store::source_by_id(&session.conn, &req.source_id)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| Fail::new(EXIT_REJECTED, "Rejected", "unknown_source"))?;
    let object = lookup_object(&session, &source.object_id)?;
    let facts = facts::object_facts(
        &session.root,
        &source.object_id,
        Some(&object.relpath),
        &source.content_sha256,
        source.byte_length,
    );
    if facts.id_rejected {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_id_rejected")
                .extra("source_id", json!(source.source_id)),
        );
    }
    if let Some(message) = &facts.io_error {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_unreadable")
                .extra("source_id", json!(source.source_id))
                .extra("os_error", json!(message))
                .extra("new_effect", json!(false)),
        );
    }
    if facts.missing {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_missing")
                .extra("source_id", json!(source.source_id))
                .extra("object_id", json!(source.object_id)),
        );
    }
    if !facts.hash_matches || !facts.length_matches {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "hash_mismatch")
                .extra("source_id", json!(source.source_id))
                .extra("object_id", json!(source.object_id))
                .extra("expected_sha256", json!(source.content_sha256))
                .extra("actual_sha256", json!(facts.actual_hash)),
        );
    }
    let relpath = facts.canonical_relpath.clone().unwrap_or_default();
    let path_rejected = facts.relpath_differs;
    let file_path = session.root.join(&relpath);
    let hash = facts.actual_hash.clone().unwrap_or_default();
    let len = facts.actual_len.unwrap_or(0);
    match place_output(&session.root, &req.out) {
        Place::Inside => {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_inside_vault"));
        }
        Place::Unresolved(message) => {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_unresolved")
                .extra("os_error", json!(message)));
        }
        Place::MissingParent => {
            return Err(Fail::new(
                EXIT_REJECTED,
                "Rejected",
                "output_parent_missing",
            ));
        }
        Place::Exists => {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_exists"));
        }
        Place::Create => {}
    }
    if let Err(err) = copy_exclusive(&file_path, &req.out) {
        return Err(match err {
            FreshCopy::Exists => Fail::new(EXIT_REJECTED, "Rejected", "output_exists"),
            FreshCopy::Io {
                err,
                cleanup,
                cleanup_error,
            } => {
                let mut fail = Fail::new(EXIT_INTEGRITY, "IntegrityAlert", "io_error")
                    .extra("during", json!("read_copy"))
                    .extra("os_error", json!(err.to_string()))
                    .extra("preservation", json!(PRESERVED))
                    .extra("object_intact", json!(true))
                    .extra("new_effect", json!(false))
                    .extra("injected", json!(false))
                    .extra("output_written", json!(false))
                    .extra("output_cleanup", json!(cleanup))
                    .extra("source_id", json!(source.source_id))
                    .extra("object_id", json!(source.object_id))
                    .extra("content_sha256", json!(hash));
                if let Some(message) = cleanup_error {
                    fail = fail.extra("output_cleanup_error", json!(message));
                }
                fail
            }
        });
    }
    let mut body = honesty("Read");
    body.insert("source_id".into(), json!(source.source_id));
    body.insert("object_id".into(), json!(source.object_id));
    body.insert("content_sha256".into(), json!(hash));
    body.insert("hash_alg".into(), json!("sha256"));
    body.insert("byte_length".into(), json!(len));
    body.insert("preservation".into(), json!(PRESERVED));
    body.insert("arrangement".into(), json!(NOT_ARRANGED));
    body.insert("searchability".into(), json!(NOT_SEARCHABLE));
    body.insert("object_path_rejected".into(), json!(path_rejected));
    body.insert("new_effect".into(), json!(false));
    Ok(Value::Object(body))
}

enum Place {
    Inside,
    Unresolved(String),
    MissingParent,
    Exists,
    Create,
}

/// Where a new `--out` file would be created, before this call creates anything.
/// Ancestor symlinks are followed. The final component is not. Identity is the
/// directory's device and inode, not a string prefix. Linux only.
fn place_output(vault: &Path, dst: &Path) -> Place {
    let vault_id = match vault_identity(vault) {
        Ok(id) => id,
        Err(err) => return Place::Unresolved(err.to_string()),
    };
    let abs = match absolute_existing_base(dst) {
        Ok(path) => path,
        Err(err) => return Place::Unresolved(err.to_string()),
    };
    let mut parts = path_parts(&abs);
    let Some(file_name) = parts.pop() else {
        return Place::Unresolved("output path has no final component".into());
    };
    let mut cur = PathBuf::from("/");
    let mut missing = false;
    for part in parts {
        if missing {
            break;
        }
        let next = cur.join(&part);
        match fs::symlink_metadata(&next) {
            Ok(_) => match resolved_directory(&next) {
                Ok(dir) => cur = dir,
                Err(err) => return Place::Unresolved(err.to_string()),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => missing = true,
            Err(err) => return Place::Unresolved(err.to_string()),
        }
    }
    match directory_is_vault_or_inside(vault_id, &cur) {
        Ok(true) => return Place::Inside,
        Ok(false) => {}
        Err(err) => return Place::Unresolved(err.to_string()),
    }
    if missing {
        return Place::MissingParent;
    }
    match fs::symlink_metadata(cur.join(file_name)) {
        Ok(_) => Place::Exists,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Place::Create,
        Err(err) => Place::Unresolved(err.to_string()),
    }
}

fn vault_identity(vault: &Path) -> io::Result<(u64, u64)> {
    let real = fs::canonicalize(vault)?;
    directory_id(&real)
}

fn absolute_existing_base(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn path_parts(path: &Path) -> Vec<OsString> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_os_string()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect()
}

fn resolved_directory(path: &Path) -> io::Result<PathBuf> {
    let real = fs::canonicalize(path)?;
    let meta = fs::metadata(&real)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output ancestor is not a directory",
        ));
    }
    Ok(real)
}

fn directory_id(path: &Path) -> io::Result<(u64, u64)> {
    let meta = fs::metadata(path)?;
    Ok((meta.dev(), meta.ino()))
}

fn directory_is_vault_or_inside(vault_id: (u64, u64), dir: &Path) -> io::Result<bool> {
    let mut cursor = fs::canonicalize(dir)?;
    for _ in 0..256 {
        if directory_id(&cursor)? == vault_id {
            return Ok(true);
        }
        let parent = cursor.join("..");
        if directory_id(&parent)? == directory_id(&cursor)? {
            return Ok(false);
        }
        let next = fs::canonicalize(&parent)?;
        if next == cursor {
            return Ok(false);
        }
        cursor = next;
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "output directory identity did not resolve",
    ))
}

enum FreshCopy {
    Exists,
    Io {
        err: io::Error,
        cleanup: &'static str,
        cleanup_error: Option<String>,
    },
}

/// Create `dst` only when its final component is absent. An existing file,
/// directory, or symlink — dangling or not — is left untouched. Linux
/// `O_CREAT|O_EXCL|O_NOFOLLOW` covers a name that appears after the check.
/// `output_written` is not this function's concern: `cleanup` says whether a
/// file this call created was removed.
fn copy_exclusive(src: &Path, dst: &Path) -> Result<(), FreshCopy> {
    match fs::symlink_metadata(dst) {
        Ok(_) => return Err(FreshCopy::Exists),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(FreshCopy::Io {
                err,
                cleanup: "not_created",
                cleanup_error: None,
            })
        }
    }
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dst)
        .map_err(|err| {
            if err.kind() == io::ErrorKind::AlreadyExists || refused_output(&err) {
                FreshCopy::Exists
            } else {
                FreshCopy::Io {
                    err,
                    cleanup: "not_created",
                    cleanup_error: None,
                }
            }
        })?;
    let copied = (|| -> io::Result<()> {
        let mut reader = File::open(src)?;
        let permissions = reader.metadata()?.permissions();
        io::copy(&mut reader, &mut writer)?;
        writer.flush()?;
        writer.set_permissions(permissions)?;
        Ok(())
    })();
    if let Err(err) = copied {
        let (cleanup, cleanup_error) = match fs::remove_file(dst) {
            Ok(()) => ("removed", None),
            Err(remove_err) => ("failed", Some(remove_err.to_string())),
        };
        return Err(FreshCopy::Io {
            err,
            cleanup,
            cleanup_error,
        });
    }
    Ok(())
}

fn refused_output(err: &io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(code) if code == libc::EEXIST || code == libc::ELOOP
    )
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
        Err(fail) => note_uncommitted(fail).into_report(),
    }
}

fn cancel_inner(req: CancelRequest) -> Result<Report, Fail> {
    if !is_command_id(&req.command_id) || req.generation < 1 {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_cancel"));
    }
    if req.kind == "fragment" {
        return fragment::cancel_fragment(req);
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
    guard_vault(&req.vault)?;
    let lock = match lockfile::VaultLock::try_acquire(&req.vault.join("writer.lock"))
        .map_err(|err| unseen_io(err, "lock"))?
    {
        Some(lock) => lock,
        None => return cancel_without_lock(&req),
    };
    let _lock = lock;
    accept_wal_format(&req.vault.join("vault.db"))?;
    let conn = open_writer(&req.vault)?;
    if let Some(existing) =
        store::command(&conn, &req.command_id).map_err(|err| unseen_db(err, "command_lookup"))?
    {
        let generation = result_generation(&existing.result_json)?;
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
    if journal.kind == "fragment" || journal.kind == "decision" {
        return Err(fragment_journal_conflict());
    }
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

fn guard_vault(path: &Path) -> Result<i64, Fail> {
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
    let magic = filesystem_magic(path).map_err(|err| unseen_io(err, "statfs"))?;
    if network_filesystem(magic) {
        return Err(unsupported_fs(magic));
    }
    inspect_version_readonly(path)?;
    Ok(magic)
}

fn open_session(path: &Path) -> Result<Session, Fail> {
    let magic = guard_vault(path)?;
    let lock = lockfile::VaultLock::try_acquire(&path.join("writer.lock"))
        .map_err(|err| unseen_io(err, "lock"))?
        .ok_or_else(|| {
            Fail::new(EXIT_LOCKED, "Locked", "writer_busy").extra("created_new_vault", json!(false))
        })?;
    accept_wal_format(&path.join("vault.db"))?;
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
    // A failed read has not examined any object. That is the same class as an
    // unreadable vault.db: unknown, with no preservation verdict. Bytes that
    // were read and do not match stay NotSynthetic below, and are not rewritten.
    let bytes = fs::read(path.join("SYNTHETIC_ONLY"))
        .map_err(|err| vault_unreadable(&err, "banner_unreadable", "banner"))?;
    Ok(bytes == VAULT_BANNER.as_bytes())
}

fn sqlite_header_ok(path: &Path) -> Result<bool, Fail> {
    // Opening the file failed, or the read itself failed. That is this call's
    // inability to see the database, not a COMMIT result and not evidence that
    // the stored bytes are unpreserved. A short or mismatched header is still
    // a damaged file, because those bytes were actually read.
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(err) => return Err(vault_unreadable(&err, "vault_unreadable", "sqlite_header")),
    };
    let mut buf = [0u8; 16];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(err) => return Err(vault_unreadable(&err, "vault_unreadable", "sqlite_header")),
    };
    Ok(n == 16 && &buf == b"SQLite format 3\0")
}

fn vault_unreadable(err: &io::Error, reason: &'static str, during: &str) -> Fail {
    Fail::new(EXIT_UNCERTAIN, "Uncertain", reason)
        .extra("during", json!(during))
        .extra("os_error", json!(err.to_string()))
        .extra("new_effect", Value::Null)
        .extra("injected", json!(false))
}

fn inspect_version_readonly(path: &Path) -> Result<i64, Fail> {
    let (version, format, schema) = visible_main_format(&path.join("vault.db"))?;
    if version == 2 || version == 3 {
        let expected = if version == 2 { "2" } else { "3" };
        if format.as_deref() == Some(expected) && schema.as_deref() == Some("1") {
            return Ok(version);
        }
        return Err(Fail::new(
            EXIT_VERSION,
            "VersionUnsupported",
            "vault_format_unverified",
        )
        .extra("schema_version_found", json!(version))
        .extra("created_new_vault", json!(false)));
    }
    if version > 3 {
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
    if schema.as_deref() != Some("1") {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "schema_mismatch")
                .extra("created_new_vault", json!(false)),
        );
    }
    Ok(1)
}

fn visible_main_format(db: &Path) -> Result<(i64, Option<String>, Option<String>), Fail> {
    let uri = readonly_uri(db)?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "sqlite_unreadable")
            .extra("created_new_vault", json!(false))
            .extra("published", json!(false))
    })?;
    let version = store::user_version(&conn).map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "user_version_unreadable")
            .extra("created_new_vault", json!(false))
            .extra("published", json!(false))
    })?;
    let format = store::meta(&conn, "vault_format").map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "meta_unreadable")
            .extra("created_new_vault", json!(false))
            .extra("published", json!(false))
    })?;
    let schema = store::meta(&conn, "schema_version").map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "meta_unreadable")
            .extra("created_new_vault", json!(false))
            .extra("published", json!(false))
    })?;
    Ok((version, format, schema))
}

fn file_uri(path: &Path, query: &str) -> Result<String, Fail> {
    let abs = path
        .canonicalize()
        .map_err(|err| unseen_io(err, "canonicalize"))?;
    let text = abs.to_string_lossy();
    if text.contains('?') || text.contains('#') {
        return Err(Fail::new(
            EXIT_REJECTED,
            "Rejected",
            "vault_path_unsupported",
        ));
    }
    Ok(format!("file:{text}?{query}"))
}

/// Version inspection only. immutable=1 does not see a commit that is still in the WAL.
fn readonly_uri(path: &Path) -> Result<String, Fail> {
    file_uri(path, "mode=ro&immutable=1")
}

/// Main-file header, WAL-visible version, and the meta that version needs.
/// Read-only: no writer configuration, no cleanup, and no checkpoint.
/// A caller about to open a writer already holds `writer.lock`. A query or a
/// busy cancel uses the snapshot it already opened and does not take that lock.
/// This open may create or touch `-shm`. It is not a claim that the directory
/// received no files.
fn accept_wal_format(db: &Path) -> Result<i64, Fail> {
    // No WAL frames means the main file is the effective database. Skipping the
    // extra open avoids creating an empty `-wal` on a checkpointed vault.
    let mut wal_name = db.as_os_str().to_os_string();
    wal_name.push("-wal");
    let frames = fs::metadata(Path::new(&wal_name))
        .map(|meta| meta.is_file() && meta.len() > 0)
        .unwrap_or(false);
    if !frames {
        let (header, format, schema) = visible_main_format(db)?;
        return judge_format(header, header, format.as_deref(), schema.as_deref());
    }
    let uri = wal_visible_uri(db)?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "sqlite_unreadable")
            .extra("created_new_vault", json!(false))
    })?;
    accept_connection_format(db, &conn)
}

fn accept_connection_format(db: &Path, conn: &Connection) -> Result<i64, Fail> {
    let _ = conn.busy_timeout(std::time::Duration::from_millis(0));
    let (header, _, _) = visible_main_format(db)?;
    let (effective, format, schema) = effective_view(conn)?;
    judge_format(header, effective, format.as_deref(), schema.as_deref())
}

fn effective_view(conn: &Connection) -> Result<(i64, Option<String>, Option<String>), Fail> {
    let version = store::user_version(conn).map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "user_version_unreadable")
            .extra("created_new_vault", json!(false))
    })?;
    let format = store::meta(conn, "vault_format").map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "meta_unreadable")
            .extra("created_new_vault", json!(false))
    })?;
    let schema = store::meta(conn, "schema_version").map_err(|_| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "meta_unreadable")
            .extra("created_new_vault", json!(false))
    })?;
    Ok((version, format, schema))
}

fn judge_format(
    header: i64,
    effective: i64,
    format: Option<&str>,
    schema: Option<&str>,
) -> Result<i64, Fail> {
    if effective > 3 {
        return Err(
            Fail::new(EXIT_VERSION, "VersionUnsupported", "newer_schema")
                .extra("schema_version_found", json!(effective))
                .extra("created_new_vault", json!(false)),
        );
    }
    if effective != 1 && effective != 2 && effective != 3 {
        return Err(
            Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "unexpected_user_version")
                .extra("schema_version_found", json!(effective))
                .extra("created_new_vault", json!(false)),
        );
    }
    if header != effective {
        let seen = match format {
            Some(value) => json!(value),
            None => Value::Null,
        };
        return Err(
            Fail::new(EXIT_VERSION, "VersionUnsupported", "format_inconsistent")
                .extra("schema_version_found", json!(effective))
                .extra("header_user_version", json!(header))
                .extra("vault_format_seen", seen)
                .extra("created_new_vault", json!(false)),
        );
    }
    if effective == 1 {
        if schema != Some("1") {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "schema_mismatch")
                    .extra("created_new_vault", json!(false)),
            );
        }
        if format.is_some() && format != Some("1") {
            return Err(Fail::new(
                EXIT_VERSION,
                "VersionUnsupported",
                "vault_format_unverified",
            )
            .extra("schema_version_found", json!(1))
            .extra("created_new_vault", json!(false)));
        }
        return Ok(1);
    }
    let expected = if effective == 2 { "2" } else { "3" };
    if format != Some(expected) || schema != Some("1") {
        return Err(Fail::new(
            EXIT_VERSION,
            "VersionUnsupported",
            "vault_format_unverified",
        )
        .extra("schema_version_found", json!(effective))
        .extra("created_new_vault", json!(false)));
    }
    Ok(effective)
}

/// Command lookup. mode=ro without immutable sees a committed row while the writer still holds the lock.
fn wal_visible_uri(path: &Path) -> Result<String, Fail> {
    file_uri(path, "mode=ro")
}

fn open_writer(path: &Path) -> Result<Connection, Fail> {
    let conn = Connection::open(path.join("vault.db")).map_err(|err| unseen_db(err, "open"))?;
    store::configure(&conn).map_err(|err| unseen_db(err, "pragma"))?;
    Ok(conn)
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
        if paused {
            output
                .write_all(&buf[..n])
                .map_err(|err| io_fail(err, "write_staging"))?;
            written += n as u64;
            continue;
        }
        // One pause, before this read is fully in the staging file and before publish.
        // A one-byte read stops with nothing of that byte written yet.
        let hold = if n == 1 { 0 } else { n - 1 };
        if hold > 0 {
            output
                .write_all(&buf[..hold])
                .map_err(|err| io_fail(err, "write_staging"))?;
            output
                .flush()
                .map_err(|err| io_fail(err, "flush_staging"))?;
            written += hold as u64;
        }
        hooks.stop(
            "during_copy",
            checkpoint_map(command_id, written, Some(expected_len), None, false, false),
        )?;
        if take_cancel(root, command_id, generation, "preservation")? {
            return Err(cancelled());
        }
        output
            .write_all(&buf[hold..n])
            .map_err(|err| io_fail(err, "write_staging"))?;
        written += (n - hold) as u64;
        paused = true;
    }
    if !paused {
        hooks.stop(
            "during_copy",
            checkpoint_map(command_id, written, Some(expected_len), None, false, false),
        )?;
        if take_cancel(root, command_id, generation, "preservation")? {
            return Err(cancelled());
        }
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

fn lookup_object(session: &Session, object_id: &str) -> Result<ObjectRow, Fail> {
    store::object_by_id(&session.conn, object_id)
        .map_err(|err| unseen_db(err, "object_lookup"))?
        .ok_or_else(|| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_row_missing"))
}

fn unwind(root: &Path, journal: &mut Journal, phase: &Phase, fail: Fail) -> Result<Fail, Fail> {
    if !fail.keep_staging {
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

fn take_cancel(
    root: &Path,
    command_id: &str,
    generation: i64,
    expected_kind: &str,
) -> Result<bool, Fail> {
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
        || kind.as_deref().unwrap_or("preservation") != expected_kind
    {
        return Ok(false);
    }
    let _ = fs::remove_file(&path);
    Ok(true)
}

fn write_control(root: &Path, command_id: &str, generation: i64, kind: &str) -> Result<(), Fail> {
    fs::create_dir_all(root.join("control")).map_err(|err| io_fail(err, "control_dir"))?;
    let body = format!("cancel\ngeneration={generation}\nkind={kind}\n");
    atomic_write(&root.join("control").join(command_id), body.as_bytes())
        .map_err(|err| io_fail(err, "control_write"))
}

fn result_generation(json_text: &str) -> Result<i64, Fail> {
    let value: Value = serde_json::from_str(json_text).map_err(|_| {
        Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "command_result_unreadable",
        )
        .extra("new_effect", json!(false))
    })?;
    let Some(map) = value.as_object() else {
        return Err(Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "command_result_not_object",
        )
        .extra("new_effect", json!(false)));
    };
    Ok(map
        .get("generation")
        .and_then(|generation| generation.as_i64())
        .unwrap_or(1))
}

fn already_committed(row: &CommandRow) -> Result<Value, Fail> {
    let mut value: Value = serde_json::from_str(&row.result_json).map_err(|_| {
        Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "command_result_unreadable",
        )
        .extra("new_effect", json!(false))
    })?;
    let Some(map) = value.as_object_mut() else {
        return Err(Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "command_result_not_object",
        )
        .extra("new_effect", json!(false)));
    };
    map.insert("outcome".into(), json!("AlreadyCommitted"));
    map.insert("new_effect".into(), json!(false));
    map.insert("user_ready".into(), json!(false));
    map.insert("security_complete".into(), json!(false));
    map.insert("purchase_id".into(), Value::Null);
    map.insert("entitlement_id".into(), Value::Null);
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

fn validate_hook_names(
    stop: Option<&str>,
    inject: Option<&str>,
    reachable_stops: &[&str],
    recognized_injects: &[&str],
    reachable_injects: &[&str],
) -> Result<(), Fail> {
    if let Some(point) = stop {
        if !reachable_stops.contains(&point) {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "unknown_stop_point").extra("point", json!(point))
            );
        }
    }
    if let Some(point) = inject {
        if !recognized_injects.contains(&point) {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "unknown_inject_point").extra("point", json!(point))
            );
        }
        if !reachable_injects.contains(&point) {
            return Err(Fail::new(EXIT_FAULT_MISSED, "FaultNotReached", point)
                .extra("injected", json!(true))
                .extra("preservation", json!(NOT_PRESERVED))
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

pub(super) fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([0xff]);
        }
        hasher.update(part.as_bytes());
    }
    hex_encode(&hasher.finalize())
}

fn display_name_fallback() -> String {
    "source".into()
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn cancel_without_lock(req: &CancelRequest) -> Result<Report, Fail> {
    if let Some(row) = read_committed(&req.vault, &req.command_id)? {
        let generation = result_generation(&row.result_json)?;
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
    if journal.kind == "fragment" || journal.kind == "decision" {
        return Err(fragment_journal_conflict());
    }
    if journal.generation != req.generation {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "generation_mismatch")
            .extra("cancel_applied", json!(false)));
    }
    accept_wal_format(&req.vault.join("vault.db"))?;
    write_control(&req.vault, &req.command_id, req.generation, &req.kind)?;
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

fn fragment_journal_conflict() -> Fail {
    Fail::new(EXIT_REJECTED, "Rejected", "command_kind_conflict")
        .extra("cancel_applied", json!(false))
        .extra("new_effect", json!(false))
}

fn read_committed(root: &Path, command_id: &str) -> Result<Option<CommandRow>, Fail> {
    let db = root.join("vault.db");
    if !sqlite_header_ok(&db)? {
        return Err(Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "sqlite_header")
            .extra("created_new_vault", json!(false)));
    }
    let uri = wal_visible_uri(&db)?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| uncertain(command_id, "wal_open", &err.to_string()))?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(0));
    accept_connection_format(&db, &conn)?;
    store::command(&conn, command_id)
        .map_err(|err| uncertain(command_id, "wal_query", &err.to_string()))
}

enum SourceView {
    Absent,
    Unreadable(String),
    NotFile,
    TooLarge(u64),
    Ready { hash: String, len: u64 },
}

/// What the external path looks like on this call. This does not say whether
/// a previously hosted object is intact; the caller already checked that.
fn classify_source(path: &Path) -> SourceView {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return SourceView::Absent,
        Err(err) => return SourceView::Unreadable(err.to_string()),
    };
    if !meta.file_type().is_file() {
        return SourceView::NotFile;
    }
    if meta.len() > MAX_OBJECT_BYTES {
        return SourceView::TooLarge(meta.len());
    }
    match sha256_file(path) {
        Ok((hash, len)) => SourceView::Ready { hash, len },
        Err(err) => SourceView::Unreadable(err.to_string()),
    }
}

/// First import of a command that has no hosted object yet. A missing or
/// unreadable source here means this command was not preserved.
fn require_new_source(path: &Path) -> Result<fs::Metadata, Fail> {
    let meta = fs::symlink_metadata(path).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "source_missing")
                .extra("preservation", json!(NOT_PRESERVED))
                .extra("injected", json!(false))
        } else {
            io_fail(err, "source_metadata")
        }
    })?;
    if !meta.file_type().is_file() {
        return Err(
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "source_not_a_file")
                .extra("preservation", json!(NOT_PRESERVED))
                .extra("injected", json!(false)),
        );
    }
    if meta.len() > MAX_OBJECT_BYTES {
        return Err(
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "object_too_large")
                .extra("preservation", json!(NOT_PRESERVED))
                .extra("byte_length", json!(meta.len()))
                .extra("limit", json!(MAX_OBJECT_BYTES))
                .extra("injected", json!(false)),
        );
    }
    Ok(meta)
}

fn replay_committed(
    session: &Session,
    req: &ImportRequest,
    existing: &CommandRow,
) -> Result<Value, Fail> {
    if req.hooks.inject_point.is_some() || req.hooks.stop_at.is_some() {
        return Err(Fail::new(
            EXIT_FAULT_MISSED,
            "FaultNotReached",
            "command_already_committed",
        )
        .extra("injected", json!(req.hooks.inject_point.is_some()))
        .extra("new_effect", json!(false))
        .extra("command_id", json!(req.command_id)));
    }
    let mut receipt = already_committed(existing)?;
    let source_id = receipt
        .get("source_id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            Fail::new(
                EXIT_RECOVERY,
                "RecoveryRequired",
                "command_result_incomplete",
            )
            .extra("new_effect", json!(false))
        })?;
    let source = store::source_by_id(&session.conn, source_id)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "source_row_missing")
                .extra("new_effect", json!(false))
                .extra("command_id", json!(req.command_id))
        })?;
    confirm_source_bytes(session, &source, &req.command_id)?;
    match classify_source(&req.source) {
        // The external file is gone, so its bytes are not read again. The hosted
        // hash was checked above, and the requested media type is still part of
        // the import fingerprint. Display name is not.
        SourceView::Absent => {
            let fingerprint = fingerprint(&["import", &source.content_sha256, &req.media_type]);
            if fingerprint != existing.payload_fingerprint {
                return Err(checked_conflict(&req.command_id, &source)
                    .extra("requested_media_type", json!(req.media_type))
                    .extra("source_check", json!("absent")));
            }
            if let Some(map) = receipt.as_object_mut() {
                map.insert("source_check".into(), json!("absent"));
                map.insert(
                    "replay".into(),
                    json!("historical_receipt_source_not_rechecked"),
                );
                map.insert("payload_matches".into(), json!(true));
            }
            Ok(receipt)
        }
        SourceView::Unreadable(message) => Err(external_unverified(
            &receipt,
            &req.command_id,
            &req.media_type,
            "source_unreadable",
            "unreadable",
            Some(message),
        )),
        SourceView::NotFile => Err(external_unverified(
            &receipt,
            &req.command_id,
            &req.media_type,
            "source_not_a_file",
            "not_a_file",
            None,
        )),
        SourceView::TooLarge(len) => Err(checked_conflict(&req.command_id, &source)
            .extra("byte_length", json!(len))
            .extra("requested_media_type", json!(req.media_type))),
        SourceView::Ready { hash, len } => {
            let fingerprint = fingerprint(&["import", &hash, &req.media_type]);
            if fingerprint != existing.payload_fingerprint || len as i64 != source.byte_length {
                return Err(checked_conflict(&req.command_id, &source)
                    .extra("requested_media_type", json!(req.media_type)));
            }
            if let Some(map) = receipt.as_object_mut() {
                map.insert("source_check".into(), json!("matched"));
            }
            Ok(receipt)
        }
    }
}

fn confirm_source_bytes(
    session: &Session,
    source: &SourceRow,
    command_id: &str,
) -> Result<(), Fail> {
    let object = store::object_by_id(&session.conn, &source.object_id)
        .map_err(|err| unseen_db(err, "object_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_row_missing")
                .extra("new_effect", json!(false))
                .extra("command_id", json!(command_id))
        })?;
    let facts = facts::object_facts(
        &session.root,
        &object.object_id,
        Some(&object.relpath),
        &source.content_sha256,
        source.byte_length,
    );
    hosted_facts_ok(&facts).map_err(|fail| fail.extra("command_id", json!(command_id)))
}

fn require_hosted_object(
    root: &Path,
    object: &ObjectRow,
    expected_hash: &str,
    expected_len: i64,
) -> Result<(), Fail> {
    let facts = facts::object_facts(
        root,
        &object.object_id,
        Some(&object.relpath),
        expected_hash,
        expected_len,
    );
    hosted_facts_ok(&facts)
}

fn hosted_facts_ok(facts: &ObjectFacts) -> Result<(), Fail> {
    if facts.id_rejected {
        return Err(bad_object("object_id_rejected"));
    }
    if let Some(message) = &facts.io_error {
        return Err(bad_object("object_unreadable").extra("os_error", json!(message)));
    }
    if facts.missing {
        return Err(bad_object("object_missing"));
    }
    if !facts.hash_matches || !facts.length_matches {
        return Err(bad_object("hash_mismatch")
            .extra("actual_sha256", json!(facts.actual_hash))
            .extra("byte_length", json!(facts.actual_len)));
    }
    Ok(())
}

fn bad_object(reason: &str) -> Fail {
    Fail::new(EXIT_RECOVERY, "RecoveryRequired", reason)
        .extra("new_effect", json!(false))
        .extra("injected", json!(false))
}

fn payload_conflict(command_id: &str) -> Fail {
    Fail::new(EXIT_REJECTED, "Rejected", "payload_conflict")
        .extra("command_id", json!(command_id))
        .extra("new_effect", json!(false))
        .extra("payload_matches", json!(false))
        .extra("request_result", json!("rejected"))
}

fn checked_conflict(command_id: &str, source: &SourceRow) -> Fail {
    payload_conflict(command_id)
        .extra("preservation", json!(PRESERVED))
        .extra("object_intact", json!(true))
        .extra("source_id", json!(source.source_id))
        .extra("object_id", json!(source.object_id))
        .extra("content_sha256", json!(source.content_sha256))
        .extra("media_type", json!(source.media_type))
}

/// Hosted bytes for this command were checked. This call could not read the
/// external path, so the payload match is unknown and the hosted source stays
/// preserved. Media type is not compared here: unread bytes are not a conflict.
fn external_unverified(
    receipt: &Value,
    command_id: &str,
    requested_media: &str,
    reason: &'static str,
    source_check: &str,
    os_error: Option<String>,
) -> Fail {
    let mut fail = Fail::new(EXIT_INTEGRITY, "ExternalUnverified", reason)
        .extra("preservation", json!(PRESERVED))
        .extra("object_intact", json!(true))
        .extra("new_effect", json!(false))
        .extra("injected", json!(false))
        .extra("payload_verified", json!(false))
        .extra("payload_matches", Value::Null)
        .extra("source_check", json!(source_check))
        .extra("journal_label_update", json!("not_attempted"))
        .extra("request_result", json!("external_unverified"))
        .extra("command_id", json!(command_id))
        .extra("requested_media_type", json!(requested_media));
    if let Some(message) = os_error {
        fail = fail.extra("os_error", json!(message));
    }
    for key in [
        "source_id",
        "object_id",
        "content_sha256",
        "byte_length",
        "media_type",
        "occurrence_id",
    ] {
        if let Some(value) = receipt.get(key) {
            fail = fail.extra(key, value.clone());
        }
    }
    fail
}

fn label_miss_json(misses: &[journal::LabelMiss]) -> Value {
    json!(misses
        .iter()
        .map(|miss| {
            json!({
                "command_id": miss.command_id,
                "during": miss.during,
                "os_error": miss.os_error,
            })
        })
        .collect::<Vec<_>>())
}

fn stamp_label_misses_value(mut body: Value, misses: &[journal::LabelMiss]) -> Value {
    if misses.is_empty() {
        return body;
    }
    if let Some(map) = body.as_object_mut() {
        map.insert("journal_label_update".into(), json!("failed"));
        map.insert("journal_write_errors".into(), label_miss_json(misses));
    }
    body
}

fn stamp_label_misses_fail(fail: Fail, misses: &[journal::LabelMiss]) -> Fail {
    if misses.is_empty() {
        return fail;
    }
    fail.extra("journal_label_update", json!("failed"))
        .extra("journal_write_errors", label_miss_json(misses))
}

/// This command has no committed row. Exit 15 may say so. Any preservation
/// value already chosen by a replay stays as it is.
fn note_uncommitted(fail: Fail) -> Fail {
    if fail.code != EXIT_NOT_PRESERVED || fail.extra.contains_key("preservation") {
        return fail;
    }
    fail.extra("preservation", json!(NOT_PRESERVED))
}

/// An exit 15 that never learned whether history was committed must not keep
/// the NotPreserved outcome. Callers that already set preservation are left alone.
fn demote_unscoped(mut fail: Fail) -> Fail {
    if fail.code != EXIT_NOT_PRESERVED || fail.extra.contains_key("preservation") {
        return fail;
    }
    fail.code = EXIT_UNCERTAIN;
    fail.outcome = "Uncertain";
    fail.reason = "vault_unseen".into();
    fail.extra.insert("new_effect".into(), Value::Null);
    fail
}

fn stamp_journal_list(mut body: Value, message: Option<&str>) -> Value {
    let Some(message) = message else {
        return body;
    };
    if let Some(map) = body.as_object_mut() {
        map.insert("journal_list".into(), json!("failed"));
        map.insert("journal_list_error".into(), json!(message));
    }
    body
}

fn stamp_journal_list_fail(fail: Fail, message: Option<&str>) -> Fail {
    let Some(message) = message else {
        return fail;
    };
    fail.extra("journal_list", json!("failed"))
        .extra("journal_list_error", json!(message))
}

fn attach_notice(report: &mut Report, code: &str, during: &str, os_error: &str) {
    let Some(body) = report.body.as_object_mut() else {
        return;
    };
    if let Some(alerts) = body
        .entry("alerts")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
    {
        alerts.push(json!({
            "code": code,
            "during": during,
            "os_error": os_error,
        }));
    }
    body.insert("new_effect".into(), json!(false));
    if report.code == EXIT_OK {
        report.code = EXIT_INTEGRITY;
        body.insert("outcome".into(), json!("IntegrityAlert"));
    }
}

fn apply_label_misses(report: &mut Report, misses: &[journal::LabelMiss]) {
    if misses.is_empty() {
        return;
    }
    let Some(body) = report.body.as_object_mut() else {
        return;
    };
    if let Some(alerts) = body
        .entry("alerts")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
    {
        for miss in misses {
            alerts.push(json!({
                "code": "journal_label_not_updated",
                "command_id": miss.command_id,
                "during": miss.during,
                "os_error": miss.os_error,
            }));
        }
    }
    body.insert("journal_label_update".into(), json!("failed"));
    body.insert("journal_write_errors".into(), label_miss_json(misses));
    body.insert("new_effect".into(), json!(false));
    if report.code == EXIT_OK {
        report.code = EXIT_INTEGRITY;
        body.insert("outcome".into(), json!("IntegrityAlert"));
    }
}

fn refuse_unreached_object_fsync(hooks: &HookRun) -> Result<(), Fail> {
    if hooks.hooks.inject_point.as_deref() == Some("object_fsync") {
        return Err(
            Fail::new(EXIT_FAULT_MISSED, "FaultNotReached", "object_fsync")
                .extra("injected", json!(true))
                .extra("preservation", json!(NOT_PRESERVED))
                .extra("new_effect", json!(false))
                .extra("published", json!(false)),
        );
    }
    Ok(())
}

fn finish_after_commit(
    result: Value,
    after: impl FnOnce() -> Result<(), Fail>,
) -> Result<Value, Fail> {
    match after() {
        Ok(()) => Ok(result),
        Err(fail) => Ok(note_post_commit(result, &fail)),
    }
}

fn note_post_commit(mut result: Value, fail: &Fail) -> Value {
    if let Some(map) = result.as_object_mut() {
        map.insert("post_commit_io".into(), json!(true));
        map.insert("outcome".into(), json!("Committed"));
        map.insert("preservation".into(), json!(PRESERVED));
        map.insert("new_effect".into(), json!(true));
        map.insert("injected".into(), json!(false));
        if let Some(during) = fail.extra.get("during") {
            map.insert("during".into(), during.clone());
        }
        if let Some(os_error) = fail.extra.get("os_error") {
            map.insert("os_error".into(), os_error.clone());
        }
    }
    result
}

fn settle_commit(root: &Path, command_id: &str, err: rusqlite::Error) -> Result<Value, Fail> {
    match read_committed(root, command_id) {
        Ok(Some(row)) => {
            let mut body = already_committed(&row)?;
            if let Some(map) = body.as_object_mut() {
                map.insert("commit_error".into(), json!(err.to_string()));
                map.insert("visible_despite_commit_error".into(), json!(true));
            }
            Ok(body)
        }
        Ok(None) => Err(db_fail(err, "db_commit")),
        Err(_) => Err(uncertain(
            command_id,
            "after_commit_error",
            &err.to_string(),
        )),
    }
}

fn uncertain(command_id: &str, during: &str, detail: &str) -> Fail {
    Fail::new(EXIT_UNCERTAIN, "Uncertain", "commit_visibility_unknown")
        .extra("command_id", json!(command_id))
        .extra("during", json!(during))
        .extra("detail", json!(detail))
        .extra("new_effect", Value::Null)
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
