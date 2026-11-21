use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::crypto::{
    self, AuthInput, Credential, CredentialError, CryptoError, NewPassphraseInput, ProtectionMode,
    VaultCipher, VaultKeys,
};
use crate::facts::{self, ObjectFacts};
use crate::fsutil::{
    self, atomic_write, filesystem_magic, network_filesystem, sha256_file, MAX_OBJECT_BYTES,
};
use crate::ids::{
    self, canonical_object_relpath, hex_encode, is_command_id, is_display_name, is_media_type,
    is_prefixed_id,
};
use crate::journal::{self, path as journal_path, staging_path, Journal};
#[cfg(target_os = "linux")]
use crate::lockfile::sigstop;
use crate::lockfile::{self, VaultLock, LOCK_TEXT};
use crate::model::{
    apply_format_honesty, capabilities, capabilities_for, db_fail, honesty, io_fail,
    platform_limits, unseen_db, unseen_io, Fail, Report, EXIT_AUTH, EXIT_DAMAGED,
    EXIT_FAULT_MISSED, EXIT_INTEGRITY, EXIT_LOCKED, EXIT_NOT_PRESERVED, EXIT_NOT_SYNTHETIC,
    EXIT_NOT_VAULT, EXIT_OK, EXIT_RECOVERY, EXIT_REJECTED, EXIT_STOP_MISSED, EXIT_UNCERTAIN,
    EXIT_UNSUPPORTED_FS, EXIT_USAGE, EXIT_VERSION, NOT_ARRANGED, NOT_PRESERVED, NOT_SEARCHABLE,
    PRESERVED, PRODUCTION_BANNER_FILE, PRODUCTION_BANNER_OS_DISK, PRODUCTION_BANNER_PASSPHRASE,
    SYNTHETIC_BANNER_FILE, VAULT_BANNER,
};
use crate::store::{self, CommandRow, ObjectRow, OccurrenceRow, SourceRow};

#[path = "csv_ingest.rs"]
mod csv_ingest;
#[path = "decide.rs"]
mod decide;
#[path = "export.rs"]
mod export;
#[path = "fragment.rs"]
mod fragment;
#[path = "html.rs"]
mod html;
#[path = "pdf.rs"]
mod pdf;
#[path = "redact.rs"]
mod redact;
#[path = "review.rs"]
mod review;

pub use csv_ingest::{submit_csv, CsvRequest};
pub use decide::{
    correct_fact, query_facts, record_manual_fact, undo_decision, CorrectRequest,
    ManualFactRequest, UndoRequest,
};
pub use export::{export_copy, ExportRequest};
pub use fragment::{query_candidates, submit_fragment, FragmentRequest};
pub use html::{submit_html, HtmlRequest};
pub use pdf::{submit_pdf, PdfRequest};
pub use redact::{redact_template, RedactRequest};
pub use review::review_sheet;

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
    "before_checkpoint",
    "before_main_verify",
    "before_publish",
    "before_final_set",
    "before_sync_vault_db",
    "before_sync_synthetic_only",
    "before_sync_writer_lock",
    "before_sync_objects",
    "before_sync_staging",
    "before_sync_journal",
    "before_sync_control",
    "before_sync_stage",
    "before_dir_publish",
    "before_parent_sync",
    "after_publish",
];
const INIT_INJECTS: &[&str] = &[
    "version_checkpoint",
    "version_verify",
    "checkpoint_unexpected",
    "writer_close",
    "verify_close",
    "publish_dir_fsync",
];
const FORMAT_1_EXCLUDED_STOPS: &[&str] = &[
    "version_set",
    "version_checkpoint",
    "before_publish",
    "after_publish",
];
const FORMAT_1_EXCLUDED_INJECTS: &[&str] = &["version_checkpoint", "version_verify"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InjectAction {
    Enospc,
    IoError,
}

#[derive(Clone, Default)]
pub struct Hooks {
    pub stop_at: Option<String>,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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

    fn stop(&mut self, point: &str, details: Map<String, Value>) -> Result<(), Fail> {
        let requested = self.hooks.stop_at.as_deref() == Some(point);
        stop_checkpoint(&self.hooks, point, details)?;
        if requested {
            self.stop_reached = true;
        }
        Ok(())
    }

    /// Writes the witness when this point was requested. Does not decide the
    /// outcome: callers before publish still report NotPreserved, and the
    /// publish-directory fsync caller reports the directory as already renamed.
    fn note_inject(&mut self, point: &str, details: Map<String, Value>) -> Result<bool, Fail> {
        if self.hooks.inject_point.as_deref() != Some(point) {
            return Ok(false);
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
        body.insert("action".into(), json!(action_name(action)));
        let bytes = serde_json::to_vec_pretty(&Value::Object(body))
            .map_err(|_| Fail::new(EXIT_USAGE, "Usage", "witness_encode"))?;
        atomic_write(&path, &bytes).map_err(|err| io_fail(err, "witness"))?;
        self.inject_reached = true;
        Ok(true)
    }

    fn inject(&mut self, point: &str, details: Map<String, Value>) -> Result<(), Fail> {
        if !self.note_inject(point, details)? {
            return Ok(());
        }
        let action = self
            .hooks
            .inject_action
            .ok_or_else(|| Fail::new(EXIT_USAGE, "Usage", "inject_action_required"))?;
        let reason = match action {
            InjectAction::Enospc => "injected_enospc",
            InjectAction::IoError => "injected_io_error",
        };
        Err(Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", reason)
            .keep_staging()
            .extra("injected", Value::Bool(true))
            .extra("inject_point", json!(point))
            .extra("inject_action", json!(action_name(action)))
            .extra(
                "note",
                json!("injected fault, not a full device and not a failed disk"),
            )
            .extra("preservation", json!(NOT_PRESERVED)))
    }
}

pub(super) fn stop_checkpoint(
    hooks: &Hooks,
    point: &str,
    mut details: Map<String, Value>,
) -> Result<(), Fail> {
    if hooks.stop_at.as_deref() != Some(point) {
        return Ok(());
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = &mut details;
        Err(
            Fail::new(EXIT_USAGE, "Usage", "stop_hook_not_supported_on_platform")
                .extra("point", json!(point)),
        )
    }
    #[cfg(target_os = "linux")]
    {
        let path = hooks.checkpoint.clone().ok_or_else(|| {
            Fail::new(EXIT_USAGE, "Usage", "checkpoint_required").extra("point", json!(point))
        })?;
        details.insert("point".into(), json!(point));
        let bytes = serde_json::to_vec_pretty(&Value::Object(details))
            .map_err(|_| Fail::new(EXIT_USAGE, "Usage", "checkpoint_encode"))?;
        atomic_write(&path, &bytes).map_err(|err| io_fail(err, "checkpoint"))?;
        sigstop();
        Ok(())
    }
}

fn action_name(action: InjectAction) -> &'static str {
    match action {
        InjectAction::Enospc => "enospc",
        InjectAction::IoError => "io_error",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaultBannerKind {
    Synthetic,
    Production(ProtectionMode),
}

struct Session {
    root: PathBuf,
    conn: Connection,
    _lock: VaultLock,
    magic: i64,
    #[allow(dead_code)]
    vault_format: i64,
    protection: Option<ProtectionMode>,
    cipher: VaultCipher,
}

pub struct InitRequest {
    pub vault: PathBuf,
    pub format: u8,
    pub protection: Option<ProtectionMode>,
    pub auth: AuthInput,
    pub recovery_key_out: Option<PathBuf>,
    pub hooks: Hooks,
}

pub struct RotatePassphraseRequest {
    pub vault: PathBuf,
    pub auth: AuthInput,
    pub new_passphrase: NewPassphraseInput,
    pub rotate_recovery_key: bool,
    pub recovery_key_out: Option<PathBuf>,
}

struct Phase {
    published: bool,
}

pub fn help_report() -> Report {
    let mut body = honesty("Help");
    body.insert(
        "note".into(),
        json!("T1 preserves original bytes in a synthetic vault. It does not arrange, search, or accept real data. fragment publishes source-stated candidates for one user-submitted text fragment. On a new format 3 vault, that same publish also records a stated paid amount and a stated service period as facts. Those facts are source statements, not verified payments, purchases, or account links. correct can replace the stated USD amount text, and undo appends a compensating decision. export writes one offline package outside the vault: originals, candidates, current facts, and the decision chain. It does not choose a protection mode. PDF extraction is not available. Security is not complete. This is not a user-ready product. format 4 takes local UTF-8 HTML files and keeps each distinct byte sequence as its own source. The same bytes reuse that source and do not create another fact. Its extraction is a limited static-text pass, not browser visibility, and it does not migrate an older vault. review writes one UTF-8 text sheet of stored facts and candidates outside the vault. It does not change them, and it is not a Windows screen. Optional --package names an existing export directory. The sheet writes originals/OBJECT only after that file exists and its bytes match the stored hash."),
    );
    body.insert(
        "commands".into(),
        json!([
            "help",
            "version",
            "init --vault PATH [--format 1|2|3|4|5|6] [--protection passphrase|os-disk] [--passphrase-file PATH|--passphrase-stdin] [--recovery-key-out PATH]",
            "rotate-passphrase --vault PATH [--passphrase-file PATH|--recovery-key-file PATH] --new-passphrase-file PATH [--rotate-recovery-key] [--recovery-key-out PATH]",
            "open --vault PATH [--passphrase-file PATH|--recovery-key-file PATH]",
            "verify --vault PATH [--passphrase-file PATH|--recovery-key-file PATH]",
            "import --vault PATH --command-id ID --media-type TYPE --source PATH [--batch-id ID] [--display-name NAME]",
            "read --vault PATH --source-id ID --out PATH",
            "marker --vault PATH --command-id ID --source-id ID --value TEXT",
            "fragment --vault PATH --command-id ID --fragment PATH [--batch-id ID]",
            "html --vault PATH --command-id ID --source PATH [--batch-id ID]",
            "pdf --vault PATH --command-id ID --source PATH [--batch-id ID]",
            "csv --vault PATH --command-id ID --source PATH [--batch-id ID]",
            "annotate --vault PATH --source-id ID [--vendor NAME] [--plan NAME] [--account LABEL] [--cycle monthly|yearly|quarterly|weekly|one_time|unknown] [--category NAME] [--bill-date YYYY-MM-DD] [--notes TEXT]",
            "record-fact --vault PATH --command-id ID --source-id ID [--currency CODE --amount-text TEXT] [--period-start YYYY-MM-DD --period-end YYYY-MM-DD] --actor-id ID",
            "candidates --vault PATH [--source-id ID]",
            "facts --vault PATH [--source-id ID] [--fact-id ID]",
            "review --vault PATH --out PATH [--package PATH]",
            "export --vault PATH --out PATH [--allow-partial yes] [--acknowledge-plaintext yes]",
            "verify-package --package PATH",
            "redact (--source PATH | --vault PATH --source-id ID) --out PATH [--redact-term STR]...",
            "correct --vault PATH --command-id ID --fact-id ID --expected-revision N --amount-text TEXT [--currency CODE] --actor-id ID",
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
            "21": EXIT_UNCERTAIN,
            "22": EXIT_AUTH
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
    body.insert("supported_vault_formats".into(), json!([1, 2, 3, 4, 5, 6]));
    body.insert("capabilities".into(), capabilities());
    body.insert("platform_limits".into(), platform_limits(None));
    Report {
        code: EXIT_OK,
        body: Value::Object(body),
    }
}

pub fn init_vault(req: InitRequest) -> Report {
    match init_inner(req) {
        Ok(report) => report,
        Err(fail) => note_uncommitted(fail).into_report(),
    }
}

fn init_inner(req: InitRequest) -> Result<Report, Fail> {
    let path = req.vault.as_path();
    let format = req.format;
    let hooks = req.hooks;
    let protection = req.protection;
    let recovery_key_out = req.recovery_key_out;
    // A trailing slash makes this lstat follow the final symlink. A dangling
    // name then looks missing, and a loop or file fails the stat itself.
    // The directory entry is the same name with those slashes removed.
    // This does not canonicalize, so the final symlink is not replaced by
    // its target. Ancestor components are unchanged.
    match fs::symlink_metadata(crate::platform::final_directory_entry(path)) {
        Ok(_) => {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "vault_path_exists")
                .extra("created_new_vault", json!(false)));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(io_fail(err, "init_stat").extra("created_new_vault", json!(false))),
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
        if protection.is_some() {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "protection_requires_format_6")
                    .extra("created_new_vault", json!(false)),
            );
        }
        if recovery_key_out.is_some() {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "recovery_key_out_requires_passphrase")
                    .extra("created_new_vault", json!(false)),
            );
        }
        let stop = hooks.stop_at.as_deref();
        let point = hooks.inject_point.as_deref();
        let stop_ok = match stop {
            None => true,
            Some(name) => INIT_STOPS.contains(&name) && !FORMAT_1_EXCLUDED_STOPS.contains(&name),
        };
        let inject_ok = match point {
            None => true,
            Some(name) => {
                INIT_INJECTS.contains(&name) && !FORMAT_1_EXCLUDED_INJECTS.contains(&name)
            }
        };
        if !stop_ok || !inject_ok {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "format_1_has_no_version_hook")
                    .extra("created_new_vault", json!(false)),
            );
        }
    } else if format == 2 || format == 3 || format == 4 || format == 5 {
        if protection.is_some() {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "protection_requires_format_6")
                    .extra("created_new_vault", json!(false)),
            );
        }
        if recovery_key_out.is_some() {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "recovery_key_out_requires_passphrase")
                    .extra("created_new_vault", json!(false)),
            );
        }
        validate_hook_names(
            hooks.stop_at.as_deref(),
            hooks.inject_point.as_deref(),
            INIT_STOPS,
            INIT_INJECTS,
            INIT_INJECTS,
        )?;
    } else if format == 6 {
        let Some(mode) = protection else {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "protection_required_for_format_6")
                    .extra("created_new_vault", json!(false)),
            );
        };
        if mode == ProtectionMode::OsDisk && recovery_key_out.is_some() {
            return Err(
                Fail::new(EXIT_USAGE, "Usage", "recovery_key_out_requires_passphrase")
                    .extra("created_new_vault", json!(false)),
            );
        }
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

    let passphrase = if format == 6 && protection == Some(ProtectionMode::Passphrase) {
        let secret = map_cred_err(req.auth.resolve_passphrase_only())?.ok_or_else(|| {
            Fail::new(EXIT_AUTH, "KeyRequired", "passphrase_required")
                .extra("created_new_vault", json!(false))
                .extra("vault_format", json!(6))
                .extra("protection", json!(crypto::PROTECTION_PASSPHRASE))
                .extra("synthetic_only", json!(false))
        })?;
        Some(secret)
    } else {
        None
    };

    if let Some(ref key_out) = recovery_key_out {
        validate_new_key_out_before_init(path, key_out)?;
    }

    let scratch = parent.join(format!(
        ".subvault-init-{}",
        ids::random_id("tmp").map_err(|err| io_fail(err, "random"))?
    ));
    fs::create_dir(&scratch).map_err(|err| io_fail(err, "init_dir"))?;
    let mut hooks = HookRun::new(hooks);
    let built = (|| -> Result<(String, Option<zeroize::Zeroizing<String>>), Fail> {
        let (banner_file, banner_text) = if format == 6 {
            let text = match protection.unwrap() {
                ProtectionMode::Passphrase => PRODUCTION_BANNER_PASSPHRASE,
                ProtectionMode::OsDisk => PRODUCTION_BANNER_OS_DISK,
            };
            (PRODUCTION_BANNER_FILE, text)
        } else {
            (SYNTHETIC_BANNER_FILE, VAULT_BANNER)
        };
        fs::write(scratch.join(banner_file), banner_text).map_err(|err| io_fail(err, "banner"))?;
        for dir in ["objects", "staging", "journal", "control"] {
            fs::create_dir(scratch.join(dir)).map_err(|err| io_fail(err, "init_dir"))?;
        }
        fs::write(scratch.join("writer.lock"), LOCK_TEXT)
            .map_err(|err| io_fail(err, "lock_file"))?;
        let conn =
            Connection::open(scratch.join("vault.db")).map_err(|err| db_fail(err, "init_open"))?;
        store::configure(&conn).map_err(|err| db_fail(err, "init_pragma"))?;
        let vault_id = ids::random_id("vlt_").map_err(|err| io_fail(err, "random"))?;
        let mut recovery_key_text = None;
        if format == 6 {
            let mode = protection.unwrap();
            store::init_real_schema(&conn, &vault_id, mode)
                .map_err(|err| db_fail(err, "init_schema"))?;
            if mode == ProtectionMode::Passphrase {
                let dek = crypto::generate_dek().map_err(crypto_init_fail)?;
                let (rec_raw, rec_str) =
                    crypto::generate_recovery_key().map_err(crypto_init_fail)?;
                let now = now_ms();
                let pass_slot = crypto::wrap_dek_with_passphrase(
                    &vault_id,
                    &dek,
                    passphrase.as_ref().expect("passphrase checked"),
                    now,
                )
                .map_err(crypto_init_fail)?;
                let rec_slot = crypto::wrap_dek_with_recovery(&vault_id, &dek, &rec_raw, now)
                    .map_err(crypto_init_fail)?;
                store::upsert_keyslot(&conn, &pass_slot)
                    .map_err(|err| db_fail(err, "init_keyslot"))?;
                store::upsert_keyslot(&conn, &rec_slot)
                    .map_err(|err| db_fail(err, "init_keyslot"))?;
                recovery_key_text = Some(rec_str);
            }
            hooks.stop("version_set", init_point(false, format))?;
            hooks.inject("version_checkpoint", init_point(false, format))?;
        } else {
            store::init_schema(&conn, &vault_id).map_err(|err| db_fail(err, "init_schema"))?;
            if format != 1 {
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
                if format == 4 {
                    conn.execute_batch(store::HTML_DDL)
                        .map_err(|err| db_fail(err, "init_schema"))?;
                    conn.execute(
                        "INSERT INTO meta(key, value) VALUES ('html_body_schema', ?1)",
                        [store::HTML_BODY_SCHEMA],
                    )
                    .map_err(|err| db_fail(err, "init_schema"))?;
                    conn.execute_batch(store::FACT_DDL)
                        .map_err(|err| db_fail(err, "init_schema"))?;
                }
                if format == 5 {
                    conn.execute_batch(store::PDF_DDL)
                        .map_err(|err| db_fail(err, "init_schema"))?;
                    conn.execute(
                        "INSERT INTO meta(key, value) VALUES ('pdf_body_schema', ?1)",
                        [store::PDF_BODY_SCHEMA],
                    )
                    .map_err(|err| db_fail(err, "init_schema"))?;
                    conn.execute_batch(store::FACT_DDL)
                        .map_err(|err| db_fail(err, "init_schema"))?;
                }
                hooks.stop("version_set", init_point(false, format))?;
                hooks.inject("version_checkpoint", init_point(false, format))?;
            }
        }
        seal_init_stage(&mut hooks, &scratch, conn, format, &vault_id, protection)?;
        Ok((vault_id, recovery_key_text))
    })();
    let (vault_id, recovery_key_text) = match built {
        Ok(pair) => pair,
        Err(fail) => {
            return Err(finish_init_scratch(
                &scratch,
                fail.extra("created_new_vault", json!(false)),
            ));
        }
    };
    // The early check is only a friendly diagnosis. This second check reports
    // a name that appeared while the private directory was being built. The
    // stop is after it, so the atomic publish is what refuses a later name.
    match fs::symlink_metadata(crate::platform::final_directory_entry(path)) {
        Ok(_) => {
            return Err(finish_init_scratch(
                &scratch,
                Fail::new(EXIT_REJECTED, "Rejected", "vault_path_exists")
                    .extra("created_new_vault", json!(false)),
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(finish_init_scratch(
                &scratch,
                io_fail(err, "init_stat").extra("created_new_vault", json!(false)),
            ));
        }
    }
    if let Err(fail) = hooks.stop("before_dir_publish", init_point(false, format)) {
        return Err(finish_init_scratch(
            &scratch,
            fail.extra("created_new_vault", json!(false)),
        ));
    }
    let mut wrote_key_out = false;
    if let (Some(ref key_out), Some(ref key_text)) = (&recovery_key_out, &recovery_key_text) {
        if let Err(fail) = write_recovery_key_file(key_out, key_text) {
            return Err(finish_init_scratch(
                &scratch,
                fail.extra("created_new_vault", json!(false)),
            ));
        }
        wrote_key_out = true;
    }
    let published = match publish_init_dir(&scratch, path) {
        Ok(dir) => dir,
        Err(fail) => {
            if wrote_key_out {
                if let Some(ref key_out) = recovery_key_out {
                    let _ = fs::remove_file(key_out);
                }
            }
            return Err(finish_init_scratch(&scratch, fail));
        }
    };
    // Rename has succeeded. Later returns are published receipts. Dropping
    // `published` only closes the handle; it does not sync.
    let mut point = init_point(true, format);
    published.checkpoint_identity(&mut point);
    if let Err(fail) = hooks.stop("before_parent_sync", point) {
        return Ok(report_published_init(
            fail, &vault_id, format, protection, magic, path, false,
        ));
    }
    // Explicit sync of the same file renameat2 used. Release it after the
    // call returns. Drop does not sync.
    if let Err(err) = published.sync() {
        drop(published);
        return Ok(report_published_init(
            io_fail(err, "publish_dir_fsync"),
            &vault_id,
            format,
            protection,
            magic,
            path,
            false,
        ));
    }
    drop(published);
    let injected = match hooks.note_inject("publish_dir_fsync", init_point(true, format)) {
        Ok(value) => value,
        Err(fail) => {
            return Ok(report_published_init(
                fail, &vault_id, format, protection, magic, path, true,
            ));
        }
    };
    if injected {
        let Some(action) = hooks.hooks.inject_action else {
            return Ok(report_published_init(
                Fail::new(EXIT_INTEGRITY, "Initialized", "inject_action_required")
                    .extra("during", json!("publish_dir_fsync"))
                    .extra("injected", json!(true)),
                &vault_id,
                format,
                protection,
                magic,
                path,
                true,
            ));
        };
        return Ok(report_published_init(
            directory_witness("publish_dir_fsync", action),
            &vault_id,
            format,
            protection,
            magic,
            path,
            true,
        ));
    }
    // Sync on the rename parent file has returned Ok. A later checkpoint
    // write stays published. directory_durable=false means final durability
    // is unconfirmed. It does not rewrite that Ok.
    match hooks.stop("after_publish", init_point(true, format)) {
        Ok(()) => Ok(init_ready(
            &vault_id,
            format,
            protection,
            magic,
            recovery_key_out.as_deref(),
            recovery_key_text.as_deref().map(String::as_str),
        )),
        Err(fail) => Ok(report_published_init(
            fail, &vault_id, format, protection, magic, path, true,
        )),
    }
}

fn publish_init_dir(scratch: &Path, path: &Path) -> Result<crate::platform::PublishedDir, Fail> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_file = crate::platform::open_dir_for_publish(parent)
        .map_err(|err| io_fail(err, "init_rename").extra("created_new_vault", json!(false)))?;
    let from = scratch.file_name().ok_or_else(|| {
        io_fail(
            io::Error::new(io::ErrorKind::InvalidInput, "init staging has no file name"),
            "init_rename",
        )
        .extra("created_new_vault", json!(false))
    })?;
    let destination = crate::platform::final_directory_entry(path);
    let to = destination.file_name().ok_or_else(|| {
        io_fail(
            io::Error::new(io::ErrorKind::InvalidInput, "init target has no file name"),
            "init_rename",
        )
        .extra("created_new_vault", json!(false))
    })?;
    crate::platform::rename_dir_noreplace(&parent_file, from, to).map_err(init_rename_failure)?;
    Ok(crate::platform::PublishedDir::hold(parent_file))
}

