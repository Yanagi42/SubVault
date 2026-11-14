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

pub const VAULT_BANNER: &str = "\
SYNTHETIC-ONLY DISPOSABLE VAULT
This directory is a synthetic SubVault T1 store.
It is not an approved store for real personal data.
Protection and recovery preferences are unanswered.
Plaintext bytes here are a temporary test mode, not a completed security design.
No telemetry, no network, and no real-data import.
Not a user-ready product.
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
}

impl Fail {
    pub fn new(code: i32, outcome: &'static str, reason: impl Into<String>) -> Self {
        Self {
            code,
            outcome,
            reason: reason.into(),
            extra: Map::new(),
        }
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

pub fn capabilities() -> Value {
    serde_json::json!({
        "preservation": "available",
        "arrangement": "not_available",
        "full_text_search": "not_available",
        "extraction": "not_available",
        "purchases": "not_available",
        "entitlements": "not_available",
        "real_data_import": "not_available"
    })
}

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
