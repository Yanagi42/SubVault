//! One offline package under the existing writer lock and `--out` path rules.
//! The package is records, current-fact CSV, and original bytes. It is not a
//! copy of the live vault directory, a backup, or a protection choice.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::facts;
use crate::fsutil::{self, sha256_file};
use crate::ids::{self, canonical_object_relpath};
use crate::model::{
    honesty, unseen_db, Fail, Report, EXIT_FAULT_MISSED, EXIT_INTEGRITY, EXIT_RECOVERY,
    EXIT_REJECTED,
};
use crate::store::{self, ObjectRow, OccurrenceRow, SourceRow};

use super::{validate_hook_names, Hooks, InjectAction, Place};

const PACKAGE_VERSION: &str = "offline-exit-1";
const EXPORT_INJECTS: &[&str] = &["export_write", "export_dir_fsync"];

pub struct ExportRequest {
    pub vault: PathBuf,
    pub out: PathBuf,
    pub allow_partial: bool,
    pub hooks: Hooks,
}

pub fn export_copy(req: ExportRequest) -> Report {
    match export_inner(req) {
        Ok((code, body)) => Report { code, body },
        Err(fail) => fail.into_report(),
    }
}

fn export_inner(req: ExportRequest) -> Result<(i32, Value), Fail> {
    validate_hook_names(
        req.hooks.stop_at.as_deref(),
        req.hooks.inject_point.as_deref(),
        &["before_dir_publish", "before_parent_sync"],
        EXPORT_INJECTS,
        EXPORT_INJECTS,
    )?;
    if let Err(fail) = super::guard_vault(&req.vault) {
        return Err(early(&req.hooks, fail));
    }
    let place = super::place_output(&req.vault, &req.out);
    if let Some(fail) = place_error(place) {
        return Err(early(&req.hooks, fail));
    }
    let loaded = match read_locked(&req.vault) {
        Ok(loaded) => loaded,
        Err(fail) => return Err(early(&req.hooks, fail)),
    };
    if let Some(fail) = loaded.blocking {
        return Err(early(&req.hooks, fail));
    }
    if !loaded.defects.is_empty() && !req.allow_partial {
        return Err(early(
            &req.hooks,
            recovery(defect_reason(&loaded.defects))
                .extra("missing", Value::Array(loaded.defects.clone())),
        ));
    }

    let parent = req
        .out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp = parent.join(format!(
        ".export-{}",
        ids::random_id("pkg").map_err(|err| early(&req.hooks, copy_fail(err, "export_temp")))?
    ));
    fs::create_dir(&temp).map_err(|err| early(&req.hooks, copy_fail(err, "export_temp")))?;
    let scratch = Scratch { temp: temp.clone() };
    if let Err(fail) = reach_inject(&req.hooks) {
        return Err(scratch.fail(fail));
    }

    let completeness = if loaded.defects.is_empty() {
        "complete"
    } else {
        "partial"
    };
    let records_bytes = serde_json::to_vec_pretty(&loaded.records)
        .map_err(|_| scratch.fail(recovery("export_encode")))?;
    let snapshot_id = sha256_bytes(&records_bytes);
    let mut files: Vec<Written> = Vec::new();
    files.push(Written {
        path: "records.json".into(),
        bytes: records_bytes,
        object_id: None,
    });
    if let Some(csv) = loaded.csv.as_ref() {
        files.push(Written {
            path: "facts.csv".into(),
            bytes: csv.clone(),
            object_id: None,
        });
    }
    for copy in &loaded.copies {
        let hashed = sha256_bytes(&copy.bytes);
        if hashed != copy.content_sha256 || copy.bytes.len() as i64 != copy.byte_length {
            return Err(scratch.fail(recovery("export_verify_failed")));
        }
        files.push(Written {
            path: copy.package_path.clone(),
            bytes: copy.bytes.clone(),
            object_id: Some(copy.object_id.clone()),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = manifest_value(
        &snapshot_id,
        completeness,
        loaded.vault_format,
        &files,
        &loaded.defects,
        &loaded.counts,
    );
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| scratch.fail(recovery("export_encode")))?;
    files.push(Written {
        path: "manifest.json".into(),
        bytes: manifest_bytes,
        object_id: None,
    });
    for file in &files {
        let path = scratch.temp.join(&file.path);
        write_new(&path, &file.bytes)
            .map_err(|err| scratch.fail(copy_fail(err, "export_write")))?;
    }
    if let Err(message) = verify_tree(&scratch.temp, &files) {
        return Err(
            scratch.fail(recovery("export_verify_failed").extra("os_error", json!(message)))
        );
    }
    fsutil::fsync_dir(&scratch.temp).map_err(|err| scratch.fail(copy_fail(err, "export_sync")))?;
    match fs::symlink_metadata(&req.out) {
        Ok(_) => return Err(scratch.fail(rejected("output_exists"))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(scratch.fail(copy_fail(err, "export_publish"))),
    }
    let mut stop_details = Map::new();
    stop_details.insert("published".into(), json!(false));
    super::stop_checkpoint(&req.hooks, "before_dir_publish", stop_details)
        .map_err(|fail| scratch.fail(fail))?;
    let published = match publish_export_dir(&scratch.temp, &req.out) {
        Ok(dir) => dir,
        Err(err) => return Err(scratch.fail(export_publish_failure(err))),
    };
    // Rename succeeded. scratch still names the old temporary path, so a
    // later scratch.fail must not represent this package. Dropping `published`
    // only closes the fd; it does not sync.
    let mut held = Map::new();
    held.insert("published".into(), json!(true));
    if let Ok((dev, ino)) = published.device_and_inode() {
        held.insert("parent_dev".into(), json!(dev));
        held.insert("parent_ino".into(), json!(ino));
    }
    if let Err(fail) = super::stop_checkpoint(&req.hooks, "before_parent_sync", held) {
        return Ok((
            EXIT_INTEGRITY,
            export_published_fault(&req, completeness, &snapshot_id, &loaded, &fail, false),
        ));
    }
    if let Err(err) = published.sync() {
        drop(published);
        return Ok((
            EXIT_INTEGRITY,
            export_published_fault(
                &req,
                completeness,
                &snapshot_id,
                &loaded,
                &sync_io(err, "export_dir_fsync"),
                false,
            ),
        ));
    }
    drop(published);
    match note_export_dir(&req.hooks) {
        Ok(false) => Ok((0, published_body(&req, completeness, &snapshot_id, &loaded))),
        Ok(true) => {
            let Some(action) = req.hooks.inject_action else {
                return Ok((
                    EXIT_INTEGRITY,
                    export_published_fault(
                        &req,
                        completeness,
                        &snapshot_id,
                        &loaded,
                        &Fail::new(EXIT_INTEGRITY, "Exported", "inject_action_required")
                            .extra("during", json!("export_dir_fsync"))
                            .extra("injected", json!(true)),
                        true,
                    ),
                ));
            };
            Ok((
                EXIT_INTEGRITY,
                export_published_fault(
                    &req,
                    completeness,
                    &snapshot_id,
                    &loaded,
                    &export_witness(action),
                    true,
                ),
            ))
        }
        Err(fail) => Ok((
            EXIT_INTEGRITY,
            export_published_fault(&req, completeness, &snapshot_id, &loaded, &fail, true),
        )),
    }
}

fn published_body(
    req: &ExportRequest,
    completeness: &str,
    snapshot_id: &str,
    loaded: &Loaded,
) -> Value {
    let mut body = honesty(if completeness == "complete" {
        "Exported"
    } else {
        "Partial"
    });
    body.insert("slice".into(), json!("T5-exit"));
    body.insert("completeness".into(), json!(completeness));
    body.insert("snapshot_id".into(), json!(snapshot_id));
    body.insert("vault_format".into(), json!(loaded.vault_format));
    body.insert("protection".into(), json!("undecided"));
    body.insert("allow_partial".into(), json!(req.allow_partial));
    body.insert("output".into(), json!(req.out.display().to_string()));
    body.insert("output_written".into(), json!(true));
    body.insert("new_effect".into(), json!(false));
    body.insert("read_only".into(), json!(true));
    body.insert("live_directory_copy".into(), json!(false));
    body.insert("missing".into(), Value::Array(loaded.defects.clone()));
    body.insert("counts".into(), loaded.counts.clone());
    Value::Object(body)
}

/// `parent_fsync_returned_ok` follows the sync call. `directory_durable=false`
/// means the final directory's durability is unconfirmed. `output` stays the
/// requested path already stored by `published_body`.
fn export_published_fault(
    req: &ExportRequest,
    completeness: &str,
    snapshot_id: &str,
    loaded: &Loaded,
    fail: &Fail,
    parent_fsync_returned_ok: bool,
) -> Value {
    let Value::Object(mut body) = published_body(req, completeness, snapshot_id, loaded) else {
        unreachable!("published export body is an object");
    };
    let during = fail
        .extra
        .get("during")
        .and_then(Value::as_str)
        .unwrap_or("export_dir_fsync")
        .to_string();
    let injected = fail
        .extra
        .get("injected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    body.insert("published".into(), json!(true));
    body.insert("directory_durable".into(), json!(false));
    body.insert(
        "parent_fsync_returned_ok".into(),
        json!(parent_fsync_returned_ok),
    );
    body.insert("reason".into(), json!(fail.reason.as_str()));
    body.insert("during".into(), json!(during));
    body.insert("injected".into(), json!(injected));
    for key in ["inject_point", "inject_action", "note", "os_error"] {
        if let Some(value) = fail.extra.get(key) {
            body.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(body)
}

fn export_witness(action: InjectAction) -> Fail {
    let (reason, name) = match action {
        InjectAction::Enospc => ("injected_enospc", "enospc"),
        InjectAction::IoError => ("injected_io_error", "io_error"),
    };
    Fail::new(EXIT_INTEGRITY, "Exported", reason)
        .extra("during", json!("export_dir_fsync"))
        .extra("injected", json!(true))
        .extra("inject_point", json!("export_dir_fsync"))
        .extra("inject_action", json!(name))
        .extra(
            "note",
            json!("injected fault, not a full device and not a failed disk"),
        )
}

fn sync_io(err: io::Error, during: &str) -> Fail {
    Fail::new(
        EXIT_INTEGRITY,
        "Exported",
        if crate::platform::is_enospc(&err) {
            "enospc"
        } else {
            "io_error"
        },
    )
    .extra("during", json!(during))
    .extra("os_error", json!(err.to_string()))
    .extra("injected", json!(false))
}

fn note_export_dir(hooks: &Hooks) -> Result<bool, Fail> {
    if hooks.inject_point.as_deref() != Some("export_dir_fsync") {
        return Ok(false);
    }
    let action = hooks.inject_action.ok_or_else(|| {
        Fail::new(EXIT_INTEGRITY, "Exported", "inject_action_required")
            .extra("during", json!("export_dir_fsync"))
            .extra("injected", json!(false))
    })?;
    let path = hooks.witness.clone().ok_or_else(|| {
        Fail::new(EXIT_INTEGRITY, "Exported", "witness_required")
            .extra("during", json!("export_dir_fsync"))
            .extra("injected", json!(false))
    })?;
    let name = match action {
        InjectAction::Enospc => "enospc",
        InjectAction::IoError => "io_error",
    };
    let body = json!({
        "reached": true,
        "point": "export_dir_fsync",
        "action": name,
        "published": true
    });
    let bytes = serde_json::to_vec_pretty(&body)
        .map_err(|_| Fail::new(EXIT_INTEGRITY, "Exported", "export_encode"))?;
    fsutil::atomic_write(&path, &bytes).map_err(|err| sync_io(err, "witness"))?;
    Ok(true)
}

const STAGING_CLEANUP_REASON: &str = "automatic path deletion is disabled because a path is not ownership; this is the original staging path, and its current location and current owner are unknown";

struct Scratch {
    temp: PathBuf,
}

impl Scratch {
    /// The temporary directory exists. Failure does not delete that path.
    /// `Drop` does not delete it either. A name is not ownership.
    fn fail(&self, fail: Fail) -> Fail {
        fail.extra("output_cleanup", json!("not_attempted"))
            .extra("output_cleanup_reason", json!(STAGING_CLEANUP_REASON))
            .extra(
                "output_staging_path",
                json!(self.temp.display().to_string()),
            )
            .extra("output_written", json!(false))
            .extra("completeness", json!("incomplete"))
    }
}

struct Written {
    path: String,
    bytes: Vec<u8>,
    object_id: Option<String>,
}

struct CopyItem {
    object_id: String,
    package_path: String,
    content_sha256: String,
    byte_length: i64,
    bytes: Vec<u8>,
}

fn read_locked(vault: &Path) -> Result<Loaded, Fail> {
    let _lock = crate::lockfile::VaultLock::try_acquire(&vault.join("writer.lock"))
        .map_err(|err| crate::model::unseen_io(err, "lock"))?
        .ok_or_else(|| {
            Fail::new(crate::model::EXIT_LOCKED, "Locked", "writer_busy")
                .extra("created_new_vault", json!(false))
        })?;
    let uri = super::wal_visible_uri(&vault.join("vault.db"))?;
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| read_err(err, "export_open"))?;
    super::accept_connection_format(&vault.join("vault.db"), &conn)?;
    load_snapshot(&conn, vault)
}

struct Loaded {
    vault_format: i64,
    records: Value,
    csv: Option<Vec<u8>>,
    copies: Vec<CopyItem>,
    defects: Vec<Value>,
    counts: Value,
    blocking: Option<Fail>,
}

fn load_snapshot(conn: &Connection, root: &Path) -> Result<Loaded, Fail> {
    let vault_format = store::user_version(conn).map_err(|err| read_err(err, "user_version"))?;
    let sources = store::list_sources(conn).map_err(|err| read_err(err, "list_sources"))?;
    let occurrences =
        store::list_occurrences(conn).map_err(|err| read_err(err, "list_occurrences"))?;
    let objects = store::list_objects(conn).map_err(|err| read_err(err, "list_objects"))?;
    let has_body = store::table_exists(conn, "source_versions")
        .map_err(|err| read_err(err, "source_versions"))?;
    let has_facts = store::table_exists(conn, "facts").map_err(|err| read_err(err, "facts"))?;
    if has_facts && !has_body {
        return Ok(blocked(vault_format, recovery("reference_broken")));
    }
    let versions = if has_body {
        Some(load_versions(conn)?)
    } else if store::table_exists(conn, "candidates").map_err(|err| read_err(err, "candidates"))? {
        return Ok(blocked(vault_format, recovery("reference_broken")));
    } else {
        None
    };
    let facts = if has_facts {
        Some(load_facts(conn)?)
    } else {
        None
    };
    let decisions = if has_facts {
        Some(load_decisions(conn)?)
    } else {
        None
    };
    let candidates = if has_body {
        Some(load_candidates(conn)?)
    } else {
        None
    };

    let mut object_ids = BTreeSet::new();
    for row in &objects {
        if !object_ids.insert(row.object_id.clone()) {
            return Ok(blocked(vault_format, recovery("reference_broken")));
        }
    }
    for source in &sources {
        let Some(object) = objects.iter().find(|row| row.object_id == source.object_id) else {
            return Ok(blocked(
                vault_format,
                recovery("reference_broken").extra("source_id", json!(source.source_id)),
            ));
        };
        if object.content_sha256 != source.content_sha256
            || object.byte_length != source.byte_length
        {
            return Ok(blocked(
                vault_format,
                recovery("reference_broken").extra("source_id", json!(source.source_id)),
            ));
        }
    }
    for occurrence in &occurrences {
        if !sources
            .iter()
            .any(|row| row.source_id == occurrence.source_id)
        {
            return Ok(blocked(
                vault_format,
                recovery("reference_broken")
                    .extra("occurrence_id", json!(occurrence.occurrence_id)),
            ));
        }
    }
    if let Some(versions) = &versions {
        for version in versions {
            let Some(object) = objects
                .iter()
                .find(|row| row.object_id == version.object_id)
            else {
                return Ok(blocked(
                    vault_format,
                    recovery("reference_broken")
                        .extra("source_version_id", json!(version.source_version_id)),
                ));
            };
            if object.content_sha256 != version.content_sha256
                || object.byte_length != version.byte_length
                || !sources.iter().any(|row| row.source_id == version.source_id)
            {
                return Ok(blocked(
                    vault_format,
                    recovery("reference_broken")
                        .extra("source_version_id", json!(version.source_version_id)),
                ));
            }
        }
    }
    if let Some(candidates) = &candidates {
        for candidate in candidates {
            let Some(version) = versions.as_ref().and_then(|rows| {
                rows.iter()
                    .find(|row| row.source_version_id == candidate.source_version_id)
            }) else {
                return Ok(blocked(
                    vault_format,
                    recovery("reference_broken")
                        .extra("candidate_id", json!(candidate.candidate_id)),
                ));
            };
            if version.source_id != candidate.source_id || version.object_id != candidate.object_id
            {
                return Ok(blocked(
                    vault_format,
                    recovery("reference_broken")
                        .extra("candidate_id", json!(candidate.candidate_id)),
                ));
            }
        }
    }
    let cited = facts.as_ref().map(|rows| {
        rows.iter()
            .map(|row| row.candidate_id.clone())
            .collect::<BTreeSet<_>>()
    });
    if let Some(facts) = &facts {
        let Some(candidates) = &candidates else {
            return Ok(blocked(vault_format, recovery("reference_broken")));
        };
        for fact in facts {
            let Some(candidate) = candidates
                .iter()
                .find(|row| row.candidate_id == fact.candidate_id)
            else {
                return Ok(blocked(
                    vault_format,
                    recovery("reference_broken").extra("fact_id", json!(fact.fact_id)),
                ));
            };
            if candidate.source_id != fact.source_id
                || candidate.source_version_id != fact.source_version_id
            {
                return Ok(blocked(
                    vault_format,
                    recovery("reference_broken").extra("fact_id", json!(fact.fact_id)),
                ));
            }
        }
    }
    if let Some(decisions) = &decisions {
        let Some(facts) = &facts else {
            return Ok(blocked(vault_format, recovery("reference_broken")));
        };
        let Some(candidates) = &candidates else {
            return Ok(blocked(vault_format, recovery("reference_broken")));
        };
        for decision in decisions {
            if !facts.iter().any(|row| row.fact_id == decision.fact_id)
                || !candidates
                    .iter()
                    .any(|row| row.candidate_id == decision.candidate_id)
            {
                return Ok(blocked(
                    vault_format,
                    recovery("reference_broken").extra("decision_id", json!(decision.decision_id)),
                ));
            }
        }
        if let Err(fail) = check_chains(facts, decisions) {
            return Ok(blocked(vault_format, fail));
        }
    }

    let mut defects = Vec::new();
    let mut copies = Vec::new();
    let mut object_json = Vec::new();
    let mut blocking = None;
    for row in &objects {
        match classify_object(root, row) {
            ObjectCopy::Block(fail) => {
                blocking = Some(fail);
                break;
            }
            ObjectCopy::Defect { reason } => {
                let source_ids: Vec<_> = sources
                    .iter()
                    .filter(|source| source.object_id == row.object_id)
                    .map(|source| json!(source.source_id))
                    .collect();
                defects.push(json!({
                    "object_id": row.object_id,
                    "reason": reason,
                    "content_sha256": row.content_sha256,
                    "byte_length": row.byte_length,
                    "source_ids": source_ids
                }));
                object_json.push(json!({
                    "object_id": row.object_id,
                    "content_sha256": row.content_sha256,
                    "byte_length": row.byte_length,
                    "stored_relpath": row.relpath,
                    "stored_relpath_differs": canonical_object_relpath(&row.object_id).as_deref() != Some(row.relpath.as_str()),
                    "package_path": Value::Null,
                    "copy_state": reason
                }));
            }
            ObjectCopy::Ready {
                relpath_differs,
                package_path,
                bytes,
            } => {
                copies.push(CopyItem {
                    object_id: row.object_id.clone(),
                    package_path: package_path.clone(),
                    content_sha256: row.content_sha256.clone(),
                    byte_length: row.byte_length,
                    bytes,
                });
                object_json.push(json!({
                    "object_id": row.object_id,
                    "content_sha256": row.content_sha256,
                    "byte_length": row.byte_length,
                    "stored_relpath": row.relpath,
                    "stored_relpath_differs": relpath_differs,
                    "package_path": package_path,
                    "copy_state": "copied"
                }));
            }
        }
    }
    defects.sort_by(|a, b| a["object_id"].as_str().cmp(&b["object_id"].as_str()));

    let candidate_json = candidates.as_ref().map(|rows| {
        rows.iter()
            .map(|row| {
                json!({
                    "candidate_id": row.candidate_id,
                    "run_id": row.run_id,
                    "attempt_id": row.attempt_id,
                    "source_version_id": row.source_version_id,
                    "source_id": row.source_id,
                    "object_id": row.object_id,
                    "ordinal": row.ordinal,
                    "kind": row.kind,
                    "usable": row.usable,
                    "excerpt": row.excerpt,
                    "value": row.value,
                    "locator": row.locator,
                    "adopted": row.adopted,
                    "cited_by_fact": cited.as_ref().is_some_and(|ids| ids.contains(&row.candidate_id))
                })
            })
            .collect::<Vec<_>>()
    });
    let fact_json = facts.as_ref().map(|rows| {
        rows.iter()
            .map(|row| {
                let decision_ids: Vec<_> = decisions
                    .as_ref()
                    .map(|items| {
                        items
                            .iter()
                            .filter(|item| item.fact_id == row.fact_id)
                            .map(|item| json!(item.decision_id))
                            .collect()
                    })
                    .unwrap_or_default();
                json!({
                    "fact_id": row.fact_id,
                    "source_id": row.source_id,
                    "source_version_id": row.source_version_id,
                    "run_id": row.run_id,
                    "candidate_id": row.candidate_id,
                    "fact_type": row.fact_type,
                    "revision": row.revision,
                    "value": row.value,
                    "decision_ids": decision_ids
                })
            })
            .collect::<Vec<_>>()
    });
    let decision_json = decisions.as_ref().map(|rows| {
        rows.iter()
            .map(|row| {
                json!({
                    "decision_id": row.decision_id,
                    "fact_id": row.fact_id,
                    "command_id": row.command_id,
                    "candidate_id": row.candidate_id,
                    "slot": row.slot,
                    "kind": row.kind,
                    "actor_kind": row.actor_kind,
                    "actor_id": row.actor_id,
                    "recorded_at": row.recorded_at,
                    "rule_version": row.rule_version,
                    "prior_value": row.prior,
                    "new_value": row.new_value,
                    "target_decision_id": row.target
                })
            })
            .collect::<Vec<_>>()
    });
    let csv = fact_json
        .as_ref()
        .map(|rows| facts_csv(rows, decisions.as_deref().unwrap_or(&[])));
    let not_cited = candidate_json.as_ref().map(|rows| {
        rows.iter()
            .filter(|row| row.get("cited_by_fact") == Some(&Value::Bool(false)))
            .count() as i64
    });
    let counts = json!({
        "sources": sources.len() as i64,
        "occurrences": occurrences.len() as i64,
        "objects": objects.len() as i64,
        "objects_copied": copies.len() as i64,
        "objects_not_copied": defects.len() as i64,
        "candidates": count_value(candidates.as_ref().map(|rows| rows.len())),
        "candidates_not_cited": count_value(not_cited.map(|n| n as usize)),
        "facts": count_value(facts.as_ref().map(|rows| rows.len())),
        "decisions": count_value(decisions.as_ref().map(|rows| rows.len()))
    });
    let records = json!({
        "package_version": PACKAGE_VERSION,
        "vault_format": vault_format,
        "protection": "undecided",
        "security_complete": false,
        "synthetic_only": true,
        "live_directory_copy": false,
        "field_notes": field_notes(vault_format),
        "sources": source_json(&sources),
        "occurrences": occurrence_json(&occurrences),
        "objects": object_json,
        "source_versions": array_or_null(versions.map(|rows| rows.into_iter().map(|row| row.json()).collect())),
        "candidates": array_or_null(candidate_json),
        "facts": array_or_null(fact_json),
        "decisions": array_or_null(decision_json)
    });
    Ok(Loaded {
        vault_format,
        records,
        csv,
        copies,
        defects,
        counts,
        blocking,
    })
}

fn blocked(vault_format: i64, blocking: Fail) -> Loaded {
    Loaded {
        vault_format,
        records: Value::Null,
        csv: None,
        copies: Vec::new(),
        defects: Vec::new(),
        counts: Value::Null,
        blocking: Some(blocking),
    }
}

enum ObjectCopy {
    Ready {
        relpath_differs: bool,
        package_path: String,
        bytes: Vec<u8>,
    },
    Defect {
        reason: &'static str,
    },
    Block(Fail),
}

fn classify_object(root: &Path, row: &ObjectRow) -> ObjectCopy {
    let facts = facts::object_facts(
        root,
        &row.object_id,
        Some(&row.relpath),
        &row.content_sha256,
        row.byte_length,
    );
    if facts.id_rejected {
        return ObjectCopy::Block(
            recovery("object_id_rejected").extra("object_id", json!(row.object_id)),
        );
    }
    if let Some(message) = facts.io_error {
        return ObjectCopy::Block(
            recovery("object_unreadable")
                .extra("object_id", json!(row.object_id))
                .extra("os_error", json!(message)),
        );
    }
    let package_path = format!("originals/{}", row.object_id);
    if facts.missing {
        return ObjectCopy::Defect {
            reason: "object_missing",
        };
    }
    if !facts.hash_matches {
        return ObjectCopy::Defect {
            reason: "hash_mismatch",
        };
    }
    if !facts.length_matches {
        return ObjectCopy::Defect {
            reason: "length_mismatch",
        };
    }
    let Some(relpath) = facts.canonical_relpath else {
        return ObjectCopy::Block(
            recovery("object_id_rejected").extra("object_id", json!(row.object_id)),
        );
    };
    let bytes = match fs::read(root.join(relpath)) {
        Ok(bytes) => bytes,
        Err(err) => {
            return ObjectCopy::Block(
                recovery("object_unreadable")
                    .extra("object_id", json!(row.object_id))
                    .extra("os_error", json!(err.to_string())),
            );
        }
    };
    if sha256_bytes(&bytes) != row.content_sha256 || bytes.len() as i64 != row.byte_length {
        return ObjectCopy::Block(
            recovery("export_verify_failed").extra("object_id", json!(row.object_id)),
        );
    }
    ObjectCopy::Ready {
        relpath_differs: facts.relpath_differs,
        package_path,
        bytes,
    }
}

struct VersionRow {
    source_version_id: String,
    source_id: String,
    object_id: String,
    content_sha256: String,
    byte_length: i64,
    media_type: String,
    role: String,
    eml: i64,
}

impl VersionRow {
    fn json(self) -> Value {
        json!({
            "source_version_id": self.source_version_id,
            "source_id": self.source_id,
            "object_id": self.object_id,
            "content_sha256": self.content_sha256,
            "byte_length": self.byte_length,
            "media_type": self.media_type,
            "role": self.role,
            "eml": false
        })
    }
}

struct CandidateRow {
    candidate_id: String,
    run_id: String,
    attempt_id: String,
    source_version_id: String,
    source_id: String,
    object_id: String,
    ordinal: i64,
    kind: String,
    usable: bool,
    excerpt: String,
    value: Value,
    locator: Value,
    adopted: bool,
}

struct FactRow {
    fact_id: String,
    source_id: String,
    source_version_id: String,
    run_id: String,
    candidate_id: String,
    fact_type: String,
    revision: i64,
    value: Value,
}

struct DecisionRow {
    decision_id: String,
    fact_id: String,
    command_id: String,
    candidate_id: String,
    slot: String,
    kind: String,
    actor_kind: String,
    actor_id: String,
    recorded_at: i64,
    rule_version: Option<String>,
    prior: Value,
    new_value: Value,
    target: Option<String>,
}

fn load_versions(conn: &Connection) -> Result<Vec<VersionRow>, Fail> {
    let mut stmt = conn
        .prepare(
            "SELECT source_version_id, source_id, object_id, content_sha256, byte_length, media_type, role, eml FROM source_versions ORDER BY source_version_id",
        )
        .map_err(|err| read_err(err, "source_versions"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(VersionRow {
                source_version_id: row.get(0)?,
                source_id: row.get(1)?,
                object_id: row.get(2)?,
                content_sha256: row.get(3)?,
                byte_length: row.get(4)?,
                media_type: row.get(5)?,
                role: row.get(6)?,
                eml: row.get(7)?,
            })
        })
        .map_err(|err| read_err(err, "source_versions"))?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|err| read_err(err, "source_versions"))?;
        if row.eml != 0 {
            return Err(recovery("reference_broken")
                .extra("source_version_id", json!(row.source_version_id)));
        }
        out.push(row);
    }
    Ok(out)
}

fn load_candidates(conn: &Connection) -> Result<Vec<CandidateRow>, Fail> {
    let joined: i64 = conn
        .query_row(
            "SELECT count(*) FROM candidates c JOIN source_versions sv ON sv.source_version_id = c.source_version_id",
            [],
            |row| row.get(0),
        )
        .map_err(|err| read_err(err, "candidates"))?;
    let total: i64 = conn
        .query_row("SELECT count(*) FROM candidates", [], |row| row.get(0))
        .map_err(|err| read_err(err, "candidates"))?;
    if joined != total {
        return Err(recovery("reference_broken"));
    }
    let mut stmt = conn
        .prepare(
            "SELECT c.candidate_id, c.run_id, c.attempt_id, c.source_version_id, c.ordinal, c.kind, c.usable, c.excerpt, c.value_json, c.locator_json, c.adopted, sv.source_id, sv.object_id FROM candidates c JOIN source_versions sv ON sv.source_version_id = c.source_version_id ORDER BY sv.source_id, c.ordinal, c.candidate_id",
        )
        .map_err(|err| read_err(err, "candidates"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
            ))
        })
        .map_err(|err| read_err(err, "candidates"))?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|err| read_err(err, "candidates"))?;
        if row.10 != 0 || (row.6 != 0 && row.6 != 1) {
            return Err(recovery("reference_broken").extra("candidate_id", json!(row.0)));
        }
        out.push(CandidateRow {
            candidate_id: row.0.clone(),
            run_id: row.1,
            attempt_id: row.2,
            source_version_id: row.3,
            ordinal: row.4,
            kind: row.5,
            usable: row.6 == 1,
            excerpt: row.7,
            value: json_object(&row.8, "candidate_value_unreadable")
                .map_err(|fail| fail.extra("candidate_id", json!(row.0)))?,
            locator: json_object(&row.9, "locator_unreadable")
                .map_err(|fail| fail.extra("candidate_id", json!(row.0)))?,
            adopted: false,
            source_id: row.11,
            object_id: row.12,
        });
    }
    Ok(out)
}

