use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("synthetic/corpus")
}

#[derive(Debug)]
struct Out {
    code: Option<i32>,
    signal: Option<i32>,
    stdout: String,
    stderr: String,
    elapsed_ms: u128,
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("subvault-{label}-{}-{n}", std::process::id()));
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

fn record(scenario: &str, args: &[String], out: &Out) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/t1-evidence.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    let line = json!({
        "scenario": scenario,
        "argv": args,
        "exit": out.code,
        "signal": out.signal,
        "elapsed_ms": out.elapsed_ms,
        "stdout": out.stdout,
        "stderr": out.stderr
    });
    writeln!(file, "{line}").unwrap();
}

fn from_output(output: Output, elapsed: Duration) -> Out {
    use std::os::unix::process::ExitStatusExt;
    Out {
        code: output.status.code(),
        signal: output.status.signal(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        elapsed_ms: elapsed.as_millis(),
    }
}

fn run(scenario: &str, args: &[&str]) -> Out {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let start = Instant::now();
    let output = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let out = from_output(output, start.elapsed());
    record(scenario, &owned, &out);
    out
}

fn parse(out: &Out) -> Value {
    serde_json::from_str(&out.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err})\nstdout:\n{}\nstderr:\n{}",
            out.stdout, out.stderr
        )
    })
}

fn assert_honest(value: &Value) {
    assert_eq!(value["slice"], "T1", "{value}");
    assert_eq!(value["user_ready"], false, "{value}");
    assert_eq!(value["security_complete"], false, "{value}");
    assert_eq!(value["synthetic_only"], true, "{value}");
    assert!(value["purchase_id"].is_null(), "{value}");
    assert!(value["entitlement_id"].is_null(), "{value}");
}

fn vault_init(tmp: &Tmp) -> Value {
    let out = run(
        "init",
        &[
            "init",
            "--vault",
            tmp.path().join("vault").to_str().unwrap(),
        ],
    );
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let body = parse(&out);
    assert_eq!(body["outcome"], "Initialized");
    assert_honest(&body);
    assert_eq!(body["protection"], "unanswered_synthetic_plaintext");
    assert_eq!(body["primary_input"], "unanswered");
    assert_eq!(body["capabilities"]["arrangement"], "not_available");
    assert_eq!(body["capabilities"]["full_text_search"], "not_available");
    assert_eq!(body["capabilities"]["extraction"], "not_available");
    assert_eq!(body["capabilities"]["purchases"], "not_available");
    assert_eq!(body["capabilities"]["real_data_import"], "not_available");
    assert_eq!(body["platform_limits"]["windows_native"], "not_tested");
    assert_eq!(body["platform_limits"]["power_loss"], "not_tested");
    assert_eq!(body["platform_limits"]["bad_disk"], "not_tested");
    assert_eq!(
        body["platform_limits"]["sync_directory"],
        "not_supported_and_not_detected"
    );
    body
}

fn vault(tmp: &Tmp) -> PathBuf {
    tmp.path().join("vault")
}

fn import(tmp: &Tmp, command: &str, media: &str, source: &Path) -> Value {
    let out = run(
        "import",
        &[
            "import",
            "--vault",
            vault(tmp).to_str().unwrap(),
            "--command-id",
            command,
            "--media-type",
            media,
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(out.code, Some(0), "{}\n{}", out.stdout, out.stderr);
    let body = parse(&out);
    assert_eq!(body["outcome"], "Committed", "{body}");
    assert_honest(&body);
    assert_eq!(body["preservation"], "preserved");
    assert_eq!(body["arrangement"], "not_arranged");
    assert_eq!(body["searchability"], "not_searchable");
    assert_eq!(body["new_effect"], true);
    assert_ne!(body["source_id"], body["content_sha256"]);
    body
}

fn open_vault(tmp: &Tmp) -> (i32, Value) {
    let out = run("open", &["open", "--vault", vault(tmp).to_str().unwrap()]);
    let code = out.code.expect("open exited by signal");
    let body = parse(&out);
    assert_honest(&body);
    (code, body)
}

fn assert_denied(message: &str) {
    assert!(
        message.contains("Permission denied") || message.contains("os error 13"),
        "{message}"
    );
}

fn command_source<'a>(status: &'a Value, command_id: &str) -> &'a Value {
    let source_id = status["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["command_id"] == command_id)
        .unwrap_or_else(|| panic!("no occurrence for {command_id}"))["source_id"]
        .clone();
    status["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source_id"] == source_id)
        .unwrap()
}

fn sha256(path: &Path) -> String {
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn files_with_bytes(root: &Path, bytes: &[u8]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, &mut |path, meta| {
        if meta.is_file() && meta.len() == bytes.len() as u64 && fs::read(path).unwrap() == bytes {
            found.push(path.to_path_buf());
        }
    });
    found
}

fn proper_prefixes(root: &Path, bytes: &[u8]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, &mut |path, meta| {
        let len = meta.len() as usize;
        if meta.is_file() && len > 0 && len < bytes.len() {
            let got = fs::read(path).unwrap();
            if bytes.starts_with(&got) {
                found.push(path.to_path_buf());
            }
        }
    });
    found
}

fn walk(root: &Path, visit: &mut dyn FnMut(&Path, fs::Metadata)) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else {
                visit(&path, meta);
            }
        }
    }
}

