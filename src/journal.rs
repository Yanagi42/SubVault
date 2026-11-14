use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{json, Map, Value};

use crate::fsutil::atomic_write;
use crate::ids::{is_command_id, is_lower_hex, is_prefixed_id};
use crate::model::{io_fail, unseen_db, Fail, EXIT_NOT_PRESERVED};
use crate::store;

// One field list is the journal codec. Unknown JSON keys are ignored.
// A missing optional key and JSON null are both absent.
// payload_fingerprint stays optional: an in-progress journal can be
// written before the bytes are hashed, and a missing fingerprint is not
// damage. kind is import or marker. state is started, copying,
// object_ready, aborted, or committed. There is no second journal version.
macro_rules! journal_ty {
    (req_str) => { String };
    (req_i64) => { i64 };
    (opt_str) => { Option<String> };
    (opt_i64) => { Option<i64> };
    (opt_bool) => { Option<bool> };
}

macro_rules! journal_encode {
    (req_str, $v:expr) => {
        Value::String($v.clone())
    };
    (req_i64, $v:expr) => {
        json!(*$v)
    };
    (opt_str, $v:expr) => {
        match $v {
            Some(text) => Value::String(text.clone()),
            None => Value::Null,
        }
    };
    (opt_i64, $v:expr) => {
        match $v {
            Some(n) => json!(*n),
            None => Value::Null,
        }
    };
    (opt_bool, $v:expr) => {
        match $v {
            Some(flag) => Value::Bool(*flag),
            None => Value::Null,
        }
    };
}

macro_rules! journal_decode {
    (req_str, $v:expr) => {
        match $v {
            Some(Value::String(text)) => Ok(text.clone()),
            _ => Err(()),
        }
    };
    (req_i64, $v:expr) => {
        match $v {
            Some(Value::Number(n)) => n.as_i64().ok_or(()),
            _ => Err(()),
        }
    };
    (opt_str, $v:expr) => {
        match $v {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            _ => Err(()),
        }
    };
    (opt_i64, $v:expr) => {
        match $v {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => Ok(Some(n.as_i64().ok_or(())?)),
            _ => Err(()),
        }
    };
    (opt_bool, $v:expr) => {
        match $v {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(flag)) => Ok(Some(*flag)),
            _ => Err(()),
        }
    };
}

macro_rules! define_journal {
    ($($name:ident : $kind:ident),* $(,)?) => {
        #[derive(Clone)]
        pub(crate) struct Journal {
            $(pub $name: journal_ty!($kind),)*
        }

        impl Journal {
            fn to_value(&self) -> Value {
                let mut map = Map::new();
                $(map.insert(stringify!($name).to_string(), journal_encode!($kind, &self.$name));)*
                Value::Object(map)
            }

            fn from_value(value: &Value) -> Result<Self, ()> {
                Ok(Self {
                    $($name: journal_decode!($kind, value.get(stringify!($name)))?,)*
                })
            }
        }
    };
}

define_journal! {
    command_id: req_str,
    kind: req_str,
    generation: req_i64,
    state: req_str,
    batch_id: opt_str,
    display_name: opt_str,
    external_path: opt_str,
    media_type: opt_str,
    occurrence_id: opt_str,
    object_id: opt_str,
    content_sha256: opt_str,
    byte_length: opt_i64,
    payload_fingerprint: opt_str,
    storage_reused: opt_bool,
    reason: opt_str,
    marker_value: opt_str,
    source_id: opt_str,
}

fn accept(journal: &Journal) -> Result<(), ()> {
    if !is_command_id(&journal.command_id) {
        return Err(());
    }
    if journal.kind != "import" && journal.kind != "marker" {
        return Err(());
    }
    if journal.generation < 1 {
        return Err(());
    }
    if !matches!(
        journal.state.as_str(),
        "started" | "copying" | "object_ready" | "aborted" | "committed"
    ) {
        return Err(());
    }
    if let Some(id) = &journal.object_id {
        if !is_prefixed_id(id, "obj_") {
            return Err(());
        }
    }
    if let Some(id) = &journal.occurrence_id {
        if !is_prefixed_id(id, "occ_") {
            return Err(());
        }
    }
    if let Some(id) = &journal.source_id {
        if !is_prefixed_id(id, "src_") {
            return Err(());
        }
    }
    if let Some(hash) = &journal.content_sha256 {
        if !is_lower_hex(hash, 64) {
            return Err(());
        }
    }
    Ok(())
}

pub(crate) fn path(root: &Path, command_id: &str) -> PathBuf {
    root.join("journal").join(format!("{command_id}.json"))
}

pub(crate) fn staging_path(root: &Path, command_id: &str) -> PathBuf {
    root.join("staging").join(format!("{command_id}.partial"))
}

pub(crate) fn write(root: &Path, journal: &Journal) -> Result<(), Fail> {
    let bytes = serde_json::to_vec_pretty(&journal.to_value())
        .map_err(|_| Fail::new(EXIT_NOT_PRESERVED, "NotPreserved", "journal_encode"))?;
    atomic_write(&path(root, &journal.command_id), &bytes)
        .map_err(|err| io_fail(err, "journal_write"))
}

