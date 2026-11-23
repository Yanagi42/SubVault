//! Standalone read-only verifier for a SubVault `offline-exit-1` package.
//!
//! Verifies `manifest.json`, `records.json` (`snapshot_id`), every listed file's
//! byte length and SHA-256 hash, tree completeness (no symlinks, no extra or
//! missing files), and `records.json` <-> `facts.csv` consistency — without
//! requiring access to the live vault.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::fsutil::MAX_OBJECT_BYTES;
use crate::ids::hex_encode;
use crate::model::{honesty, Report, EXIT_OK, EXIT_RECOVERY, EXIT_REJECTED};

const PACKAGE_VERSION: &str = "offline-exit-1";
const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;

pub fn verify_package(package: &Path) -> Report {
    let meta = match fs::symlink_metadata(package) {
        Ok(meta) => meta,
        Err(err) => {
            return invalid_reject("package_missing", Some(err.to_string()));
        }
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return invalid_reject("package_not_directory", None);
    }

    let manifest_path = package.join("manifest.json");
    let manifest_bytes = match read_regular_file(&manifest_path, MAX_METADATA_BYTES) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return invalid(
                "manifest_unreadable",
                vec![json!({ "path": "manifest.json", "reason": reason })],
            );
        }
    };
    let manifest: Value = match serde_json::from_slice::<Value>(&manifest_bytes) {
        Ok(val) if val.is_object() => val,
        _ => {
            return invalid(
                "manifest_invalid_json",
                vec![json!({ "path": "manifest.json", "reason": "invalid_json" })],
            );
        }
    };

    let Some(version) = manifest.get("package_version").and_then(Value::as_str) else {
        return invalid(
            "manifest_missing_version",
            vec![json!({ "path": "manifest.json", "reason": "missing_package_version" })],
        );
    };
    if version != PACKAGE_VERSION {
        return invalid(
            "unsupported_package_version",
            vec![json!({ "path": "manifest.json", "found": version, "expected": PACKAGE_VERSION })],
        );
    }

    let Some(snapshot_id) = manifest.get("snapshot_id").and_then(Value::as_str) else {
        return invalid(
            "manifest_missing_snapshot_id",
            vec![json!({ "path": "manifest.json", "reason": "missing_snapshot_id" })],
        );
    };
    if !is_sha256_hex(snapshot_id) {
        return invalid(
            "manifest_invalid_snapshot_id",
            vec![json!({ "path": "manifest.json", "reason": "invalid_snapshot_id" })],
        );
    }

    let Some(completeness) = manifest.get("completeness").and_then(Value::as_str) else {
        return invalid(
            "manifest_missing_completeness",
            vec![json!({ "path": "manifest.json", "reason": "missing_completeness" })],
        );
    };
    if completeness != "complete" && completeness != "partial" {
        return invalid(
            "manifest_invalid_completeness",
            vec![json!({ "path": "manifest.json", "found": completeness })],
        );
    }

    let vault_format = manifest.get("vault_format").and_then(Value::as_i64);
    let missing_count = manifest
        .get("missing")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if completeness == "complete" && missing_count > 0 {
        return invalid(
            "completeness_contradiction",
            vec![json!({ "path": "manifest.json", "reason": "complete_with_missing_entries" })],
        );
    }

    let Some(listed) = manifest.get("files").and_then(Value::as_array) else {
        return invalid(
            "manifest_missing_files",
            vec![json!({ "path": "manifest.json", "reason": "missing_files_array" })],
        );
    };

    let mut defects: Vec<Value> = Vec::new();
    let mut expected_paths = BTreeSet::new();
    expected_paths.insert("manifest.json".to_string());
    let mut verified_map: BTreeMap<String, (String, u64)> = BTreeMap::new();
    let mut records_bytes: Option<Vec<u8>> = None;
    let mut csv_bytes: Option<Vec<u8>> = None;
    let mut verified_originals = 0usize;

    for entry in listed {
        let Some(rel) = entry.get("path").and_then(Value::as_str) else {
            defects.push(json!({ "reason": "invalid_manifest_entry" }));
            continue;
        };
        let Some(expected_hash) = entry.get("sha256").and_then(Value::as_str) else {
            defects.push(json!({ "path": rel, "reason": "missing_sha256" }));
            continue;
        };
        let Some(expected_len) = entry.get("byte_length").and_then(Value::as_u64) else {
            defects.push(json!({ "path": rel, "reason": "missing_byte_length" }));
            continue;
        };
        if !safe_rel_path(rel) || rel == "manifest.json" || !is_sha256_hex(expected_hash) {
            defects.push(json!({ "path": rel, "reason": "unsafe_or_invalid_manifest_entry" }));
            continue;
        }
        if !expected_paths.insert(rel.to_string()) {
            defects.push(json!({ "path": rel, "reason": "duplicate_manifest_entry" }));
            continue;
        }
        let full = package.join(rel);
        let (actual_hash, actual_len) = if rel == "records.json" || rel == "facts.csv" {
            let bytes = match read_regular_file(&full, MAX_METADATA_BYTES) {
                Ok(bytes) => bytes,
                Err(reason) => {
                    defects.push(json!({ "path": rel, "reason": reason }));
                    continue;
                }
            };
            let len = bytes.len() as u64;
            let hash = sha256_bytes(&bytes);
            if len == expected_len && hash == expected_hash {
                if rel == "records.json" {
                    records_bytes = Some(bytes);
                } else {
                    csv_bytes = Some(bytes);
                }
            }
            (hash, len)
        } else {
            match hash_regular_file(&full, MAX_OBJECT_BYTES) {
                Ok(pair) => pair,
                Err(reason) => {
                    defects.push(json!({ "path": rel, "reason": reason }));
                    continue;
                }
            }
        };
        if actual_len != expected_len {
            defects.push(json!({
                "path": rel,
                "reason": "length_mismatch",
                "expected": expected_len,
                "actual": actual_len
            }));
            continue;
        }
        if actual_hash != expected_hash {
            defects.push(json!({
                "path": rel,
                "reason": "hash_mismatch",
                "expected": expected_hash,
                "actual": actual_hash
            }));
            continue;
        }
        if rel.starts_with("originals/") {
            verified_originals += 1;
        }
        verified_map.insert(rel.to_string(), (actual_hash, actual_len));
    }

    // Walk the directory tree to detect symlinks or unexpected extra files
    let mut disk_files = BTreeSet::new();
    if let Err(err_defects) = scan_tree(package, package, &mut disk_files) {
        defects.extend(err_defects);
    } else {
        for found in &disk_files {
            if !expected_paths.contains(found) {
                defects.push(json!({ "path": found, "reason": "unexpected_file" }));
            }
        }
    }

    // Check records.json and snapshot_id
    let records = match &records_bytes {
        None => {
            if !defects
                .iter()
                .any(|d| d.get("path").and_then(Value::as_str) == Some("records.json"))
            {
                defects.push(json!({ "path": "records.json", "reason": "missing_records_entry" }));
            }
            None
        }
        Some(bytes) => {
            let hash = sha256_bytes(bytes);
            if hash != snapshot_id {
                defects.push(json!({
                    "path": "records.json",
                    "reason": "snapshot_id_mismatch",
                    "expected": snapshot_id,
                    "actual": hash
                }));
            }
            match serde_json::from_slice::<Value>(bytes) {
                Ok(val) if val.is_object() => {
                    if val.get("package_version").and_then(Value::as_str) != Some(PACKAGE_VERSION) {
                        defects.push(json!({
                            "path": "records.json",
                            "reason": "records_version_mismatch"
                        }));
                    }
                    if let (Some(mf), Some(rf)) = (
                        vault_format,
                        val.get("vault_format").and_then(Value::as_i64),
                    ) {
                        if mf != rf {
                            defects.push(json!({
                                "path": "records.json",
                                "reason": "records_vault_format_mismatch",
                                "expected": mf,
                                "actual": rf
                            }));
                        }
                    }
                    Some(val)
                }
                _ => {
                    defects.push(json!({
                        "path": "records.json",
                        "reason": "records_invalid_json"
                    }));
                    None
                }
            }
        }
    };

    // Cross-check records.objects package_path hashes against verified originals
    if let Some(records) = &records {
        if let Some(objects) = records.get("objects").and_then(Value::as_array) {
            for obj in objects {
                let Some(pkg_path) = obj.get("package_path").and_then(Value::as_str) else {
                    if completeness == "complete" {
                        let oid = obj
                            .get("object_id")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        defects.push(json!({
                            "path": format!("originals/{oid}"),
                            "reason": "records_object_missing_package_path"
                        }));
                    }
                    continue;
                };
                let expected_hash = obj.get("content_sha256").and_then(Value::as_str);
                let expected_len = obj.get("byte_length").and_then(Value::as_u64);
                match verified_map.get(pkg_path) {
                    None => {
                        if !defects
                            .iter()
                            .any(|d| d.get("path").and_then(Value::as_str) == Some(pkg_path))
                        {
                            defects.push(json!({
                                "path": pkg_path,
                                "reason": "records_object_missing_from_manifest"
                            }));
                        }
                    }
                    Some((actual_hash, actual_len)) => {
                        if expected_hash != Some(actual_hash.as_str())
                            || expected_len != Some(*actual_len)
                        {
                            defects.push(json!({
                                "path": pkg_path,
                                "reason": "records_object_hash_mismatch"
                            }));
                        }
                    }
                }
            }
        }
    }

    // Cross-check facts.csv against records.json facts when facts exist
    let mut facts_csv_checked = false;
    if let Some(records) = &records {
        let expects_csv = records.get("facts").and_then(Value::as_array).is_some();
        match &csv_bytes {
            Some(csv) => {
                facts_csv_checked = true;
                match std::str::from_utf8(csv) {
                    Err(_) => {
                        defects.push(json!({ "path": "facts.csv", "reason": "csv_invalid_utf8" }))
                    }
                    Ok(text) => {
                        if !facts_match_csv(records, text) {
                            defects.push(json!({ "path": "facts.csv", "reason": "csv_mismatch" }));
                        }
                    }
                }
            }
            None if expects_csv
                && !defects
                    .iter()
                    .any(|d| d.get("path").and_then(Value::as_str) == Some("facts.csv")) =>
            {
                defects.push(json!({ "path": "facts.csv", "reason": "missing_facts_csv" }));
            }
            None => {}
        }
    }

    if !defects.is_empty() {
        let primary = defects[0]
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("package_invalid")
            .to_string();
        return invalid(&primary, defects);
    }

    let mut body = honesty("Verified");
    body.insert("slice".into(), json!("T5-exit"));
    body.insert("read_only".into(), json!(true));
    body.insert("new_effect".into(), json!(false));
    body.insert("package_version".into(), json!(PACKAGE_VERSION));
    body.insert("snapshot_id".into(), json!(snapshot_id));
    body.insert("completeness".into(), json!(completeness));
    body.insert("vault_format".into(), json!(vault_format));
    if let Some(protection) = manifest.get("protection").cloned() {
        body.insert("protection".into(), protection);
    }
    if let Some(synthetic) = manifest.get("synthetic_only").cloned() {
        body.insert("synthetic_only".into(), synthetic);
    }
    body.insert("verified_files".into(), json!(verified_map.len()));
    body.insert("verified_originals".into(), json!(verified_originals));
    body.insert("facts_csv_checked".into(), json!(facts_csv_checked));
    body.insert("missing_count".into(), json!(missing_count));
    body.insert("defects".into(), json!([]));
    Report {
        code: EXIT_OK,
        body: Value::Object(body),
    }
}

