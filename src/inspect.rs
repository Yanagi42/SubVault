use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::facts;
use crate::fsutil::sha256_file;
use crate::ids::is_prefixed_id;
use crate::journal;
use crate::model::{
    capabilities, honesty, platform_limits, unseen_db, Fail, Report, EXIT_INTEGRITY, EXIT_OK,
    EXIT_RECOVERY, NOT_ARRANGED, NOT_PRESERVED, NOT_SEARCHABLE, PRESERVED,
};
use crate::store;

pub(crate) fn report(root: &Path, conn: &Connection, magic: i64) -> Result<Report, Fail> {
    let sources = store::list_sources(conn).map_err(|err| unseen_db(err, "list_sources"))?;
    let objects = store::list_objects(conn).map_err(|err| unseen_db(err, "list_objects"))?;
    let occurrences =
        store::list_occurrences(conn).map_err(|err| unseen_db(err, "list_occurrences"))?;
    let commands = store::list_commands(conn).map_err(|err| unseen_db(err, "list_commands"))?;
    let journals = journal::scan(root);
    let mut alerts = Vec::new();
    if let Some(message) = &journals.unlisted {
        alerts.push(json!({
            "code": "journal_unlisted",
            "during": "read_journal",
            "os_error": message,
        }));
    }
    let mut dangling = Vec::new();
    for name in &journals.unexpected {
        alerts.push(json!({"code": "unexpected_journal_name", "name": name}));
    }
    for name in &journals.invalid {
        alerts.push(json!({"code": "journal_invalid", "name": name}));
    }
    let db_object_ids: std::collections::BTreeSet<&str> = objects
        .iter()
        .map(|object| object.object_id.as_str())
        .collect();
    let mut unreferenced = Vec::new();
    let object_dir = root.join("objects");
    if object_dir.is_dir() {
        // A listing or orphan-hash failure is about files this call could not
        // read. It does not decide whether committed sources were preserved.
        match fs::read_dir(&object_dir) {
            Err(err) => {
                alerts.push(json!({
                    "code": "objects_unlisted",
                    "during": "read_objects",
                    "os_error": err.to_string(),
                }));
            }
            Ok(entries) => {
                let mut names = Vec::new();
                let mut list_error = None;
                for entry in entries {
                    match entry {
                        Ok(entry) => names.push(entry.file_name().to_string_lossy().to_string()),
                        Err(err) => {
                            list_error = Some(err.to_string());
                            break;
                        }
                    }
                }
                if let Some(message) = list_error {
                    alerts.push(json!({
                        "code": "objects_unlisted",
                        "during": "read_objects",
                        "os_error": message,
                    }));
                } else {
                    names.sort();
                    for name in names {
                        if !is_prefixed_id(&name, "obj_") {
                            alerts.push(json!({"code": "unexpected_object_name", "name": name}));
                            continue;
                        }
                        if !db_object_ids.contains(name.as_str()) {
                            let path = object_dir.join(&name);
                            match sha256_file(&path) {
                                Ok((hash, len)) => {
                                    unreferenced.push(json!({
                                        "object_id": name,
                                        "content_sha256": hash,
                                        "hash_alg": "sha256",
                                        "byte_length": len,
                                        "object_relpath": format!("objects/{name}")
                                    }));
                                }
                                Err(err) => {
                                    alerts.push(json!({
                                        "code": "orphan_unreadable",
                                        "name": name,
                                        "during": "hash_orphan",
                                        "os_error": err.to_string(),
                                    }));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut source_json = Vec::new();
    for source in &sources {
        let stored = objects
            .iter()
            .find(|object| object.object_id == source.object_id);
        let facts = facts::object_facts(
            root,
            &source.object_id,
            stored.map(|object| object.relpath.as_str()),
            &source.content_sha256,
            source.byte_length,
        );
        let relpath = facts.canonical_relpath.clone().unwrap_or_default();
        // preservation stays the stored verdict only when this call hashed the
        // file. A read error leaves both fields null: the row is still the
        // historical commit, and this open did not prove the bytes.
        let preservation;
        let intact;
        if let Some(message) = &facts.io_error {
            if facts.relpath_differs {
                alerts.push(json!({"code": "object_path_rejected", "source_id": source.source_id, "object_id": source.object_id}));
            }
            alerts.push(json!({
                "code": "object_unreadable",
                "source_id": source.source_id,
                "object_id": source.object_id,
                "during": "hash_object",
                "os_error": message,
            }));
            preservation = Value::Null;
            intact = Value::Null;
        } else {
            let mut intact_bool = false;
            if facts.id_rejected {
                alerts.push(json!({"code": "object_id_rejected", "source_id": source.source_id, "object_id": source.object_id}));
                dangling.push(json!({"source_id": source.source_id, "object_id": source.object_id, "reason": "object_id_rejected"}));
            } else if facts.relpath_differs {
                alerts.push(json!({"code": "object_path_rejected", "source_id": source.source_id, "object_id": source.object_id}));
            }
            if !facts.id_rejected && facts.missing {
                alerts.push(json!({"code": "object_missing", "source_id": source.source_id, "object_id": source.object_id}));
                dangling.push(json!({"source_id": source.source_id, "object_id": source.object_id, "reason": "object_missing"}));
            } else if !facts.id_rejected && (!facts.hash_matches || !facts.length_matches) {
                alerts.push(json!({
                    "code": "hash_mismatch",
                    "source_id": source.source_id,
                    "object_id": source.object_id,
                    "expected_sha256": source.content_sha256,
                    "actual_sha256": facts.actual_hash
                }));
                dangling.push(json!({"source_id": source.source_id, "object_id": source.object_id, "reason": "hash_mismatch"}));
            } else if !facts.id_rejected {
                intact_bool = true;
            }
            preservation = json!(PRESERVED);
            intact = json!(intact_bool);
        }
        source_json.push(json!({
            "source_id": source.source_id,
            "object_id": source.object_id,
            "content_sha256": source.content_sha256,
            "hash_alg": "sha256",
            "byte_length": source.byte_length,
            "media_type": source.media_type,
            "user_marker": source.user_marker,
            "preservation": preservation,
            "arrangement": NOT_ARRANGED,
            "searchability": NOT_SEARCHABLE,
            "purchase_id": null,
            "entitlement_id": null,
            "object_relpath": relpath,
            "object_intact": intact
        }));
    }
    let command_ids: std::collections::BTreeSet<&str> =
        commands.iter().map(|row| row.command_id.as_str()).collect();
    let mut incomplete = Vec::new();
    if journals.unlisted.is_none() {
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
                "preservation": NOT_PRESERVED,
                "source_id": null,
                "purchase_id": null
            }));
        }
    }
    let invalid_journal = journals.unlisted.is_none()
        && (!journals.invalid.is_empty() || !journals.unexpected.is_empty());
    let (code, outcome) = if !dangling.is_empty() || invalid_journal {
        (EXIT_RECOVERY, "RecoveryRequired")
    } else if !alerts.is_empty() {
        (EXIT_INTEGRITY, "IntegrityAlert")
    } else {
        (EXIT_OK, "Ready")
    };
    let mut code = code;
    let mut outcome = outcome;
    let vault_id = match store::meta(conn, "vault_id") {
        Ok(value) => value.unwrap_or_default(),
        Err(err) => {
            alerts.push(json!({
                "code": "meta_unreadable",
                "during": "meta",
                "os_error": err.to_string(),
            }));
            if code == EXIT_OK {
                code = EXIT_INTEGRITY;
                outcome = "IntegrityAlert";
            }
            String::new()
        }
    };
    let vault_id_json = if vault_id.is_empty()
        && alerts
            .iter()
            .any(|alert| alert["code"] == "meta_unreadable")
    {
        Value::Null
    } else {
        json!(vault_id)
    };
    let mut body = honesty(outcome);
    body.insert("vault_id".into(), vault_id_json);
    body.insert("disposable".into(), json!(true));
    body.insert("protection".into(), json!("unanswered_synthetic_plaintext"));
    body.insert("primary_input".into(), json!("unanswered"));
    body.insert("schema_version".into(), json!(1));
    body.insert("capabilities".into(), capabilities());
    body.insert("platform_limits".into(), platform_limits(Some(magic)));
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
    let read_unknown = alerts.iter().any(|alert| {
        matches!(
            alert.get("code").and_then(|value| value.as_str()),
            Some(
                "object_unreadable"
                    | "objects_unlisted"
                    | "orphan_unreadable"
                    | "journal_unlisted"
                    | "meta_unreadable",
            )
        )
    });
    body.insert("alerts".into(), Value::Array(alerts));
    if read_unknown {
        body.insert("new_effect".into(), json!(false));
    }
    body.insert(
        "temporary_file_count".into(),
        match journal::count_staging(root) {
            Some(count) => json!(count),
            None => Value::Null,
        },
    );
    body.insert("created_new_vault".into(), json!(false));
    Ok(Report {
        code,
        body: Value::Object(body),
    })
}