fn load_facts(conn: &Connection) -> Result<Vec<FactRow>, Fail> {
    let mut stmt = conn
        .prepare(
            "SELECT fact_id, source_id, source_version_id, run_id, candidate_id, fact_type, value_json, revision FROM facts ORDER BY source_id, fact_id",
        )
        .map_err(|err| read_err(err, "facts"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(|err| read_err(err, "facts"))?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|err| read_err(err, "facts"))?;
        out.push(FactRow {
            fact_id: row.0.clone(),
            source_id: row.1,
            source_version_id: row.2,
            run_id: row.3,
            candidate_id: row.4,
            fact_type: row.5,
            value: json_object(&row.6, "fact_value_unreadable")
                .map_err(|fail| fail.extra("fact_id", json!(row.0)))?,
            revision: row.7,
        });
    }
    Ok(out)
}

fn load_decisions(conn: &Connection) -> Result<Vec<DecisionRow>, Fail> {
    let mut stmt = conn
        .prepare(
            "SELECT decision_id, fact_id, command_id, candidate_id, slot, kind, actor_kind, actor_id, recorded_at, rule_version, prior_value_json, new_value_json, target_decision_id FROM decisions ORDER BY fact_id, rowid",
        )
        .map_err(|err| read_err(err, "decisions"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, Option<String>>(12)?,
            ))
        })
        .map_err(|err| read_err(err, "decisions"))?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|err| read_err(err, "decisions"))?;
        out.push(DecisionRow {
            decision_id: row.0.clone(),
            fact_id: row.1,
            command_id: row.2,
            candidate_id: row.3,
            slot: row.4,
            kind: row.5,
            actor_kind: row.6,
            actor_id: row.7,
            recorded_at: row.8,
            rule_version: row.9,
            prior: json_optional(&row.10, "prior_value_unreadable")
                .map_err(|fail| fail.extra("decision_id", json!(row.0)))?,
            new_value: json_object(&row.11, "new_value_unreadable")
                .map_err(|fail| fail.extra("decision_id", json!(row.0)))?,
            target: row.12,
        });
    }
    Ok(out)
}