fn invalid_reject(reason: &str, os_error: Option<String>) -> Report {
    let mut body = honesty("Rejected");
    body.insert("slice".into(), json!("T5-exit"));
    body.insert("read_only".into(), json!(true));
    body.insert("new_effect".into(), json!(false));
    body.insert("reason".into(), json!(reason));
    if let Some(err) = os_error {
        body.insert("os_error".into(), json!(err));
    }
    Report {
        code: EXIT_REJECTED,
        body: Value::Object(body),
    }
}

fn invalid(reason: &str, defects: Vec<Value>) -> Report {
    let mut body = honesty("PackageInvalid");
    body.insert("slice".into(), json!("T5-exit"));
    body.insert("read_only".into(), json!(true));
    body.insert("new_effect".into(), json!(false));
    body.insert("reason".into(), json!(reason));
    body.insert("defects".into(), Value::Array(defects));
    Report {
        code: EXIT_RECOVERY,
        body: Value::Object(body),
    }
}

fn read_regular_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, &'static str> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err("missing_file"),
        Err(_) => return Err("unreadable_file"),
    };
    if meta.file_type().is_symlink() {
        return Err("symlink_in_package");
    }
    if !meta.file_type().is_file() {
        return Err("not_a_regular_file");
    }
    if meta.len() > max_bytes {
        return Err("file_too_large");
    }
    let file = File::open(path).map_err(|_| "unreadable_file")?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "unreadable_file")?;
    if bytes.len() as u64 > max_bytes {
        return Err("file_too_large");
    }
    Ok(bytes)
}