fn manifest() -> Value {
    let bytes = fs::read(corpus().join("byte-expectations.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn file_entry<'a>(manifest: &'a Value, suffix: &str) -> &'a Value {
    manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"].as_str().unwrap().ends_with(suffix))
        .unwrap()
}

fn spawn(args: &[&str]) -> std::process::Child {
    Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn wait_checkpoint(child: &mut std::process::Child, path: &Path) -> Value {
    let start = Instant::now();
    loop {
        if path.is_file() {
            return serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        }
        if let Some(status) = child.try_wait().unwrap() {
            let mut stdout = String::new();
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stdout.take() {
                let _ = std::io::Read::read_to_string(&mut pipe, &mut stdout);
            }
            if let Some(mut pipe) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
            }
            panic!("child exited before checkpoint: {status}\n{stdout}\n{stderr}");
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("checkpoint timeout {}", path.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn kill_child(child: std::process::Child, scenario: &str) -> Out {
    let mut child = child;
    child.kill().unwrap();
    let start = Instant::now();
    let output = child.wait_with_output().unwrap();
    let out = from_output(output, start.elapsed());
    record(scenario, &[], &out);
    assert_eq!(out.signal, Some(9), "expected SIGKILL, got {out:?}");
    assert!(
        out.stdout.trim().is_empty(),
        "killed process printed a receipt:\n{}",
        out.stdout
    );
    out
}

#[test]
fn corpus_bytes_match_independent_expectations() {
    let out = Command::new("python3")
        .arg("synthetic/generate_corpus.py")
        .arg("--check")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let manifest = manifest();
    assert_eq!(manifest["synthetic_only"], true);
    assert_eq!(manifest["t1_must_not_interpret_documents"], true);
    let logical: Value =
        serde_json::from_slice(&fs::read(corpus().join("logical-expectations.json")).unwrap())
            .unwrap();
    assert_eq!(logical["t1_implements_these_judgments"], false);
    let mut hashes = std::collections::BTreeMap::new();
    for entry in manifest["files"].as_array().unwrap() {
        let path = corpus().join(entry["path"].as_str().unwrap());
        let data = fs::read(&path).unwrap();
        assert_eq!(data.len() as u64, entry["byte_length"].as_u64().unwrap());
        assert_eq!(sha256(&path), entry["sha256"].as_str().unwrap());
        hashes.insert(entry["path"].as_str().unwrap().to_string(), sha256(&path));
        if entry["format_role"] == "text-pdf" {
            assert!(data.starts_with(b"%PDF-1.4\n"), "{}", path.display());
            assert!(data.windows(5).any(|w| w == b"%%EOF") || data.ends_with(b"%%EOF\n"));
            assert!(!data.starts_with(b"Northline") && !data.starts_with(b"Paperlantern"));
        }
    }
    assert_eq!(
        hashes["originals/v-2026-09-receipt.pdf"],
        hashes["originals/renamed/NLV-september-statement-copy.pdf"]
    );
    let csv = fs::read(corpus().join("originals/store-export-work.csv")).unwrap();
    assert!(!csv.contains(&b'\r'));
    let csv_text = std::str::from_utf8(&csv).unwrap();
    assert!(csv_text.contains("\"annual plan, store export row\""));
}

#[test]
fn preserved_bytes_survive_source_removal_and_are_not_normalized() {
    let tmp = Tmp::new("bytes");
    vault_init(&tmp);
    let manifest = manifest();
    let canary = file_entry(&manifest, "canary/no-normalize.bin");
    let pdf = file_entry(&manifest, "v-2026-09-receipt.pdf");
    let canary_path = corpus().join(canary["path"].as_str().unwrap());
    let pdf_path = corpus().join(pdf["path"].as_str().unwrap());
    let imported = import(
        &tmp,
        "canary",
        canary["media_type"].as_str().unwrap(),
        &canary_path,
    );
    assert_eq!(imported["content_sha256"], canary["sha256"]);
    assert_eq!(imported["byte_length"], canary["byte_length"]);
    assert!(!imported.to_string().contains("SENTINEL_BODY_9f3c2a"));
    let pdf_import = import(&tmp, "pdf", pdf["media_type"].as_str().unwrap(), &pdf_path);
    assert_eq!(pdf_import["content_sha256"], pdf["sha256"]);
    assert!(!pdf_import.to_string().contains("SYN-V-2026-09-001"));
    assert!(!pdf_import.to_string().contains("Amount paid"));
    let source_id = pdf_import["source_id"].as_str().unwrap().to_string();
    let original = fs::read(&pdf_path).unwrap();
    let copy_src = tmp.path().join("external.pdf");
    fs::copy(&pdf_path, &copy_src).unwrap();
    let second = import(&tmp, "pdf-copy-path", "application/pdf", &copy_src);
    assert_eq!(second["source_id"], pdf_import["source_id"]);
    assert_eq!(second["storage_reused"], true);
    assert_ne!(second["occurrence_id"], pdf_import["occurrence_id"]);
    fs::write(&copy_src, b"replaced external bytes").unwrap();
    fs::remove_file(&copy_src).unwrap();
    let out_path = tmp.path().join("hosted.pdf");
    let read_again = run(
        "read-after-source-gone",
        &[
            "read",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--source-id",
            &source_id,
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    assert_eq!(read_again.code, Some(0), "{}", read_again.stdout);
    assert_eq!(fs::read(&out_path).unwrap(), original);
    let hosted = vault(&tmp).join(format!(
        "objects/{}",
        pdf_import["object_id"].as_str().unwrap()
    ));
    let hosted_meta = fs::metadata(&hosted).unwrap();
    let source_meta = fs::metadata(&pdf_path).unwrap();
    assert_ne!(hosted_meta.ino(), source_meta.ino());
    assert_eq!(hosted_meta.nlink(), 1);
    assert_eq!(files_with_bytes(&vault(&tmp), &original).len(), 1);
    assert_eq!(fs::read(&hosted).unwrap(), original);
    assert!(pdf_path.is_file());
}

#[test]
fn same_bytes_do_not_mint_a_purchase_and_distinct_files_stay_distinct() {
    let tmp = Tmp::new("identity");
    vault_init(&tmp);
    let september = corpus().join("originals/v-2026-09-receipt.pdf");
    let renamed = corpus().join("originals/renamed/NLV-september-statement-copy.pdf");
    let personal = corpus().join("originals/c-personal-2026-invoice.pdf");
    let work = corpus().join("originals/c-work-2026-invoice.pdf");
    let first = import(&tmp, "sept", "application/pdf", &september);
    let again = import(&tmp, "renamed", "application/pdf", &renamed);
    assert_eq!(first["source_id"], again["source_id"]);
    assert_eq!(first["object_id"], again["object_id"]);
    assert_ne!(first["occurrence_id"], again["occurrence_id"]);
    assert_eq!(again["storage_reused"], true);
    assert_eq!(again["display_name"], "NLV-september-statement-copy.pdf");
    assert_ne!(first["display_name"], again["display_name"]);
    assert!(again["purchase_id"].is_null());
    let bytes = fs::read(&september).unwrap();
    assert_eq!(files_with_bytes(&vault(&tmp), &bytes).len(), 1);
    let personal_row = import(&tmp, "personal", "application/pdf", &personal);
    let work_row = import(&tmp, "work", "application/pdf", &work);
    assert_ne!(personal_row["source_id"], work_row["source_id"]);
    assert_ne!(personal_row["content_sha256"], work_row["content_sha256"]);
    let (code, status) = open_vault(&tmp);
    assert_eq!(code, 0);
    assert_eq!(status["sources"].as_array().unwrap().len(), 3);
    for source in status["sources"].as_array().unwrap() {
        assert_eq!(source["arrangement"], "not_arranged");
        assert_eq!(source["searchability"], "not_searchable");
        assert_eq!(source["preservation"], "preserved");
        assert!(source["purchase_id"].is_null());
        assert!(source["object_intact"].as_bool().unwrap());
        let rel = source["object_relpath"].as_str().unwrap();
        assert!(!rel.contains(".."));
        let hosted = fs::read(vault(&tmp).join(rel)).unwrap();
        assert_eq!(sha256(&vault(&tmp).join(rel)), source["content_sha256"]);
        assert_eq!(hosted.len() as u64, source["byte_length"].as_u64().unwrap());
    }
    assert!(status["dangling_references"].as_array().unwrap().is_empty());
    assert!(!status.to_string().contains("Amount paid"));
    assert!(!status.to_string().contains("15.99"));
}

#[test]
fn open_failures_do_not_create_or_reset_a_vault() {
    let missing = Tmp::new("missing").path().join("nope");
    let missing_out = run(
        "open-missing",
        &["open", "--vault", missing.to_str().unwrap()],
    );
    assert_eq!(missing_out.code, Some(10));
    assert_eq!(parse(&missing_out)["outcome"], "NotAVault");
    assert!(!missing.exists());

    let empty = Tmp::new("empty");
    let empty_dir = empty.path().join("empty");
    fs::create_dir(&empty_dir).unwrap();
    let before = fs::read_dir(&empty_dir).unwrap().count();
    let empty_out = run(
        "open-empty",
        &["open", "--vault", empty_dir.to_str().unwrap()],
    );
    assert_eq!(empty_out.code, Some(10));
    assert_eq!(fs::read_dir(&empty_dir).unwrap().count(), before);

    let tmp = Tmp::new("twice");
    vault_init(&tmp);
    let marker = fs::read(vault(&tmp).join("SYNTHETIC_ONLY")).unwrap();
    let again = run(
        "init-twice",
        &["init", "--vault", vault(&tmp).to_str().unwrap()],
    );
    assert_ne!(again.code, Some(0));
    assert_eq!(
        fs::read(vault(&tmp).join("SYNTHETIC_ONLY")).unwrap(),
        marker
    );
    let (code, _) = open_vault(&tmp);
    assert_eq!(code, 0);

    let occupied = Tmp::new("occupied");
    let file = occupied.path().join("not-a-dir");
    fs::write(&file, b"keep me").unwrap();
    let occupied_out = run("init-file", &["init", "--vault", file.to_str().unwrap()]);
    assert_ne!(occupied_out.code, Some(0));
    assert_eq!(fs::read(&file).unwrap(), b"keep me");
}

#[test]
fn corrupt_header_and_newer_version_are_not_replaced() {
    let tmp = Tmp::new("damage-open");
    vault_init(&tmp);
    import(
        &tmp,
        "keep",
        "application/pdf",
        &corpus().join("originals/harbor-notes-2023-invoice.pdf"),
    );
    let copy_corrupt = tmp.path().join("corrupt");
    let copy_version = tmp.path().join("version");
    assert!(Command::new("cp")
        .args([
            "-a",
            vault(&tmp).to_str().unwrap(),
            copy_corrupt.to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("cp")
        .args([
            "-a",
            vault(&tmp).to_str().unwrap(),
            copy_version.to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());

    let db = copy_corrupt.join("vault.db");
    let mut bytes = fs::read(&db).unwrap();
    bytes[..16].copy_from_slice(b"not a database!\0");
    fs::write(&db, &bytes).unwrap();
    let before = sha256(&db);
    let out = run(
        "open-corrupt",
        &["open", "--vault", copy_corrupt.to_str().unwrap()],
    );
    assert_eq!(out.code, Some(13), "{}", out.stdout);
    assert_eq!(parse(&out)["outcome"], "ReadOnlyDamaged");
    assert_eq!(sha256(&db), before);
    assert!(fs::read(&db).unwrap().starts_with(b"not a database!\0"));

    let version_db = copy_version.join("vault.db");
    let sql = Command::new("sqlite3")
        .arg(&version_db)
        .arg("PRAGMA user_version=2;")
        .status()
        .unwrap();
    assert!(sql.success());
    let before_version = fs::read(&version_db).unwrap();
    let vout = run(
        "open-version",
        &["open", "--vault", copy_version.to_str().unwrap()],
    );
    assert_eq!(vout.code, Some(14), "{}", vout.stdout);
    let body = parse(&vout);
    assert_eq!(body["outcome"], "VersionUnsupported");
    assert_eq!(body["schema_version_found"], 2);
    assert_eq!(fs::read(&version_db).unwrap(), before_version);
    let count = Command::new("sqlite3")
        .arg(&version_db)
        .arg("SELECT count(*) FROM sources;")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(count.stdout).unwrap().trim(), "1");
    let pragma = Command::new("sqlite3")
        .arg(&version_db)
        .arg("PRAGMA user_version;")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(pragma.stdout).unwrap().trim(), "2");

    let zero = copy_corrupt.join("vault.db");
    fs::write(&zero, b"").unwrap();
    let zout = run(
        "open-zero",
        &["open", "--vault", copy_corrupt.to_str().unwrap()],
    );
    assert_eq!(zout.code, Some(13));
    assert_eq!(fs::metadata(&zero).unwrap().len(), 0);
}

#[test]
fn hostile_paths_are_not_followed() {
    let tmp = Tmp::new("hostile");
    vault_init(&tmp);
    let imported = import(
        &tmp,
        "can",
        "application/octet-stream",
        &corpus().join("originals/canary/no-normalize.bin"),
    );
    let source_id = imported["source_id"].as_str().unwrap().to_string();
    let object_id = imported["object_id"].as_str().unwrap().to_string();
    let secret = tmp.path().join("secret-outside");
    fs::write(&secret, b"OUTSIDE_SECRET_NOT_OBJECT").unwrap();
    let path_copy = tmp.path().join("path-copy");
    let id_copy = tmp.path().join("id-copy");
    assert!(Command::new("cp")
        .args([
            "-a",
            vault(&tmp).to_str().unwrap(),
            path_copy.to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("cp")
        .args([
            "-a",
            vault(&tmp).to_str().unwrap(),
            id_copy.to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());
    let status = Command::new("sqlite3")
        .arg(path_copy.join("vault.db"))
        .arg(format!(
            "UPDATE objects SET relpath='{}';",
            secret.display()
        ))
        .status()
        .unwrap();
    assert!(status.success());
    let opened = run(
        "open-hostile-relpath",
        &["open", "--vault", path_copy.to_str().unwrap()],
    );
    assert_eq!(opened.code, Some(3), "{}", opened.stdout);
    let body = parse(&opened);
    assert!(body["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["code"] == "object_path_rejected"));
    assert_eq!(body["sources"][0]["object_intact"], true);
    assert!(!opened.stdout.contains("OUTSIDE_SECRET_NOT_OBJECT"));
    assert!(!opened.stderr.contains("OUTSIDE_SECRET_NOT_OBJECT"));
    let out_path = tmp.path().join("read.bin");
    let read = run(
        "read-hostile-relpath",
        &[
            "read",
            "--vault",
            path_copy.to_str().unwrap(),
            "--source-id",
            &source_id,
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    assert_eq!(read.code, Some(0), "{}", read.stdout);
    assert_eq!(
        fs::read(&out_path).unwrap(),
        fs::read(corpus().join("originals/canary/no-normalize.bin")).unwrap()
    );
    assert!(!read.stdout.contains("OUTSIDE_SECRET_NOT_OBJECT"));
    assert_eq!(fs::read(&secret).unwrap(), b"OUTSIDE_SECRET_NOT_OBJECT");

    let sql = format!(
        "UPDATE objects SET object_id='../../secret-outside', relpath='../../secret-outside' WHERE object_id='{object_id}'; UPDATE sources SET object_id='../../secret-outside' WHERE source_id='{source_id}';"
    );
    assert!(Command::new("sqlite3")
        .arg(id_copy.join("vault.db"))
        .arg(sql)
        .status()
        .unwrap()
        .success());
    let bad_read = run(
        "read-hostile-id",
        &[
            "read",
            "--vault",
            id_copy.to_str().unwrap(),
            "--source-id",
            &source_id,
            "--out",
            tmp.path().join("nope.bin").to_str().unwrap(),
        ],
    );
    assert_ne!(bad_read.code, Some(0));
    assert!(!bad_read.stdout.contains("OUTSIDE_SECRET_NOT_OBJECT"));
    assert!(!bad_read.stderr.contains("OUTSIDE_SECRET_NOT_OBJECT"));
    assert!(!tmp.path().join("nope.bin").exists());
    assert_eq!(fs::read(&secret).unwrap(), b"OUTSIDE_SECRET_NOT_OBJECT");
}

#[test]
fn single_writer_ignores_pid_and_expiry_text() {
    let tmp = Tmp::new("lock");
    vault_init(&tmp);
    let lock_path = vault(&tmp).join("writer.lock");
    let original = fs::read_to_string(&lock_path).unwrap();
    assert!(original.contains("not a lock"));
    assert!(!original.contains("pid="));
    assert!(!original.contains("expires="));
    import(
        &tmp,
        "seed",
        "application/pdf",
        &corpus().join("originals/v-2026-10-receipt.pdf"),
    );
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), original);

    fs::write(
        &lock_path,
        format!("pid={}\nexpires=2099-01-01T00:00:00Z\n", std::process::id()),
    )
    .unwrap();
    let despite_text = import(
        &tmp,
        "despite-text",
        "application/pdf",
        &corpus().join("originals/v-2026-10-credit-note.pdf"),
    );
    assert_eq!(despite_text["outcome"], "Committed");
    let leftover = fs::read_to_string(&lock_path).unwrap();
    assert!(leftover.contains("expires=2099-01-01T00:00:00Z"));
    assert!(leftover.contains(&std::process::id().to_string()));

    let checkpoint = tmp.path().join("held.json");
    let mut holder = spawn(&[
        "hold",
        "--vault",
        vault(&tmp).to_str().unwrap(),
        "--stop-at",
        "lock_held",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let point = wait_checkpoint(&mut holder, &checkpoint);
    assert_eq!(point["point"], "lock_held");
    fs::write(&lock_path, "pid=999999\nexpires=1970-01-01T00:00:00Z\n").unwrap();
    let start = Instant::now();
    let locked = run(
        "locked",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "while-held",
            "--media-type",
            "application/pdf",
            "--source",
            corpus()
                .join("originals/lumen-draft-onetime.pdf")
                .to_str()
                .unwrap(),
        ],
    );
    assert!(
        locked.elapsed_ms < 3000,
        "writer call blocked for {}ms; flock is nonblocking and has no expiry wait",
        locked.elapsed_ms
    );
    assert_eq!(locked.code, Some(11), "{}", locked.stdout);
    assert_eq!(parse(&locked)["outcome"], "Locked");
    assert!(start.elapsed() < Duration::from_secs(3));
    kill_child(holder, "kill-holder");
    let after = import(
        &tmp,
        "after-kill",
        "application/pdf",
        &corpus().join("originals/lumen-draft-onetime.pdf"),
    );
    assert_eq!(after["outcome"], "Committed");
    let (code, status) = open_vault(&tmp);
    assert_eq!(code, 0);
    assert!(status["commands"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["command_id"] != "while-held"));
}

#[test]
fn kill_points_keep_old_bytes_and_do_not_report_success() {
    let points = [
        "before_copy",
        "during_copy",
        "after_copy_before_publish",
        "object_ready",
        "before_db_commit",
        "after_db_commit",
        "before_receipt",
    ];
    for point in points {
        let tmp = Tmp::new(point);
        vault_init(&tmp);
        let old = import(
            &tmp,
            "old",
            "application/pdf",
            &corpus().join("originals/v-2026-09-receipt.pdf"),
        );
        let source_id = old["source_id"].as_str().unwrap().to_string();
        let marked = run(
            "marker",
            &[
                "marker",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--command-id",
                "mark",
                "--source-id",
                &source_id,
                "--value",
                "human-edit-v1",
            ],
        );
        assert_eq!(marked.code, Some(0), "{}", marked.stdout);
        let old_bytes = fs::read(corpus().join("originals/v-2026-09-receipt.pdf")).unwrap();
        let new_path = corpus().join("originals/store-export-work.csv");
        let new_bytes = fs::read(&new_path).unwrap();
        let checkpoint = tmp.path().join("checkpoint.json");
        let mut child = spawn(&[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "new",
            "--media-type",
            "text/csv",
            "--source",
            new_path.to_str().unwrap(),
            "--stop-at",
            point,
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ]);
        let reached = wait_checkpoint(&mut child, &checkpoint);
        assert_eq!(reached["point"], point);
        assert_eq!(reached["command_id"], "new");
        let db = reached["db_committed"].as_bool().unwrap();
        let published = reached["published"].as_bool().unwrap();
        if matches!(point, "after_db_commit" | "before_receipt") {
            assert!(db && published);
        } else {
            assert!(!db);
        }
        if point == "during_copy" {
            let written = reached["bytes_written"].as_u64().unwrap();
            let total = reached["byte_length"].as_u64().unwrap();
            assert!(written > 0 && written < total);
            assert!(!published);
        }
        if point == "before_copy" {
            assert_eq!(reached["bytes_written"], 0);
            assert!(!published);
        }
        kill_child(child, &format!("kill-{point}"));
        let copies_before_open = files_with_bytes(&vault(&tmp), &new_bytes);
        let prefixes = proper_prefixes(&vault(&tmp), &new_bytes);
        match point {
            "before_copy" => {
                assert!(copies_before_open.is_empty());
                assert!(prefixes.is_empty());
            }
            "during_copy" => {
                assert!(copies_before_open.is_empty());
                assert!(!prefixes.is_empty(), "partial copy was not on disk");
            }
            "after_copy_before_publish"
            | "object_ready"
            | "before_db_commit"
            | "after_db_commit"
            | "before_receipt" => {
                assert!(
                    !copies_before_open.is_empty(),
                    "{point} did not leave the copied bytes"
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(files_with_bytes(&vault(&tmp), &old_bytes).len(), 1);
        let (code, status) = open_vault(&tmp);
        let new_sources = status["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["content_sha256"] == sha256(&new_path))
            .count();
        let committed = status["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["command_id"] == "new");
        if db {
            assert_eq!(code, 0, "{status}");
            assert!(committed);
            assert_eq!(new_sources, 1);
        } else {
            assert!(!committed, "{status}");
            assert_eq!(new_sources, 0, "{status}");
            let incomplete = status["incomplete_commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["command_id"] == "new")
                .unwrap();
            assert_eq!(incomplete["preservation"], "not_preserved");
            assert!(incomplete["purchase_id"].is_null());
            assert!(incomplete["source_id"].is_null());
        }
        assert!(status["dangling_references"].as_array().unwrap().is_empty());
        let old_source = status["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source_id"] == source_id)
            .unwrap();
        assert_eq!(old_source["user_marker"], "human-edit-v1");
        assert_eq!(old_source["object_intact"], true);
        if point == "during_copy" {
            assert!(proper_prefixes(&vault(&tmp), &new_bytes).is_empty());
        }
        let out_path = tmp.path().join("old.pdf");
        let read = run(
            "read-old",
            &[
                "read",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--source-id",
                &source_id,
                "--out",
                out_path.to_str().unwrap(),
            ],
        );
        assert_eq!(read.code, Some(0), "{}", read.stdout);
        assert_eq!(fs::read(&out_path).unwrap(), old_bytes);

        if matches!(
            point,
            "before_copy"
                | "during_copy"
                | "after_copy_before_publish"
                | "object_ready"
                | "before_db_commit"
        ) {
            let resumed = import(&tmp, "new", "text/csv", &new_path);
            assert_eq!(resumed["outcome"], "Committed");
            assert_eq!(files_with_bytes(&vault(&tmp), &new_bytes).len(), 1);
            let retry = run(
                "retry",
                &[
                    "import",
                    "--vault",
                    vault(&tmp).to_str().unwrap(),
                    "--command-id",
                    "new",
                    "--media-type",
                    "text/csv",
                    "--source",
                    new_path.to_str().unwrap(),
                ],
            );
            assert_eq!(retry.code, Some(0), "{}", retry.stdout);
            let body = parse(&retry);
            assert_eq!(body["outcome"], "AlreadyCommitted");
            assert_eq!(body["new_effect"], false);
            assert_eq!(body["source_id"], resumed["source_id"]);
            let (_, status) = open_vault(&tmp);
            let occurrences = status["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|o| o["command_id"] == "new")
                .count();
            assert_eq!(occurrences, 1);
        }
        if matches!(point, "after_db_commit" | "before_receipt") {
            let retry = run(
                "lost-receipt-retry",
                &[
                    "import",
                    "--vault",
                    vault(&tmp).to_str().unwrap(),
                    "--command-id",
                    "new",
                    "--media-type",
                    "text/csv",
                    "--source",
                    new_path.to_str().unwrap(),
                ],
            );
            assert_eq!(retry.code, Some(0), "{}", retry.stdout);
            let body = parse(&retry);
            assert_eq!(body["outcome"], "AlreadyCommitted");
            assert_eq!(body["new_effect"], false);
            let (_, status) = open_vault(&tmp);
            assert_eq!(
                status["occurrences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|o| o["command_id"] == "new")
                    .count(),
                1
            );
            let again = open_vault(&tmp);
            assert_eq!(status, again.1);
        }
    }
}

#[test]
fn cancel_during_copy_and_after_commit() {
    let tmp = Tmp::new("cancel");
    vault_init(&tmp);
    let old = import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    let old_bytes = fs::read(corpus().join("originals/v-2026-09-receipt.pdf")).unwrap();
    let source = corpus().join("originals/c-work-2026-invoice.pdf");
    let source_bytes = fs::read(&source).unwrap();
    let checkpoint = tmp.path().join("cp.json");
    let mut child = spawn(&[
        "import",
        "--vault",
        vault(&tmp).to_str().unwrap(),
        "--command-id",
        "mid",
        "--media-type",
        "application/pdf",
        "--source",
        source.to_str().unwrap(),
        "--stop-at",
        "during_copy",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let reached = wait_checkpoint(&mut child, &checkpoint);
    assert!(reached["bytes_written"].as_u64().unwrap() < reached["byte_length"].as_u64().unwrap());
    let pending = run(
        "cancel-pending",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "mid",
            "--generation",
            "1",
        ],
    );
    assert_eq!(pending.code, Some(0), "{}", pending.stdout);
    assert_eq!(parse(&pending)["outcome"], "CancelPending");
    assert!(Command::new("kill")
        .args(["-CONT", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let writer = from_output(child.wait_with_output().unwrap(), Duration::ZERO);
    record("cancel-writer", &[], &writer);
    assert_eq!(writer.code, Some(0), "{}", writer.stdout);
    let writer_body = parse(&writer);
    assert_eq!(writer_body["outcome"], "Cancelled");
    assert_eq!(writer_body["preservation"], "not_preserved");
    assert_eq!(writer_body["cancel_applied"], true);
    assert_eq!(
        sha256(&source),
        sha256(&corpus().join("originals/c-work-2026-invoice.pdf"))
    );
    assert!(files_with_bytes(&vault(&tmp), &source_bytes).is_empty());
    let (_, status) = open_vault(&tmp);
    assert!(status["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["source_id"] == old["source_id"]));
    assert_eq!(files_with_bytes(&vault(&tmp), &old_bytes).len(), 1);
    let second = run(
        "cancel-again",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "mid",
            "--generation",
            "1",
        ],
    );
    assert_eq!(second.code, Some(0), "{}", second.stdout);
    assert_eq!(parse(&second)["preservation"], "not_preserved");
    let (_, after_second) = open_vault(&tmp);
    assert_eq!(after_second["sources"].as_array().unwrap().len(), 1);

    let done = run(
        "cancel-committed",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "old",
            "--generation",
            "1",
        ],
    );
    assert_eq!(done.code, Some(0), "{}", done.stdout);
    let done_body = parse(&done);
    assert_eq!(done_body["outcome"], "AlreadyCommitted");
    assert_eq!(done_body["cancel_applied"], false);
    assert_eq!(files_with_bytes(&vault(&tmp), &old_bytes).len(), 1);
    let wrong = run(
        "cancel-generation",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "old",
            "--generation",
            "2",
        ],
    );
    assert_eq!(wrong.code, Some(16));
    assert_eq!(parse(&wrong)["reason"], "generation_mismatch");
    assert_eq!(files_with_bytes(&vault(&tmp), &old_bytes).len(), 1);
    let before_dirs = fs::read_dir(vault(&tmp))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let extraction = run(
        "cancel-extraction",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "old",
            "--generation",
            "1",
            "--kind",
            "extraction",
        ],
    );
    assert_eq!(extraction.code, Some(16), "{}", extraction.stdout);
    assert_eq!(
        parse(&extraction)["reason"],
        "extraction_cancel_not_available"
    );
    let after_dirs = fs::read_dir(vault(&tmp))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(before_dirs, after_dirs);
    assert!(!vault(&tmp).join("extraction").exists());
}

#[test]
fn injected_faults_reach_and_do_not_preserve() {
    let tmp = Tmp::new("faults");
    vault_init(&tmp);
    let old = import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    let source_id = old["source_id"].as_str().unwrap().to_string();
    assert_eq!(
        run(
            "marker",
            &[
                "marker",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--command-id",
                "mark",
                "--source-id",
                &source_id,
                "--value",
                "human-edit-v1",
            ],
        )
        .code,
        Some(0)
    );
    let source = corpus().join("originals/lumen-draft-onetime.pdf");
    let source_bytes = fs::read(&source).unwrap();
    let source_hash = sha256(&source);
    let witness = tmp.path().join("witness.json");
    let failed = run(
        "inject-enospc",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "full",
            "--media-type",
            "application/pdf",
            "--source",
            source.to_str().unwrap(),
            "--inject",
            "enospc:after_staging_fsync",
            "--witness",
            witness.to_str().unwrap(),
        ],
    );
    assert_eq!(failed.code, Some(15), "{}", failed.stdout);
    let body = parse(&failed);
    assert_eq!(body["outcome"], "NotPreserved");
    assert_eq!(body["injected"], true);
    assert_eq!(body["reason"], "injected_enospc");
    assert!(body["note"].as_str().unwrap().contains("injected fault"));
    let wit: Value = serde_json::from_slice(&fs::read(&witness).unwrap()).unwrap();
    assert_eq!(wit["reached"], true);
    assert_eq!(wit["point"], "after_staging_fsync");
    assert_eq!(wit["action"], "enospc");
    assert_eq!(wit["content_sha256"], source_hash);
    assert_eq!(wit["db_committed"], false);
    assert_eq!(wit["published"], false);
    assert!(files_with_bytes(&vault(&tmp), &source_bytes)
        .iter()
        .all(|path| path.starts_with(vault(&tmp).join("staging"))));
    let (_, status) = open_vault(&tmp);
    assert!(status["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["content_sha256"] != source_hash));
    assert!(files_with_bytes(&vault(&tmp), &source_bytes).is_empty());
    let incomplete = status["incomplete_commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["command_id"] == "full")
        .unwrap();
    assert_eq!(incomplete["preservation"], "not_preserved");
    assert_eq!(incomplete["content_sha256"], source_hash);
    let marker = status["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source_id"] == source_id)
        .unwrap();
    assert_eq!(marker["user_marker"], "human-edit-v1");
    let recovered = import(&tmp, "full", "application/pdf", &source);
    assert_eq!(recovered["content_sha256"], source_hash);
    assert_eq!(files_with_bytes(&vault(&tmp), &source_bytes).len(), 1);

    let other = corpus().join("originals/harbor-notes-2023-invoice.pdf");
    let other_bytes = fs::read(&other).unwrap();
    let witness_db = tmp.path().join("db.json");
    let db_fail = run(
        "inject-db",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "dbc",
            "--media-type",
            "application/pdf",
            "--source",
            other.to_str().unwrap(),
            "--inject",
            "io_error:db_commit",
            "--witness",
            witness_db.to_str().unwrap(),
        ],
    );
    assert_eq!(db_fail.code, Some(15), "{}", db_fail.stdout);
    assert_eq!(parse(&db_fail)["injected"], true);
    assert_eq!(parse(&db_fail)["reason"], "injected_io_error");
    assert_eq!(parse(&db_fail)["inject_point"], "db_commit");
    let db_wit: Value = serde_json::from_slice(&fs::read(&witness_db).unwrap()).unwrap();
    assert_eq!(db_wit["point"], "db_commit");
    assert_eq!(db_wit["db_committed"], false);
    assert_eq!(db_wit["content_sha256"], sha256(&other));
    let (_, mid) = open_vault(&tmp);
    assert!(mid["commands"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["command_id"] != "dbc"));
    assert!(mid["unreferenced_objects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["content_sha256"] == sha256(&other)));
    assert_eq!(files_with_bytes(&vault(&tmp), &other_bytes).len(), 1);
    let resumed = import(&tmp, "dbc", "application/pdf", &other);
    assert_eq!(resumed["storage_reused"], true);
    assert_eq!(files_with_bytes(&vault(&tmp), &other_bytes).len(), 1);

    let third = corpus().join("originals/c-cancel-notice-masked.pdf");
    let witness_obj = tmp.path().join("obj.json");
    let obj_fail = run(
        "inject-object",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "obj",
            "--media-type",
            "application/pdf",
            "--source",
            third.to_str().unwrap(),
            "--inject",
            "io_error:object_fsync",
            "--witness",
            witness_obj.to_str().unwrap(),
        ],
    );
    assert_eq!(obj_fail.code, Some(15), "{}", obj_fail.stdout);
    let obj_wit: Value = serde_json::from_slice(&fs::read(&witness_obj).unwrap()).unwrap();
    assert_eq!(obj_wit["point"], "object_fsync");
    assert_eq!(obj_wit["published"], true);
    assert_eq!(obj_wit["content_sha256"], json!(sha256(&third)));
    let third_bytes = fs::read(&third).unwrap();
    let before_open = files_with_bytes(&vault(&tmp), &third_bytes);
    assert!(
        before_open
            .iter()
            .any(|p| p.starts_with(vault(&tmp).join("objects"))),
        "object_fsync removed the copied bytes before the fault returned: {before_open:?}"
    );
    let (_, after_obj) = open_vault(&tmp);
    assert_eq!(files_with_bytes(&vault(&tmp), &third_bytes).len(), 1);
    assert!(after_obj["unreferenced_objects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["content_sha256"] == json!(sha256(&third))));
    assert!(after_obj["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["content_sha256"] != json!(sha256(&third))));
    assert_eq!(
        after_obj["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source_id"] == source_id)
            .unwrap()["user_marker"],
        "human-edit-v1"
    );

    let missed_witness = tmp.path().join("miss.json");
    let missed = run(
        "inject-missed",
        &[
            "marker",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "nope",
            "--source-id",
            &source_id,
            "--value",
            "should-not-apply",
            "--inject",
            "enospc:object_fsync",
            "--witness",
            missed_witness.to_str().unwrap(),
        ],
    );
    assert_eq!(missed.code, Some(17), "{}", missed.stdout);
    assert_eq!(parse(&missed)["outcome"], "FaultNotReached");
    assert!(!missed_witness.exists());
    let (_, still) = open_vault(&tmp);
    assert_eq!(
        still["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source_id"] == source_id)
            .unwrap()["user_marker"],
        "human-edit-v1"
    );
    let unknown = run(
        "inject-unknown",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "unknown",
            "--media-type",
            "application/pdf",
            "--source",
            source.to_str().unwrap(),
            "--inject",
            "enospc:not_a_point",
            "--witness",
            tmp.path().join("unused.json").to_str().unwrap(),
        ],
    );
    assert_eq!(unknown.code, Some(2), "{}", unknown.stdout);
}

#[test]
fn tamper_missing_and_planted_names_do_not_become_purchases() {
    let tmp = Tmp::new("tamper");
    vault_init(&tmp);
    let imported = import(
        &tmp,
        "doc",
        "application/pdf",
        &corpus().join("originals/v-2026-10-receipt.pdf"),
    );
    let source_id = imported["source_id"].as_str().unwrap().to_string();
    assert_eq!(
        run(
            "marker",
            &[
                "marker",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--command-id",
                "mark",
                "--source-id",
                &source_id,
                "--value",
                "human-edit-v1",
            ],
        )
        .code,
        Some(0)
    );
    let (_, clean) = open_vault(&tmp);
    let rel = clean["sources"][0]["object_relpath"].as_str().unwrap();
    let object_path = vault(&tmp).join(rel);
    let mut object_bytes = fs::read(&object_path).unwrap();
    object_bytes[20] ^= 0xff;
    fs::write(&object_path, &object_bytes).unwrap();
    let tampered_hash = sha256(&object_path);
    let (code, status) = open_vault(&tmp);
    assert_eq!(code, 12, "{status}");
    assert_eq!(status["outcome"], "RecoveryRequired");
    assert_eq!(status["sources"][0]["user_marker"], "human-edit-v1");
    assert_eq!(status["sources"][0]["object_intact"], false);
    assert!(status["dangling_references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["reason"] == "hash_mismatch"));
    let missing_out = tmp.path().join("tampered-out");
    let read = run(
        "read-tampered",
        &[
            "read",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--source-id",
            &source_id,
            "--out",
            missing_out.to_str().unwrap(),
        ],
    );
    assert_eq!(read.code, Some(12), "{}", read.stdout);
    assert!(!missing_out.exists());
    let other = import(
        &tmp,
        "other",
        "text/csv",
        &corpus().join("originals/store-export-work.csv"),
    );
    assert_eq!(other["outcome"], "Committed");
    let same = run(
        "reimport-damaged",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "again",
            "--media-type",
            "application/pdf",
            "--source",
            corpus()
                .join("originals/v-2026-10-receipt.pdf")
                .to_str()
                .unwrap(),
        ],
    );
    assert_eq!(same.code, Some(12), "{}", same.stdout);
    assert_eq!(parse(&same)["outcome"], "RecoveryRequired");
    assert_eq!(sha256(&object_path), tampered_hash);
    let (_, still) = open_vault(&tmp);
    assert_eq!(
        still["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["source_id"] == source_id)
            .count(),
        1
    );
    assert_eq!(
        still["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source_id"] == source_id)
            .unwrap()["user_marker"],
        "human-edit-v1"
    );

    fs::remove_file(&object_path).unwrap();
    let (code, missing) = open_vault(&tmp);
    assert_eq!(code, 12);
    assert!(missing["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["code"] == "object_missing"));
    assert_eq!(
        missing["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source_id"] == source_id)
            .unwrap()["user_marker"],
        "human-edit-v1"
    );

    fs::write(
        vault(&tmp).join("objects/purchase-SYN-V"),
        b"not a purchase",
    )
    .unwrap();
    fs::write(
        vault(&tmp).join("journal/purchase-SYN-V.json"),
        br#"{"purchase_id":"purchase-SYN-V","state":"object_ready"}"#,
    )
    .unwrap();
    fs::write(vault(&tmp).join("staging/orphan.partial"), b"temp").unwrap();
    let (code, planted) = open_vault(&tmp);
    assert_eq!(code, 12, "{planted}");
    assert!(planted["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["code"] == "unexpected_object_name" || a["code"] == "journal_invalid"));
    assert_eq!(
        planted["sources"].as_array().unwrap().len(),
        missing["sources"].as_array().unwrap().len()
    );
    assert!(fs::read(vault(&tmp).join("objects/purchase-SYN-V")).unwrap() == b"not a purchase");
    assert!(vault(&tmp).join("journal/purchase-SYN-V.json").is_file());
    assert!(!vault(&tmp).join("staging/orphan.partial").exists());
    let recover1 = run(
        "recover",
        &["recover", "--vault", vault(&tmp).to_str().unwrap()],
    );
    let recover2 = run(
        "recover-again",
        &["recover", "--vault", vault(&tmp).to_str().unwrap()],
    );
    assert_eq!(parse(&recover1), parse(&recover2));
    assert_eq!(
        parse(&recover2)["sources"].as_array().unwrap().len(),
        missing["sources"].as_array().unwrap().len()
    );
    assert!(fs::read(vault(&tmp).join("objects/purchase-SYN-V")).unwrap() == b"not a purchase");
}

#[test]
fn reimport_records_occurrences_and_keeps_marker() {
    let tmp = Tmp::new("reimport");
    vault_init(&tmp);
    let manifest = manifest();
    let mut first_ids = Vec::new();
    for (index, entry) in manifest["files"].as_array().unwrap().iter().enumerate() {
        let path = corpus().join(entry["path"].as_str().unwrap());
        let body = import(
            &tmp,
            &format!("b1-{index}"),
            entry["media_type"].as_str().unwrap(),
            &path,
        );
        assert_eq!(body["content_sha256"], entry["sha256"]);
        first_ids.push(body["source_id"].clone());
        assert!(!body.to_string().contains("Amount paid"));
        assert!(!body.to_string().contains("SENTINEL_BODY_9f3c2a"));
    }
    let work_index = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| {
            f["path"]
                .as_str()
                .unwrap()
                .ends_with("c-work-2026-invoice.pdf")
        })
        .unwrap();
    let work_id = first_ids[work_index].as_str().unwrap().to_string();
    let marked = run(
        "marker",
        &[
            "marker",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "human",
            "--source-id",
            &work_id,
            "--value",
            "human-edit-v1",
        ],
    );
    assert_eq!(marked.code, Some(0), "{}", marked.stdout);
    for (index, entry) in manifest["files"].as_array().unwrap().iter().enumerate() {
        let path = corpus().join(entry["path"].as_str().unwrap());
        let body = import(
            &tmp,
            &format!("b2-{index}"),
            entry["media_type"].as_str().unwrap(),
            &path,
        );
        assert_eq!(body["storage_reused"], true);
        assert_eq!(body["source_id"], first_ids[index]);
        assert!(body["purchase_id"].is_null());
    }
    let (_, status) = open_vault(&tmp);
    let files = manifest["files"].as_array().unwrap().len();
    assert_eq!(status["occurrences"].as_array().unwrap().len(), files * 2);
    let unique_hashes = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["sha256"].as_str().unwrap().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        status["sources"].as_array().unwrap().len(),
        unique_hashes.len()
    );
    assert_eq!(
        status["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source_id"] == work_id)
            .unwrap()["user_marker"],
        "human-edit-v1"
    );
    for source in status["sources"].as_array().unwrap() {
        assert_eq!(source["arrangement"], "not_arranged");
        assert_eq!(source["searchability"], "not_searchable");
        assert!(source["object_intact"].as_bool().unwrap());
    }
    assert!(!status.to_string().contains("Amount paid"));
    assert!(!status.to_string().contains("expired"));
    let september = fs::read(corpus().join("originals/v-2026-09-receipt.pdf")).unwrap();
    assert_eq!(files_with_bytes(&vault(&tmp), &september).len(), 1);
    let conflict = run(
        "payload-conflict",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "b1-0",
            "--media-type",
            manifest["files"][0]["media_type"].as_str().unwrap(),
            "--source",
            corpus()
                .join("originals/lumen-draft-onetime.pdf")
                .to_str()
                .unwrap(),
        ],
    );
    assert_eq!(conflict.code, Some(16), "{}", conflict.stdout);
    assert_eq!(parse(&conflict)["reason"], "payload_conflict");
    let (_, after) = open_vault(&tmp);
    assert_eq!(
        after["occurrences"].as_array().unwrap().len(),
        status["occurrences"].as_array().unwrap().len()
    );
}

#[test]
fn limits_empty_bytes_and_honesty_text() {
    let tmp = Tmp::new("limits");
    vault_init(&tmp);
    let empty = tmp.path().join("empty.bin");
    File::create(&empty).unwrap();
    let imported = import(&tmp, "empty", "application/octet-stream", &empty);
    assert_eq!(
        imported["content_sha256"],
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(imported["byte_length"], 0);
    let big = tmp.path().join("big.bin");
    let file = File::create(&big).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    let refused = run(
        "too-big",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "big",
            "--media-type",
            "application/octet-stream",
            "--source",
            big.to_str().unwrap(),
        ],
    );
    assert_eq!(refused.code, Some(15), "{}", refused.stdout);
    assert_eq!(parse(&refused)["reason"], "object_too_large");
    let mut largest = 0u64;
    walk(&vault(&tmp), &mut |_, meta| {
        largest = largest.max(meta.len());
    });
    assert!(largest < 1024 * 1024, "large file was written: {largest}");
    let help = run("help", &["help"]);
    assert_eq!(help.code, Some(0));
    let help_body = parse(&help);
    assert_honest(&help_body);
    assert!(help_body["note"]
        .as_str()
        .unwrap()
        .contains("not a user-ready product"));
    assert!(help_body["note"]
        .as_str()
        .unwrap()
        .contains("Security is not complete"));
    let version = run("version", &["version"]);
    assert_eq!(parse(&version)["security_complete"], false);
    assert_eq!(parse(&version)["user_ready"], false);
    let banner = fs::read_to_string(vault(&tmp).join("SYNTHETIC_ONLY")).unwrap();
    assert!(banner.contains("SYNTHETIC-ONLY DISPOSABLE VAULT"));
    assert!(banner.contains("not an approved store"));
    assert!(banner.contains("Not a user-ready product"));
    fs::write(vault(&tmp).join("SYNTHETIC_ONLY"), "real data please\n").unwrap();
    let refused_banner = run(
        "not-synthetic",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "no",
            "--media-type",
            "application/pdf",
            "--source",
            corpus()
                .join("originals/v-2026-09-receipt.pdf")
                .to_str()
                .unwrap(),
        ],
    );
    assert_ne!(refused_banner.code, Some(0));
    assert_eq!(
        fs::read_to_string(vault(&tmp).join("SYNTHETIC_ONLY")).unwrap(),
        "real data please\n"
    );
}

#[test]
fn payload_conflict_and_marker_retry_do_not_double_apply() {
    let tmp = Tmp::new("idem");
    vault_init(&tmp);
    let first = import(
        &tmp,
        "c1",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    let source_id = first["source_id"].as_str().unwrap();
    let other_path = tmp.path().join("same.pdf");
    fs::copy(
        corpus().join("originals/v-2026-09-receipt.pdf"),
        &other_path,
    )
    .unwrap();
    let retry = run(
        "same-bytes-new-path",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "c1",
            "--media-type",
            "application/pdf",
            "--source",
            other_path.to_str().unwrap(),
        ],
    );
    assert_eq!(retry.code, Some(0), "{}", retry.stdout);
    assert_eq!(parse(&retry)["outcome"], "AlreadyCommitted");
    assert_eq!(parse(&retry)["occurrence_id"], first["occurrence_id"]);
    let marked = run(
        "marker",
        &[
            "marker",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "m",
            "--source-id",
            source_id,
            "--value",
            "human-edit-v1",
        ],
    );
    assert_eq!(marked.code, Some(0));
    let again = run(
        "marker-retry",
        &[
            "marker",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "m",
            "--source-id",
            source_id,
            "--value",
            "human-edit-v1",
        ],
    );
    assert_eq!(parse(&again)["outcome"], "AlreadyCommitted");
    let clash = run(
        "marker-clash",
        &[
            "marker",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "m",
            "--source-id",
            source_id,
            "--value",
            "other-value",
        ],
    );
    assert_eq!(clash.code, Some(16));
    let clash_body = parse(&clash);
    assert_eq!(clash_body["reason"], "payload_conflict");
    assert_eq!(clash_body["outcome"], "Rejected");
    assert_eq!(clash_body["new_effect"], false);
    assert_eq!(clash_body["preservation"], "preserved");
    assert_eq!(clash_body["object_intact"], true);
    assert_eq!(clash_body["payload_matches"], false);
    assert_eq!(clash_body["source_id"].as_str(), Some(source_id));
    let (_, status) = open_vault(&tmp);
    assert_eq!(status["sources"][0]["user_marker"], "human-edit-v1");
    assert_eq!(status["occurrences"].as_array().unwrap().len(), 1);
    let verify = run(
        "verify",
        &["verify", "--vault", vault(&tmp).to_str().unwrap()],
    );
    assert_eq!(parse(&verify), status);
}

fn object_names(root: &Path) -> Vec<String> {
    let mut names = fs::read_dir(root.join("objects"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn spawn_env(args: &[&str], env: &[(&str, &str)]) -> std::process::Child {
    let mut cmd = Command::new(bin());
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.spawn().unwrap()
}

fn continue_child(child: std::process::Child, scenario: &str) -> Out {
    assert!(Command::new("kill")
        .args(["-CONT", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let out = from_output(child.wait_with_output().unwrap(), Duration::ZERO);
    record(scenario, &[], &out);
    out
}

fn mark_old(tmp: &Tmp, source_id: &str) {
    let marked = run(
        "marker",
        &[
            "marker",
            "--vault",
            vault(tmp).to_str().unwrap(),
            "--command-id",
            "mark",
            "--source-id",
            source_id,
            "--value",
            "human-edit-v1",
        ],
    );
    assert_eq!(marked.code, Some(0), "{}", marked.stdout);
}

fn short_read_library(dir: &Path) -> PathBuf {
    let source = dir.join("shortread.c");
    fs::write(
        &source,
        r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
static ssize_t (*real_read)(int, void *, size_t);
ssize_t read(int fd, void *buf, size_t count) {
    if (!real_read) real_read = (ssize_t (*)(int, void *, size_t))dlsym(RTLD_NEXT, "read");
    char link[64], dest[PATH_MAX];
    snprintf(link, sizeof link, "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, dest, sizeof dest - 1);
    if (n > 0) {
        dest[n] = 0;
        if (strstr(dest, "SHORTREAD-") && count > 1) count = 1;
    }
    return real_read(fd, buf, count);
}
"#,
    )
    .unwrap();
    let library = dir.join("libshortread.so");
    let built = Command::new("gcc")
        .args(["-shared", "-fPIC", "-O2", "-o"])
        .arg(&library)
        .arg(&source)
        .arg("-ldl")
        .status()
        .unwrap();
    assert!(built.success(), "gcc did not build the short-read probe");
    library
}

struct RestoreMode {
    path: PathBuf,
    mode: u32,
}

impl Drop for RestoreMode {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}

#[test]
fn cancel_sees_wal_commit_then_kill() {
    for point in ["after_db_commit", "before_receipt"] {
        let tmp = Tmp::new(point);
        vault_init(&tmp);
        let old = import(
            &tmp,
            "old",
            "application/pdf",
            &corpus().join("originals/v-2026-09-receipt.pdf"),
        );
        let source_id = old["source_id"].as_str().unwrap().to_string();
        mark_old(&tmp, &source_id);
        let source = corpus().join("originals/store-export-work.csv");
        let checkpoint = tmp.path().join("cp.json");
        let mut child = spawn(&[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "new",
            "--media-type",
            "text/csv",
            "--source",
            source.to_str().unwrap(),
            "--stop-at",
            point,
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ]);
        let reached = wait_checkpoint(&mut child, &checkpoint);
        assert_eq!(reached["db_committed"], true);
        let cancel = run(
            "cancel-wal",
            &[
                "cancel",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--command-id",
                "new",
                "--generation",
                "1",
            ],
        );
        assert_eq!(cancel.code, Some(0), "{}", cancel.stdout);
        let body = parse(&cancel);
        assert_eq!(body["outcome"], "AlreadyCommitted");
        assert_eq!(body["cancel_applied"], false);
        assert_eq!(body["preservation"], "preserved");
        assert!(!vault(&tmp).join("control").join("new").exists());
        kill_child(child, "kill-after-cancel");
        let (code, status) = open_vault(&tmp);
        assert_eq!(code, 0, "{status}");
        assert!(status["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command["command_id"] == "new"));
        assert_eq!(
            status["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["command_id"] == "new")
                .count(),
            1
        );
        let kept = status["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["source_id"] == source_id)
            .unwrap();
        assert_eq!(kept["user_marker"], "human-edit-v1");
        assert_eq!(kept["object_intact"], true);
        assert!(status["dangling_references"].as_array().unwrap().is_empty());
    }
}

#[test]
fn during_copy_stops_before_publish_for_short_inputs() {
    let tmp = Tmp::new("short-copy");
    vault_init(&tmp);
    let old = import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    mark_old(&tmp, old["source_id"].as_str().unwrap());
    let before = object_names(&vault(&tmp));

    for (name, bytes, written) in [
        ("empty", &b""[..], 0),
        ("one", &b"Q"[..], 0),
        ("two", &b"AB"[..], 1),
    ] {
        let source = tmp.path().join(name);
        fs::write(&source, bytes).unwrap();
        let checkpoint = tmp.path().join(format!("{name}.json"));
        let _ = fs::remove_file(&checkpoint);
        let mut child = spawn(&[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            name,
            "--media-type",
            "application/octet-stream",
            "--source",
            source.to_str().unwrap(),
            "--stop-at",
            "during_copy",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ]);
        let reached = wait_checkpoint(&mut child, &checkpoint);
        assert_eq!(reached["point"], "during_copy", "{name}");
        assert_eq!(reached["published"], false, "{name}");
        assert_eq!(reached["bytes_written"], written, "{name}");
        assert_eq!(reached["db_committed"], false, "{name}");
        assert_eq!(object_names(&vault(&tmp)), before, "{name}");
        if name == "two" {
            let partial = fs::read(vault(&tmp).join("staging").join("two.partial")).unwrap();
            assert_eq!(partial, b"A");
        }
        kill_child(child, name);
    }

    let one = tmp.path().join("one-finish");
    fs::write(&one, b"Q").unwrap();
    let checkpoint = tmp.path().join("finish.json");
    let mut child = spawn(&[
        "import",
        "--vault",
        vault(&tmp).to_str().unwrap(),
        "--command-id",
        "finish-one",
        "--media-type",
        "application/octet-stream",
        "--source",
        one.to_str().unwrap(),
        "--stop-at",
        "during_copy",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let reached = wait_checkpoint(&mut child, &checkpoint);
    assert_eq!(reached["bytes_written"], 0);
    assert_eq!(object_names(&vault(&tmp)), before);
    let finished = continue_child(child, "finish-one");
    assert_eq!(finished.code, Some(0), "{}", finished.stdout);
    let body = parse(&finished);
    assert_eq!(body["outcome"], "Committed");
    let out = tmp.path().join("q.out");
    let read = run(
        "read-one",
        &[
            "read",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--source-id",
            body["source_id"].as_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(read.code, Some(0), "{}", read.stdout);
    assert_eq!(fs::read(&out).unwrap(), b"Q");
    let before = object_names(&vault(&tmp));

    let library = short_read_library(tmp.path());
    let source = tmp.path().join("SHORTREAD-body.bin");
    let payload = b"abcdefghijklmnopqrstuvwxyz";
    fs::write(&source, payload).unwrap();
    let checkpoint = tmp.path().join("short.json");
    let mut child = spawn_env(
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "short",
            "--media-type",
            "application/octet-stream",
            "--source",
            source.to_str().unwrap(),
            "--stop-at",
            "during_copy",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ],
        &[("LD_PRELOAD", library.to_str().unwrap())],
    );
    let reached = wait_checkpoint(&mut child, &checkpoint);
    assert_eq!(reached["published"], false);
    assert_eq!(reached["bytes_written"], 0);
    assert_eq!(object_names(&vault(&tmp)), before);
    assert_eq!(fs::read(&source).unwrap(), payload);
    let finished = continue_child(child, "short-finish");
    assert_eq!(finished.code, Some(0), "{}", finished.stdout);
    let body = parse(&finished);
    assert_eq!(body["outcome"], "Committed");
    assert_eq!(body["byte_length"], payload.len());
    let out = tmp.path().join("short.out");
    let read = run(
        "read-short",
        &[
            "read",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--source-id",
            body["source_id"].as_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(read.code, Some(0), "{}", read.stdout);
    assert_eq!(fs::read(&out).unwrap(), payload);
    let (_, status) = open_vault(&tmp);
    assert_eq!(
        status["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["source_id"] == old["source_id"])
            .unwrap()["user_marker"],
        "human-edit-v1"
    );
}

#[test]
fn committed_import_replays_without_the_external_file() {
    let tmp = Tmp::new("replay");
    vault_init(&tmp);
    let source = tmp.path().join("copy.pdf");
    fs::copy(corpus().join("originals/v-2026-10-receipt.pdf"), &source).unwrap();
    let first = import(&tmp, "pdf", "application/pdf", &source);
    let occurrences = open_vault(&tmp).1["occurrences"].as_array().unwrap().len();
    fs::remove_file(&source).unwrap();
    let again = run(
        "replay-absent",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "application/pdf",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(again.code, Some(0), "{}", again.stdout);
    let body = parse(&again);
    assert_eq!(body["outcome"], "AlreadyCommitted");
    assert_eq!(body["new_effect"], false);
    assert_eq!(body["source_check"], "absent");
    assert_eq!(body["source_id"], first["source_id"]);
    assert_eq!(
        open_vault(&tmp).1["occurrences"].as_array().unwrap().len(),
        occurrences
    );
    let out = tmp.path().join("hosted.pdf");
    let read = run(
        "read-replay",
        &[
            "read",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--source-id",
            first["source_id"].as_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(read.code, Some(0), "{}", read.stdout);
    assert_eq!(
        fs::read(&out).unwrap(),
        fs::read(corpus().join("originals/v-2026-10-receipt.pdf")).unwrap()
    );
    let other_type = run(
        "replay-absent-other-type",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "text/plain",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(other_type.code, Some(0), "{}", other_type.stdout);
    assert_eq!(parse(&other_type)["outcome"], "AlreadyCommitted");
    assert_eq!(parse(&other_type)["source_check"], "absent");
    assert_eq!(parse(&other_type)["new_effect"], false);

    fs::write(&source, b"different-but-present").unwrap();
    let conflict = run(
        "replay-conflict",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "application/pdf",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(conflict.code, Some(16), "{}", conflict.stdout);
    let conflict_body = parse(&conflict);
    assert_eq!(conflict_body["reason"], "payload_conflict");
    assert_eq!(conflict_body["outcome"], "Rejected");
    assert_eq!(conflict_body["new_effect"], false);
    assert_eq!(conflict_body["preservation"], "preserved");
    assert_eq!(conflict_body["object_intact"], true);
    assert_eq!(conflict_body["payload_matches"], false);
    assert_eq!(conflict_body["source_id"], first["source_id"]);
    assert_eq!(
        open_vault(&tmp).1["occurrences"].as_array().unwrap().len(),
        occurrences
    );

    fs::write(
        &source,
        fs::read(corpus().join("originals/v-2026-10-receipt.pdf")).unwrap(),
    )
    .unwrap();
    let mode = fs::metadata(&source).unwrap().mode();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o0)).unwrap();
    let blocked = run(
        "replay-unreadable",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "application/pdf",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(blocked.code, Some(3), "{}", blocked.stdout);
    let blocked_body = parse(&blocked);
    assert_eq!(blocked_body["outcome"], "ExternalUnverified");
    assert_eq!(blocked_body["reason"], "source_unreadable");
    assert_eq!(blocked_body["source_check"], "unreadable");
    assert_eq!(blocked_body["new_effect"], false);
    assert_eq!(blocked_body["injected"], false);
    assert_eq!(blocked_body["preservation"], "preserved");
    assert_eq!(blocked_body["object_intact"], true);
    assert_eq!(blocked_body["payload_verified"], false);
    assert!(blocked_body["payload_matches"].is_null());
    assert_eq!(blocked_body["journal_label_update"], "not_attempted");
    assert_eq!(blocked_body["source_id"], first["source_id"]);
    assert_eq!(blocked_body["content_sha256"], first["content_sha256"]);
    assert_eq!(blocked_body["media_type"], "application/pdf");
    assert_denied(blocked_body["os_error"].as_str().unwrap());
    let (open_code, open_body) = open_vault(&tmp);
    assert_eq!(open_code, 0, "{open_body}");
    assert_eq!(open_body["outcome"], "Ready");
    assert_eq!(open_body["sources"].as_array().unwrap().len(), 1);
    assert_eq!(open_body["sources"][0]["preservation"], "preserved");
    assert_eq!(open_body["sources"][0]["object_intact"], true);
    assert!(open_body["dangling_references"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        open_body["occurrences"].as_array().unwrap().len(),
        occurrences
    );
    let hosted = vault(&tmp)
        .join("objects")
        .join(first["object_id"].as_str().unwrap());
    assert_eq!(
        fs::read(&hosted).unwrap(),
        fs::read(corpus().join("originals/v-2026-10-receipt.pdf")).unwrap()
    );
    fs::set_permissions(&source, fs::Permissions::from_mode(mode)).unwrap();
    fs::remove_file(&source).unwrap();
    fs::create_dir(&source).unwrap();
    let not_file = run(
        "replay-not-a-file",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "text/plain",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(not_file.code, Some(3), "{}", not_file.stdout);
    let not_file_body = parse(&not_file);
    assert_eq!(not_file_body["outcome"], "ExternalUnverified");
    assert_eq!(not_file_body["reason"], "source_not_a_file");
    assert_eq!(not_file_body["source_check"], "not_a_file");
    assert_eq!(not_file_body["new_effect"], false);
    assert_eq!(not_file_body["preservation"], "preserved");
    assert_eq!(not_file_body["object_intact"], true);
    assert_eq!(not_file_body["payload_verified"], false);
    assert!(not_file_body["payload_matches"].is_null());
    assert_ne!(not_file_body["reason"], "payload_conflict");
    assert_eq!(not_file_body["requested_media_type"], "text/plain");
    assert_eq!(not_file_body["media_type"], "application/pdf");
    assert_eq!(not_file_body["source_id"], first["source_id"]);
    let (after_code, after) = open_vault(&tmp);
    assert_eq!(after_code, 0, "{after}");
    assert_eq!(after["sources"][0]["preservation"], "preserved");
    assert_eq!(after["sources"][0]["object_intact"], true);
    assert_eq!(after["sources"][0]["media_type"], "application/pdf");
    assert_eq!(after["occurrences"].as_array().unwrap().len(), occurrences);
    assert_eq!(
        fs::read(&hosted).unwrap(),
        fs::read(corpus().join("originals/v-2026-10-receipt.pdf")).unwrap()
    );
}

#[test]
fn retry_does_not_repair_a_damaged_hosted_object() {
    let tmp = Tmp::new("hosted");
    vault_init(&tmp);
    let source = corpus().join("originals/v-2026-10-receipt.pdf");
    let first = import(&tmp, "pdf", "application/pdf", &source);
    let object = vault(&tmp)
        .join("objects")
        .join(first["object_id"].as_str().unwrap());
    let original = fs::read(&object).unwrap();
    let mut damaged = original.clone();
    damaged[20] ^= 0xff;
    fs::write(&object, &damaged).unwrap();
    let names = object_names(&vault(&tmp));
    let again = run(
        "retry-tampered",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "application/pdf",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(again.code, Some(12), "{}", again.stdout);
    let body = parse(&again);
    assert_eq!(body["outcome"], "RecoveryRequired");
    assert_eq!(body["new_effect"], false);
    assert_eq!(body["reason"], "hash_mismatch");
    assert_eq!(fs::read(&object).unwrap(), damaged);
    assert_eq!(object_names(&vault(&tmp)), names);

    fs::write(&object, &original).unwrap();
    fs::remove_file(&object).unwrap();
    let missing = run(
        "retry-missing",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "application/pdf",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(missing.code, Some(12), "{}", missing.stdout);
    assert_eq!(parse(&missing)["reason"], "object_missing");
    assert_eq!(parse(&missing)["new_effect"], false);
    assert!(!object.exists());
    assert!(files_with_bytes(&vault(&tmp), &original).is_empty());
}

#[test]
fn post_commit_journal_permission_is_not_not_preserved() {
    let tmp = Tmp::new("journal-mode");
    vault_init(&tmp);
    import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    let source = corpus().join("originals/store-export-work.csv");
    let checkpoint = tmp.path().join("cp.json");
    let mut child = spawn(&[
        "import",
        "--vault",
        vault(&tmp).to_str().unwrap(),
        "--command-id",
        "new",
        "--media-type",
        "text/csv",
        "--source",
        source.to_str().unwrap(),
        "--stop-at",
        "after_db_commit",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let reached = wait_checkpoint(&mut child, &checkpoint);
    assert_eq!(reached["db_committed"], true);
    let journal = vault(&tmp).join("journal");
    let mode = fs::metadata(&journal).unwrap().mode();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o555)).unwrap();
    let _restore = RestoreMode {
        path: journal,
        mode,
    };
    let writer = continue_child(child, "journal-eacces");
    assert_eq!(writer.code, Some(0), "{}", writer.stdout);
    let body = parse(&writer);
    assert_eq!(body["outcome"], "Committed");
    assert_eq!(body["preservation"], "preserved");
    assert_eq!(body["post_commit_io"], true);
    assert_eq!(body["during"], "journal_write");
    assert_eq!(body["injected"], false);
    let os_error = body["os_error"].as_str().unwrap();
    assert!(
        os_error.contains("Permission denied") || os_error.contains("os error 13"),
        "{os_error}"
    );
    drop(_restore);
    let (code, status) = open_vault(&tmp);
    assert_eq!(code, 0, "{status}");
    assert_eq!(
        status["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["command_id"] == "new")
            .count(),
        1
    );
    let retry = run(
        "retry-after-eacces",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "new",
            "--media-type",
            "text/csv",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(retry.code, Some(0), "{}", retry.stdout);
    assert_eq!(parse(&retry)["outcome"], "AlreadyCommitted");
    assert_eq!(parse(&retry)["new_effect"], false);
}

#[test]
fn cancel_does_not_initialize_a_damaged_database() {
    let tmp = Tmp::new("cancel-damage");
    vault_init(&tmp);
    let imported = import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/harbor-notes-2023-invoice.pdf"),
    );
    let object = vault(&tmp)
        .join("objects")
        .join(imported["object_id"].as_str().unwrap());
    let object_bytes = fs::read(&object).unwrap();

    let zero = tmp.path().join("zero");
    assert!(Command::new("cp")
        .args(["-a", vault(&tmp).to_str().unwrap(), zero.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let zero_db = zero.join("vault.db");
    fs::write(&zero_db, b"").unwrap();
    let zero_object = zero
        .join("objects")
        .join(imported["object_id"].as_str().unwrap());
    let before_object = fs::read(&zero_object).unwrap();
    let cancel = run(
        "cancel-zero",
        &[
            "cancel",
            "--vault",
            zero.to_str().unwrap(),
            "--command-id",
            "old",
            "--generation",
            "1",
        ],
    );
    assert_eq!(cancel.code, Some(13), "{}", cancel.stdout);
    assert_eq!(parse(&cancel)["outcome"], "ReadOnlyDamaged");
    assert_eq!(fs::read(&zero_db).unwrap(), b"");
    assert_eq!(fs::metadata(&zero_db).unwrap().len(), 0);
    assert_eq!(fs::read(&zero_object).unwrap(), before_object);

    let bad = tmp.path().join("bad");
    assert!(Command::new("cp")
        .args(["-a", vault(&tmp).to_str().unwrap(), bad.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let bad_db = bad.join("vault.db");
    let mut bytes = fs::read(&bad_db).unwrap();
    bytes[..16].copy_from_slice(b"not a database!\0");
    fs::write(&bad_db, &bytes).unwrap();
    let before = fs::read(&bad_db).unwrap();
    let cancel = run(
        "cancel-bad-header",
        &[
            "cancel",
            "--vault",
            bad.to_str().unwrap(),
            "--command-id",
            "old",
            "--generation",
            "1",
        ],
    );
    assert_eq!(cancel.code, Some(13), "{}", cancel.stdout);
    assert_eq!(fs::read(&bad_db).unwrap(), before);

    let newer = tmp.path().join("newer");
    assert!(Command::new("cp")
        .args(["-a", vault(&tmp).to_str().unwrap(), newer.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let newer_db = newer.join("vault.db");
    assert!(Command::new("sqlite3")
        .arg(&newer_db)
        .arg("PRAGMA user_version=2;")
        .status()
        .unwrap()
        .success());
    let before = fs::read(&newer_db).unwrap();
    let cancel = run(
        "cancel-version",
        &[
            "cancel",
            "--vault",
            newer.to_str().unwrap(),
            "--command-id",
            "old",
            "--generation",
            "1",
        ],
    );
    assert_eq!(cancel.code, Some(14), "{}", cancel.stdout);
    assert_eq!(parse(&cancel)["outcome"], "VersionUnsupported");
    assert_eq!(fs::read(&newer_db).unwrap(), before);
    assert_eq!(fs::read(&object).unwrap(), object_bytes);
}

#[test]
fn non_object_receipt_is_not_a_successful_replay() {
    let tmp = Tmp::new("receipt");
    vault_init(&tmp);
    import(
        &tmp,
        "pdf",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    let db = vault(&tmp).join("vault.db");
    let sources_before = open_vault(&tmp).1["sources"].clone();
    for raw in ["null", "[]", "7", "\"ok\""] {
        assert!(Command::new("sqlite3")
            .arg(&db)
            .arg(format!(
                "UPDATE commands SET result_json = '{raw}' WHERE command_id = 'pdf';"
            ))
            .status()
            .unwrap()
            .success());
        let again = run(
            "non-object",
            &[
                "import",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--command-id",
                "pdf",
                "--media-type",
                "application/pdf",
                "--source",
                corpus()
                    .join("originals/v-2026-09-receipt.pdf")
                    .to_str()
                    .unwrap(),
            ],
        );
        assert_eq!(again.code, Some(12), "{raw} {}", again.stdout);
        assert_eq!(parse(&again)["reason"], "command_result_not_object");
        assert_eq!(parse(&again)["new_effect"], false);
    }
    assert!(Command::new("sqlite3")
        .arg(&db)
        .arg("UPDATE commands SET result_json = '{' WHERE command_id = 'pdf';")
        .status()
        .unwrap()
        .success());
    let broken = run(
        "broken-receipt",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "pdf",
            "--media-type",
            "application/pdf",
            "--source",
            corpus()
                .join("originals/v-2026-09-receipt.pdf")
                .to_str()
                .unwrap(),
        ],
    );
    assert_eq!(broken.code, Some(12), "{}", broken.stdout);
    assert_eq!(parse(&broken)["reason"], "command_result_unreadable");
    let (_, status) = open_vault(&tmp);
    assert_eq!(status["sources"], sources_before);
}

#[test]
fn killed_imports_resume_once_across_recover() {
    for point in ["during_copy", "after_copy_before_publish"] {
        let tmp = Tmp::new(point);
        vault_init(&tmp);
        let old = import(
            &tmp,
            "old",
            "application/pdf",
            &corpus().join("originals/v-2026-09-receipt.pdf"),
        );
        let source_id = old["source_id"].as_str().unwrap().to_string();
        mark_old(&tmp, &source_id);
        let source = corpus().join("originals/store-export-work.csv");
        let source_bytes = fs::read(&source).unwrap();
        for round in 0..2 {
            let checkpoint = tmp.path().join(format!("cp-{round}.json"));
            let mut child = spawn(&[
                "import",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--command-id",
                "new",
                "--media-type",
                "text/csv",
                "--source",
                source.to_str().unwrap(),
                "--stop-at",
                point,
                "--checkpoint",
                checkpoint.to_str().unwrap(),
            ]);
            let reached = wait_checkpoint(&mut child, &checkpoint);
            assert_eq!(reached["db_committed"], false);
            assert_eq!(reached["published"], false);
            kill_child(child, point);
            let recovered = run(
                "recover-between",
                &["recover", "--vault", vault(&tmp).to_str().unwrap()],
            );
            assert_eq!(recovered.code, Some(0), "{}", recovered.stdout);
            let status = parse(&recovered);
            assert!(status["commands"]
                .as_array()
                .unwrap()
                .iter()
                .all(|command| command["command_id"] != "new"));
            assert!(status["dangling_references"].as_array().unwrap().is_empty());
            let kept = status["sources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["source_id"] == source_id)
                .unwrap();
            assert_eq!(kept["user_marker"], "human-edit-v1");
            assert_eq!(kept["object_intact"], true);
        }
        let done = import(&tmp, "new", "text/csv", &source);
        assert_eq!(done["outcome"], "Committed");
        assert_eq!(files_with_bytes(&vault(&tmp), &source_bytes).len(), 1);
        let retry = run(
            "retry-matrix",
            &[
                "import",
                "--vault",
                vault(&tmp).to_str().unwrap(),
                "--command-id",
                "new",
                "--media-type",
                "text/csv",
                "--source",
                source.to_str().unwrap(),
            ],
        );
        assert_eq!(retry.code, Some(0), "{}", retry.stdout);
        assert_eq!(parse(&retry)["outcome"], "AlreadyCommitted");
        assert_eq!(parse(&retry)["new_effect"], false);
        let (_, status) = open_vault(&tmp);
        assert_eq!(
            status["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["command_id"] == "new")
                .count(),
            1
        );
        assert_eq!(
            status["sources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["source_id"] == source_id)
                .unwrap()["user_marker"],
            "human-edit-v1"
        );
        assert!(status["dangling_references"].as_array().unwrap().is_empty());
    }
}

#[test]
fn hosted_history_survives_a_failed_journal_label_and_unreadable_source() {
    let tmp = Tmp::new("label-boundary");
    vault_init(&tmp);
    let old = import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    let source_id = old["source_id"].as_str().unwrap().to_string();
    mark_old(&tmp, &source_id);
    let csv = corpus().join("originals/store-export-work.csv");
    let checkpoint = tmp.path().join("cp.json");
    let mut child = spawn(&[
        "import",
        "--vault",
        vault(&tmp).to_str().unwrap(),
        "--command-id",
        "new",
        "--media-type",
        "text/csv",
        "--source",
        csv.to_str().unwrap(),
        "--stop-at",
        "after_db_commit",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let reached = wait_checkpoint(&mut child, &checkpoint);
    assert_eq!(reached["db_committed"], true);
    let journal_dir = vault(&tmp).join("journal");
    let mode = fs::metadata(&journal_dir).unwrap().mode();
    fs::set_permissions(&journal_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let _restore = RestoreMode {
        path: journal_dir.clone(),
        mode,
    };
    let writer = continue_child(child, "label-eacces");
    assert_eq!(writer.code, Some(0), "{}", writer.stdout);
    let committed = parse(&writer);
    assert_eq!(committed["outcome"], "Committed");
    assert_eq!(committed["preservation"], "preserved");
    assert_eq!(committed["new_effect"], true);
    assert_eq!(committed["post_commit_io"], true);
    assert_eq!(committed["during"], "journal_write");
    assert_eq!(committed["injected"], false);
    assert_denied(committed["os_error"].as_str().unwrap());
    let new_id = committed["object_id"].as_str().unwrap().to_string();
    let old_bytes = fs::read(
        vault(&tmp)
            .join("objects")
            .join(old["object_id"].as_str().unwrap()),
    )
    .unwrap();
    let new_bytes = fs::read(vault(&tmp).join("objects").join(&new_id)).unwrap();
    let journals_before = journal_snapshot(&journal_dir);
    assert!(journals_before["new.json"].contains("object_ready"));

    let first = run(
        "open-label",
        &["open", "--vault", vault(&tmp).to_str().unwrap()],
    );
    let again = run(
        "open-label-again",
        &["open", "--vault", vault(&tmp).to_str().unwrap()],
    );
    let verify = run(
        "verify-label",
        &["verify", "--vault", vault(&tmp).to_str().unwrap()],
    );
    let recover = run(
        "recover-label",
        &["recover", "--vault", vault(&tmp).to_str().unwrap()],
    );
    assert_eq!(first.code, Some(3), "{}", first.stdout);
    assert_eq!(again.stdout, first.stdout);
    assert_eq!(verify.stdout, first.stdout);
    assert_eq!(recover.stdout, first.stdout);
    let status = parse(&first);
    assert_eq!(status["outcome"], "IntegrityAlert");
    assert_eq!(status["new_effect"], false);
    assert_eq!(status["journal_label_update"], "failed");
    assert!(status.get("preservation").is_none(), "{status}");
    assert!(status["dangling_references"].as_array().unwrap().is_empty());
    assert!(status["incomplete_commands"].as_array().unwrap().is_empty());
    let labels: Vec<&Value> = status["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|alert| alert["code"] == "journal_label_not_updated")
        .collect();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0]["command_id"], "new");
    assert_eq!(labels[0]["during"], "journal_write");
    assert_denied(labels[0]["os_error"].as_str().unwrap());
    assert_eq!(status["journal_write_errors"].as_array().unwrap().len(), 1);
    assert_hosted(&status, "old", "application/pdf", "human-edit-v1");
    assert_hosted(&status, "new", "text/csv", "");
    assert!(command_source(&status, "new")["user_marker"].is_null());
    assert_eq!(status["occurrences"].as_array().unwrap().len(), 2);
    assert_eq!(journal_snapshot(&journal_dir), journals_before);
    assert!(journal_dir
        .read_dir()
        .unwrap()
        .all(|entry| { !entry.unwrap().file_name().to_string_lossy().contains("tmp") }));
    assert_eq!(
        fs::read(
            vault(&tmp)
                .join("objects")
                .join(old["object_id"].as_str().unwrap())
        )
        .unwrap(),
        old_bytes
    );
    assert_eq!(
        fs::read(vault(&tmp).join("objects").join(&new_id)).unwrap(),
        new_bytes
    );

    let cancel = run(
        "cancel-label",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "new",
            "--generation",
            "1",
        ],
    );
    assert_eq!(cancel.code, Some(0), "{}", cancel.stdout);
    let cancel_body = parse(&cancel);
    assert_eq!(cancel_body["outcome"], "AlreadyCommitted");
    assert_eq!(cancel_body["cancel_applied"], false);
    assert_eq!(cancel_body["preservation"], "preserved");
    assert_eq!(cancel_body["new_effect"], false);
    assert_eq!(cancel_body["command_id"], "new");
    assert!(!vault(&tmp).join("control").join("new").exists());

    let retry = run(
        "retry-label",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "new",
            "--media-type",
            "text/csv",
            "--source",
            csv.to_str().unwrap(),
        ],
    );
    assert_eq!(retry.code, Some(0), "{}", retry.stdout);
    let retry_body = parse(&retry);
    assert_eq!(retry_body["outcome"], "AlreadyCommitted");
    assert_eq!(retry_body["preservation"], "preserved");
    assert_eq!(retry_body["new_effect"], false);
    assert_eq!(retry_body["source_check"], "matched");
    assert_eq!(retry_body["journal_label_update"], "failed");
    assert_eq!(retry_body["source_id"], committed["source_id"]);
    assert_eq!(retry_body["content_sha256"], committed["content_sha256"]);

    let missed = run(
        "hook-label",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "new",
            "--media-type",
            "text/csv",
            "--source",
            csv.to_str().unwrap(),
            "--stop-at",
            "after_db_commit",
            "--checkpoint",
            tmp.path().join("missed.json").to_str().unwrap(),
        ],
    );
    assert_eq!(missed.code, Some(17), "{}", missed.stdout);
    let missed_body = parse(&missed);
    assert_eq!(missed_body["outcome"], "FaultNotReached");
    assert_eq!(missed_body["reason"], "command_already_committed");
    assert_eq!(missed_body["new_effect"], false);
    assert!(missed_body.get("preservation").is_none(), "{missed_body}");
    assert_eq!(missed_body["journal_label_update"], "failed");

    let blocked_path = tmp.path().join("blocked.pdf");
    fs::copy(
        corpus().join("originals/v-2026-09-receipt.pdf"),
        &blocked_path,
    )
    .unwrap();
    fs::set_permissions(&blocked_path, fs::Permissions::from_mode(0o0)).unwrap();
    let unreadable = run(
        "unreadable-label",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "old",
            "--media-type",
            "text/plain",
            "--source",
            blocked_path.to_str().unwrap(),
        ],
    );
    assert_eq!(unreadable.code, Some(3), "{}", unreadable.stdout);
    let unreadable_body = parse(&unreadable);
    assert_eq!(unreadable_body["outcome"], "ExternalUnverified");
    assert_eq!(unreadable_body["reason"], "source_unreadable");
    assert_eq!(unreadable_body["source_check"], "unreadable");
    assert_ne!(unreadable_body["reason"], "payload_conflict");
    assert_eq!(unreadable_body["new_effect"], false);
    assert_eq!(unreadable_body["preservation"], "preserved");
    assert_eq!(unreadable_body["object_intact"], true);
    assert_eq!(unreadable_body["payload_verified"], false);
    assert!(unreadable_body["payload_matches"].is_null());
    assert_eq!(unreadable_body["requested_media_type"], "text/plain");
    assert_eq!(unreadable_body["media_type"], "application/pdf");
    assert_eq!(unreadable_body["source_id"], old["source_id"]);
    assert_eq!(unreadable_body["content_sha256"], old["content_sha256"]);
    assert_eq!(unreadable_body["journal_label_update"], "failed");
    assert_denied(unreadable_body["os_error"].as_str().unwrap());
    fs::set_permissions(&blocked_path, fs::Permissions::from_mode(0o644)).unwrap();
    fs::remove_file(&blocked_path).unwrap();
    fs::create_dir(&blocked_path).unwrap();
    let not_file = run(
        "directory-label",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "old",
            "--media-type",
            "application/pdf",
            "--source",
            blocked_path.to_str().unwrap(),
        ],
    );
    assert_eq!(not_file.code, Some(3), "{}", not_file.stdout);
    let not_file_body = parse(&not_file);
    assert_eq!(not_file_body["outcome"], "ExternalUnverified");
    assert_eq!(not_file_body["reason"], "source_not_a_file");
    assert_eq!(not_file_body["source_check"], "not_a_file");
    assert_eq!(not_file_body["preservation"], "preserved");
    assert_eq!(not_file_body["object_intact"], true);
    assert_eq!(not_file_body["payload_verified"], false);
    assert!(not_file_body["payload_matches"].is_null());
    assert_eq!(not_file_body["new_effect"], false);
    assert_eq!(not_file_body["journal_label_update"], "failed");

    let fresh = run(
        "fresh-while-unwritable",
        &[
            "import",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "fresh",
            "--media-type",
            "text/csv",
            "--source",
            csv.to_str().unwrap(),
        ],
    );
    assert_eq!(fresh.code, Some(15), "{}", fresh.stdout);
    let fresh_body = parse(&fresh);
    assert_eq!(fresh_body["outcome"], "NotPreserved");
    assert_eq!(fresh_body["reason"], "io_error");
    assert_eq!(fresh_body["during"], "journal_write");
    assert_eq!(fresh_body["new_effect"], false);
    assert_eq!(fresh_body["preservation"], "not_preserved");
    assert_denied(fresh_body["os_error"].as_str().unwrap());

    assert_eq!(journal_snapshot(&journal_dir), journals_before);
    assert_eq!(
        fs::read(
            vault(&tmp)
                .join("objects")
                .join(old["object_id"].as_str().unwrap())
        )
        .unwrap(),
        old_bytes
    );
    assert_eq!(
        fs::read(vault(&tmp).join("objects").join(&new_id)).unwrap(),
        new_bytes
    );
    let (still_code, still) = open_vault(&tmp);
    assert_eq!(still_code, 3, "{still}");
    assert_eq!(still["occurrences"].as_array().unwrap().len(), 2);
    assert!(still["commands"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["command_id"] != "fresh"));
    assert_hosted(&still, "old", "application/pdf", "human-edit-v1");
    assert_eq!(command_source(&still, "new")["preservation"], "preserved");
    assert_eq!(command_source(&still, "new")["object_intact"], true);

    fs::set_permissions(&journal_dir, fs::Permissions::from_mode(mode)).unwrap();
    let (ready_code, ready) = open_vault(&tmp);
    assert_eq!(ready_code, 0, "{ready}");
    assert_eq!(ready["outcome"], "Ready");
    assert!(ready.get("journal_label_update").is_none(), "{ready}");
    assert!(ready["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|alert| alert["code"] != "journal_label_not_updated"));
    assert_hosted(&ready, "old", "application/pdf", "human-edit-v1");
    assert_eq!(command_source(&ready, "new")["preservation"], "preserved");
    assert_eq!(ready["occurrences"].as_array().unwrap().len(), 2);
    let rewritten = fs::read_to_string(journal_dir.join("new.json")).unwrap();
    assert!(rewritten.contains("\"state\": \"committed\""));
    assert!(!rewritten.contains("object_ready"));
    assert_eq!(
        fs::read(
            vault(&tmp)
                .join("objects")
                .join(old["object_id"].as_str().unwrap())
        )
        .unwrap(),
        old_bytes
    );
    assert_eq!(
        fs::read(vault(&tmp).join("objects").join(&new_id)).unwrap(),
        new_bytes
    );
}

#[test]
fn uncommitted_journal_does_not_relabel_a_committed_source() {
    let tmp = Tmp::new("uncommitted-label");
    vault_init(&tmp);
    let old = import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    mark_old(&tmp, old["source_id"].as_str().unwrap());
    let source = tmp.path().join("partial.csv");
    fs::copy(corpus().join("originals/store-export-work.csv"), &source).unwrap();
    let checkpoint = tmp.path().join("cp.json");
    let mut child = spawn(&[
        "import",
        "--vault",
        vault(&tmp).to_str().unwrap(),
        "--command-id",
        "partial",
        "--media-type",
        "text/csv",
        "--source",
        source.to_str().unwrap(),
        "--stop-at",
        "during_copy",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let reached = wait_checkpoint(&mut child, &checkpoint);
    assert_eq!(reached["published"], false);
    assert_eq!(reached["db_committed"], false);
    kill_child(child, "kill-partial");
    let journal_dir = vault(&tmp).join("journal");
    let mode = fs::metadata(&journal_dir).unwrap().mode();
    let before = fs::read(journal_dir.join("partial.json")).unwrap();
    assert!(String::from_utf8_lossy(&before).contains("copying"));
    fs::set_permissions(&journal_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let _restore = RestoreMode {
        path: journal_dir.clone(),
        mode,
    };
    let first = run(
        "open-uncommitted",
        &["open", "--vault", vault(&tmp).to_str().unwrap()],
    );
    let second = run(
        "open-uncommitted-again",
        &["open", "--vault", vault(&tmp).to_str().unwrap()],
    );
    assert_eq!(first.code, Some(0), "{}", first.stdout);
    assert_eq!(second.stdout, first.stdout);
    let status = parse(&first);
    assert_eq!(status["outcome"], "Ready");
    assert!(status.get("journal_label_update").is_none(), "{status}");
    assert!(status.get("preservation").is_none(), "{status}");
    assert_hosted(&status, "old", "application/pdf", "human-edit-v1");
    let incomplete = status["incomplete_commands"].as_array().unwrap();
    assert_eq!(incomplete.len(), 1);
    assert_eq!(incomplete[0]["command_id"], "partial");
    assert_eq!(incomplete[0]["preservation"], "not_preserved");
    assert_eq!(incomplete[0]["state"], "copying");
    assert!(status["commands"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["command_id"] != "partial"));
    assert_eq!(fs::read(journal_dir.join("partial.json")).unwrap(), before);

    let cancel = run(
        "cancel-uncommitted",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "partial",
            "--generation",
            "1",
        ],
    );
    assert_eq!(cancel.code, Some(15), "{}", cancel.stdout);
    let cancel_body = parse(&cancel);
    assert_eq!(cancel_body["outcome"], "NotPreserved");
    assert_eq!(cancel_body["reason"], "io_error");
    assert_eq!(cancel_body["during"], "journal_write");
    assert_eq!(cancel_body["new_effect"], false);
    assert_denied(cancel_body["os_error"].as_str().unwrap());
    assert!(!vault(&tmp).join("control").join("partial").exists());
    assert_eq!(fs::read(journal_dir.join("partial.json")).unwrap(), before);
    let (mid_code, mid) = open_vault(&tmp);
    assert_eq!(mid_code, 0, "{mid}");
    assert_hosted(&mid, "old", "application/pdf", "human-edit-v1");
    assert_eq!(mid["incomplete_commands"][0]["command_id"], "partial");
    assert_eq!(
        mid["incomplete_commands"][0]["preservation"],
        "not_preserved"
    );

    fs::set_permissions(&journal_dir, fs::Permissions::from_mode(mode)).unwrap();
    let (ready_code, ready) = open_vault(&tmp);
    assert_eq!(ready_code, 0, "{ready}");
    assert_eq!(ready["outcome"], "Ready");
    assert_hosted(&ready, "old", "application/pdf", "human-edit-v1");
    assert_eq!(
        ready["incomplete_commands"][0]["preservation"],
        "not_preserved"
    );
    assert_eq!(ready["incomplete_commands"][0]["state"], "copying");
    assert!(status["dangling_references"].as_array().unwrap().is_empty());
}

#[test]
fn unreadable_vault_db_does_not_invent_a_preservation_verdict() {
    let tmp = Tmp::new("db-unreadable");
    vault_init(&tmp);
    let old = import(
        &tmp,
        "old",
        "application/pdf",
        &corpus().join("originals/v-2026-09-receipt.pdf"),
    );
    let object = vault(&tmp)
        .join("objects")
        .join(old["object_id"].as_str().unwrap());
    let bytes = fs::read(&object).unwrap();
    let db = vault(&tmp).join("vault.db");
    let mode = fs::metadata(&db).unwrap().mode();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o0)).unwrap();
    let _restore = RestoreMode {
        path: db.clone(),
        mode,
    };
    let open = run(
        "db-open",
        &["open", "--vault", vault(&tmp).to_str().unwrap()],
    );
    let verify = run(
        "db-verify",
        &["verify", "--vault", vault(&tmp).to_str().unwrap()],
    );
    let recover = run(
        "db-recover",
        &["recover", "--vault", vault(&tmp).to_str().unwrap()],
    );
    assert_eq!(open.code, Some(21), "{}", open.stdout);
    assert_eq!(verify.stdout, open.stdout);
    assert_eq!(recover.stdout, open.stdout);
    let again = run(
        "db-open-again",
        &["open", "--vault", vault(&tmp).to_str().unwrap()],
    );
    assert_eq!(again.stdout, open.stdout);
    let body = parse(&open);
    assert_eq!(body["outcome"], "Uncertain");
    assert_eq!(body["reason"], "vault_unreadable");
    assert_eq!(body["during"], "sqlite_header");
    assert!(body["new_effect"].is_null(), "{body}");
    assert!(body.get("preservation").is_none(), "{body}");
    assert_eq!(body["injected"], false);
    assert_denied(body["os_error"].as_str().unwrap());
    let cancel = run(
        "db-cancel",
        &[
            "cancel",
            "--vault",
            vault(&tmp).to_str().unwrap(),
            "--command-id",
            "old",
            "--generation",
            "1",
        ],
    );
    assert_eq!(cancel.code, Some(21), "{}", cancel.stdout);
    let cancel_body = parse(&cancel);
    assert_eq!(cancel_body["outcome"], "Uncertain");
    assert_eq!(cancel_body["reason"], "vault_unreadable");
    assert!(cancel_body["new_effect"].is_null(), "{cancel_body}");
    assert!(cancel_body.get("preservation").is_none(), "{cancel_body}");
    assert!(!vault(&tmp).join("control").join("old").exists());
    assert_eq!(fs::read(&object).unwrap(), bytes);

    fs::set_permissions(&db, fs::Permissions::from_mode(mode)).unwrap();
    let (code, ready) = open_vault(&tmp);
    assert_eq!(code, 0, "{ready}");
    assert_eq!(ready["outcome"], "Ready");
    assert_eq!(ready["sources"][0]["preservation"], "preserved");
    assert_eq!(ready["sources"][0]["object_intact"], true);
    assert_eq!(ready["sources"][0]["content_sha256"], old["content_sha256"]);
    assert_eq!(fs::read(&object).unwrap(), bytes);
}

fn journal_snapshot(dir: &Path) -> std::collections::BTreeMap<String, String> {
    let mut files = std::collections::BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") {
            files.insert(name, fs::read_to_string(entry.path()).unwrap());
        }
    }
    files
}

fn assert_hosted(status: &Value, command_id: &str, media: &str, marker: &str) {
    let source = command_source(status, command_id);
    assert_eq!(source["preservation"], "preserved", "{source}");
    assert_eq!(source["object_intact"], true, "{source}");
    assert_eq!(source["arrangement"], "not_arranged", "{source}");
    assert_eq!(source["searchability"], "not_searchable", "{source}");
    assert_eq!(source["media_type"], media, "{source}");
    if !marker.is_empty() {
        assert_eq!(source["user_marker"], marker, "{source}");
    }
    assert!(source["purchase_id"].is_null(), "{source}");
    assert!(source["entitlement_id"].is_null(), "{source}");
}
