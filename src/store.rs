use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub const SCHEMA: &str = r#"
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE objects (
    object_id TEXT PRIMARY KEY,
    content_sha256 TEXT NOT NULL UNIQUE,
    byte_length INTEGER NOT NULL,
    relpath TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state = 'complete')
);
CREATE TABLE sources (
    source_id TEXT PRIMARY KEY,
    object_id TEXT NOT NULL REFERENCES objects(object_id),
    content_sha256 TEXT NOT NULL UNIQUE,
    byte_length INTEGER NOT NULL,
    media_type TEXT NOT NULL,
    user_marker TEXT,
    preservation TEXT NOT NULL CHECK (preservation = 'preserved'),
    arrangement TEXT NOT NULL CHECK (arrangement = 'not_arranged'),
    searchability TEXT NOT NULL CHECK (searchability = 'not_searchable')
);
CREATE TABLE occurrences (
    occurrence_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(source_id),
    command_id TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    external_path TEXT,
    storage_reused INTEGER NOT NULL CHECK (storage_reused IN (0, 1)),
    recorded_at INTEGER NOT NULL
);
CREATE TABLE commands (
    command_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    payload_fingerprint TEXT NOT NULL,
    result_json TEXT NOT NULL,
    outcome TEXT NOT NULL,
    committed_at INTEGER NOT NULL
);
"#;

#[derive(Clone)]
pub struct ObjectRow {
    pub object_id: String,
    pub content_sha256: String,
    pub byte_length: i64,
    pub relpath: String,
}

#[derive(Clone)]
pub struct SourceRow {
    pub source_id: String,
    pub object_id: String,
    pub content_sha256: String,
    pub byte_length: i64,
    pub media_type: String,
    pub user_marker: Option<String>,
}

#[derive(Clone)]
pub struct OccurrenceRow {
    pub occurrence_id: String,
    pub source_id: String,
    pub command_id: String,
    pub batch_id: String,
    pub display_name: String,
    pub external_path: Option<String>,
    pub storage_reused: bool,
    pub recorded_at: i64,
}

#[derive(Clone)]
pub struct CommandRow {
    pub command_id: String,
    pub kind: String,
    pub payload_fingerprint: String,
    pub result_json: String,
    pub outcome: String,
}

pub fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(0))?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    Ok(())
}

pub fn init_schema(conn: &Connection, vault_id: &str) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch("PRAGMA user_version = 1;")?;
    let meta = [
        ("schema_version", "1"),
        ("vault_id", vault_id),
        ("synthetic_only", "true"),
        ("disposable", "true"),
        ("protection", "unanswered_synthetic_plaintext"),
        ("primary_input", "unanswered"),
        ("slice", "T1"),
    ];
    for (key, value) in meta {
        conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    Ok(())
}

pub fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
}

pub fn meta(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
}

pub fn command(conn: &Connection, id: &str) -> rusqlite::Result<Option<CommandRow>> {
    conn.query_row(
        "SELECT command_id, kind, payload_fingerprint, result_json, outcome FROM commands WHERE command_id = ?1",
        [id],
        |row| {
            Ok(CommandRow {
                command_id: row.get(0)?,
                kind: row.get(1)?,
                payload_fingerprint: row.get(2)?,
                result_json: row.get(3)?,
                outcome: row.get(4)?,
            })
        },
    )
    .optional()
}

pub fn object_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<ObjectRow>> {
    conn.query_row(
        "SELECT object_id, content_sha256, byte_length, relpath FROM objects WHERE object_id = ?1",
        [id],
        map_object,
    )
    .optional()
}

pub fn object_by_hash(conn: &Connection, hash: &str) -> rusqlite::Result<Option<ObjectRow>> {
    conn.query_row(
        "SELECT object_id, content_sha256, byte_length, relpath FROM objects WHERE content_sha256 = ?1",
        [hash],
        map_object,
    )
    .optional()
}

pub fn list_objects(conn: &Connection) -> rusqlite::Result<Vec<ObjectRow>> {
    let mut stmt = conn.prepare(
        "SELECT object_id, content_sha256, byte_length, relpath FROM objects ORDER BY object_id",
    )?;
    let rows = stmt.query_map([], map_object)?;
    rows.collect()
}