fn check_chains(facts: &[FactRow], decisions: &[DecisionRow]) -> Result<(), Fail> {
    for fact in facts {
        let chain: Vec<&DecisionRow> = decisions
            .iter()
            .filter(|row| row.fact_id == fact.fact_id)
            .collect();
        if chain.is_empty() || chain.len() as i64 != fact.revision {
            return Err(chain_fail(&fact.fact_id));
        }
        if chain[0].kind != "rule_adopt" || !chain[0].prior.is_null() || chain[0].target.is_some() {
            return Err(chain_fail(&fact.fact_id));
        }
        if chain.last().unwrap().new_value != fact.value {
            return Err(chain_fail(&fact.fact_id));
        }
        for (index, decision) in chain.iter().enumerate() {
            if index > 0 && decision.prior != chain[index - 1].new_value {
                return Err(chain_fail(&fact.fact_id));
            }
            match decision.kind.as_str() {
                "rule_adopt" => {
                    if index != 0 {
                        return Err(chain_fail(&fact.fact_id));
                    }
                }
                "human_correct" => {
                    if decision.target.is_some() {
                        return Err(chain_fail(&fact.fact_id));
                    }
                }
                "undo" => {
                    let Some(target_id) = &decision.target else {
                        return Err(chain_fail(&fact.fact_id));
                    };
                    let Some(pos) = chain.iter().position(|item| &item.decision_id == target_id)
                    else {
                        return Err(chain_fail(&fact.fact_id));
                    };
                    if pos >= index {
                        return Err(chain_fail(&fact.fact_id));
                    }
                    let expected = if chain[pos].prior.is_null() {
                        unknown_value(&fact.fact_type)
                    } else {
                        chain[pos].prior.clone()
                    };
                    if decision.new_value != expected {
                        return Err(chain_fail(&fact.fact_id));
                    }
                }
                _ => return Err(chain_fail(&fact.fact_id)),
            }
        }
    }
    Ok(())
}

