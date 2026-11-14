use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{json, Map, Value};

use crate::fsutil::atomic_write;
use crate::ids::{is_command_id, is_lower_hex, is_prefixed_id};
use crate::model::{self, io_fail, Fail, EXIT_NOT_PRESERVED};
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
}

pub(crate) fn scan(root: &Path) -> Result<Scan, Fail> {
    let mut scan = Scan {
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
        match read_file(&dir.join(&name)) {
            Ok(journal) if journal.command_id == command_id => scan.valid.push(journal),
            _ => scan.invalid.push(name),
        }
    }
    Ok(scan)
}

pub(crate) fn reconcile(root: &Path, conn: &Connection) -> Result<(), Fail> {
    for journal in scan(root)?.valid {
        if journal.state == "committed" {
            continue;
        }
        if store::command(conn, &journal.command_id)
            .map_err(|err| model::db_fail(err, "reconcile"))?
            .is_some()
        {
            let mut journal = journal;
            journal.state = "committed".into();
            journal.reason = None;
            write(root, &journal)?;
        }
    }
    Ok(())
}

pub(crate) fn clean_temps(root: &Path) -> Result<(), Fail> {
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

pub(crate) fn count_staging(root: &Path) -> i64 {
    let Ok(rd) = fs::read_dir(root.join("staging")) else {
        return 0;
    };
    rd.filter_map(|entry| entry.ok()).count() as i64
}
