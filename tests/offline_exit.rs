use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NOTE: &str = "\
Amount paid USD 15.99
Service period: 2026-09-01 through 2026-09-30
Account email: one@example.test
";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-exit-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("kept {}", self.path.display());
        } else {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct RestoredMode {
    path: PathBuf,
    mode: u32,
}

impl Drop for RestoredMode {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}

fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(bin()).args(args).output().unwrap();
    let code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (code, body)
}

fn write_note(tmp: &Tmp, name: &str, text: &str) -> PathBuf {
    let path = tmp.path().join(name);
    fs::write(&path, text).unwrap();
    path
}

fn init(tmp: &Tmp, format: &str) -> PathBuf {
    let vault = tmp.path().join(format!("vault-{format}"));
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        format,
    ]);
    assert_eq!(code, 0, "{body}");
    vault
}

fn fragment(vault: &Path, command: &str, file: &Path) -> Value {
    let (code, body) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--fragment",
        file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    body
}

fn sql(vault: &Path, query: &str) -> String {
    let output = Command::new("sqlite3")
        .arg(vault.join("vault.db"))
        .arg(query)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn file_sha(path: &Path) -> String {
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn object_hashes(vault: &Path) -> Vec<(String, String)> {
    let mut names: Vec<_> = fs::read_dir(vault.join("objects"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, file_sha(&path))
        })
        .collect()
}

fn journal_names(vault: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(vault.join("journal"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn tree_sha(root: &Path) -> String {
    let mut files = Vec::new();
    walk_files(root, &mut files);
    files.sort();
    let mut text = String::new();
    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        text.push_str(&rel);
        text.push(' ');
        text.push_str(&file_sha(&path));
        text.push('\n');
    }
    let output = Command::new("sha256sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = output;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let done = child.wait_with_output().unwrap();
    assert!(done.status.success());
    String::from_utf8(done.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn check_package(
    tmp: &Tmp,
    vault: &Path,
    package: &Path,
    completeness: &str,
    facts: &str,
    uncited: &str,
    files: &[&Path],
) {
    let script = tmp.path().join("check_package.py");
    fs::write(&script, PYTHON).unwrap();
    let mut args = vec![
        script.to_str().unwrap().to_string(),
        vault.display().to_string(),
        package.display().to_string(),
        completeness.into(),
        facts.into(),
        uncited.into(),
    ];
    for file in files {
        args.push(file.display().to_string());
    }
    let output = Command::new("python3").args(&args).output().unwrap();
    assert!(
        output.status.success(),
        "independent read failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("independent-ok"), "{stdout}");
}

const PYTHON: &str = r#"
import csv, hashlib, json, sqlite3, sys
from pathlib import Path

vault = Path(sys.argv[1])
package = Path(sys.argv[2])
expect = sys.argv[3]
facts_mode = sys.argv[4]
uncited = sys.argv[5]
externals = [Path(item) for item in sys.argv[6:]]

def digest(data):
    return hashlib.sha256(data).hexdigest()

manifest = json.loads((package / "manifest.json").read_text())
records = json.loads((package / "records.json").read_text())
record_bytes = (package / "records.json").read_bytes()
assert manifest["completeness"] == expect, manifest["completeness"]
assert manifest["snapshot_id"] == digest(record_bytes)
assert manifest["protection"] == "undecided"
assert manifest["security_complete"] is False
assert manifest["live_directory_copy"] is False
assert records["protection"] == "undecided"
assert records["live_directory_copy"] is False
assert "未定" in records["field_notes"]["protection"]
assert not (package / "vault.db").exists()
assert not (package / "writer.lock").exists()
assert not (package / "journal").exists()
assert not (package / "SYNTHETIC_ONLY").exists()

found = sorted(str(path.relative_to(package)) for path in package.rglob("*") if path.is_file())
listed = sorted(item["path"] for item in manifest["files"])
assert found == sorted(listed + ["manifest.json"]), (found, listed)
for item in manifest["files"]:
    data = (package / item["path"]).read_bytes()
    assert digest(data) == item["sha256"], item["path"]
    assert len(data) == item["byte_length"]
    if item["path"].startswith("originals/"):
        assert item["object_id"] == item["path"].split("/", 1)[1]

uri = "file:" + str(vault / "vault.db") + "?mode=ro"
conn = sqlite3.connect(uri, uri=True)
conn.row_factory = sqlite3.Row

def has_table(name):
    return conn.execute(
        "select 1 from sqlite_master where type='table' and name=?", (name,)
    ).fetchone() is not None

db_sources = conn.execute(
    "select source_id, object_id, content_sha256, byte_length, media_type, user_marker from sources order by source_id"
).fetchall()
assert [row["source_id"] for row in db_sources] == [row["source_id"] for row in records["sources"]]
for db, rec in zip(db_sources, records["sources"]):
    assert rec["object_id"] == db["object_id"]
    assert rec["content_sha256"] == db["content_sha256"]
    assert rec["byte_length"] == db["byte_length"]
    assert rec["media_type"] == db["media_type"]
    assert rec["user_marker"] == db["user_marker"]

db_occ = conn.execute(
    "select occurrence_id, source_id, command_id, batch_id, display_name, external_path, storage_reused, recorded_at from occurrences order by recorded_at, occurrence_id"
).fetchall()
assert [row["occurrence_id"] for row in db_occ] == [row["occurrence_id"] for row in records["occurrences"]]
for db, rec in zip(db_occ, records["occurrences"]):
    assert rec["source_id"] == db["source_id"]
    assert rec["external_path"] == db["external_path"]
    assert rec["display_name"] == db["display_name"]
    assert rec["storage_reused"] == (db["storage_reused"] == 1)

db_objects = conn.execute(
    "select object_id, content_sha256, byte_length, relpath from objects order by object_id"
).fetchall()
assert [row["object_id"] for row in db_objects] == [row["object_id"] for row in records["objects"]]
for db, rec in zip(db_objects, records["objects"]):
    assert rec["content_sha256"] == db["content_sha256"]
    assert rec["byte_length"] == db["byte_length"]
    assert rec["stored_relpath"] == db["relpath"]
    if rec["copy_state"] == "copied":
        blob = (package / rec["package_path"]).read_bytes()
        assert blob == (vault / "objects" / rec["object_id"]).read_bytes()
        assert digest(blob) == rec["content_sha256"]
        assert len(blob) == rec["byte_length"]
    else:
        assert rec["package_path"] is None
        assert not (package / "originals" / rec["object_id"]).exists()

if records["source_versions"] is None:
    assert not has_table("source_versions")
else:
    db_versions = conn.execute(
        "select source_version_id, source_id, object_id, content_sha256, byte_length, media_type, role, eml from source_versions order by source_version_id"
    ).fetchall()
    assert [row["source_version_id"] for row in db_versions] == [row["source_version_id"] for row in records["source_versions"]]
    for db, rec in zip(db_versions, records["source_versions"]):
        assert rec["content_sha256"] == db["content_sha256"]
        assert rec["object_id"] == db["object_id"]
        assert db["eml"] == 0 and rec["eml"] is False

if facts_mode == "no":
    assert records["facts"] is None and records["decisions"] is None
    assert not (package / "facts.csv").exists()
else:
    assert records["facts"] is not None and records["decisions"] is not None
    db_facts = conn.execute(
        "select fact_id, source_id, candidate_id, fact_type, value_json, revision from facts order by source_id, fact_id"
    ).fetchall()
    assert [row["fact_id"] for row in db_facts] == [row["fact_id"] for row in records["facts"]]
    for db, rec in zip(db_facts, records["facts"]):
        assert json.loads(db["value_json"]) == rec["value"]
        assert db["revision"] == rec["revision"]
        assert db["fact_type"] == rec["fact_type"]
    db_dec = conn.execute(
        "select decision_id, fact_id, command_id, candidate_id, slot, kind, actor_kind, actor_id, rule_version, prior_value_json, new_value_json, target_decision_id from decisions order by fact_id, rowid"
    ).fetchall()
    assert [row["decision_id"] for row in db_dec] == [row["decision_id"] for row in records["decisions"]]
    for db, rec in zip(db_dec, records["decisions"]):
        assert db["kind"] == rec["kind"]
        assert db["command_id"] == rec["command_id"]
        assert db["target_decision_id"] == rec["target_decision_id"]
        assert json.loads(db["new_value_json"]) == rec["new_value"]
        if db["prior_value_json"] is None:
            assert rec["prior_value"] is None
        else:
            assert json.loads(db["prior_value_json"]) == rec["prior_value"]
    with (package / "facts.csv").open(newline="") as handle:
        table = list(csv.DictReader(handle))
    assert [row["fact_id"] for row in table] == [row["fact_id"] for row in records["facts"]]
    for rec, csv_row in zip(records["facts"], table):
        chain = [item for item in records["decisions"] if item["fact_id"] == rec["fact_id"]]
        assert [item["decision_id"] for item in chain] == rec["decision_ids"]
        assert len(chain) == rec["revision"]
        assert chain[0]["kind"] == "rule_adopt" and chain[0]["prior_value"] is None
        assert chain[-1]["new_value"] == rec["value"]
        for earlier, later in zip(chain, chain[1:]):
            assert later["prior_value"] == earlier["new_value"]
        amount = rec["value"].get("amount_text")
        assert csv_row["amount_text"] == ("" if amount is None else amount)
        assert csv_row["amount_text"] != "0"
        assert csv_row["latest_decision_id"] == chain[-1]["decision_id"]
        if rec["fact_type"] == "stated_amount_paid" and rec["revision"] >= 3:
            amounts = [item["new_value"].get("amount_text") for item in chain]
            assert "10.00" in amounts
            assert "15.99" in amounts
            assert rec["value"]["amount_text"] == "15.99"
            assert csv_row["amount_text"] == "15.99"

if records["candidates"] is None:
    assert not has_table("candidates")
else:
    db_cand = conn.execute(
        "select c.candidate_id, c.excerpt, c.kind, c.usable, c.adopted, c.value_json, c.locator_json from candidates c join source_versions sv on sv.source_version_id = c.source_version_id order by sv.source_id, c.ordinal, c.candidate_id"
    ).fetchall()
    assert [row["candidate_id"] for row in db_cand] == [row["candidate_id"] for row in records["candidates"]]
    for db, rec in zip(db_cand, records["candidates"]):
        assert rec["excerpt"] == db["excerpt"]
        assert rec["kind"] == db["kind"]
        assert db["adopted"] == 0 and rec["adopted"] is False
        assert rec["usable"] == (db["usable"] == 1)
        assert json.loads(db["value_json"]) == rec["value"]
        assert json.loads(db["locator_json"]) == rec["locator"]
        assert "byte_start" in rec["locator"] or "scalar_start" in rec["locator"] or rec["locator"]
    if uncited == "yes":
        assert any(row["cited_by_fact"] is False for row in records["candidates"])
    if records["facts"] is not None:
        cited = {row["candidate_id"] for row in records["facts"]}
        for rec in records["candidates"]:
            assert rec["cited_by_fact"] == (rec["candidate_id"] in cited)

for item in manifest["missing"]:
    db = conn.execute(
        "select content_sha256, byte_length from objects where object_id=?",
        (item["object_id"],),
    ).fetchone()
    assert db is not None
    assert db["content_sha256"] == item["content_sha256"]
    assert not (package / "originals" / item["object_id"]).exists()
    assert item["object_id"] in [row["object_id"] for row in records["objects"]]

copied = [row for row in records["objects"] if row["copy_state"] == "copied"]
assert manifest["counts"]["objects_copied"] == len(copied)
assert manifest["counts"]["objects_not_copied"] == len(manifest["missing"])

for external in externals:
    blob = external.read_bytes()
    assert any(
        row["package_path"] and (package / row["package_path"]).read_bytes() == blob
        for row in records["objects"]
    ), external

print("independent-ok")
"#;

#[test]
fn export_keeps_originals_unused_candidates_current_facts_and_the_whole_chain() {
    let tmp = Tmp::new("chain");
    let vault = init(&tmp, "3");
    let file = write_note(&tmp, "stated.txt", NOTE);
    let published = fragment(&vault, "a", &file);
    let source_id = published["source_id"].as_str().unwrap();
    let (code, listed) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{listed}");
    let paid = listed["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fact| fact["fact_type"] == "stated_amount_paid" && fact["source_id"] == source_id)
        .unwrap();
    let fact_id = paid["fact_id"].as_str().unwrap();
    let (code, corrected) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "b",
        "--fact-id",
        fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "10.00",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{corrected}");
    let decision_b = corrected["decision_id"].as_str().unwrap().to_string();
    let first = tmp.path().join("pack-corrected");
    let hashes = object_hashes(&vault);
    let journals = journal_names(&vault);
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        first.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    assert_eq!(exported["outcome"], "Exported");
    assert_eq!(exported["completeness"], "complete");
    assert_eq!(exported["slice"], "T5-exit");
    assert_eq!(exported["protection"], "undecided");
    assert_eq!(exported["security_complete"], false);
    assert_eq!(exported["new_effect"], false);
    assert_eq!(exported["output_written"], true);
    assert_eq!(exported["live_directory_copy"], false);
    assert!(exported.get("preservation").is_none(), "{exported}");
    check_package(&tmp, &vault, &first, "complete", "yes", "yes", &[&file]);
    assert_eq!(journal_names(&vault), journals);
    let first_bytes = tree_sha(&first);
    let corrected_amount = fs::read_to_string(first.join("facts.csv")).unwrap();
    assert!(corrected_amount.contains("10.00"), "{corrected_amount}");

    let (code, undone) = run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "undo-b",
        "--decision-id",
        &decision_b,
        "--expected-revision",
        "2",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{undone}");
    let journals = journal_names(&vault);
    let second = tmp.path().join("pack-restored");
    let (code, restored) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        second.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{restored}");
    assert_eq!(restored["completeness"], "complete");
    assert_ne!(restored["snapshot_id"], exported["snapshot_id"]);
    check_package(&tmp, &vault, &second, "complete", "yes", "yes", &[&file]);
    assert_eq!(
        tree_sha(&first),
        first_bytes,
        "the earlier package was overwritten"
    );
    let current = fs::read_to_string(second.join("facts.csv")).unwrap();
    assert!(current.contains("15.99"), "{current}");
    let chain = fs::read_to_string(second.join("records.json")).unwrap();
    assert!(
        chain.contains("10.00"),
        "the corrected amount dropped out of the chain"
    );
    assert_eq!(object_hashes(&vault), hashes);
    assert_eq!(journal_names(&vault), journals, "export added a journal");
    assert_eq!(
        sql(&vault, "select count(*) from decisions where kind='undo';"),
        "1"
    );

    let before = tree_sha(&first);
    let (code, again) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        first.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{again}");
    assert_eq!(again["reason"], "output_exists");
    assert_eq!(again["output_written"], false);
    assert_eq!(tree_sha(&first), before);
}

#[test]
fn format_1_exports_original_bytes_and_format_2_exports_candidates_without_inventing_facts() {
    let tmp = Tmp::new("formats");
    let vault1 = init(&tmp, "1");
    let empty = tmp.path().join("empty");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault1.to_str().unwrap(),
        "--out",
        empty.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["completeness"], "complete");
    assert_eq!(body["counts"]["objects"], 0);
    assert!(body["counts"]["facts"].is_null(), "{body}");
    check_package(&tmp, &vault1, &empty, "complete", "no", "no", &[]);

    let source = write_note(&tmp, "plain.txt", "offline-exit-bytes\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault1.to_str().unwrap(),
        "--command-id",
        "imp-1",
        "--media-type",
        "text/plain",
        "--source",
        source.to_str().unwrap(),
        "--display-name",
        "plain.txt",
    ]);
    assert_eq!(code, 0, "{imported}");
    let hashes = object_hashes(&vault1);
    let packed = tmp.path().join("format1");
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault1.to_str().unwrap(),
        "--out",
        packed.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    check_package(&tmp, &vault1, &packed, "complete", "no", "no", &[&source]);
    assert_eq!(object_hashes(&vault1), hashes);
    let records: Value =
        serde_json::from_slice(&fs::read(packed.join("records.json")).unwrap()).unwrap();
    assert!(records["candidates"].is_null(), "{records}");
    assert!(records["facts"].is_null(), "{records}");

    let vault2 = init(&tmp, "2");
    let file = write_note(&tmp, "body.txt", NOTE);
    fragment(&vault2, "frag-2", &file);
    let (code, facts) = run(&["facts", "--vault", vault2.to_str().unwrap()]);
    assert_eq!(code, 14, "{facts}");
    let pack = tmp.path().join("format2");
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault2.to_str().unwrap(),
        "--out",
        pack.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    assert_eq!(exported["vault_format"], 2);
    check_package(&tmp, &vault2, &pack, "complete", "no", "yes", &[&file]);
    assert!(!pack.join("facts.csv").exists());
}

#[test]
fn missing_or_mismatched_bytes_are_not_published_as_complete() {
    let tmp = Tmp::new("partial");
    let vault = init(&tmp, "3");
    let kept = write_note(&tmp, "kept.txt", NOTE);
    let other = write_note(
        &tmp,
        "other.txt",
        "Amount paid USD 4.00\nService period: 2026-10-01 through 2026-10-31\nAccount email: two@example.test\n",
    );
    fragment(&vault, "kept", &kept);
    fragment(&vault, "other", &other);
    let other_hash = file_sha(&other);
    let object_id = sql(
        &vault,
        &format!("select object_id from objects where content_sha256='{other_hash}';"),
    );
    let object_path = vault.join("objects").join(&object_id);
    fs::remove_file(&object_path).unwrap();
    let blocked = tmp.path().join("blocked");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        blocked.to_str().unwrap(),
    ]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["reason"], "object_missing");
    assert_eq!(body["completeness"], "incomplete");
    assert_eq!(body["output_written"], false);
    assert_eq!(body["output_cleanup"], "not_created");
    assert!(body.get("output_staging_path").is_none(), "{body}");
    assert!(body.get("output_cleanup_reason").is_none(), "{body}");
    assert!(!blocked.exists());
    assert!(body.get("preservation").is_none(), "{body}");

    let partial = tmp.path().join("partial");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        partial.to_str().unwrap(),
        "--allow-partial",
        "yes",
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Partial");
    assert_eq!(body["completeness"], "partial");
    assert_eq!(body["allow_partial"], true);
    check_package(&tmp, &vault, &partial, "partial", "yes", "yes", &[&kept]);
    let manifest: Value =
        serde_json::from_slice(&fs::read(partial.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["missing"][0]["object_id"], object_id);
    assert_eq!(manifest["missing"][0]["reason"], "object_missing");
    assert!(!partial.join("originals").join(&object_id).exists());

    let kept_hash = file_sha(&kept);
    let kept_id = sql(
        &vault,
        &format!("select object_id from objects where content_sha256='{kept_hash}';"),
    );
    let flipped = vault.join("objects").join(&kept_id);
    let mut bytes = fs::read(&flipped).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&flipped, &bytes).unwrap();
    let bad = tmp.path().join("bad");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        bad.to_str().unwrap(),
    ]);
    assert_eq!(code, 12, "{body}");
    assert!(
        body["reason"] == "hash_mismatch" || body["reason"] == "export_incomplete",
        "{body}"
    );
    assert!(!bad.exists());
    let allowed = tmp.path().join("bad-partial");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        allowed.to_str().unwrap(),
        "--allow-partial",
        "yes",
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["completeness"], "partial");
    let mut packaged = Vec::new();
    walk_files(&allowed, &mut packaged);
    assert!(packaged.iter().all(|path| fs::read(path).unwrap() != bytes));
}

#[test]
fn export_refuses_vault_interior_and_existing_names_before_creating_a_package() {
    let tmp = Tmp::new("paths");
    let vault = init(&tmp, "3");
    let file = write_note(&tmp, "stated.txt", NOTE);
    fragment(&vault, "a", &file);
    let inside = vault.join("pack");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        inside.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_inside_vault");
    assert!(!inside.exists());

    let via = tmp.path().join("via");
    fs::create_dir(&via).unwrap();
    symlink(&vault, via.join("link")).unwrap();
    let linked = via.join("link").join("pack");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        linked.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_inside_vault");
    assert!(!vault.join("pack").exists());

    let existing = tmp.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("KEEP"), b"keep").unwrap();
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        existing.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_exists");
    assert_eq!(body["output_cleanup"], "not_created");
    assert!(body.get("output_staging_path").is_none(), "{body}");
    assert_eq!(fs::read(existing.join("KEEP")).unwrap(), b"keep");
    let early_stages: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".export-"))
        .collect();
    assert!(early_stages.is_empty(), "{early_stages:?}");

    let dangling = tmp.path().join("dangling");
    symlink("missing-target", &dangling).unwrap();
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        dangling.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_exists");
    assert!(dangling
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());

    let missing_parent = tmp.path().join("missing").join("pack");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        missing_parent.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_parent_missing");
    assert!(!tmp.path().join("missing").exists());

    let (code, body) = run(&[
        "init",
        "--vault",
        tmp.path().join("other").to_str().unwrap(),
        "--allow-partial",
        "yes",
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "allow_partial_only_on_export");
}