fn init_rename_failure(err: io::Error) -> Fail {
    match crate::platform::classify_rename_dir(&err) {
        crate::platform::RenameDirClass::Exists => {
            Fail::new(EXIT_REJECTED, "Rejected", "vault_path_exists")
                .extra("created_new_vault", json!(false))
                .extra("during", json!("init_rename"))
                .extra("os_error", json!(err.to_string()))
                .extra("injected", json!(false))
        }
        crate::platform::RenameDirClass::Unavailable => Fail::new(
            EXIT_UNSUPPORTED_FS,
            "Unsupported",
            "rename_noreplace_unavailable",
        )
        .extra("created_new_vault", json!(false))
        .extra("during", json!("init_rename"))
        .extra("os_error", json!(err.to_string()))
        .extra("injected", json!(false)),
        crate::platform::RenameDirClass::NoSpace
        | crate::platform::RenameDirClass::Permission
        | crate::platform::RenameDirClass::Busy
        | crate::platform::RenameDirClass::Invalid
        | crate::platform::RenameDirClass::CrossDevice
        | crate::platform::RenameDirClass::Other => {
            io_fail(err, "init_rename").extra("created_new_vault", json!(false))
        }
    }
}

const STAGING_CLEANUP_REASON: &str = "automatic path deletion is disabled because a path is not ownership; this is the original staging path, and its current location and current owner are unknown";

/// The private directory exists. Failure does not delete that path: a name is
/// not ownership, so a recursive delete could remove a replacement. The receipt
/// says deletion was not attempted and repeats the original path. It does not
/// say that a directory is still there, where it is, or who it belongs to.
/// There is no `Drop` that deletes this directory.
fn finish_init_scratch(scratch: &Path, fail: Fail) -> Fail {
    fail.extra("staging_cleanup", json!("not_attempted"))
        .extra("staging_cleanup_reason", json!(STAGING_CLEANUP_REASON))
        .extra("staging_path", json!(scratch.display().to_string()))
}

fn init_ready(
    vault_id: &str,
    format: u8,
    protection: Option<ProtectionMode>,
    magic: i64,
    recovery_key_out: Option<&Path>,
    recovery_key_text: Option<&str>,
) -> Report {
    let mut body = init_body(vault_id, format, protection, magic);
    if let Some(out) = recovery_key_out {
        body.insert("recovery_key_out".into(), json!(out.display().to_string()));
    } else if let Some(key) = recovery_key_text {
        body.insert("recovery_key".into(), json!(key));
    }
    Report {
        code: EXIT_OK,
        body: Value::Object(body),
    }
}

fn init_body(
    vault_id: &str,
    format: u8,
    protection: Option<ProtectionMode>,
    magic: i64,
) -> Map<String, Value> {
    let mut body = honesty("Initialized");
    body.insert("vault_id".into(), json!(vault_id));
    body.insert("vault_format".into(), json!(format));
    body.insert("created_new_vault".into(), json!(true));
    body.insert(
        "protection".into(),
        json!(if format == 6 {
            protection
                .map(ProtectionMode::as_meta)
                .unwrap_or(crypto::PROTECTION_OS_DISK)
        } else {
            crypto::PROTECTION_SYNTHETIC
        }),
    );
    body.insert(
        "primary_input".into(),
        json!(if format == 6 {
            "local_static_files"
        } else {
            "unanswered"
        }),
    );
    body.insert("capabilities".into(), capabilities_for(i64::from(format)));
    body.insert("platform_limits".into(), platform_limits(Some(magic)));
    body.insert("new_effect".into(), json!(true));
    apply_format_honesty(&mut body, i64::from(format), protection);
    body
}

fn directory_witness(point: &str, action: InjectAction) -> Fail {
    let reason = match action {
        InjectAction::Enospc => "injected_enospc",
        InjectAction::IoError => "injected_io_error",
    };
    Fail::new(EXIT_INTEGRITY, "Initialized", reason)
        .extra("during", json!(point))
        .extra("injected", json!(true))
        .extra("inject_point", json!(point))
        .extra("inject_action", json!(action_name(action)))
        .extra(
            "note",
            json!("injected fault, not a full device and not a failed disk"),
        )
}

/// Published-directory receipt. The rename has already succeeded.
///
/// Extras are copied first. Published facts are written after that, so a
/// failure's `new_effect=false` cannot cover them. Staging-cleanup keys are
/// dropped. `parent_fsync_returned_ok` is true only when `sync_all` on the
/// rename parent file returned `Ok`. `directory_durable=false` means final
/// directory durability is unconfirmed. It does not mean the sync call failed.
/// `output` is the requested path string. It is not updated if that parent is
/// later renamed, and it is not an identity check.
fn report_published_init(
    fail: Fail,
    vault_id: &str,
    format: u8,
    protection: Option<ProtectionMode>,
    magic: i64,
    output: &Path,
    parent_fsync_returned_ok: bool,
) -> Report {
    let mut body = init_body(vault_id, format, protection, magic);
    for (key, value) in fail.extra {
        if matches!(
            key.as_str(),
            "preservation"
                | "staging_cleanup"
                | "staging_cleanup_reason"
                | "staging_path"
                | "output_cleanup"
                | "output_cleanup_reason"
                | "output_staging_path"
        ) {
            continue;
        }
        body.insert(key, value);
    }
    body.insert("outcome".into(), json!("Initialized"));
    body.insert("reason".into(), json!(fail.reason));
    body.insert("published".into(), json!(true));
    body.insert("created_new_vault".into(), json!(true));
    body.insert("new_effect".into(), json!(true));
    body.insert("directory_durable".into(), json!(false));
    body.insert(
        "parent_fsync_returned_ok".into(),
        json!(parent_fsync_returned_ok),
    );
    body.insert("output".into(), json!(output.display().to_string()));
    Report {
        code: EXIT_INTEGRITY,
        body: Value::Object(body),
    }
}

