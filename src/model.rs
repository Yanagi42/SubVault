use serde_json::{Map, Value};

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_INTEGRITY: i32 = 3;
pub const EXIT_NOT_VAULT: i32 = 10;
pub const EXIT_LOCKED: i32 = 11;
pub const EXIT_RECOVERY: i32 = 12;
pub const EXIT_DAMAGED: i32 = 13;
pub const EXIT_VERSION: i32 = 14;
pub const EXIT_NOT_PRESERVED: i32 = 15;
pub const EXIT_REJECTED: i32 = 16;
pub const EXIT_FAULT_MISSED: i32 = 17;
pub const EXIT_UNSUPPORTED_FS: i32 = 18;
pub const EXIT_NOT_SYNTHETIC: i32 = 19;
pub const EXIT_STOP_MISSED: i32 = 20;
pub const EXIT_UNCERTAIN: i32 = 21;
pub const EXIT_AUTH: i32 = 22;

pub const PRESERVED: &str = "preserved";
pub const NOT_PRESERVED: &str = "not_preserved";
pub const NOT_ARRANGED: &str = "not_arranged";
pub const NOT_SEARCHABLE: &str = "not_searchable";

pub const SYNTHETIC_BANNER_FILE: &str = "SYNTHETIC_ONLY";
pub const PRODUCTION_BANNER_FILE: &str = "SUBVAULT_VAULT";

pub const VAULT_BANNER: &str = "\
SYNTHETIC-ONLY DISPOSABLE VAULT
This directory is a synthetic SubVault T1 store.
It is not an approved store for real personal data.
Protection and recovery preferences are unanswered.
Plaintext bytes here are a temporary test mode, not a completed security design.
No telemetry, no network, and no real-data import.
Not a user-ready product.
";

pub const PRODUCTION_BANNER_PASSPHRASE: &str = "\
SUBVAULT LOCAL-FIRST VAULT (FORMAT 6)
This directory is a real-data SubVault store (isolated from synthetic Formats 1-5).
Protection mode: passphrase_aead_v1 (Argon2id + HKDF-SHA256 + XChaCha20-Poly1305).
Originals, staging, journals, and sensitive SQLite fields are encrypted at rest.
SQLite structural metadata (random IDs, row counts, timestamps, media_type) remains unencrypted.
Offline recovery key wraps the vault DEK; losing both passphrase and recovery key permanently loses access.
No telemetry, no bank credentials, and no inbox OAuth.
";

pub const PRODUCTION_BANNER_OS_DISK: &str = "\
SUBVAULT LOCAL-FIRST VAULT (FORMAT 6)
This directory is a real-data SubVault store (isolated from synthetic Formats 1-5).
Protection mode: os_disk_assumed (plaintext application store; relies on OS full-disk encryption such as BitLocker/LUKS/FileVault).
Do not place this directory on unencrypted removable media or untrusted sync folders.
No telemetry, no bank credentials, and no inbox OAuth.
";

pub struct Report {
    pub code: i32,
    pub body: Value,
}

pub struct Fail {
    pub code: i32,
    pub outcome: &'static str,
    pub reason: String,
    pub extra: Map<String, Value>,
    /// When set, unwind leaves the staging file for a later open to remove.
    /// Real I/O errors leave this false. Injected probes set it explicitly.
    pub keep_staging: bool,
}

impl Fail {
    pub fn new(code: i32, outcome: &'static str, reason: impl Into<String>) -> Self {
        Self {
            code,
            outcome,
            reason: reason.into(),
            extra: Map::new(),
            keep_staging: false,
        }
    }

    pub fn keep_staging(mut self) -> Self {
        self.keep_staging = true;
        self
    }

    pub fn extra(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    pub fn into_report(self) -> Report {
        let mut body = honesty(self.outcome);
        body.insert("reason".to_string(), Value::String(self.reason));
        body.insert("new_effect".to_string(), Value::Bool(false));
        for (key, value) in self.extra {
            body.insert(key, value);
        }
        Report {
            code: self.code,
            body: Value::Object(body),
        }
    }
}

pub fn honesty(outcome: &str) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("program".into(), Value::String("subvault".into()));
    body.insert("slice".into(), Value::String("T1".into()));
    body.insert("outcome".into(), Value::String(outcome.into()));
    body.insert("synthetic_only".into(), Value::Bool(true));
    body.insert("user_ready".into(), Value::Bool(false));
    body.insert("security_complete".into(), Value::Bool(false));
    body.insert("purchase_id".into(), Value::Null);
    body.insert("entitlement_id".into(), Value::Null);
    body
}

pub fn apply_format_honesty(
    body: &mut Map<String, Value>,
    vault_format: i64,
    protection: Option<crate::crypto::ProtectionMode>,
) {
    if vault_format >= 6 {
        body.insert("synthetic_only".into(), Value::Bool(false));
        body.insert("user_ready".into(), Value::Bool(false));
        let prot = protection
            .map(crate::crypto::ProtectionMode::as_meta)
            .unwrap_or(crate::crypto::PROTECTION_OS_DISK);
        body.insert("protection".into(), Value::String(prot.into()));
        body.insert(
            "security_complete".into(),
            Value::Bool(matches!(
                protection,
                Some(crate::crypto::ProtectionMode::Passphrase)
            )),
        );
    }
}