pub fn source_by_hash(conn: &Connection, hash: &str) -> rusqlite::Result<Option<SourceRow>> {
    conn.query_row(
        "SELECT source_id, object_id, content_sha256, byte_length, media_type, user_marker FROM sources WHERE content_sha256 = ?1",
        [hash],
        map_source,
    )
    .optional()
}

pub fn source_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<SourceRow>> {
    conn.query_row(
        "SELECT source_id, object_id, content_sha256, byte_length, media_type, user_marker FROM sources WHERE source_id = ?1",
        [id],
        map_source,
    )
    .optional()
}

pub fn list_sources(conn: &Connection) -> rusqlite::Result<Vec<SourceRow>> {
    let mut stmt = conn.prepare(
        "SELECT source_id, object_id, content_sha256, byte_length, media_type, user_marker FROM sources ORDER BY source_id",
    )?;
    let rows = stmt.query_map([], map_source)?;
    rows.collect()
}

pub fn list_occurrences(conn: &Connection) -> rusqlite::Result<Vec<OccurrenceRow>> {
    let mut stmt = conn.prepare(
        "SELECT occurrence_id, source_id, command_id, batch_id, display_name, external_path, storage_reused, recorded_at FROM occurrences ORDER BY recorded_at, occurrence_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(OccurrenceRow {
            occurrence_id: row.get(0)?,
            source_id: row.get(1)?,
            command_id: row.get(2)?,
            batch_id: row.get(3)?,
            display_name: row.get(4)?,
            external_path: row.get(5)?,
            storage_reused: row.get::<_, i64>(6)? != 0,
            recorded_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn list_commands(conn: &Connection) -> rusqlite::Result<Vec<CommandRow>> {
    let mut stmt = conn.prepare(
        "SELECT command_id, kind, payload_fingerprint, result_json, outcome FROM commands ORDER BY committed_at, command_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CommandRow {
            command_id: row.get(0)?,
            kind: row.get(1)?,
            payload_fingerprint: row.get(2)?,
            result_json: row.get(3)?,
            outcome: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn checkpoint(conn: &Connection) -> rusqlite::Result<()> {
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointRow {
    pub busy: i64,
    pub log: i64,
    pub checkpointed: i64,
}

#[derive(Debug)]
pub enum SqlAnswer<T> {
    Value(T),
    Sqlite(rusqlite::Error),
    Shape(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
pub enum TruncateVerdict {
    Complete,
    Busy(CheckpointRow),
    Abnormal(CheckpointRow),
}

/// One row from bundled SQLite 3.46 `PRAGMA journal_mode`.
/// The mode name is the lowercase string from `sqlite3JournalModename`.
pub fn read_journal_mode(conn: &Connection) -> SqlAnswer<String> {
    let mut stmt = match conn.prepare("PRAGMA journal_mode") {
        Ok(stmt) => stmt,
        Err(err) => return SqlAnswer::Sqlite(err),
    };
    if stmt.column_count() != 1 {
        return SqlAnswer::Shape("column_count");
    }
    let mut rows = match stmt.query([]) {
        Ok(rows) => rows,
        Err(err) => return SqlAnswer::Sqlite(err),
    };
    let mode = match rows.next() {
        Ok(Some(row)) => match row.get(0) {
            Ok(mode) => mode,
            Err(_) => return SqlAnswer::Shape("mode"),
        },
        Ok(None) => return SqlAnswer::Shape("no_row"),
        Err(err) => return SqlAnswer::Sqlite(err),
    };
    match rows.next() {
        Ok(None) => SqlAnswer::Value(mode),
        Ok(Some(_)) => SqlAnswer::Shape("extra_row"),
        Err(err) => SqlAnswer::Sqlite(err),
    }
}

/// One `PRAGMA wal_checkpoint(TRUNCATE)` row: busy, log, checkpointed.
/// `SQLITE_BUSY` is a row with busy=1, not a statement error.
pub fn read_truncate_checkpoint(conn: &Connection) -> SqlAnswer<CheckpointRow> {
    let mut stmt = match conn.prepare("PRAGMA wal_checkpoint(TRUNCATE)") {
        Ok(stmt) => stmt,
        Err(err) => return SqlAnswer::Sqlite(err),
    };
    if stmt.column_count() != 3 {
        return SqlAnswer::Shape("column_count");
    }
    let mut rows = match stmt.query([]) {
        Ok(rows) => rows,
        Err(err) => return SqlAnswer::Sqlite(err),
    };
    let row = match rows.next() {
        Ok(Some(row)) => row,
        Ok(None) => return SqlAnswer::Shape("no_row"),
        Err(err) => return SqlAnswer::Sqlite(err),
    };
    let busy = match row.get(0) {
        Ok(value) => value,
        Err(_) => return SqlAnswer::Shape("busy"),
    };
    let log = match row.get(1) {
        Ok(value) => value,
        Err(_) => return SqlAnswer::Shape("log"),
    };
    let checkpointed = match row.get(2) {
        Ok(value) => value,
        Err(_) => return SqlAnswer::Shape("checkpointed"),
    };
    let row = CheckpointRow {
        busy,
        log,
        checkpointed,
    };
    match rows.next() {
        Ok(None) => SqlAnswer::Value(row),
        Ok(Some(_)) => SqlAnswer::Shape("extra_row"),
        Err(err) => SqlAnswer::Sqlite(err),
    }
}

/// Judge a TRUNCATE row for the journal mode that was actually read back.
///
/// WAL success is busy=0, log=0, checkpointed=0: SQLite's TRUNCATE note says
/// both frame counts are zero because the log was truncated to zero bytes.
/// Non-WAL success is busy=0, log=-1, checkpointed=-1. All-zero is not success
/// in that mode. busy!=0 is blocked even when the other columns are not zero.
/// A statement that returned `Ok` is not itself a completed checkpoint.
pub fn judge_truncate_checkpoint(journal_mode: &str, row: CheckpointRow) -> TruncateVerdict {
    if row.busy != 0 {
        return TruncateVerdict::Busy(row);
    }
    if journal_mode == "wal" {
        if row.log == 0 && row.checkpointed == 0 {
            return TruncateVerdict::Complete;
        }
        return TruncateVerdict::Abnormal(row);
    }
    if row.log == -1 && row.checkpointed == -1 {
        return TruncateVerdict::Complete;
    }
    TruncateVerdict::Abnormal(row)
}

fn map_object(row: &rusqlite::Row<'_>) -> rusqlite::Result<ObjectRow> {
    Ok(ObjectRow {
        object_id: row.get(0)?,
        content_sha256: row.get(1)?,
        byte_length: row.get(2)?,
        relpath: row.get(3)?,
    })
}

fn map_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRow> {
    Ok(SourceRow {
        source_id: row.get(0)?,
        object_id: row.get(1)?,
        content_sha256: row.get(2)?,
        byte_length: row.get(3)?,
        media_type: row.get(4)?,
        user_marker: row.get(5)?,
    })
}

pub fn insert_object(tx: &Transaction<'_>, object: &ObjectRow) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO objects(object_id, content_sha256, byte_length, relpath, state) VALUES (?1, ?2, ?3, ?4, 'complete')",
        params![object.object_id, object.content_sha256, object.byte_length, object.relpath],
    )?;
    Ok(())
}

pub fn insert_source(tx: &Transaction<'_>, source: &SourceRow) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO sources(source_id, object_id, content_sha256, byte_length, media_type, user_marker, preservation, arrangement, searchability) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'preserved', 'not_arranged', 'not_searchable')",
        params![
            source.source_id,
            source.object_id,
            source.content_sha256,
            source.byte_length,
            source.media_type,
            source.user_marker
        ],
    )?;
    Ok(())
}

pub fn insert_occurrence(tx: &Transaction<'_>, row: &OccurrenceRow) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO occurrences(occurrence_id, source_id, command_id, batch_id, display_name, external_path, storage_reused, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            row.occurrence_id,
            row.source_id,
            row.command_id,
            row.batch_id,
            row.display_name,
            row.external_path,
            if row.storage_reused { 1 } else { 0 },
            row.recorded_at
        ],
    )?;
    Ok(())
}

pub fn insert_command(
    tx: &Transaction<'_>,
    row: &CommandRow,
    committed_at: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO commands(command_id, kind, payload_fingerprint, result_json, outcome, committed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            row.command_id,
            row.kind,
            row.payload_fingerprint,
            row.result_json,
            row.outcome,
            committed_at
        ],
    )?;
    Ok(())
}

pub const BODY_SCHEMA: &str = "1";

pub const BODY_TABLES: &[&str] = &[
    "source_versions",
    "extraction_runs",
    "extraction_attempts",
    "candidates",
];

pub const FACT_TABLES: &[&str] = &["facts", "decisions", "t3_receipts"];

pub const BODY_DDL: &str = r#"
CREATE TABLE source_versions (
    source_version_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(source_id),
    object_id TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    byte_length INTEGER NOT NULL,
    media_type TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role = 'user_provided_fragment'),
    eml INTEGER NOT NULL CHECK (eml = 0)
);
CREATE TABLE extraction_runs (
    run_id TEXT PRIMARY KEY,
    logical_key TEXT NOT NULL UNIQUE,
    source_version_id TEXT NOT NULL REFERENCES source_versions(source_version_id),
    source_id TEXT NOT NULL,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    normalization TEXT NOT NULL,
    join_version TEXT NOT NULL,
    rules_version TEXT NOT NULL,
    config_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'succeeded', 'failed', 'cancelled'))
);
CREATE TABLE extraction_attempts (
    attempt_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES extraction_runs(run_id),
    attempt_no INTEGER NOT NULL,
    command_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'cancelled', 'late', 'abandoned', 'partial')),
    published INTEGER NOT NULL CHECK (published IN (0, 1)),
    detail_json TEXT NOT NULL,
    UNIQUE (run_id, attempt_no)
);
CREATE TABLE candidates (
    candidate_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES extraction_runs(run_id),
    attempt_id TEXT NOT NULL REFERENCES extraction_attempts(attempt_id),
    source_version_id TEXT NOT NULL REFERENCES source_versions(source_version_id),
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL,
    usable INTEGER NOT NULL CHECK (usable IN (0, 1)),
    excerpt TEXT NOT NULL,
    value_json TEXT NOT NULL,
    locator_json TEXT NOT NULL,
    adopted INTEGER NOT NULL CHECK (adopted = 0),
    UNIQUE (run_id, ordinal)
);
"#;