fn init_point(published: bool, format: u8) -> Map<String, Value> {
    map_of(&[
        ("published", json!(published)),
        ("vault_format", json!(format)),
    ])
}

fn unpublished(fail: Fail) -> Fail {
    fail.extra("created_new_vault", json!(false))
        .extra("published", json!(false))
}

fn seal_init_stage(
    hooks: &mut HookRun,
    scratch: &Path,
    conn: Connection,
    format: u8,
    vault_id: &str,
    protection: Option<ProtectionMode>,
) -> Result<(), Fail> {
    let mode = match store::read_journal_mode(&conn) {
        store::SqlAnswer::Value(mode) if mode == "wal" => mode,
        other => return Err(journal_mode_fail(other)),
    };
    hooks.stop("before_checkpoint", init_point(false, format))?;
    if let Some(fail) = injected_checkpoint_row(hooks, format)? {
        return Err(fail);
    }
    let row = match store::read_truncate_checkpoint(&conn) {
        store::SqlAnswer::Value(row) => row,
        other => return Err(checkpoint_read_fail(other)),
    };
    match store::judge_truncate_checkpoint(&mode, row) {
        store::TruncateVerdict::Complete => {}
        store::TruncateVerdict::Busy(row) => {
            return Err(checkpoint_row_fail(row, "checkpoint_busy"))
        }
        store::TruncateVerdict::Abnormal(row) => {
            return Err(checkpoint_row_fail(row, "checkpoint_status_unexpected"));
        }
    }
    hooks
        .inject("writer_close", init_point(false, format))
        .map_err(|fail| fail.extra("during", json!("writer_close")))?;
    close_checked(conn, "writer_close")?;

    hooks.stop("before_main_verify", init_point(false, format))?;
    if format != 1 {
        hooks.stop("version_checkpoint", init_point(false, format))?;
    }
    let verify = open_immutable(&scratch.join("vault.db"))?;
    let verdict =
        read_main_meta(&verify).and_then(|main| compare_main(format, vault_id, protection, &main));
    if let Err(fail) = verdict {
        return Err(prefer_verify_error(verify, fail));
    }
    hooks
        .inject("verify_close", init_point(false, format))
        .map_err(|fail| fail.extra("during", json!("verify_close")))?;
    close_checked(verify, "verify_close")?;
    if format != 1 {
        hooks.inject("version_verify", init_point(false, format))?;
        hooks.stop("before_publish", init_point(false, format))?;
    }
    hooks.stop("before_final_set", init_point(false, format))?;
    let banner_file = if format == 6 {
        PRODUCTION_BANNER_FILE
    } else {
        SYNTHETIC_BANNER_FILE
    };
    require_final_set(scratch, banner_file)?;
    for (stop, during, name) in [
        ("before_sync_vault_db", "sync_vault_db", "vault.db"),
        (
            "before_sync_synthetic_only",
            "sync_synthetic_only",
            banner_file,
        ),
        ("before_sync_writer_lock", "sync_writer_lock", "writer.lock"),
    ] {
        hooks.stop(stop, init_point(false, format))?;
        sync_regular_file(&scratch.join(name), during)?;
    }
    for (stop, during, name) in [
        ("before_sync_objects", "sync_objects", "objects"),
        ("before_sync_staging", "sync_staging", "staging"),
        ("before_sync_journal", "sync_journal", "journal"),
        ("before_sync_control", "sync_control", "control"),
    ] {
        hooks.stop(stop, init_point(false, format))?;
        sync_directory(&scratch.join(name), during)?;
    }
    hooks.stop("before_sync_stage", init_point(false, format))?;
    sync_directory(scratch, "sync_stage")
}

fn journal_mode_fail(answer: store::SqlAnswer<String>) -> Fail {
    match answer {
        store::SqlAnswer::Sqlite(err) => unpublished(db_fail(err, "journal_mode")),
        store::SqlAnswer::Shape(shape) => unpublished(Fail::new(
            EXIT_NOT_PRESERVED,
            "NotPreserved",
            "journal_mode_status_unexpected",
        ))
        .extra("during", json!("journal_mode"))
        .extra("injected", json!(false))
        .extra("journal_mode_shape", json!(shape)),
        store::SqlAnswer::Value(mode) => unpublished(Fail::new(
            EXIT_NOT_PRESERVED,
            "NotPreserved",
            "journal_mode_not_wal",
        ))
        .extra("during", json!("journal_mode"))
        .extra("injected", json!(false))
        .extra("journal_mode", json!(mode)),
    }
}

fn checkpoint_row_fail(row: store::CheckpointRow, reason: &'static str) -> Fail {
    unpublished(Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", reason))
        .extra("during", json!("init_checkpoint"))
        .extra("injected", json!(false))
        .extra("checkpoint_busy", json!(row.busy))
        .extra("checkpoint_log", json!(row.log))
        .extra("checkpoint_checkpointed", json!(row.checkpointed))
}

fn checkpoint_read_fail(answer: store::SqlAnswer<store::CheckpointRow>) -> Fail {
    match answer {
        store::SqlAnswer::Sqlite(err) => unpublished(db_fail(err, "init_checkpoint")),
        store::SqlAnswer::Shape(shape) => unpublished(Fail::new(
            EXIT_NOT_PRESERVED,
            "NotPreserved",
            "checkpoint_status_unexpected",
        ))
        .extra("during", json!("init_checkpoint"))
        .extra("injected", json!(false))
        .extra("checkpoint_shape", json!(shape)),
        store::SqlAnswer::Value(row) => checkpoint_row_fail(row, "checkpoint_status_unexpected"),
    }
}

/// The row is chosen here so the judge runs on a non-success TRUNCATE shape.
/// It is not a row SQLite returned.
fn injected_checkpoint_row(hooks: &mut HookRun, format: u8) -> Result<Option<Fail>, Fail> {
    if !hooks.note_inject("checkpoint_unexpected", init_point(false, format))? {
        return Ok(None);
    }
    let row = store::CheckpointRow {
        busy: 0,
        log: 1,
        checkpointed: 0,
    };
    let fail = match store::judge_truncate_checkpoint("wal", row) {
        store::TruncateVerdict::Abnormal(row) => {
            checkpoint_row_fail(row, "checkpoint_status_unexpected")
        }
        store::TruncateVerdict::Busy(row) => checkpoint_row_fail(row, "checkpoint_busy"),
        store::TruncateVerdict::Complete => {
            return Err(Fail::new(
                EXIT_USAGE,
                "Usage",
                "checkpoint_injection_not_abnormal",
            ));
        }
    };
    Ok(Some(
        fail.extra("injected", json!(true))
            .extra("inject_point", json!("checkpoint_unexpected"))
            .extra(
                "note",
                json!("injected checkpoint status row, not a return from SQLite"),
            ),
    ))
}

/// `Connection::close` reports the first `sqlite3_close` result. On failure the
/// connection is returned and dropped; that later drop is not a second success.
fn close_checked(conn: Connection, during: &'static str) -> Result<(), Fail> {
    match conn.close() {
        Ok(()) => Ok(()),
        Err((conn, err)) => {
            drop(conn);
            Err(unpublished(db_fail(err, during)))
        }
    }
}

fn prefer_verify_error(conn: Connection, main: Fail) -> Fail {
    match conn.close() {
        Ok(()) => main,
        Err((conn, err)) => {
            drop(conn);
            main.extra("verify_close_error", json!(err.to_string()))
        }
    }
}

fn open_immutable(db: &Path) -> Result<Connection, Fail> {
    let uri = readonly_uri(db)?;
    Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| {
        unpublished(Fail::new(
            EXIT_DAMAGED,
            "ReadOnlyDamaged",
            "sqlite_unreadable",
        ))
        .extra("during", json!("init_verify"))
    })
}