fn hash_regular_file(path: &Path, max_bytes: u64) -> Result<(String, u64), &'static str> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err("missing_file"),
        Err(_) => return Err("unreadable_file"),
    };
    if meta.file_type().is_symlink() {
        return Err("symlink_in_package");
    }
    if !meta.file_type().is_file() {
        return Err("not_a_regular_file");
    }
    if meta.len() > max_bytes {
        return Err("file_too_large");
    }
    let mut file = File::open(path).map_err(|_| "unreadable_file")?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|_| "unreadable_file")?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > max_bytes {
            return Err("file_too_large");
        }
        hasher.update(&buf[..n]);
    }
    Ok((hex_encode(&hasher.finalize()), total))
}

fn scan_tree(root: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<(), Vec<Value>> {
    let entries = fs::read_dir(dir).map_err(|_| {
        vec![json!({ "path": dir.display().to_string(), "reason": "unreadable_dir" })]
    })?;
    let mut problems = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            problems.push(json!({ "reason": "unreadable_dir_entry" }));
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            problems
                .push(json!({ "path": path.display().to_string(), "reason": "unreadable_file" }));
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.to_str())
            .map(|s| s.replace('\\', "/"))
            .unwrap_or_default();
        if meta.file_type().is_symlink() {
            problems.push(json!({ "path": rel, "reason": "symlink_in_package" }));
            continue;
        }
        if meta.is_dir() {
            if let Err(sub) = scan_tree(root, &path, out) {
                problems.extend(sub);
            }
        } else if meta.is_file() {
            out.insert(rel);
        } else {
            problems.push(json!({ "path": rel, "reason": "not_a_regular_file" }));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn safe_rel_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return false;
    }
    path.split('/')
        .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn facts_match_csv(records: &Value, csv_text: &str) -> bool {
    let Some(facts) = records.get("facts").and_then(Value::as_array) else {
        return false;
    };
    let Some(rows) = parse_csv(csv_text) else {
        return false;
    };
    let Some(header) = rows.first() else {
        return false;
    };
    let idx = |name: &str| header.iter().position(|cell| cell == name);
    let (
        Some(fact_id),
        Some(source_id),
        Some(candidate_id),
        Some(fact_type),
        Some(revision),
        Some(statement),
        Some(currency),
        Some(amount),
        Some(start),
        Some(end),
        Some(latest),
    ) = (
        idx("fact_id"),
        idx("source_id"),
        idx("candidate_id"),
        idx("fact_type"),
        idx("revision"),
        idx("statement"),
        idx("currency"),
        idx("amount_text"),
        idx("period_start"),
        idx("period_end"),
        idx("latest_decision_id"),
    )
    else {
        return false;
    };

    let mut csv_keys = BTreeSet::new();
    for row in rows.iter().skip(1) {
        if row.iter().all(String::is_empty) {
            continue;
        }
        if row.len() != header.len() {
            return false;
        }
        let Ok(rev) = row[revision].parse::<i64>() else {
            return false;
        };
        csv_keys.insert(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row[fact_id],
            row[source_id],
            row[candidate_id],
            row[fact_type],
            rev,
            row[statement],
            row[currency],
            row[amount],
            row[start],
            row[end],
            row[latest]
        ));
    }

    let mut rec_keys = BTreeSet::new();
    for fact in facts {
        let (Some(fid), Some(sid), Some(cid), Some(ftype), Some(rev), Some(val), Some(decs)) = (
            fact.get("fact_id").and_then(Value::as_str),
            fact.get("source_id").and_then(Value::as_str),
            fact.get("candidate_id").and_then(Value::as_str),
            fact.get("fact_type").and_then(Value::as_str),
            fact.get("revision").and_then(Value::as_i64),
            fact.get("value"),
            fact.get("decision_ids").and_then(Value::as_array),
        ) else {
            return false;
        };
        let latest_id = decs.last().and_then(Value::as_str).unwrap_or("");
        rec_keys.insert(format!(
            "{fid}\t{sid}\t{cid}\t{ftype}\t{rev}\t{}\t{}\t{}\t{}\t{}\t{latest_id}",
            val_field(val, "statement"),
            val_field(val, "currency"),
            val_field(val, "amount_text"),
            val_field(val, "period_start"),
            val_field(val, "period_end"),
        ));
    }

    csv_keys == rec_keys
}

fn val_field(val: &Value, key: &str) -> String {
    match val.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn parse_csv(text: &str) -> Option<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(ch);
            }
            continue;
        }
        match ch {
            '"' if cell.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut cell)),
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {}
            _ => cell.push(ch),
        }
    }
    if quoted {
        return None;
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Some(rows)
}