pub(crate) fn read_file(path: &Path) -> Result<Journal, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let journal = Journal::from_value(&value)?;
    accept(&journal)?;
    Ok(journal)
}

pub(crate) struct Scan {
    pub valid: Vec<Journal>,
    pub invalid: Vec<String>,
    pub unexpected: Vec<String>,
    /// Directory listing failed. `valid` stays empty so callers do not invent
    /// incomplete commands from a listing they do not have.
    pub unlisted: Option<String>,
}

pub(crate) struct SideNotice {
    pub code: &'static str,
    pub during: &'static str,
    pub os_error: String,
}

pub(crate) struct Reconcile {
    pub misses: Vec<LabelMiss>,
    pub journal_unlisted: Option<String>,
}

pub(crate) fn scan(root: &Path) -> Scan {
    let mut scan = Scan {
        valid: Vec::new(),
        invalid: Vec::new(),
        unexpected: Vec::new(),
        unlisted: None,
    };
    let dir = root.join("journal");
    if !dir.exists() {
        return scan;
    }
    let mut names = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) => {
            scan.unlisted = Some(err.to_string());
            return scan;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().to_string()),
            Err(err) => {
                scan.unlisted = Some(err.to_string());
                scan.valid.clear();
                scan.invalid.clear();
                scan.unexpected.clear();
                return scan;
            }
        }
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
        match read_file(&dir.join(&name)) {
            Ok(journal) if journal.command_id == command_id => scan.valid.push(journal),
            _ => scan.invalid.push(name),
        }
    }
    scan
}

pub(crate) struct LabelMiss {
    pub command_id: String,
    pub during: String,
    pub os_error: String,
}

/// Rewrite a journal label only when that command row is already committed.
/// A failed rewrite is returned to the caller. It is not a verdict that the
/// committed row or its hosted bytes were lost.
pub(crate) fn reconcile(root: &Path, conn: &Connection) -> Result<Reconcile, Fail> {
    let found = scan(root);
    if found.unlisted.is_some() {
        return Ok(Reconcile {
            misses: Vec::new(),
            journal_unlisted: found.unlisted,
        });
    }
    let mut missed = Vec::new();
    for journal in found.valid {
        if journal.state == "committed" {
            continue;
        }
        if store::command(conn, &journal.command_id)
            .map_err(|err| unseen_db(err, "reconcile"))?
            .is_some()
        {
            let mut journal = journal;
            journal.state = "committed".into();
            journal.reason = None;
            if let Err(fail) = write(root, &journal) {
                missed.push(LabelMiss {
                    command_id: journal.command_id,
                    during: fail
                        .extra
                        .get("during")
                        .and_then(|value| value.as_str())
                        .unwrap_or("journal_write")
                        .to_string(),
                    os_error: fail
                        .extra
                        .get("os_error")
                        .and_then(|value| value.as_str())
                        .unwrap_or(fail.reason.as_str())
                        .to_string(),
                });
            }
        }
    }
    Ok(Reconcile {
        misses: missed,
        journal_unlisted: None,
    })
}

pub(crate) fn clean_temps(root: &Path) -> Vec<SideNotice> {
    let mut notices = Vec::new();
    collect_dir(
        &root.join("staging"),
        &mut notices,
        "staging_unlisted",
        "read_staging",
        true,
    );
    collect_dir(
        &root.join("journal"),
        &mut notices,
        "journal_unlisted",
        "read_journal",
        false,
    );
    notices
}

fn collect_dir(
    dir: &Path,
    notices: &mut Vec<SideNotice>,
    list_code: &'static str,
    list_during: &'static str,
    every_file: bool,
) {
    if !dir.is_dir() {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            notices.push(SideNotice {
                code: list_code,
                during: list_during,
                os_error: err.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                notices.push(SideNotice {
                    code: list_code,
                    during: list_during,
                    os_error: err.to_string(),
                });
                return;
            }
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let temporary =
            name.ends_with(".json.tmp") || (name.starts_with('.') && name.ends_with(".tmp"));
        if !every_file && !temporary {
            continue;
        }
        let meta = match fs::symlink_metadata(entry.path()) {
            Ok(meta) => meta,
            Err(err) => {
                notices.push(SideNotice {
                    code: if every_file {
                        "staging_unreadable"
                    } else {
                        "journal_tmp_unreadable"
                    },
                    during: if every_file {
                        "staging_meta"
                    } else {
                        "tmp_meta"
                    },
                    os_error: err.to_string(),
                });
                continue;
            }
        };
        if meta.is_file() || meta.file_type().is_symlink() {
            if let Err(err) = fs::remove_file(entry.path()) {
                notices.push(SideNotice {
                    code: if every_file {
                        "staging_not_removed"
                    } else {
                        "journal_tmp_not_removed"
                    },
                    during: if every_file {
                        "remove_staging"
                    } else {
                        "remove_tmp"
                    },
                    os_error: err.to_string(),
                });
            }
        }
    }
}

pub(crate) fn count_staging(root: &Path) -> Option<i64> {
    match fs::read_dir(root.join("staging")) {
        Ok(rd) => Some(rd.filter_map(|entry| entry.ok()).count() as i64),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Some(0),
        Err(_) => None,
    }
}