/// Format 3 only. These tables are created with the vault, not by a later
/// migration. `source_versions` stays the existing body table.
pub const FACT_DDL: &str = r#"
CREATE TABLE facts (
    fact_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(source_id),
    source_version_id TEXT NOT NULL REFERENCES source_versions(source_version_id),
    run_id TEXT NOT NULL REFERENCES extraction_runs(run_id),
    candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id),
    fact_type TEXT NOT NULL CHECK (fact_type IN ('stated_amount_paid', 'stated_service_period')),
    value_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    UNIQUE (candidate_id, fact_type)
);
CREATE TABLE decisions (
    decision_id TEXT PRIMARY KEY,
    fact_id TEXT NOT NULL REFERENCES facts(fact_id),
    command_id TEXT NOT NULL,
    candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id),
    slot TEXT NOT NULL CHECK (slot IN ('stated_amount_paid', 'stated_service_period')),
    kind TEXT NOT NULL CHECK (kind IN ('rule_adopt', 'human_correct', 'undo')),
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('rule', 'human')),
    actor_id TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
    rule_version TEXT,
    prior_value_json TEXT,
    new_value_json TEXT NOT NULL,
    target_decision_id TEXT REFERENCES decisions(decision_id),
    CHECK (
        (kind = 'rule_adopt' AND actor_kind = 'rule' AND rule_version IS NOT NULL
            AND target_decision_id IS NULL AND prior_value_json IS NULL)
        OR (kind = 'human_correct' AND actor_kind = 'human' AND rule_version IS NULL
            AND target_decision_id IS NULL AND prior_value_json IS NOT NULL)
        OR (kind = 'undo' AND actor_kind = 'human' AND rule_version IS NULL
            AND target_decision_id IS NOT NULL AND prior_value_json IS NOT NULL)
    )
);
CREATE UNIQUE INDEX decisions_command_candidate_slot
    ON decisions(command_id, candidate_id, slot);