pub fn capabilities() -> Value {
    capabilities_for(1)
}

pub fn capabilities_for(vault_format: i64) -> Value {
    serde_json::json!({
        "preservation": "available",
        "arrangement": "not_available",
        "full_text_search": "not_available",
        "extraction": "not_available",
        "body_fragment": "available",
        "html_static_text": "limited",
        "pdf_static_text": "limited",
        "html_rendering": "not_available",
        "html_network": "not_available",
        "stated_facts": "available",
        "pdf_extraction": "not_available",
        "ocr": "not_available",
        "purchases": "not_available",
        "entitlements": "not_available",
        "real_data_import": if vault_format >= 6 { "available" } else { "not_available" },
        "at_rest_encryption": if vault_format >= 6 { "application_aead_v1" } else { "not_available" }
    })
}

pub(crate) fn io_fail(err: std::io::Error, during: &str) -> Fail {
    let enospc = crate::platform::is_enospc(&err);
    Fail::new(
        EXIT_NOT_PRESERVED,
        "NotPreserved",
        if enospc { "enospc" } else { "io_error" },
    )
    .extra("injected", Value::Bool(false))
    .extra("during", Value::String(during.into()))
    .extra("os_error", Value::String(err.to_string()))
    .extra("new_effect", Value::Bool(false))
}

pub(crate) fn db_fail(err: rusqlite::Error, during: &str) -> Fail {
    if let Some(detail) = crate::store::cipher_tamper_detail(&err) {
        return Fail::new(EXIT_RECOVERY, "RecoveryRequired", "ciphertext_auth_failed")
            .extra("during", Value::String(during.into()))
            .extra("detail", Value::String(detail))
            .extra("new_effect", Value::Bool(false));
    }
    let full = matches!(
        &err,
        rusqlite::Error::SqliteFailure(ffi, _) if ffi.code == rusqlite::ErrorCode::DiskFull
    );
    Fail::new(
        EXIT_NOT_PRESERVED,
        "NotPreserved",
        if full { "enospc" } else { "db_error" },
    )
    .extra("injected", Value::Bool(false))
    .extra("during", Value::String(during.into()))
    .extra("os_error", Value::String(err.to_string()))
    .extra("new_effect", Value::Bool(false))
}

/// The call has not seen the historical rows, so it must not call them preserved
/// or not preserved. `during` names the step. There is no source list.
pub(crate) fn unseen_io(err: std::io::Error, during: &str) -> Fail {
    unseen(during, err.to_string())
}

pub(crate) fn unseen_db(err: rusqlite::Error, during: &str) -> Fail {
    if let Some(detail) = crate::store::cipher_tamper_detail(&err) {
        return Fail::new(EXIT_RECOVERY, "RecoveryRequired", "ciphertext_auth_failed")
            .extra("during", Value::String(during.into()))
            .extra("detail", Value::String(detail))
            .extra("new_effect", Value::Bool(false));
    }
    unseen(during, err.to_string())
}

pub(crate) fn cipher_tamper_fail(err: crate::crypto::CryptoError, during: &str) -> Fail {
    Fail::new(EXIT_RECOVERY, "RecoveryRequired", "ciphertext_auth_failed")
        .extra("during", Value::String(during.into()))
        .extra("detail", Value::String(err.to_string()))
        .extra("new_effect", Value::Bool(false))
}

fn unseen(during: &str, detail: String) -> Fail {
    Fail::new(EXIT_UNCERTAIN, "Uncertain", "vault_unseen")
        .extra("during", Value::String(during.into()))
        .extra("os_error", Value::String(detail))
        .extra("new_effect", Value::Null)
        .extra("injected", Value::Bool(false))
}

#[cfg(target_os = "linux")]
pub fn platform_limits(magic: Option<i64>) -> Value {
    serde_json::json!({
        "writer_lock": "unix_flock_exclusive_nonblocking",
        "pid_and_expiry_are_not_a_lock": true,
        "windows_native": "not_tested",
        "power_loss": "not_tested",
        "bad_disk": "not_tested",
        "sync_directory": "not_supported_and_not_detected",
        "network_filesystem": "nfs_and_cifs_magic_refused_not_mounted_in_tests",
        "filesystem_magic": magic.map(|m| format!("0x{m:x}")),
        "source_snapshot": "read_loop_not_atomic"
    })
}

#[cfg(target_os = "windows")]
pub fn platform_limits(magic: Option<i64>) -> Value {
    serde_json::json!({
        "writer_lock": "windows_lockfileex_exclusive_nonblocking",
        "pid_and_expiry_are_not_a_lock": true,
        "windows_native": "tested_x86_64_windows_msvc",
        "directory_identity": "windows_file_id_128",
        "directory_publish": "windows_file_rename_info_noreplace",
        "directory_fsync": "windows_flush_or_ntfs_metadata_journal",
        "power_loss": "not_tested",
        "bad_disk": "not_tested",
        "sync_directory": "not_supported_and_not_detected",
        "network_filesystem": "windows_drive_remote_and_unc_refused",
        "filesystem_magic": magic.map(|m| format!("0x{m:x}")),
        "source_snapshot": "read_loop_not_atomic"
    })
}