fn unknown_value(fact_type: &str) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("type".into(), json!(fact_type));
    value.insert("statement".into(), json!("unknown"));
    value.insert("payment_verified".into(), json!(false));
    value.insert("purchase_id".into(), Value::Null);
    value.insert("account_id".into(), Value::Null);
    value.insert("observed_at".into(), Value::Null);
    value.insert("time_status".into(), json!("unknown"));
    value.insert("current_status".into(), json!("unknown"));
    if fact_type == "stated_amount_paid" {
        value.insert("currency".into(), json!("USD"));
        value.insert("amount_text".into(), Value::Null);
    } else {
        value.insert("period_start".into(), Value::Null);
        value.insert("period_end".into(), Value::Null);
    }
    Value::Object(value)
}

fn facts_csv(facts: &[Value], decisions: &[DecisionRow]) -> Vec<u8> {
    let mut text = String::from(
        "fact_id,source_id,candidate_id,fact_type,revision,statement,currency,amount_text,period_start,period_end,latest_decision_id\n",
    );
    for fact in facts {
        let fact_id = fact["fact_id"].as_str().unwrap_or("");
        let value = &fact["value"];
        let latest = decisions
            .iter()
            .filter(|row| row.fact_id == fact_id)
            .last()
            .map(|row| row.decision_id.as_str())
            .unwrap_or("");
        let cells = [
            fact_id.to_string(),
            fact["source_id"].as_str().unwrap_or("").to_string(),
            fact["candidate_id"].as_str().unwrap_or("").to_string(),
            fact["fact_type"].as_str().unwrap_or("").to_string(),
            fact["revision"].as_i64().unwrap_or(0).to_string(),
            cell(value, "statement"),
            cell(value, "currency"),
            cell(value, "amount_text"),
            cell(value, "period_start"),
            cell(value, "period_end"),
            latest.to_string(),
        ];
        let rendered = cells
            .into_iter()
            .map(|item| csv_cell(&item))
            .collect::<Vec<_>>()
            .join(",");
        text.push_str(&rendered);
        text.push('\n');
    }
    text.into_bytes()
}