CREATE INDEX decisions_fact ON decisions(fact_id);
CREATE TABLE t3_receipts (
    command_id TEXT PRIMARY KEY,
    payload_version TEXT NOT NULL CHECK (payload_version = 't3-payload-1'),
    canonical_payload TEXT NOT NULL,
    result_json TEXT NOT NULL,
    recorded_at INTEGER NOT NULL
);
"#;

pub fn table_exists(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
}

pub fn update_marker(
    tx: &Transaction<'_>,
    source_id: &str,
    marker: &str,
) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE sources SET user_marker = ?1 WHERE source_id = ?2",
        params![marker, source_id],
    )
}

#[cfg(test)]
mod checkpoint_contract {
    use super::{
        judge_truncate_checkpoint, read_journal_mode, read_truncate_checkpoint, CheckpointRow,
        SqlAnswer, TruncateVerdict,
    };
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;

    fn row(busy: i64, log: i64, checkpointed: i64) -> CheckpointRow {
        CheckpointRow {
            busy,
            log,
            checkpointed,
        }
    }

    #[test]
    fn truncate_rows_follow_the_bundled_sqlite_contract() {
        // Static rows from sqlite 3.46 OP_Checkpoint / wal_checkpoint_v2.
        // These are not a live fault and not an injection.
        assert_eq!(
            judge_truncate_checkpoint("wal", row(0, 0, 0)),
            TruncateVerdict::Complete
        );
        assert_eq!(
            judge_truncate_checkpoint("wal", row(1, 4, 2)),
            TruncateVerdict::Busy(row(1, 4, 2))
        );
        assert_eq!(
            judge_truncate_checkpoint("wal", row(0, 4, 4)),
            TruncateVerdict::Abnormal(row(0, 4, 4))
        );
        assert_eq!(
            judge_truncate_checkpoint("delete", row(0, -1, -1)),
            TruncateVerdict::Complete
        );
        assert_eq!(
            judge_truncate_checkpoint("delete", row(0, 0, 0)),
            TruncateVerdict::Abnormal(row(0, 0, 0))
        );
        assert_eq!(
            judge_truncate_checkpoint("WAL", row(0, 0, 0)),
            TruncateVerdict::Abnormal(row(0, 0, 0))
        );
    }