#[test]
fn export_write_failure_leaves_no_package_and_does_not_change_objects() {
    let tmp = Tmp::new("inject");
    let vault = init(&tmp, "3");
    let file = write_note(&tmp, "stated.txt", NOTE);
    fragment(&vault, "a", &file);
    let hashes = object_hashes(&vault);
    for (action, reason) in [
        ("io_error", "injected_io_error"),
        ("enospc", "injected_enospc"),
    ] {
        let dest = tmp.path().join(action);
        let witness = tmp.path().join(format!("{action}.json"));
        let (code, body) = run(&[
            "export",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            dest.to_str().unwrap(),
            "--inject",
            &format!("{action}:export_write"),
            "--witness",
            witness.to_str().unwrap(),
        ]);
        assert_eq!(code, 3, "{body}");
        assert_eq!(body["outcome"], "Incomplete");
        assert_eq!(body["reason"], reason);
        assert_eq!(body["injected"], true);
        assert_eq!(body["completeness"], "incomplete");
        assert_eq!(body["output_written"], false);
        assert_eq!(body["output_cleanup"], "not_attempted");
        assert!(
            body["output_cleanup_reason"]
                .as_str()
                .unwrap()
                .contains("automatic path deletion is disabled"),
            "{body}"
        );
        assert!(body.get("output_cleanup_error").is_none(), "{body}");
        assert!(body.get("preservation").is_none(), "{body}");
        assert!(!dest.exists());
        let stage = PathBuf::from(body["output_staging_path"].as_str().unwrap());
        assert!(stage.is_dir(), "{body}");
        let witness: Value = serde_json::from_slice(&fs::read(&witness).unwrap()).unwrap();
        assert_eq!(witness["reached"], true);
        assert_eq!(witness["point"], "export_write");
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".export-"))
            .collect();
        assert!(
            leftovers.iter().any(|name| stage.ends_with(name)),
            "{leftovers:?} {stage:?}"
        );
        assert_eq!(
            leftovers.len(),
            if action == "io_error" { 1 } else { 2 },
            "{leftovers:?}"
        );
    }
    assert_eq!(object_hashes(&vault), hashes);
}