fn cell(value: &Value, key: &str) -> String {
    match value.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn csv_cell(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn source_json(rows: &[SourceRow]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            json!({
                "source_id": row.source_id,
                "object_id": row.object_id,
                "content_sha256": row.content_sha256,
                "byte_length": row.byte_length,
                "media_type": row.media_type,
                "user_marker": row.user_marker
            })
        })
        .collect()
}

fn occurrence_json(rows: &[OccurrenceRow]) -> Vec<Value> {
    rows.iter()
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
        .collect()
}

fn field_notes(vault_format: i64) -> Value {
    let mut notes = json!({
        "stated_amount_paid": "来源载明的已付金额，不是已核到账。",
        "stated_service_period": "来源载明的服务期。没有观察日，当前状态保持未知。",
        "candidate": "候选不是事实。没有被事实引用的候选仍留在包内。",
        "protection": "保护方式未定。这是合成库的明文副本，不是获准的真实资料出口。",
        "paths": "包内原件只用 originals/ 下的相对路径。出现记录里的外部路径只是当时的取得记录。"
    });
    if vault_format == 4 {
        notes["html_original"] = json!(
            "包内 HTML 只作为文本保存和展示。程序不执行它，不连接网络，也不自动加载其中的外部引用。"
        );
    }
    notes
}