struct MainMeta {
    version: i64,
    values: Vec<(&'static str, Option<String>)>,
}

fn read_main_meta(conn: &Connection) -> Result<MainMeta, Fail> {
    let version =
        store::user_version(conn).map_err(|_| verify_damaged("user_version_unreadable"))?;
    let mut values = Vec::new();
    for key in [
        "schema_version",
        "vault_format",
        "vault_id",
        "synthetic_only",
        "disposable",
        "protection",
        "primary_input",
        "slice",
        "body_schema",
        "html_body_schema",
        "pdf_body_schema",
        "real_body_schema",
    ] {
        let value = store::meta(conn, key).map_err(|_| verify_damaged("meta_unreadable"))?;
        values.push((key, value));
    }
    Ok(MainMeta { version, values })
}

fn verify_damaged(reason: &'static str) -> Fail {
    unpublished(Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", reason))
        .extra("during", json!("init_verify"))
}

fn meta_value<'a>(values: &'a [(&'static str, Option<String>)], key: &str) -> Option<&'a str> {
    values
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, value)| value.as_deref())
}

fn compare_main(
    format: u8,
    vault_id: &str,
    protection: Option<ProtectionMode>,
    main: &MainMeta,
) -> Result<(), Fail> {
    let schema = meta_value(&main.values, "schema_version");
    let companion = meta_value(&main.values, "vault_format");
    let format_text = format.to_string();
    let format_ok = format == 1 || companion == Some(format_text.as_str());
    if main.version != i64::from(format) || schema != Some("1") || !format_ok {
        return Err(unpublished(Fail::new(
            EXIT_VERSION,
            "VersionUnsupported",
            "version_not_in_main_db",
        ))
        .extra("during", json!("init_verify"))
        .extra("schema_version_found", json!(main.version))
        .extra("vault_format_seen", json!(companion)));
    }
    let expected_meta: [(&str, &str); 6] = if format == 6 {
        [
            ("vault_id", vault_id),
            ("synthetic_only", "false"),
            ("disposable", "false"),
            (
                "protection",
                protection
                    .map(ProtectionMode::as_meta)
                    .unwrap_or(crypto::PROTECTION_OS_DISK),
            ),
            ("primary_input", "local_static_files"),
            ("slice", "M4-real"),
        ]
    } else {
        [
            ("vault_id", vault_id),
            ("synthetic_only", "true"),
            ("disposable", "true"),
            ("protection", crypto::PROTECTION_SYNTHETIC),
            ("primary_input", "unanswered"),
            ("slice", "T1"),
        ]
    };
    for (key, expected) in expected_meta {
        let seen = meta_value(&main.values, key);
        if seen == Some(expected) {
            continue;
        }
        let reason = if key == "vault_id" {
            "vault_id_mismatch"
        } else {
            "meta_mismatch"
        };
        return Err(
            unpublished(Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", reason))
                .extra("during", json!("init_verify"))
                .extra("meta_key", json!(key))
                .extra("meta_seen", json!(seen)),
        );
    }
    if format == 3 {
        let seen = meta_value(&main.values, "body_schema");
        if seen != Some(store::BODY_SCHEMA) {
            return Err(unpublished(Fail::new(
                EXIT_NOT_PRESERVED,
                "NotPreserved",
                "body_schema_mismatch",
            ))
            .extra("during", json!("init_verify"))
            .extra("meta_key", json!("body_schema"))
            .extra("meta_seen", json!(seen)));
        }
    }
    if format == 4 {
        let plain = meta_value(&main.values, "body_schema");
        if plain.is_some() {
            return Err(unpublished(Fail::new(
                EXIT_NOT_PRESERVED,
                "NotPreserved",
                "body_schema_mismatch",
            ))
            .extra("during", json!("init_verify"))
            .extra("meta_key", json!("body_schema"))
            .extra("meta_seen", json!(plain)));
        }
        let seen = meta_value(&main.values, "html_body_schema");
        if seen != Some(store::HTML_BODY_SCHEMA) {
            return Err(unpublished(Fail::new(
                EXIT_NOT_PRESERVED,
                "NotPreserved",
                "html_body_schema_mismatch",
            ))
            .extra("during", json!("init_verify"))
            .extra("meta_key", json!("html_body_schema"))
            .extra("meta_seen", json!(seen)));
        }
    }
    if format == 5 {
        let plain = meta_value(&main.values, "body_schema");
        if plain.is_some() {
            return Err(unpublished(Fail::new(
                EXIT_NOT_PRESERVED,
                "NotPreserved",
                "body_schema_mismatch",
            ))
            .extra("during", json!("init_verify"))
            .extra("meta_key", json!("body_schema"))
            .extra("meta_seen", json!(plain)));
        }
        let seen = meta_value(&main.values, "pdf_body_schema");
        if seen != Some(store::PDF_BODY_SCHEMA) {
            return Err(unpublished(Fail::new(
                EXIT_NOT_PRESERVED,
                "NotPreserved",
                "pdf_body_schema_mismatch",
            ))
            .extra("during", json!("init_verify"))
            .extra("meta_key", json!("pdf_body_schema"))
            .extra("meta_seen", json!(seen)));
        }
    }
    if format == 6 {
        let seen = meta_value(&main.values, "real_body_schema");
        if seen != Some(store::REAL_BODY_SCHEMA) {
            return Err(unpublished(Fail::new(
                EXIT_NOT_PRESERVED,
                "NotPreserved",
                "real_body_schema_mismatch",
            ))
            .extra("during", json!("init_verify"))
            .extra("meta_key", json!("real_body_schema"))
            .extra("meta_seen", json!(seen)));
        }
    }
    Ok(())
}

fn require_final_set(scratch: &Path, banner_file: &str) -> Result<(), Fail> {
    let mut seen = Vec::new();
    for entry in fs::read_dir(scratch).map_err(|err| unpublished(io_fail(err, "final_set")))? {
        let entry = entry.map_err(|err| unpublished(io_fail(err, "final_set")))?;
        seen.push(entry.file_name().to_string_lossy().into_owned());
    }
    seen.sort();
    let expected = [
        banner_file,
        "control",
        "journal",
        "objects",
        "staging",
        "vault.db",
        "writer.lock",
    ];
    let mut unexpected: Vec<String> = seen
        .iter()
        .filter(|name| !expected.contains(&name.as_str()))
        .cloned()
        .collect();
    let missing: Vec<String> = expected
        .iter()
        .filter(|name| !seen.iter().any(|got| got == *name))
        .map(|name| (*name).to_string())
        .collect();
    for name in [banner_file, "vault.db", "writer.lock"] {
        if missing.iter().any(|item| item == name) {
            continue;
        }
        let meta = fs::symlink_metadata(scratch.join(name))
            .map_err(|err| unpublished(io_fail(err, "final_set")))?;
        if !meta.file_type().is_file() {
            unexpected.push(format!("{name}:not_a_file"));
        }
    }
    for name in ["objects", "staging", "journal", "control"] {
        if missing.iter().any(|item| item == name) {
            continue;
        }
        let path = scratch.join(name);
        let meta =
            fs::symlink_metadata(&path).map_err(|err| unpublished(io_fail(err, "final_set")))?;
        if !meta.file_type().is_dir() {
            unexpected.push(format!("{name}:not_a_directory"));
            continue;
        }
        let mut nested = Vec::new();
        for entry in fs::read_dir(&path).map_err(|err| unpublished(io_fail(err, "final_set")))? {
            let entry = entry.map_err(|err| unpublished(io_fail(err, "final_set")))?;
            nested.push(entry.file_name().to_string_lossy().into_owned());
        }
        nested.sort();
        for child in nested {
            unexpected.push(format!("{name}/{child}"));
        }
    }
    if unexpected.is_empty() && missing.is_empty() {
        return Ok(());
    }
    Err(unpublished(Fail::new(
        EXIT_NOT_PRESERVED,
        "NotPreserved",
        "stage_set_mismatch",
    ))
    .extra("during", json!("final_set"))
    .extra("injected", json!(false))
    .extra("unexpected", json!(unexpected))
    .extra("missing", json!(missing)))
}

fn sync_regular_file(path: &Path, during: &'static str) -> Result<(), Fail> {
    crate::platform::sync_file(path).map_err(|err| unpublished(io_fail(err, during)))
}

fn sync_directory(path: &Path, during: &'static str) -> Result<(), Fail> {
    fsutil::fsync_dir(path).map_err(|err| unpublished(io_fail(err, during)))
}

pub fn open_vault(path: &Path, auth: AuthInput) -> Report {
    reopen(path, &auth)
}

pub fn verify_vault(path: &Path, auth: AuthInput) -> Report {
    reopen(path, &auth)
}

pub fn recover_vault(path: &Path, auth: AuthInput) -> Report {
    reopen(path, &auth)
}

fn reopen(path: &Path, auth: &AuthInput) -> Report {
    match reopen_inner(path, auth) {
        Ok(report) => report,
        Err(fail) => demote_unscoped(fail).into_report(),
    }
}

fn reopen_inner(path: &Path, auth: &AuthInput) -> Result<Report, Fail> {
    let session = open_session_with_auth(path, auth)?;
    let notices = journal::clean_temps(&session.root);
    let reconciled = journal::reconcile_with_cipher(&session.root, &session.conn, &session.cipher)?;
    let decision_misses =
        decide::reconcile_decision_journals(&session.root, &session.conn, &session.cipher)?;
    let fragment_misses =
        decide::reconcile_fragment_journals(&session.root, &session.conn, &session.cipher)?;
    let mut inspection = crate::inspect::report(
        &session.root,
        &session.conn,
        session.magic,
        &session.cipher,
        session.protection,
    )?;
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

pub struct AnnotateRequest {
    pub vault: PathBuf,
    pub source_id: String,
    pub vendor: Option<String>,
    pub plan_name: Option<String>,
    pub account_label: Option<String>,
    pub billing_cycle: Option<String>,
    pub category: Option<String>,
    pub bill_date: Option<String>,
    pub notes: Option<String>,
    pub asset_kind: Option<String>,
    pub warranty_end: Option<String>,
    pub license_key: Option<String>,
    pub license_scope: Option<String>,
    pub reimburse_status: Option<String>,
    pub auth: AuthInput,
}

pub fn annotate_source(req: AnnotateRequest) -> Report {
    match annotate_inner(req) {
        Ok(report) => report,
        Err(fail) => note_uncommitted(fail).into_report(),
    }
}

fn annotate_inner(req: AnnotateRequest) -> Result<Report, Fail> {
    if !is_prefixed_id(&req.source_id, "src_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
    }
    if let Some(ref cycle) = req.billing_cycle {
        if !matches!(
            cycle.as_str(),
            "monthly" | "yearly" | "quarterly" | "weekly" | "one_time" | "unknown"
        ) {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_billing_cycle"));
        }
    }
    if let Some(ref kind) = req.asset_kind {
        if !matches!(
            kind.as_str(),
            "subscription" | "license" | "hardware" | "reimbursable" | "general"
        ) {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_asset_kind"));
        }
    }
    if let Some(ref reimb) = req.reimburse_status {
        if !matches!(
            reimb.as_str(),
            "none" | "pending" | "submitted" | "reimbursed"
        ) {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_reimburse_status"));
        }
    }
    if let Some(ref date) = req.bill_date {
        if !date.is_empty() && crate::real_extract::parse_date_token(date).is_none() {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_bill_date"));
        }
    }
    if let Some(ref date) = req.warranty_end {
        if !date.is_empty() && crate::real_extract::parse_date_token(date).is_none() {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_warranty_end"));
        }
    }
    let session = open_real_session(&req.vault, &req.auth)?;
    let _source = store::source_by_id_with_cipher(&session.conn, &req.source_id, &session.cipher)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_REJECTED, "Rejected", "unknown_source").extra("new_effect", json!(false))
        })?;
    let existing =
        store::get_annotation_with_cipher(&session.conn, &req.source_id, &session.cipher)
            .map_err(|err| unseen_db(err, "annotation_lookup"))?
            .unwrap_or_else(|| store::SourceAnnotation {
                source_id: req.source_id.clone(),
                billing_cycle: "unknown".into(),
                asset_kind: "general".into(),
                reimburse_status: "none".into(),
                ..Default::default()
            });

    let clean_opt = |incoming: Option<String>, current: Option<String>| -> Option<String> {
        match incoming {
            Some(v) => {
                let t = v.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            None => current,
        }
    };
    let clean_date = |incoming: Option<String>, current: Option<String>| -> Option<String> {
        match incoming {
            Some(v) => {
                let t = v.trim();
                if t.is_empty() {
                    None
                } else {
                    crate::real_extract::parse_date_token(t)
                }
            }
            None => current,
        }
    };

    let updated = store::SourceAnnotation {
        source_id: req.source_id.clone(),
        vendor: clean_opt(req.vendor, existing.vendor),
        plan_name: clean_opt(req.plan_name, existing.plan_name),
        account_label: clean_opt(req.account_label, existing.account_label),
        billing_cycle: req.billing_cycle.unwrap_or(existing.billing_cycle),
        category: clean_opt(req.category, existing.category),
        bill_date: clean_date(req.bill_date, existing.bill_date),
        notes: clean_opt(req.notes, existing.notes),
        user_edited: true,
        updated_at: now_ms(),
        asset_kind: req.asset_kind.unwrap_or(existing.asset_kind),
        warranty_end: clean_date(req.warranty_end, existing.warranty_end),
        license_key: clean_opt(req.license_key, existing.license_key),
        license_scope: clean_opt(req.license_scope, existing.license_scope),
        reimburse_status: req.reimburse_status.unwrap_or(existing.reimburse_status),
    };
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "annotate_begin"))?;
    store::upsert_annotation_with_cipher(&tx, &updated, &session.cipher)
        .map_err(|err| db_fail(err, "upsert_annotation"))?;
    tx.commit().map_err(|err| db_fail(err, "annotate_commit"))?;
    let _ = store::checkpoint(&session.conn);

    let mut body = honesty("Annotated");
    body.insert("slice".into(), json!("M4-real"));
    body.insert("source_id".into(), json!(updated.source_id));
    body.insert(
        "annotation".into(),
        json!({
            "source_id": updated.source_id,
            "vendor": updated.vendor,
            "plan_name": updated.plan_name,
            "account_label": updated.account_label,
            "billing_cycle": updated.billing_cycle,
            "category": updated.category,
            "bill_date": updated.bill_date,
            "notes": updated.notes,
            "asset_kind": updated.asset_kind,
            "warranty_end": updated.warranty_end,
            "license_scope": updated.license_scope,
            "has_license_key": updated.license_key.is_some(),
            "reimburse_status": updated.reimburse_status,
            "user_edited": updated.user_edited,
            "updated_at": updated.updated_at,
        }),
    );
    body.insert("new_effect".into(), json!(true));
    apply_format_honesty(&mut body, session.vault_format, session.protection);
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

#[allow(dead_code)]
pub(crate) struct UnlockedVault {
    pub vault_id: String,
    pub vault_format: i64,
    pub protection: Option<ProtectionMode>,
    pub cipher: VaultCipher,
}

#[allow(dead_code)]
pub(crate) fn unlock_vault_session(path: &Path, auth: &AuthInput) -> Result<UnlockedVault, Fail> {
    let session = open_real_session(path, auth)?;
    let vault_id = store::meta(&session.conn, "vault_id")
        .map_err(|err| unseen_db(err, "vault_id"))?
        .unwrap_or_default();
    Ok(UnlockedVault {
        vault_id,
        vault_format: session.vault_format,
        protection: session.protection,
        cipher: session.cipher,
    })
}

#[allow(dead_code)]
pub(crate) fn list_annotations(
    path: &Path,
    auth: &AuthInput,
) -> Result<Vec<store::SourceAnnotation>, Fail> {
    let session = open_session_with_auth(path, auth)?;
    store::list_annotations_with_cipher(&session.conn, &session.cipher)
        .map_err(|err| unseen_db(err, "list_annotations"))
}

#[allow(dead_code)]
pub(crate) fn read_source_bytes(
    path: &Path,
    source_id: &str,
    auth: &AuthInput,
) -> Result<(Vec<u8>, String, String), Fail> {
    if !is_prefixed_id(source_id, "src_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
    }
    let session = open_session_with_auth(path, auth)?;
    let source = store::source_by_id_with_cipher(&session.conn, source_id, &session.cipher)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| Fail::new(EXIT_REJECTED, "Rejected", "unknown_source"))?;
    let Some(relpath) = canonical_object_relpath(&source.object_id) else {
        return Err(Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "object_id_rejected",
        ));
    };
    let obj_path = session.root.join(&relpath);
    let bytes = session
        .cipher
        .decrypt_object_to_vec(&source.object_id, &obj_path)
        .map_err(|err| match err {
            CryptoError::Io(io_err) => io_fail(io_err, "read_object"),
            other => crate::model::cipher_tamper_fail(other, "decrypt_object"),
        })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    if hex_encode(&hasher.finalize()) != source.content_sha256 {
        return Err(Fail::new(
            EXIT_RECOVERY,
            "RecoveryRequired",
            "hash_mismatch",
        ));
    }
    let display_name = store::list_occurrences_with_cipher(&session.conn, &session.cipher)
        .ok()
        .and_then(|occs| {
            occs.into_iter()
                .find(|o| o.source_id == source.source_id)
                .map(|o| o.display_name)
        })
        .unwrap_or_else(|| format!("{}.bin", source.source_id));
    Ok((bytes, source.media_type, display_name))
}

#[allow(dead_code)]
pub(crate) fn delete_sources(
    path: &Path,
    source_ids: &[String],
    auth: &AuthInput,
) -> Result<usize, Fail> {
    if source_ids.is_empty() {
        return Ok(0);
    }
    for sid in source_ids {
        if !is_prefixed_id(sid, "src_") {
            return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
        }
    }
    let session = open_real_session(path, auth)?;
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|err| db_fail(err, "delete_sources_begin"))?;
    let (deleted, orphaned_objects) = store::delete_sources_atomic(&tx, source_ids)
        .map_err(|err| db_fail(err, "delete_sources"))?;
    tx.commit()
        .map_err(|err| db_fail(err, "delete_sources_commit"))?;
    let _ = store::checkpoint(&session.conn);

    for obj_id in orphaned_objects {
        if let Some(relpath) = canonical_object_relpath(&obj_id) {
            let _ = fs::remove_file(session.root.join(relpath));
        }
    }
    Ok(deleted)
}

pub fn rotate_passphrase(req: RotatePassphraseRequest) -> Report {
    match rotate_passphrase_inner(req) {
        Ok(report) => report,
        Err(fail) => note_uncommitted(fail).into_report(),
    }
}