#[test]
fn an_unreadable_object_is_not_omitted_as_a_partial_package() {
    let tmp = Tmp::new("unreadable");
    let vault = init(&tmp, "3");
    let file = write_note(&tmp, "stated.txt", NOTE);
    fragment(&vault, "a", &file);
    let object = vault
        .join("objects")
        .join(sql(&vault, "select object_id from objects;"));
    let _restore = RestoredMode {
        path: object.clone(),
        mode: 0o644,
    };
    fs::set_permissions(&object, fs::Permissions::from_mode(0o000)).unwrap();
    let no = tmp.path().join("no");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        no.to_str().unwrap(),
    ]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["reason"], "object_unreadable");
    assert!(!no.exists());
    let still = tmp.path().join("still-no");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        still.to_str().unwrap(),
        "--allow-partial",
        "yes",
    ]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["reason"], "object_unreadable");
    assert_eq!(body["completeness"], "incomplete");
    assert!(!still.exists());
}

#[test]
fn parent_dir_fsync_after_publish_keeps_the_export_package() {
    let tmp = Tmp::new("dirsync");
    let vault = init(&tmp, "1");
    let out = tmp.path().join("pkg");
    let witness = tmp.path().join("witness.json");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--inject",
        "io_error:export_dir_fsync",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 3, "{body}");
    assert_ne!(code, 15, "{body}");
    assert_eq!(body["outcome"], "Exported");
    assert_ne!(body["outcome"], "Incomplete");
    assert_eq!(body["output_written"], true);
    assert_eq!(body["published"], true);
    assert_eq!(body["directory_durable"], false);
    assert_eq!(body["parent_fsync_returned_ok"], true, "{body}");
    assert_eq!(body["completeness"], "complete");
    assert_eq!(body["reason"], "injected_io_error");
    assert_ne!(body["reason"], "directory_durability_unknown");
    assert_eq!(body["during"], "export_dir_fsync");
    assert_eq!(body["output"], out.to_str().unwrap());
    assert_eq!(body["injected"], true);
    assert!(body.get("output_cleanup").is_none(), "{body}");
    assert!(body.get("output_staging_path").is_none(), "{body}");
    let seen = serde_json::from_str::<Value>(&fs::read_to_string(&witness).unwrap()).unwrap();
    assert_eq!(seen["point"], "export_dir_fsync");
    assert_eq!(seen["published"], true);
    let records_before = fs::read(out.join("records.json")).unwrap();
    let manifest_before = fs::read(out.join("manifest.json")).unwrap();
    let script = tmp.path().join("read_pkg.py");
    fs::write(
        &script,
        r#"
import json, os, sys
root = sys.argv[1]
names = set(os.listdir(root))
assert "vault.db" not in names, names
assert "manifest.json" in names and "records.json" in names
manifest = json.load(open(os.path.join(root, "manifest.json"), encoding="utf-8"))
records = json.load(open(os.path.join(root, "records.json"), encoding="utf-8"))
assert manifest["completeness"] == "complete"
assert records["package_version"] == "offline-exit-1"
assert records["vault_format"] == 1
listed = {item["path"] for item in manifest["files"]}
assert "manifest.json" not in listed
assert "records.json" in listed
assert listed <= names
print("ok")
"#,
    )
    .unwrap();
    let checked = Command::new("python3")
        .arg(&script)
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "independent read failed\n{}\n{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let scratch: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(".export-"))
        .collect();
    assert!(scratch.is_empty(), "{scratch:?}");
    let (code, again) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{again}");
    assert_eq!(again["reason"], "output_exists");
    assert_eq!(again["output_written"], false);
    assert_eq!(fs::read(out.join("records.json")).unwrap(), records_before);
    assert_eq!(
        fs::read(out.join("manifest.json")).unwrap(),
        manifest_before
    );
    assert_eq!(sql(&vault, "SELECT count(*) FROM commands;"), "0");
}