    #[test]
    fn bundled_sqlite_returns_wal_truncate_zeros_and_non_wal_negatives() {
        // Live answers from the bundled library. Not a power loss and not an injection.
        let dir = std::env::temp_dir().join(format!(
            "subvault-checkpoint-contract-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let wal_path = dir.join("wal.db");
        let delete_path = dir.join("delete.db");

        let wal = Connection::open(&wal_path).unwrap();
        wal.pragma_update(None, "journal_mode", "WAL").unwrap();
        wal.busy_timeout(std::time::Duration::from_millis(0))
            .unwrap();
        match read_journal_mode(&wal) {
            SqlAnswer::Value(mode) => assert_eq!(mode, "wal"),
            other => panic!("journal_mode read was not the wal string: {other:?}"),
        }
        wal.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);")
            .unwrap();
        match read_truncate_checkpoint(&wal) {
            SqlAnswer::Value(got) => assert_eq!(got, row(0, 0, 0)),
            other => panic!("WAL TRUNCATE did not return one complete row: {other:?}"),
        }
        wal.close().unwrap();
        assert!(
            !companion(&wal_path, "-wal"),
            "writer close left a wal file"
        );
        assert!(
            !companion(&wal_path, "-shm"),
            "writer close left an shm file"
        );

        let plain = Connection::open(&delete_path).unwrap();
        match read_journal_mode(&plain) {
            SqlAnswer::Value(mode) => assert_eq!(mode, "delete"),
            other => panic!("default journal_mode was not delete: {other:?}"),
        }
        match read_truncate_checkpoint(&plain) {
            SqlAnswer::Value(got) => assert_eq!(got, row(0, -1, -1)),
            other => panic!("non-WAL TRUNCATE did not return 0,-1,-1: {other:?}"),
        }
        plain.close().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    fn companion(db: &PathBuf, suffix: &str) -> bool {
        let mut name = db.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name).exists()
    }
}