fn rotate_passphrase_inner(req: RotatePassphraseRequest) -> Result<Report, Fail> {
    if req.recovery_key_out.is_some() && !req.rotate_recovery_key {
        return Err(Fail::new(
            EXIT_USAGE,
            "Usage",
            "recovery_key_out_requires_rotate_recovery_key",
        ));
    }
    let new_pass = map_cred_err(req.new_passphrase.resolve())?.ok_or_else(|| {
        Fail::new(EXIT_AUTH, "KeyRequired", "new_passphrase_required")
            .extra("new_effect", json!(false))
    })?;
    let session = open_session_with_auth(&req.vault, &req.auth)?;
    if session.vault_format < 6 || session.protection != Some(ProtectionMode::Passphrase) {
        return Err(
            Fail::new(EXIT_REJECTED, "Rejected", "vault_not_passphrase_encrypted")
                .extra("vault_format", json!(session.vault_format))
                .extra("new_effect", json!(false)),
        );
    }
    if let Some(ref key_out) = req.recovery_key_out {
        match place_output(&session.root, key_out) {
            Place::Inside => {
                return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_inside_vault")
                    .extra("new_effect", json!(false)));
            }
            Place::Unresolved(msg) => {
                return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_unresolved")
                    .extra("os_error", json!(msg))
                    .extra("new_effect", json!(false)));
            }
            Place::MissingParent => {
                return Err(
                    Fail::new(EXIT_REJECTED, "Rejected", "output_parent_missing")
                        .extra("new_effect", json!(false)),
                );
            }
            Place::Exists => {
                return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_exists")
                    .extra("new_effect", json!(false)));
            }
            Place::Create => {}
        }
    }
    let keys = session.cipher.keys().ok_or_else(|| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "missing_vault_keys")
            .extra("new_effect", json!(false))
    })?;
    let now = now_ms();
    let new_pass_slot =
        crypto::wrap_dek_with_passphrase(keys.vault_id(), keys.dek(), &new_pass, now)
            .map_err(crypto_init_fail)?;
    let new_recovery = if req.rotate_recovery_key {
        let (rec_raw, rec_str) = crypto::generate_recovery_key().map_err(crypto_init_fail)?;
        let rec_slot = crypto::wrap_dek_with_recovery(keys.vault_id(), keys.dek(), &rec_raw, now)
            .map_err(crypto_init_fail)?;
        Some((rec_slot, rec_str))
    } else {
        None
    };

    if let (Some(ref key_out), Some((_, ref rec_str))) = (&req.recovery_key_out, &new_recovery) {
        write_recovery_key_file(key_out, rec_str)?;
    }

    let commit_res: Result<(), Fail> = (|| {
        let tx = session
            .conn
            .unchecked_transaction()
            .map_err(|err| db_fail(err, "rotate_begin"))?;
        store::upsert_keyslot(&tx, &new_pass_slot)
            .map_err(|err| db_fail(err, "rotate_passphrase"))?;
        if let Some((ref rec_slot, _)) = new_recovery {
            store::upsert_keyslot(&tx, rec_slot).map_err(|err| db_fail(err, "rotate_recovery"))?;
        }
        tx.commit().map_err(|err| db_fail(err, "rotate_commit"))?;
        let _ = store::checkpoint(&session.conn);
        Ok(())
    })();

    if let Err(fail) = commit_res {
        if let Some(ref key_out) = req.recovery_key_out {
            let _ = fs::remove_file(key_out);
        }
        return Err(fail);
    }

    let mut body = honesty("PassphraseRotated");
    body.insert("vault_id".into(), json!(keys.vault_id()));
    body.insert("vault_format".into(), json!(session.vault_format));
    body.insert("new_effect".into(), json!(true));
    body.insert(
        "recovery_key_rotated".into(),
        json!(req.rotate_recovery_key),
    );
    if let Some(ref key_out) = req.recovery_key_out {
        body.insert(
            "recovery_key_out".into(),
            json!(key_out.display().to_string()),
        );
    } else if let Some((_, ref rec_str)) = new_recovery {
        body.insert("recovery_key".into(), json!(rec_str.as_str()));
    }
    apply_format_honesty(&mut body, session.vault_format, session.protection);
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
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
    pub auth: AuthInput,
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
    if let Ok(4) = accept_wal_format(&req.vault.join("vault.db")) {
        return Err(Fail::new(
            EXIT_VERSION,
            "VersionUnsupported",
            "format_4_refuses_import",
        )
        .extra("new_effect", json!(false))
        .extra("vault_format", json!(4))
        .extra("created_new_vault", json!(false)));
    }
    if let Ok(5) = accept_wal_format(&req.vault.join("vault.db")) {
        return Err(Fail::new(
            EXIT_VERSION,
            "VersionUnsupported",
            "format_5_refuses_import",
        )
        .extra("new_effect", json!(false))
        .extra("vault_format", json!(5))
        .extra("created_new_vault", json!(false)));
    }
    let session = open_session_with_auth(&req.vault, &req.auth)?;
    let reconciled = journal::reconcile_with_cipher(&session.root, &session.conn, &session.cipher)?;
    let journal_unlisted = reconciled.journal_unlisted.clone();
    let label_misses = reconciled.misses;
    if let Some(existing) =
        store::command_with_cipher(&session.conn, &req.command_id, &session.cipher)
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
    decide::reject_if_t3_command(&session.conn, &req.command_id, &session.cipher)?;
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
    let mut journal = load_or_new_journal(&session.root, &req, &display_name, &session.cipher)?;
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
    journal::write_with_cipher(&session.root, &journal, &session.cipher)?;

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
        Err(fail) => Err(unwind(
            &session.root,
            &mut journal,
            &phase,
            fail,
            &session.cipher,
        )?),
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
    journal::write_with_cipher(&session.root, journal, &session.cipher)?;
    let staging = staging_path(&session.root, &command_id);
    let _ = fs::remove_file(&staging);
    let mut staged_object_id: Option<String> = None;
    let (hash, len) = if session.cipher.is_encrypted() {
        let mut candidate_id = journal
            .object_id
            .clone()
            .filter(|id| is_prefixed_id(id, "obj_"))
            .unwrap_or_default();
        if candidate_id.is_empty() || session.root.join("objects").join(&candidate_id).exists() {
            candidate_id = ids::random_id("obj_").map_err(|err| io_fail(err, "random"))?;
        }
        let pair = copy_source_encrypted(&req.source, &staging, &candidate_id, &session.cipher)?;
        staged_object_id = Some(candidate_id);
        pair
    } else {
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
        sha256_file(&staging).map_err(|err| io_fail(err, "hash_staging"))?
    };
    journal.content_sha256 = Some(hash.clone());
    journal.byte_length = Some(len as i64);
    journal::write_with_cipher(&session.root, journal, &session.cipher)?;
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

    let existing = store::object_by_hash_with_cipher(
        &session.conn,
        session.vault_format,
        &hash,
        &session.cipher,
    )
    .map_err(|err| db_fail(err, "object_lookup"))?;
    let (object_id, storage_reused) = if let Some(object) = existing {
        refuse_unreached_object_fsync(hooks)?;
        require_hosted_object_with_cipher(
            &session.root,
            &object,
            &hash,
            object.byte_length,
            &session.cipher,
        )?;
        (object.object_id, true)
    } else if let Some(orphan) =
        find_orphan_with_hash_with_cipher(&session.root, &session.conn, &hash, &session.cipher)?
    {
        refuse_unreached_object_fsync(hooks)?;
        (orphan, true)
    } else {
        let object_id = if let Some(id) = staged_object_id {
            id
        } else {
            let mut id = journal
                .object_id
                .clone()
                .filter(|id| is_prefixed_id(id, "obj_"))
                .unwrap_or_default();
            if id.is_empty() || session.root.join("objects").join(&id).exists() {
                id = ids::random_id("obj_").map_err(|err| io_fail(err, "random"))?;
            }
            id
        };
        let dest = session.root.join("objects").join(&object_id);
        fs::hard_link(&staging, &dest).map_err(|err| io_fail(err, "publish_object"))?;
        phase.published = true;
        hooks.inject(
            "object_fsync",
            checkpoint_map(&command_id, len, Some(len), Some(&hash), true, false),
        )?;
        crate::platform::sync_file(&dest).map_err(|err| io_fail(err, "object_fsync"))?;
        fsutil::fsync_dir(dest.parent().unwrap_or(&session.root))
            .map_err(|err| io_fail(err, "object_dir_fsync"))?;
        (object_id, false)
    };

    journal.object_id = Some(object_id.clone());
    journal.storage_reused = Some(storage_reused);
    journal.state = "object_ready".into();
    journal::write_with_cipher(&session.root, journal, &session.cipher)?;
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
        if store::command_with_cipher(&tx, &command_id, &session.cipher)
            .map_err(|err| db_fail(err, "command_recheck"))?
            .is_some()
        {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "command_race"));
        }
        if store::object_by_hash_with_cipher(&tx, session.vault_format, &hash, &session.cipher)
            .map_err(|err| db_fail(err, "object_recheck"))?
            .is_none()
        {
            let relpath = canonical_object_relpath(&object_id)
                .ok_or_else(|| Fail::new(EXIT_REJECTED, "Rejected", "bad_object_id"))?;
            store::insert_object_with_cipher(
                &tx,
                session.vault_format,
                &ObjectRow {
                    object_id: object_id.clone(),
                    content_sha256: hash.clone(),
                    byte_length: len as i64,
                    relpath,
                },
                &session.cipher,
            )
            .map_err(|err| db_fail(err, "insert_object"))?;
        }
        let source = if let Some(source) =
            store::source_by_hash_with_cipher(&tx, session.vault_format, &hash, &session.cipher)
                .map_err(|err| db_fail(err, "source_lookup"))?
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
            store::insert_source_with_cipher(&tx, session.vault_format, &source, &session.cipher)
                .map_err(|err| db_fail(err, "insert_source"))?;
            source
        };
        let occurrence_id = journal.occurrence_id.clone().ok_or_else(|| {
            Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "missing_occurrence_id")
        })?;
        let recorded_at = now_ms();
        store::insert_occurrence_with_cipher(
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
            &session.cipher,
        )
        .map_err(|err| db_fail(err, "insert_occurrence"))?;
        let result = import_result(
            &command_id,
            &source,
            &occurrence_id,
            storage_reused,
            journal,
            session.vault_format,
            session.protection,
        );
        let encoded = serde_json::to_string(&result)
            .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "result_encode"))?;
        store::insert_command_with_cipher(
            &tx,
            &CommandRow {
                command_id: command_id.clone(),
                kind: "import".into(),
                payload_fingerprint: fingerprint,
                result_json: encoded,
                outcome: "Committed".into(),
            },
            recorded_at,
            &session.cipher,
        )
        .map_err(|err| db_fail(err, "insert_command"))?;
        hooks.inject(
            "db_commit",
            checkpoint_map(&command_id, len, Some(len), Some(&hash), true, false),
        )?;
        match tx.commit() {
            Ok(()) => result,
            Err(err) => return settle_commit(&session.root, &command_id, err, &session.cipher),
        }
    };
    finish_after_commit(committed, || {
        hooks.stop(
            "after_db_commit",
            checkpoint_map(&command_id, len, Some(len), Some(&hash), true, true),
        )?;
        journal.state = "committed".into();
        journal.reason = None;
        journal::write_with_cipher(&session.root, journal, &session.cipher)?;
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
    pub auth: AuthInput,
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
    let session = open_session_with_auth(&req.vault, &req.auth)?;
    let fingerprint = fingerprint(&["marker", &req.source_id, &req.value]);
    let expected_fp = session
        .cipher
        .dedup_key("payload_fingerprint", &fingerprint);
    if let Some(existing) =
        store::command_with_cipher(&session.conn, &req.command_id, &session.cipher)
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
        let source =
            store::source_by_id_with_cipher(&session.conn, &req.source_id, &session.cipher)
                .map_err(|err| unseen_db(err, "source_lookup"))?
                .ok_or_else(|| {
                    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "source_row_missing")
                        .extra("new_effect", json!(false))
                })?;
        confirm_source_bytes(&session, &source, &req.command_id)?;
        if existing.payload_fingerprint != expected_fp {
            return Err(checked_conflict(&req.command_id, &source));
        }
        return already_committed(&existing);
    }
    decide::reject_if_t3_command(&session.conn, &req.command_id, &session.cipher)?;
    let source = store::source_by_id_with_cipher(&session.conn, &req.source_id, &session.cipher)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_REJECTED, "Rejected", "unknown_source")
                .extra("source_id", json!(req.source_id))
        })?;
    let existing_journal = journal::path(&session.root, &req.command_id);
    if existing_journal.exists() {
        let existing =
            journal::read_file_with_cipher(&existing_journal, &session.cipher).map_err(|_| {
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
    journal::write_with_cipher(&session.root, &journal, &session.cipher)?;
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
        let updated =
            store::update_marker_with_cipher(&tx, &req.source_id, &req.value, &session.cipher)
                .map_err(|err| db_fail(err, "update_marker"))?;
        if updated != 1 {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "unknown_source"));
        }
        let result = marker_result(
            &req,
            &source,
            recorded_at,
            session.vault_format,
            session.protection,
        );
        store::insert_command_with_cipher(
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
            &session.cipher,
        )
        .map_err(|err| db_fail(err, "insert_command"))?;
        hooks.inject(
            "db_commit",
            checkpoint_map(&req.command_id, 0, None, None, false, false),
        )?;
        match tx.commit() {
            Ok(()) => result,
            Err(err) => return settle_commit(&session.root, &req.command_id, err, &session.cipher),
        }
    };
    finish_after_commit(committed, || {
        hooks.stop(
            "after_db_commit",
            checkpoint_map(&req.command_id, 0, None, None, false, true),
        )?;
        journal.state = "committed".into();
        journal::write_with_cipher(&session.root, &journal, &session.cipher)?;
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
    pub auth: AuthInput,
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
    let session = open_session_with_auth(&req.vault, &req.auth)?;
    let source = store::source_by_id_with_cipher(&session.conn, &req.source_id, &session.cipher)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| Fail::new(EXIT_REJECTED, "Rejected", "unknown_source"))?;
    let object = lookup_object(&session, &source.object_id)?;
    let facts = facts::object_facts_with_cipher(
        &session.root,
        &source.object_id,
        Some(&object.relpath),
        &source.content_sha256,
        source.byte_length,
        &session.cipher,
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
    if let Some(detail) = &facts.auth_error {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "ciphertext_auth_failed")
                .extra("source_id", json!(source.source_id))
                .extra("object_id", json!(source.object_id))
                .extra("detail", json!(detail)),
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
    if let Err(err) =
        copy_exclusive_decrypted(&session.cipher, &source.object_id, &file_path, &req.out)
    {
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
    apply_format_honesty(&mut body, session.vault_format, session.protection);
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
/// Ancestor symlinks and junctions are followed. The final component is not.
/// Identity is the OS directory identity (`(dev, ino)` on Linux; 128-bit
/// `FILE_ID_INFO` on Windows), never a string prefix.
fn place_output(vault: &Path, dst: &Path) -> Place {
    let vault_id = match vault_identity(vault) {
        Ok(id) => id,
        Err(err) => return Place::Unresolved(err.to_string()),
    };
    let abs = match absolute_existing_base(dst) {
        Ok(path) => path,
        Err(err) => return Place::Unresolved(err.to_string()),
    };
    let (mut cur, mut parts) = root_and_parts(&abs);
    let Some(file_name) = parts.pop() else {
        return Place::Unresolved("output path has no final component".into());
    };
    let mut missing = false;
    for part in parts {
        if missing {
            break;
        }
        let next = if cfg!(windows) && part == ".." {
            match cur.parent() {
                Some(parent) => parent.to_path_buf(),
                None => cur.clone(),
            }
        } else {
            cur.join(&part)
        };
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

fn vault_identity(vault: &Path) -> io::Result<crate::platform::DirectoryId> {
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

fn root_and_parts(path: &Path) -> (PathBuf, Vec<OsString>) {
    let mut root = PathBuf::new();
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => root.push(prefix.as_os_str()),
            Component::RootDir => root.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::ParentDir => parts.push(OsString::from("..")),
        }
    }
    if root.as_os_str().is_empty() {
        root.push(std::path::MAIN_SEPARATOR_STR);
    }
    (root, parts)
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

fn directory_id(path: &Path) -> io::Result<crate::platform::DirectoryId> {
    crate::platform::directory_id(path)
}

fn directory_is_vault_or_inside(
    vault_id: crate::platform::DirectoryId,
    dir: &Path,
) -> io::Result<bool> {
    let mut cursor = fs::canonicalize(dir)?;
    for _ in 0..256 {
        let cur_id = directory_id(&cursor)?;
        if cur_id == vault_id {
            return Ok(true);
        }
        #[cfg(target_os = "linux")]
        let parent = cursor.join("..");
        #[cfg(target_os = "windows")]
        let Some(parent) = cursor.parent().map(Path::to_path_buf) else {
            return Ok(false);
        };
        if directory_id(&parent)? == cur_id {
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
fn copy_exclusive_decrypted(
    cipher: &VaultCipher,
    object_id: &str,
    src: &Path,
    dst: &Path,
) -> Result<(), FreshCopy> {
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
    let mut writer = crate::platform::open_new_nofollow(dst).map_err(|err| {
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
        let permissions = fs::metadata(src)?.permissions();
        cipher
            .copy_decrypted_object(object_id, src, &mut writer)
            .map_err(|err| match err {
                CryptoError::Io(io_err) => io_err,
                other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
            })?;
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
    crate::platform::exclusive_create_refused(err)
}

pub struct CancelRequest {
    pub vault: PathBuf,
    pub command_id: String,
    pub generation: i64,
    pub kind: String,
    pub auth: AuthInput,
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
        let session = open_session_with_auth(&req.vault, &req.auth)?;
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
    let (_, banner) = vault_shape(&req.vault)?;
    inspect_version_readonly_with_banner(&req.vault, banner)?;
    let lock = match lockfile::VaultLock::try_acquire(&req.vault.join("writer.lock"))
        .map_err(|err| unseen_io(err, "lock"))?
    {
        Some(lock) => lock,
        None => return cancel_without_lock(&req, banner),
    };
    let _lock = lock;
    let version = accept_wal_format_with_banner(&req.vault.join("vault.db"), banner)?;
    let conn = open_writer(&req.vault)?;
    let protection = match banner {
        VaultBannerKind::Synthetic => None,
        VaultBannerKind::Production(mode) => Some(mode),
    };
    let cipher = unlock_cipher(&conn, version, protection, &req.auth)?;
    if let Some(existing) = store::command_with_cipher(&conn, &req.command_id, &cipher)
        .map_err(|err| unseen_db(err, "command_lookup"))?
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
    let mut journal = journal::read_file_with_cipher(&path, &cipher).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("cancel_applied", json!(false))
    })?;
    if journal.kind == "fragment"
        || journal.kind == "decision"
        || journal.kind == "html"
        || journal.kind == "pdf"
    {
        return Err(fragment_journal_conflict());
    }
    if journal.generation != req.generation {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "generation_mismatch")
            .extra("cancel_applied", json!(false)));
    }
    journal.state = "aborted".into();
    journal.reason = Some("cancelled".into());
    journal::write_with_cipher(&req.vault, &journal, &cipher)?;
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
    apply_format_honesty(&mut body, version, protection);
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

fn vault_shape(path: &Path) -> Result<(i64, VaultBannerKind), Fail> {
    let banner = inspect_vault_banner(path)?;
    if !sqlite_header_ok(&path.join("vault.db"))? {
        return Err(Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "sqlite_header")
            .extra("created_new_vault", json!(false)));
    }
    let magic = filesystem_magic(path).map_err(|err| unseen_io(err, "statfs"))?;
    if network_filesystem(magic) {
        return Err(unsupported_fs(magic));
    }
    Ok((magic, banner))
}

fn guard_vault(path: &Path) -> Result<i64, Fail> {
    let (magic, banner) = vault_shape(path)?;
    inspect_version_readonly_with_banner(path, banner)?;
    Ok(magic)
}

fn acquire_writer_lock(path: &Path) -> Result<lockfile::VaultLock, Fail> {
    lockfile::VaultLock::try_acquire(&path.join("writer.lock"))
        .map_err(|err| unseen_io(err, "lock"))?
        .ok_or_else(|| {
            Fail::new(EXIT_LOCKED, "Locked", "writer_busy").extra("created_new_vault", json!(false))
        })
}

/// One read of a vault. The connection is declared before the lock so it
/// closes first. `BEGIN` keeps sources, facts, candidates, and decision
/// history on that connection. A writer either commits before this lock or
/// waits until the read drops it.
struct ReviewRead {
    conn: Connection,
    _lock: VaultLock,
    protection: Option<ProtectionMode>,
    cipher: VaultCipher,
}

impl ReviewRead {
    fn connection(&self) -> &Connection {
        &self.conn
    }

    fn protection(&self) -> Option<ProtectionMode> {
        self.protection
    }

    fn cipher(&self) -> &VaultCipher {
        &self.cipher
    }
}

fn open_review_read(vault: &Path, auth: &AuthInput) -> Result<(ReviewRead, i64), Fail> {
    let (_, banner) = vault_shape(vault)?;
    let lock = acquire_writer_lock(vault)?;
    let db = vault.join("vault.db");
    let uri = wal_visible_uri(&db)?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| busy_is_locked(err, "sqlite_unreadable", false))?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(0));
    conn.execute_batch("BEGIN")
        .map_err(|err| busy_is_locked(err, "user_version_unreadable", false))?;
    let version = accept_connection_format_with_banner(&db, &conn, banner)?;
    let protection = match banner {
        VaultBannerKind::Synthetic => None,
        VaultBannerKind::Production(mode) => Some(mode),
    };
    let cipher = unlock_cipher(&conn, version, protection, auth)?;
    Ok((
        ReviewRead {
            conn,
            _lock: lock,
            protection,
            cipher,
        },
        version,
    ))
}

fn open_readonly_unlocked(
    vault: &Path,
    auth: &AuthInput,
) -> Result<(Connection, i64, Option<ProtectionMode>, VaultCipher), Fail> {
    let (_, banner) = vault_shape(vault)?;
    inspect_version_readonly_with_banner(vault, banner)?;
    let db = vault.join("vault.db");
    let uri = wal_visible_uri(&db)?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| unseen_db(err, "readonly_open"))?;
    let version = accept_connection_format_with_banner(&db, &conn, banner)?;
    let protection = match banner {
        VaultBannerKind::Synthetic => None,
        VaultBannerKind::Production(mode) => Some(mode),
    };
    let cipher = unlock_cipher(&conn, version, protection, auth)?;
    Ok((conn, version, protection, cipher))
}

fn open_session(path: &Path) -> Result<Session, Fail> {
    open_session_with_auth(path, &AuthInput::default())
}

fn open_session_with_auth(path: &Path, auth: &AuthInput) -> Result<Session, Fail> {
    let (magic, banner) = vault_shape(path)?;
    let lock = acquire_writer_lock(path)?;
    inspect_version_readonly_with_banner(path, banner)?;
    let version = accept_wal_format_with_banner(&path.join("vault.db"), banner)?;
    finish_session(path, lock, magic, version, banner, auth)
}

fn open_html_session(path: &Path, auth: &AuthInput) -> Result<Session, Fail> {
    let (magic, banner) = vault_shape(path)?;
    let lock = acquire_writer_lock(path)?;
    let version = match accept_wal_format_with_banner(&path.join("vault.db"), banner)? {
        version @ 4..=6 => version,
        version => {
            return Err(
                Fail::new(EXIT_VERSION, "VersionUnsupported", "html_requires_format_4")
                    .extra("schema_version_found", json!(version))
                    .extra("vault_format", json!(version))
                    .extra("new_effect", json!(false))
                    .extra("created_new_vault", json!(false)),
            );
        }
    };
    finish_session(path, lock, magic, version, banner, auth)
}

fn open_real_session(path: &Path, auth: &AuthInput) -> Result<Session, Fail> {
    let (magic, banner) = vault_shape(path)?;
    let lock = acquire_writer_lock(path)?;
    let version = match accept_wal_format_with_banner(&path.join("vault.db"), banner)? {
        6 => 6,
        version => {
            return Err(
                Fail::new(EXIT_VERSION, "VersionUnsupported", "requires_format_6")
                    .extra("schema_version_found", json!(version))
                    .extra("vault_format", json!(version))
                    .extra("new_effect", json!(false))
                    .extra("created_new_vault", json!(false)),
            );
        }
    };
    finish_session(path, lock, magic, version, banner, auth)
}

fn open_pdf_session(path: &Path, auth: &AuthInput) -> Result<Session, Fail> {
    let (magic, banner) = vault_shape(path)?;
    let lock = acquire_writer_lock(path)?;
    let version = match accept_wal_format_with_banner(&path.join("vault.db"), banner)? {
        version @ (5 | 6) => version,
        version => {
            return Err(
                Fail::new(EXIT_VERSION, "VersionUnsupported", "pdf_requires_format_5")
                    .extra("schema_version_found", json!(version))
                    .extra("vault_format", json!(version))
                    .extra("new_effect", json!(false))
                    .extra("created_new_vault", json!(false)),
            );
        }
    };
    finish_session(path, lock, magic, version, banner, auth)
}

fn finish_session(
    path: &Path,
    lock: lockfile::VaultLock,
    magic: i64,
    vault_format: i64,
    banner: VaultBannerKind,
    auth: &AuthInput,
) -> Result<Session, Fail> {
    let protection = match banner {
        VaultBannerKind::Synthetic => None,
        VaultBannerKind::Production(mode) => Some(mode),
    };
    let conn = open_writer(path)?;
    let cipher = unlock_cipher(&conn, vault_format, protection, auth)?;
    Ok(Session {
        root: path.to_path_buf(),
        _lock: lock,
        conn,
        magic,
        vault_format,
        protection,
        cipher,
    })
}

fn unlock_cipher(
    conn: &Connection,
    vault_format: i64,
    protection: Option<ProtectionMode>,
    auth: &AuthInput,
) -> Result<VaultCipher, Fail> {
    if vault_format < 6 || protection != Some(ProtectionMode::Passphrase) {
        return Ok(VaultCipher::Plain);
    }
    let vault_id = store::meta(conn, "vault_id")
        .map_err(|err| unseen_db(err, "vault_id"))?
        .ok_or_else(|| {
            Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "missing_vault_id")
                .extra("new_effect", json!(false))
        })?;
    if let Some(ref cached) = auth.session_cipher {
        if cached.keys().is_some_and(|k| k.vault_id() == vault_id) {
            return Ok(cached.clone());
        }
        return Err(Fail::new(EXIT_AUTH, "AuthFailed", "session_vault_mismatch")
            .extra("vault_format", json!(vault_format))
            .extra("protection", json!(crypto::PROTECTION_PASSPHRASE))
            .extra("synthetic_only", json!(false))
            .extra("new_effect", json!(false)));
    }
    let cred = map_cred_err(auth.resolve())?;
    let dek = match cred {
        Credential::None => {
            return Err(Fail::new(EXIT_AUTH, "KeyRequired", "passphrase_required")
                .extra("vault_format", json!(vault_format))
                .extra("protection", json!(crypto::PROTECTION_PASSPHRASE))
                .extra("synthetic_only", json!(false))
                .extra("locked_at_rest", json!(true))
                .extra("new_effect", json!(false)));
        }
        Credential::Passphrase(pass) => {
            let slot = store::load_keyslot(conn, "passphrase")
                .map_err(|err| unseen_db(err, "keyslot"))?
                .ok_or_else(|| {
                    Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "missing_passphrase_slot")
                        .extra("new_effect", json!(false))
                })?;
            match crypto::unwrap_dek_with_passphrase(&vault_id, &slot, &pass) {
                Ok(dek) => dek,
                Err(CryptoError::AuthFailed) => {
                    return Err(Fail::new(EXIT_AUTH, "AuthFailed", "invalid_passphrase")
                        .extra("vault_format", json!(vault_format))
                        .extra("protection", json!(crypto::PROTECTION_PASSPHRASE))
                        .extra("synthetic_only", json!(false))
                        .extra("new_effect", json!(false)));
                }
                Err(err) => {
                    return Err(
                        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "keyslot_corrupt")
                            .extra("detail", json!(err.to_string()))
                            .extra("new_effect", json!(false)),
                    );
                }
            }
        }
        Credential::RecoveryKey(rec) => {
            let slot = store::load_keyslot(conn, "recovery")
                .map_err(|err| unseen_db(err, "keyslot"))?
                .ok_or_else(|| {
                    Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "missing_recovery_slot")
                        .extra("new_effect", json!(false))
                })?;
            match crypto::unwrap_dek_with_recovery(&vault_id, &slot, &rec) {
                Ok(dek) => dek,
                Err(CryptoError::AuthFailed) => {
                    return Err(Fail::new(EXIT_AUTH, "AuthFailed", "invalid_recovery_key")
                        .extra("vault_format", json!(vault_format))
                        .extra("protection", json!(crypto::PROTECTION_PASSPHRASE))
                        .extra("synthetic_only", json!(false))
                        .extra("new_effect", json!(false)));
                }
                Err(err) => {
                    return Err(
                        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "keyslot_corrupt")
                            .extra("detail", json!(err.to_string()))
                            .extra("new_effect", json!(false)),
                    );
                }
            }
        }
    };
    let keys = VaultKeys::from_dek(&vault_id, dek).map_err(|err| {
        Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "subkey_derivation_failed")
            .extra("detail", json!(err.to_string()))
            .extra("new_effect", json!(false))
    })?;
    Ok(VaultCipher::Aead(Box::new(keys)))
}

