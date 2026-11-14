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