fn manifest_value(
    snapshot_id: &str,
    completeness: &str,
    vault_format: i64,
    files: &[Written],
    missing: &[Value],
    counts: &Value,
) -> Value {
    let listed: Vec<_> = files
        .iter()
        .filter(|file| file.path != "manifest.json")
        .map(|file| {
            let mut entry = json!({
                "path": file.path,
                "sha256": sha256_bytes(&file.bytes),
                "byte_length": file.bytes.len()
            });
            if let Some(object_id) = &file.object_id {
                entry["object_id"] = json!(object_id);
            }
            entry
        })
        .collect();
    json!({
        "package_version": PACKAGE_VERSION,
        "snapshot_id": snapshot_id,
        "completeness": completeness,
        "vault_format": vault_format,
        "protection": "undecided",
        "security_complete": false,
        "synthetic_only": true,
        "live_directory_copy": false,
        "files": listed,
        "missing": missing,
        "counts": counts
    })
}

fn verify_tree(root: &Path, files: &[Written]) -> Result<(), String> {
    let found = list_files(root).map_err(|err| err.to_string())?;
    let mut expected: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    expected.sort();
    if found != expected {
        return Err(format!("package file list {found:?} != {expected:?}"));
    }
    for file in files {
        let path = root.join(&file.path);
        let disk = fs::read(&path).map_err(|err| err.to_string())?;
        if disk != file.bytes {
            return Err(format!("{} bytes changed before publish", file.path));
        }
        let (hash, len) = sha256_file(&path).map_err(|err| err.to_string())?;
        if hash != sha256_bytes(&file.bytes) || len != file.bytes.len() as u64 {
            return Err(format!("{path} hash mismatch", path = file.path));
        }
    }
    Ok(())
}