fn map_cred_err<T>(res: Result<T, CredentialError>) -> Result<T, Fail> {
    res.map_err(|err| match err {
        CredentialError::Conflict => Fail::new(EXIT_USAGE, "Usage", "conflicting_credentials")
            .extra("new_effect", json!(false)),
        CredentialError::Empty(reason) => {
            Fail::new(EXIT_USAGE, "Usage", reason).extra("new_effect", json!(false))
        }
        CredentialError::Io(io_err) => Fail::new(EXIT_USAGE, "Usage", "credential_unreadable")
            .extra("os_error", json!(io_err.to_string()))
            .extra("new_effect", json!(false)),
    })
}

fn crypto_init_fail(err: CryptoError) -> Fail {
    Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "crypto_init_failed")
        .extra("detail", json!(err.to_string()))
        .extra("created_new_vault", json!(false))
}

fn validate_new_key_out_before_init(vault_path: &Path, key_out: &Path) -> Result<(), Fail> {
    let vault_abs = absolute_existing_base(&crate::platform::final_directory_entry(vault_path))
        .map_err(|err| io_fail(err, "recovery_key_out"))?;
    let out_abs = absolute_existing_base(&crate::platform::final_directory_entry(key_out))
        .map_err(|err| io_fail(err, "recovery_key_out"))?;
    if out_abs == vault_abs || out_abs.starts_with(&vault_abs) {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_inside_vault")
            .extra("created_new_vault", json!(false)));
    }
    let Some(parent) = out_abs.parent() else {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_unresolved")
            .extra("created_new_vault", json!(false)));
    };
    if !parent.is_dir() {
        return Err(
            Fail::new(EXIT_REJECTED, "Rejected", "output_parent_missing")
                .extra("created_new_vault", json!(false)),
        );
    }
    match fs::symlink_metadata(&out_abs) {
        Ok(_) => Err(Fail::new(EXIT_REJECTED, "Rejected", "output_exists")
            .extra("created_new_vault", json!(false))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(io_fail(err, "recovery_key_out").extra("created_new_vault", json!(false))),
    }
}