fn list_files(root: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "symlink in package",
            ));
        }
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;
            let text = rel.to_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "package path is not utf-8")
            })?;
            out.push(text.to_string());
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "special file in package",
            ));
        }
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = crate::platform::open_new_nofollow(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn reach_inject(hooks: &Hooks) -> Result<(), Fail> {
    if hooks.inject_point.as_deref() != Some("export_write") {
        return Ok(());
    }
    let action = hooks
        .inject_action
        .ok_or_else(|| Fail::new(crate::model::EXIT_USAGE, "Usage", "inject_action_required"))?;
    let path = hooks
        .witness
        .clone()
        .ok_or_else(|| Fail::new(crate::model::EXIT_USAGE, "Usage", "witness_required"))?;
    let name = match action {
        InjectAction::Enospc => "enospc",
        InjectAction::IoError => "io_error",
    };
    let body = json!({"reached": true, "point": "export_write", "action": name});
    let bytes = serde_json::to_vec_pretty(&body).map_err(|_| recovery("export_encode"))?;
    fsutil::atomic_write(&path, &bytes).map_err(|err| copy_fail(err, "witness"))?;
    let reason = match action {
        InjectAction::Enospc => "injected_enospc",
        InjectAction::IoError => "injected_io_error",
    };
    Err(unpublished(EXIT_INTEGRITY, "Incomplete", reason)
        .extra("injected", json!(true))
        .extra("during", json!("export_write")))
}

fn place_error(place: Place) -> Option<Fail> {
    match place {
        Place::Create => None,
        Place::Inside => Some(rejected("output_inside_vault")),
        Place::Unresolved(message) => {
            Some(rejected("output_unresolved").extra("os_error", json!(message)))
        }
        Place::MissingParent => Some(rejected("output_parent_missing")),
        Place::Exists => Some(rejected("output_exists")),
    }
}

fn early(hooks: &Hooks, fail: Fail) -> Fail {
    if hooks.inject_point.is_none() {
        return fail;
    }
    Fail::new(
        EXIT_FAULT_MISSED,
        "FaultNotReached",
        hooks.inject_point.clone().unwrap_or_default(),
    )
    .extra("injected", json!(true))
    .extra("new_effect", json!(false))
    .extra("slice", json!("T5-exit"))
    .extra("completeness", json!("incomplete"))
    .extra("output_written", json!(false))
    .extra("output_cleanup", json!("not_created"))
    .extra("protection", json!("undecided"))
}

fn unpublished(code: i32, outcome: &'static str, reason: &'static str) -> Fail {
    Fail::new(code, outcome, reason)
        .extra("slice", json!("T5-exit"))
        .extra("completeness", json!("incomplete"))
        .extra("output_written", json!(false))
        .extra("output_cleanup", json!("not_created"))
        .extra("new_effect", json!(false))
        .extra("read_only", json!(true))
        .extra("protection", json!("undecided"))
        .extra("injected", json!(false))
}

fn recovery(reason: &'static str) -> Fail {
    unpublished(EXIT_RECOVERY, "RecoveryRequired", reason)
}

fn defect_reason(defects: &[Value]) -> &'static str {
    if defects.len() != 1 {
        return "export_incomplete";
    }
    match defects[0].get("reason").and_then(Value::as_str) {
        Some("object_missing") => "object_missing",
        Some("hash_mismatch") => "hash_mismatch",
        Some("length_mismatch") => "length_mismatch",
        _ => "export_incomplete",
    }
}