fn write_recovery_key_file(path: &Path, key_text: &str) -> Result<(), Fail> {
    let mut file = match crate::platform::open_new_nofollow(path) {
        Ok(file) => file,
        Err(err) if crate::platform::exclusive_create_refused(&err) => {
            return Err(Fail::new(EXIT_REJECTED, "Rejected", "output_exists")
                .extra("new_effect", json!(false)));
        }
        Err(err) => return Err(io_fail(err, "recovery_key_write")),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(io_fail(err, "recovery_key_chmod"));
        }
    }
    let content = format!(
        "# SubVault Format 6 Offline Recovery Key\n# Store this offline in a safe place. Losing both passphrase and recovery key permanently loses vault access.\n{key_text}\n"
    );
    if let Err(err) = file
        .write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(io_fail(err, "recovery_key_write"));
    }
    drop(file);
    let _ = fsutil::fsync_dir(path.parent().unwrap_or_else(|| Path::new(".")));
    Ok(())
}

fn inspect_vault_banner(path: &Path) -> Result<VaultBannerKind, Fail> {
    if !path.exists() {
        return Err(Fail::new(EXIT_NOT_VAULT, "NotAVault", "missing_path")
            .extra("created_new_vault", json!(false)));
    }
    if !path.is_dir() {
        return Err(Fail::new(EXIT_NOT_VAULT, "NotAVault", "not_a_directory")
            .extra("created_new_vault", json!(false)));
    }
    let has_synthetic = path.join(SYNTHETIC_BANNER_FILE).is_file();
    let has_prod = path.join(PRODUCTION_BANNER_FILE).is_file();
    if has_synthetic && has_prod {
        return Err(
            Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "mixed_vault_banners")
                .extra("created_new_vault", json!(false)),
        );
    }
    if (!has_synthetic && !has_prod) || !path.join("vault.db").is_file() {
        return Err(
            Fail::new(EXIT_NOT_VAULT, "NotAVault", "missing_vault_files")
                .extra("created_new_vault", json!(false)),
        );
    }
    if has_synthetic {
        // A failed read has not examined any object. That is the same class as an
        // unreadable vault.db: unknown, with no preservation verdict. Bytes that
        // were read and do not match stay NotSynthetic below, and are not rewritten.
        let bytes = fs::read(path.join(SYNTHETIC_BANNER_FILE))
            .map_err(|err| vault_unreadable(&err, "banner_unreadable", "banner"))?;
        if bytes != VAULT_BANNER.as_bytes() {
            return Err(
                Fail::new(EXIT_NOT_SYNTHETIC, "NotSynthetic", "banner_mismatch")
                    .extra("created_new_vault", json!(false)),
            );
        }
        Ok(VaultBannerKind::Synthetic)
    } else {
        let bytes = fs::read(path.join(PRODUCTION_BANNER_FILE))
            .map_err(|err| vault_unreadable(&err, "banner_unreadable", "banner"))?;
        if bytes == PRODUCTION_BANNER_PASSPHRASE.as_bytes() {
            Ok(VaultBannerKind::Production(ProtectionMode::Passphrase))
        } else if bytes == PRODUCTION_BANNER_OS_DISK.as_bytes() {
            Ok(VaultBannerKind::Production(ProtectionMode::OsDisk))
        } else {
            Err(
                Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", "vault_banner_mismatch")
                    .extra("created_new_vault", json!(false)),
            )
        }
    }
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
    let banner = inspect_vault_banner(path)?;
    inspect_version_readonly_with_banner(path, banner)
}

fn inspect_version_readonly_with_banner(path: &Path, banner: VaultBannerKind) -> Result<i64, Fail> {
    let view = visible_main_format(&path.join("vault.db"))?;
    judge_format(view.version, view.version, &view, banner)
}

struct FormatView {
    version: i64,
    format: Option<String>,
    schema: Option<String>,
    html_schema: Option<String>,
    pdf_schema: Option<String>,
    real_schema: Option<String>,
    synthetic_only: Option<String>,
    protection: Option<String>,
}

fn visible_main_format(db: &Path) -> Result<FormatView, Fail> {
    let uri = readonly_uri(db)?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| busy_is_locked(err, "sqlite_unreadable", true))?;
    let version = store::user_version(&conn)
        .map_err(|err| busy_is_locked(err, "user_version_unreadable", true))?;
    let format = store::meta(&conn, "vault_format")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", true))?;
    let schema = store::meta(&conn, "schema_version")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", true))?;
    let html_schema = store::meta(&conn, "html_body_schema")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", true))?;
    let pdf_schema = store::meta(&conn, "pdf_body_schema")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", true))?;
    let real_schema = store::meta(&conn, "real_body_schema")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", true))?;
    let synthetic_only = store::meta(&conn, "synthetic_only")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", true))?;
    let protection = store::meta(&conn, "protection")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", true))?;
    Ok(FormatView {
        version,
        format,
        schema,
        html_schema,
        pdf_schema,
        real_schema,
        synthetic_only,
        protection,
    })
}

fn file_uri(path: &Path, query: &str) -> Result<String, Fail> {
    let abs = path
        .canonicalize()
        .map_err(|err| unseen_io(err, "canonicalize"))?;
    let text = abs.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        if text.starts_with(r"\\?\UNC\") || text.starts_with(r"\\?\unc\") {
            return Err(Fail::new(
                EXIT_UNSUPPORTED_FS,
                "UnsupportedFilesystem",
                "network_filesystem",
            ));
        }
        let stripped = text.strip_prefix(r"\\?\").unwrap_or(&text);
        if stripped.contains('?') || stripped.contains('#') {
            return Err(Fail::new(
                EXIT_REJECTED,
                "Rejected",
                "vault_path_unsupported",
            ));
        }
        let slash = stripped.replace('\\', "/");
        Ok(format!("file:///{slash}?{query}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        if text.contains('?') || text.contains('#') {
            return Err(Fail::new(
                EXIT_REJECTED,
                "Rejected",
                "vault_path_unsupported",
            ));
        }
        Ok(format!("file:{text}?{query}"))
    }
}

/// Version inspection only. immutable=1 does not see a commit that is still in the WAL.
fn readonly_uri(path: &Path) -> Result<String, Fail> {
    file_uri(path, "mode=ro&immutable=1")
}

/// Main-file header, WAL-visible version, and the meta that version needs.
/// Read-only: no writer configuration, no cleanup, and no checkpoint.
/// This function does not take `writer.lock`. A writer takes it before calling
/// here. A review that must be one snapshot takes it before opening SQLite.
/// A locked database is `writer_busy`, not a damaged vault.
/// This open may create or touch `-shm`. It is not a claim that the directory
/// received no files.
fn accept_wal_format(db: &Path) -> Result<i64, Fail> {
    let vault_dir = db.parent().unwrap_or_else(|| Path::new("."));
    let banner = inspect_vault_banner(vault_dir)?;
    accept_wal_format_with_banner(db, banner)
}

fn accept_wal_format_with_banner(db: &Path, banner: VaultBannerKind) -> Result<i64, Fail> {
    // No WAL frames means the main file is the effective database. Skipping the
    // extra open avoids creating an empty `-wal` on a checkpointed vault.
    let mut wal_name = db.as_os_str().to_os_string();
    wal_name.push("-wal");
    let frames = fs::metadata(Path::new(&wal_name))
        .map(|meta| meta.is_file() && meta.len() > 0)
        .unwrap_or(false);
    if !frames {
        let view = visible_main_format(db)?;
        return judge_format(view.version, view.version, &view, banner);
    }
    let uri = wal_visible_uri(db)?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| busy_is_locked(err, "sqlite_unreadable", false))?;
    accept_connection_format_with_banner(db, &conn, banner)
}

fn sqlite_busy(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(ffi, _) if ffi.code == rusqlite::ErrorCode::DatabaseBusy
    )
}

/// A busy read is lock contention. It is not evidence the vault is damaged.
fn busy_is_locked(err: rusqlite::Error, damaged: &'static str, published: bool) -> Fail {
    if sqlite_busy(&err) {
        return Fail::new(EXIT_LOCKED, "Locked", "writer_busy")
            .extra("created_new_vault", json!(false))
            .extra("new_effect", json!(false));
    }
    let mut fail = Fail::new(EXIT_DAMAGED, "ReadOnlyDamaged", damaged)
        .extra("created_new_vault", json!(false));
    if published {
        fail = fail.extra("published", json!(false));
    }
    fail
}

fn accept_connection_format(db: &Path, conn: &Connection) -> Result<i64, Fail> {
    let vault_dir = db.parent().unwrap_or_else(|| Path::new("."));
    let banner = inspect_vault_banner(vault_dir)?;
    accept_connection_format_with_banner(db, conn, banner)
}

fn accept_connection_format_with_banner(
    db: &Path,
    conn: &Connection,
    banner: VaultBannerKind,
) -> Result<i64, Fail> {
    let _ = conn.busy_timeout(std::time::Duration::from_millis(0));
    let main = visible_main_format(db)?;
    let eff = effective_view(conn)?;
    judge_format(main.version, eff.version, &eff, banner)
}

fn effective_view(conn: &Connection) -> Result<FormatView, Fail> {
    let version = store::user_version(conn)
        .map_err(|err| busy_is_locked(err, "user_version_unreadable", false))?;
    let format = store::meta(conn, "vault_format")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", false))?;
    let schema = store::meta(conn, "schema_version")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", false))?;
    let html_schema = store::meta(conn, "html_body_schema")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", false))?;
    let pdf_schema = store::meta(conn, "pdf_body_schema")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", false))?;
    let real_schema = store::meta(conn, "real_body_schema")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", false))?;
    let synthetic_only = store::meta(conn, "synthetic_only")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", false))?;
    let protection = store::meta(conn, "protection")
        .map_err(|err| busy_is_locked(err, "meta_unreadable", false))?;
    Ok(FormatView {
        version,
        format,
        schema,
        html_schema,
        pdf_schema,
        real_schema,
        synthetic_only,
        protection,
    })
}

fn consistent_format_4(view: &FormatView) -> bool {
    view.format.as_deref() == Some("4")
        && view.schema.as_deref() == Some("1")
        && view.html_schema.as_deref() == Some(store::HTML_BODY_SCHEMA)
}

fn consistent_format_5(view: &FormatView) -> bool {
    view.format.as_deref() == Some("5")
        && view.schema.as_deref() == Some("1")
        && view.pdf_schema.as_deref() == Some(store::PDF_BODY_SCHEMA)
}

fn consistent_format_6(view: &FormatView, mode: ProtectionMode) -> bool {
    view.format.as_deref() == Some("6")
        && view.schema.as_deref() == Some("1")
        && view.real_schema.as_deref() == Some(store::REAL_BODY_SCHEMA)
        && view.synthetic_only.as_deref() == Some("false")
        && view.protection.as_deref() == Some(mode.as_meta())
}