fn rejected(reason: &'static str) -> Fail {
    unpublished(EXIT_REJECTED, "Rejected", reason)
}

fn chain_fail(fact_id: &str) -> Fail {
    recovery("decision_chain_inconsistent").extra("fact_id", json!(fact_id))
}

fn publish_export_dir(temp: &Path, out: &Path) -> io::Result<crate::platform::PublishedDir> {
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_file = File::open(parent)?;
    let from = temp.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "export staging has no file name",
        )
    })?;
    let to = out.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "export target has no file name",
        )
    })?;
    crate::platform::rename_dir_noreplace(&parent_file, from, to)?;
    Ok(crate::platform::PublishedDir::hold(parent_file))
}

pub(super) fn export_publish_failure(err: io::Error) -> Fail {
    match crate::platform::classify_rename_dir(&err) {
        crate::platform::RenameDirClass::Exists => rejected("output_exists")
            .extra("during", json!("export_publish"))
            .extra("os_error", json!(err.to_string())),
        crate::platform::RenameDirClass::Unavailable => unpublished(
            crate::model::EXIT_UNSUPPORTED_FS,
            "Unsupported",
            "rename_noreplace_unavailable",
        )
        .extra("during", json!("export_publish"))
        .extra("os_error", json!(err.to_string())),
        crate::platform::RenameDirClass::NoSpace
        | crate::platform::RenameDirClass::Permission
        | crate::platform::RenameDirClass::Busy
        | crate::platform::RenameDirClass::Invalid
        | crate::platform::RenameDirClass::CrossDevice
        | crate::platform::RenameDirClass::Other => copy_fail(err, "export_publish"),
    }
}

fn copy_fail(err: io::Error, during: &str) -> Fail {
    let enospc = crate::platform::is_enospc(&err);
    unpublished(
        EXIT_INTEGRITY,
        "Incomplete",
        if enospc { "enospc" } else { "io_error" },
    )
    .extra("during", json!(during))
    .extra("os_error", json!(err.to_string()))
}

fn read_err(err: rusqlite::Error, during: &str) -> Fail {
    unseen_db(err, during)
        .extra("slice", json!("T5-exit"))
        .extra("completeness", json!("incomplete"))
        .extra("output_written", json!(false))
        .extra("output_cleanup", json!("not_created"))
        .extra("read_only", json!(true))
        .extra("protection", json!("undecided"))
}

fn json_object(text: &str, reason: &'static str) -> Result<Value, Fail> {
    let value: Value = serde_json::from_str(text).map_err(|_| recovery(reason))?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(recovery(reason))
    }
}

fn json_optional(text: &Option<String>, reason: &'static str) -> Result<Value, Fail> {
    match text {
        None => Ok(Value::Null),
        Some(text) => json_object(text, reason),
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    ids::hex_encode(&hasher.finalize())
}

fn array_or_null(rows: Option<Vec<Value>>) -> Value {
    match rows {
        None => Value::Null,
        Some(rows) => Value::Array(rows),
    }
}

fn count_value(count: Option<usize>) -> Value {
    match count {
        None => Value::Null,
        Some(count) => json!(count as i64),
    }
}

#[cfg(test)]
mod published_sync_static {
    use super::*;

    fn loaded() -> Loaded {
        Loaded {
            vault_format: 1,
            records: json!({}),
            csv: None,
            copies: Vec::new(),
            defects: Vec::new(),
            counts: json!({}),
            blocking: None,
        }
    }

    fn req() -> ExportRequest {
        ExportRequest {
            vault: PathBuf::from("/vault"),
            out: PathBuf::from("/requested/pkg"),
            allow_partial: false,
            hooks: Hooks {
                stop_at: None,
                checkpoint: None,
                inject_action: None,
                inject_point: None,
                witness: None,
            },
        }
    }

    /// Constructed `io::Error`. `sync_all` is not called. Not an observed
    /// fsync failure, and not EACCES from opening a path.
    #[test]
    fn constructed_sync_error_stays_published_with_the_actual_reason() {
        let fail = sync_io(
            io::Error::from_raw_os_error(libc::ENOSPC),
            "export_dir_fsync",
        );
        let body = export_published_fault(&req(), "complete", "snap", &loaded(), &fail, false);
        assert_eq!(body["outcome"], "Exported");
        assert_eq!(body["reason"], "enospc");
        assert_ne!(body["reason"], "directory_durability_unknown");
        assert_eq!(body["during"], "export_dir_fsync");
        assert_eq!(body["injected"], false);
        assert_eq!(body["published"], true);
        assert_eq!(body["directory_durable"], false);
        assert_eq!(body["parent_fsync_returned_ok"], false);
        assert_eq!(body["output_written"], true);
        assert_eq!(body["output"], "/requested/pkg");
        assert_eq!(body["completeness"], "complete");
        assert!(body.get("output_cleanup").is_none(), "{body}");
        assert!(body.get("output_staging_path").is_none(), "{body}");
    }

    #[test]
    fn witness_after_sync_ok_does_not_rewrite_that_ok_as_a_sync_failure() {
        let fail = export_witness(InjectAction::IoError);
        let body = export_published_fault(&req(), "complete", "snap", &loaded(), &fail, true);
        assert_eq!(body["reason"], "injected_io_error");
        assert_ne!(body["reason"], "directory_durability_unknown");
        assert_eq!(body["parent_fsync_returned_ok"], true);
        assert_eq!(body["directory_durable"], false);
        assert_eq!(body["published"], true);
        assert_eq!(body["output"], "/requested/pkg");
        assert!(body["note"].as_str().unwrap().contains("not a failed disk"));
        assert!(body.get("output_cleanup").is_none(), "{body}");
    }
}