fn judge_format(
    header: i64,
    effective: i64,
    view: &FormatView,
    banner: VaultBannerKind,
) -> Result<i64, Fail> {
    let format = view.format.as_deref();
    let schema = view.schema.as_deref();
    if effective == 6 {
        if let VaultBannerKind::Production(mode) = banner {
            if header == 6 && consistent_format_6(view, mode) {
                return Ok(6);
            }
        }
        return Err(
            Fail::new(EXIT_VERSION, "VersionUnsupported", "newer_schema")
                .extra("schema_version_found", json!(effective))
                .extra("created_new_vault", json!(false)),
        );
    }
    if matches!(banner, VaultBannerKind::Production(_)) {
        return Err(Fail::new(
            EXIT_VERSION,
            "VersionUnsupported",
            "vault_banner_format_mismatch",
        )
        .extra("schema_version_found", json!(effective))
        .extra("created_new_vault", json!(false)));
    }
    // A user_version of 5 is format 5 only when the header and the format meta
    // agree. A bumped older vault stays newer_schema and is not migrated.
    if effective == 5 {
        if header == 5 && consistent_format_5(view) {
            return Ok(5);
        }
        return Err(
            Fail::new(EXIT_VERSION, "VersionUnsupported", "newer_schema")
                .extra("schema_version_found", json!(effective))
                .extra("created_new_vault", json!(false)),
        );
    }
    // A user_version of 4 is format 4 only when the header and the format meta
    // agree. A format 2 or 3 vault whose version was bumped to 4 stays
    // newer_schema and is not migrated.
    if effective == 4 {
        if header == 4 && consistent_format_4(view) {
            return Ok(4);
        }
        return Err(
            Fail::new(EXIT_VERSION, "VersionUnsupported", "newer_schema")
                .extra("schema_version_found", json!(effective))
                .extra("created_new_vault", json!(false)),
        );
    }
    if effective > 6 {
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
    cipher: &VaultCipher,
) -> Result<Journal, Fail> {
    let path = journal_path(root, &req.command_id);
    if path.exists() {
        let journal = journal::read_file_with_cipher(&path, cipher).map_err(|_| {
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

fn copy_source_encrypted(
    src: &Path,
    dest: &Path,
    object_id: &str,
    cipher: &VaultCipher,
) -> Result<(String, u64), Fail> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| io_fail(err, "staging_dir"))?;
    }
    let input = File::open(src).map_err(|err| io_fail(err, "read_source"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|err| io_fail(err, "create_staging"))?;
    let (hash, len) = cipher
        .encrypt_object_stream(object_id, input, &mut output)
        .map_err(|err| match err {
            CryptoError::Io(io_err) => io_fail(io_err, "write_staging"),
            other => Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "object_encrypt_failed")
                .extra("detail", json!(other.to_string())),
        })?;
    output
        .sync_all()
        .map_err(|err| io_fail(err, "staging_fsync"))?;
    if let Some(parent) = dest.parent() {
        fsutil::fsync_dir(parent).map_err(|err| io_fail(err, "staging_dir_fsync"))?;
    }
    Ok((hash, len))
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

fn find_orphan_with_hash_with_cipher(
    root: &Path,
    conn: &Connection,
    hash: &str,
    cipher: &VaultCipher,
) -> Result<Option<String>, Fail> {
    let objects = store::list_objects_with_cipher(conn, cipher)
        .map_err(|err| db_fail(err, "list_objects"))?;
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
        match cipher.verify_object_file(&name, &dir.join(&name)) {
            Ok((file_hash, _)) if file_hash == hash => return Ok(Some(name)),
            Ok(_) => {}
            Err(CryptoError::Io(err)) => return Err(io_fail(err, "hash_orphan")),
            Err(_) => {}
        }
    }
    Ok(None)
}

fn lookup_object(session: &Session, object_id: &str) -> Result<ObjectRow, Fail> {
    store::object_by_id_with_cipher(&session.conn, object_id, &session.cipher)
        .map_err(|err| unseen_db(err, "object_lookup"))?
        .ok_or_else(|| Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_row_missing"))
}

fn unwind(
    root: &Path,
    journal: &mut Journal,
    phase: &Phase,
    fail: Fail,
    cipher: &VaultCipher,
) -> Result<Fail, Fail> {
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
        journal::write_with_cipher(root, journal, cipher)?;
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
    vault_format: i64,
    protection: Option<ProtectionMode>,
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
    apply_format_honesty(&mut body, vault_format, protection);
    Value::Object(body)
}

fn marker_result(
    req: &MarkerRequest,
    source: &SourceRow,
    recorded_at: i64,
    vault_format: i64,
    protection: Option<ProtectionMode>,
) -> Value {
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
    apply_format_honesty(&mut body, vault_format, protection);
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

fn cancel_without_lock(req: &CancelRequest, banner: VaultBannerKind) -> Result<Report, Fail> {
    let (_, _, _, cipher) = open_readonly_unlocked(&req.vault, &req.auth)?;
    if let Some(row) = read_committed_with_cipher(&req.vault, &req.command_id, &cipher)? {
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
    let journal = journal::read_file_with_cipher(&path, &cipher).map_err(|_| {
        Fail::new(EXIT_RECOVERY, "RecoveryRequired", "journal_invalid")
            .extra("cancel_applied", json!(false))
    })?;
    if journal.kind == "fragment"
        || journal.kind == "decision"
        || journal.kind == "html"
        || journal.kind == "pdf"
    {
        return Err(fragment_journal_conflict());
    }
    if journal.generation != req.generation {
        return Err(Fail::new(EXIT_REJECTED, "Rejected", "generation_mismatch")
            .extra("cancel_applied", json!(false)));
    }
    accept_wal_format_with_banner(&req.vault.join("vault.db"), banner)?;
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

fn read_committed_with_cipher(
    root: &Path,
    command_id: &str,
    cipher: &VaultCipher,
) -> Result<Option<CommandRow>, Fail> {
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
    store::command_with_cipher(&conn, command_id, cipher)
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
    let source = store::source_by_id_with_cipher(&session.conn, source_id, &session.cipher)
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
            let fp = fingerprint(&["import", &source.content_sha256, &req.media_type]);
            let expected_fp = session.cipher.dedup_key("payload_fingerprint", &fp);
            if expected_fp != existing.payload_fingerprint {
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
            let fp = fingerprint(&["import", &hash, &req.media_type]);
            let expected_fp = session.cipher.dedup_key("payload_fingerprint", &fp);
            if expected_fp != existing.payload_fingerprint || len as i64 != source.byte_length {
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
    let object = store::object_by_id_with_cipher(&session.conn, &source.object_id, &session.cipher)
        .map_err(|err| unseen_db(err, "object_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_row_missing")
                .extra("new_effect", json!(false))
                .extra("command_id", json!(command_id))
        })?;
    let facts = facts::object_facts_with_cipher(
        &session.root,
        &object.object_id,
        Some(&object.relpath),
        &source.content_sha256,
        source.byte_length,
        &session.cipher,
    );
    hosted_facts_ok(&facts).map_err(|fail| fail.extra("command_id", json!(command_id)))
}

fn require_hosted_object_with_cipher(
    root: &Path,
    object: &ObjectRow,
    expected_hash: &str,
    expected_len: i64,
    cipher: &VaultCipher,
) -> Result<(), Fail> {
    let facts = facts::object_facts_with_cipher(
        root,
        &object.object_id,
        Some(&object.relpath),
        expected_hash,
        expected_len,
        cipher,
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
    if let Some(detail) = &facts.auth_error {
        return Err(bad_object("ciphertext_auth_failed").extra("detail", json!(detail)));
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

fn settle_commit(
    root: &Path,
    command_id: &str,
    err: rusqlite::Error,
    cipher: &VaultCipher,
) -> Result<Value, Fail> {
    match read_committed_with_cipher(root, command_id, cipher) {
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

#[cfg(test)]
mod rename_receipts {
    use super::*;

    fn os(code: i32) -> io::Error {
        io::Error::from_raw_os_error(code)
    }

    fn init_report(err: io::Error) -> Report {
        note_uncommitted(init_rename_failure(err)).into_report()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rename_errno_receipts_stay_in_their_existing_classes() {
        let exists = init_report(os(libc::EEXIST));
        assert_eq!(exists.code, EXIT_REJECTED);
        let exists = exists.body;
        assert_eq!(exists["outcome"], "Rejected");
        assert_eq!(exists["reason"], "vault_path_exists");
        assert_eq!(exists["created_new_vault"], false);
        assert_eq!(exists["during"], "init_rename");
        assert_eq!(exists["injected"], false);
        assert!(exists.get("preservation").is_none(), "{exists}");

        let unavailable = init_report(os(libc::ENOSYS));
        assert_eq!(unavailable.code, EXIT_UNSUPPORTED_FS);
        let unavailable = unavailable.body;
        assert_eq!(unavailable["outcome"], "Unsupported");
        assert_eq!(unavailable["reason"], "rename_noreplace_unavailable");
        assert_ne!(unavailable["outcome"], "NotPreserved");
        assert!(unavailable["os_error"]
            .as_str()
            .unwrap()
            .contains("os error 38"));

        let invalid = init_report(os(libc::EINVAL));
        assert_eq!(invalid.code, EXIT_NOT_PRESERVED);
        let invalid = invalid.body;
        assert_eq!(invalid["outcome"], "NotPreserved");
        assert_eq!(invalid["reason"], "io_error");
        assert_eq!(invalid["preservation"], "not_preserved");
        assert!(invalid["os_error"]
            .as_str()
            .unwrap()
            .contains("os error 22"));
        assert_ne!(invalid["reason"], "rename_noreplace_unavailable");

        let cross = init_report(os(libc::EXDEV));
        assert_eq!(cross.code, EXIT_NOT_PRESERVED);
        let cross = cross.body;
        assert_eq!(cross["outcome"], "NotPreserved");
        assert_eq!(cross["reason"], "io_error");
        assert!(cross["os_error"].as_str().unwrap().contains("os error 18"));
        assert_ne!(cross["outcome"], "Unsupported");
        assert_ne!(cross["reason"], "rename_noreplace_unavailable");

        let access = init_report(os(libc::EACCES)).body;
        let perm = init_report(os(libc::EPERM)).body;
        let busy = init_report(os(libc::EBUSY)).body;
        let full = init_report(os(libc::ENOSPC)).body;
        assert_eq!(access["reason"], "io_error");
        assert_eq!(perm["reason"], "io_error");
        assert_eq!(busy["reason"], "io_error");
        assert_eq!(full["reason"], "enospc");
        assert_ne!(access["reason"], "writer_busy");
        assert_ne!(busy["reason"], "writer_busy");
        assert_ne!(full["reason"], "writer_busy");
        assert!(access["os_error"].as_str().unwrap().contains("os error 13"));
        assert!(perm["os_error"].as_str().unwrap().contains("os error 1"));
        assert!(busy["os_error"].as_str().unwrap().contains("os error 16"));
        assert!(full["os_error"].as_str().unwrap().contains("os error 28"));
        assert_ne!(access["os_error"], busy["os_error"]);
        assert_ne!(busy["os_error"], full["os_error"]);
        assert_ne!(access["os_error"], perm["os_error"]);

        let export_exists = export::export_publish_failure(os(libc::EEXIST)).into_report();
        assert_eq!(export_exists.code, EXIT_REJECTED);
        assert_eq!(export_exists.body["reason"], "output_exists");
        assert_eq!(export_exists.body["output_written"], false);
        assert_eq!(export_exists.body["during"], "export_publish");
        assert_eq!(export_exists.body["injected"], false);

        let export_unavailable = export::export_publish_failure(os(libc::ENOSYS)).into_report();
        assert_eq!(export_unavailable.code, EXIT_UNSUPPORTED_FS);
        assert_eq!(
            export_unavailable.body["reason"],
            "rename_noreplace_unavailable"
        );
        assert_eq!(export_unavailable.body["output_written"], false);

        let export_invalid = export::export_publish_failure(os(libc::EINVAL)).into_report();
        assert_eq!(export_invalid.code, EXIT_INTEGRITY);
        assert_eq!(export_invalid.body["outcome"], "Incomplete");
        assert_eq!(export_invalid.body["reason"], "io_error");
        assert!(export_invalid.body["os_error"]
            .as_str()
            .unwrap()
            .contains("os error 22"));
        assert_ne!(export_invalid.body["outcome"], "Unsupported");

        let export_cross = export::export_publish_failure(os(libc::EXDEV)).into_report();
        assert_eq!(export_cross.code, EXIT_INTEGRITY);
        assert_eq!(export_cross.body["reason"], "io_error");
        assert!(export_cross.body["os_error"]
            .as_str()
            .unwrap()
            .contains("os error 18"));
        assert_ne!(export_cross.body["reason"], "rename_noreplace_unavailable");

        let export_access = export::export_publish_failure(os(libc::EACCES)).into_report();
        let export_busy = export::export_publish_failure(os(libc::EBUSY)).into_report();
        let export_full = export::export_publish_failure(os(libc::ENOSPC)).into_report();
        assert_eq!(export_access.code, EXIT_INTEGRITY);
        assert_eq!(export_busy.code, EXIT_INTEGRITY);
        assert_eq!(export_full.code, EXIT_INTEGRITY);
        assert_eq!(export_access.body["reason"], "io_error");
        assert_eq!(export_busy.body["reason"], "io_error");
        assert_eq!(export_full.body["reason"], "enospc");
        assert_ne!(export_access.body["reason"], "writer_busy");
        assert_ne!(export_busy.body["reason"], "writer_busy");
        assert_ne!(export_access.code, EXIT_LOCKED);
        assert_ne!(export_busy.code, EXIT_LOCKED);
        assert_ne!(export_full.code, EXIT_LOCKED);
        assert_ne!(export_access.body["os_error"], export_busy.body["os_error"]);
        assert_ne!(export_busy.body["os_error"], export_full.body["os_error"]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rename_win32_receipts_stay_in_their_classes() {
        let exists = init_report(os(183)); // ERROR_ALREADY_EXISTS
        assert_eq!(exists.code, EXIT_REJECTED);
        assert_eq!(exists.body["outcome"], "Rejected");
        assert_eq!(exists.body["reason"], "vault_path_exists");
        assert_eq!(exists.body["created_new_vault"], false);

        let unavailable = init_report(os(50)); // ERROR_NOT_SUPPORTED
        assert_eq!(unavailable.code, EXIT_UNSUPPORTED_FS);
        assert_eq!(unavailable.body["reason"], "rename_noreplace_unavailable");

        let cross = init_report(os(17)); // ERROR_NOT_SAME_DEVICE
        assert_eq!(cross.code, EXIT_NOT_PRESERVED);
        assert_eq!(cross.body["reason"], "io_error");

        let full = init_report(os(112)); // ERROR_DISK_FULL
        assert_eq!(full.code, EXIT_NOT_PRESERVED);
        assert_eq!(full.body["reason"], "enospc");

        let export_exists = export::export_publish_failure(os(80)).into_report(); // ERROR_FILE_EXISTS
        assert_eq!(export_exists.code, EXIT_REJECTED);
        assert_eq!(export_exists.body["reason"], "output_exists");

        let export_full = export::export_publish_failure(os(112)).into_report();
        assert_eq!(export_full.code, EXIT_INTEGRITY);
        assert_eq!(export_full.body["reason"], "enospc");
    }
}

#[cfg(test)]
mod published_sync_static {
    use super::*;

    /// Constructed `io::Error` values. `sync_all` is not called. This is not
    /// an observed fsync failure, and it is not EACCES from `File::open`.
    #[test]
    fn constructed_sync_error_stays_published_with_the_actual_reason() {
        let cases = [
            (crate::platform::test_enospc_error(), "enospc"),
            (io::Error::other("simulated sync io error"), "io_error"),
        ];
        for (err, reason) in cases {
            let fail = io_fail(err, "publish_dir_fsync");
            let report = report_published_init(
                fail,
                "vlt_static",
                1,
                None,
                0,
                Path::new("/requested/vault"),
                false,
            );
            assert_eq!(report.code, EXIT_INTEGRITY);
            let body = report.body.as_object().unwrap();
            assert_eq!(body["outcome"], "Initialized");
            assert_eq!(body["reason"], reason);
            assert_ne!(body["reason"], "directory_durability_unknown");
            assert_eq!(body["during"], "publish_dir_fsync");
            assert_eq!(body["injected"], false);
            assert_eq!(body["published"], true);
            assert_eq!(body["created_new_vault"], true);
            assert_eq!(body["new_effect"], true);
            assert_eq!(body["directory_durable"], false);
            assert_eq!(body["parent_fsync_returned_ok"], false);
            assert_eq!(body["output"], "/requested/vault");
            assert!(body.get("preservation").is_none(), "{body:?}");
            assert!(body.get("staging_cleanup").is_none(), "{body:?}");
            assert!(body.get("staging_path").is_none(), "{body:?}");
        }
    }

    #[test]
    fn witness_after_sync_ok_does_not_rewrite_that_ok_as_a_sync_failure() {
        let fail = directory_witness("publish_dir_fsync", InjectAction::IoError);
        let report = report_published_init(
            fail,
            "vlt_static",
            3,
            None,
            0,
            Path::new("/requested/vault"),
            true,
        );
        assert_eq!(report.code, EXIT_INTEGRITY);
        let body = report.body.as_object().unwrap();
        assert_eq!(body["reason"], "injected_io_error");
        assert_ne!(body["reason"], "directory_durability_unknown");
        assert_eq!(body["during"], "publish_dir_fsync");
        assert_eq!(body["injected"], true);
        assert_eq!(body["parent_fsync_returned_ok"], true);
        assert_eq!(body["directory_durable"], false);
        assert_eq!(body["published"], true);
        assert_eq!(body["output"], "/requested/vault");
        assert!(body["note"].as_str().unwrap().contains("not a failed disk"));
    }
}
