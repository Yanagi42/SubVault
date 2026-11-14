use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

fn old_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let src = PathBuf::from("/tmp/subvault-cc0ee1e");
        if !src.join("Cargo.toml").is_file() {
            let status = Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "--detach",
                    "/tmp/subvault-cc0ee1e",
                    "cc0ee1eae9182c75d2d6a1dd8ef07afd1c77588a",
                ])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("git worktree");
            assert!(status.success(), "old worktree was not created");
        }
        let status = Command::new("cargo")
            .args([
                "build",
                "--offline",
                "--locked",
                "--manifest-path",
                "/tmp/subvault-cc0ee1e/Cargo.toml",
                "--target-dir",
                "/tmp/subvault-cc0ee1e-target",
            ])
            .status()
            .expect("old cargo");
        assert!(status.success(), "old cc0ee1e binary did not build");
        PathBuf::from("/tmp/subvault-cc0ee1e-target/debug/subvault")
    })
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-format-{label}-{}-{}",
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

struct Stopped {
    child: Option<std::process::Child>,
}

impl Stopped {
    fn spawn(args: &[&str]) -> Self {
        let child = Command::new(bin())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self { child: Some(child) }
    }

    fn child(&mut self) -> &mut std::process::Child {
        self.child.as_mut().unwrap()
    }

    fn reap(mut self) {
        let output = self.child.take().unwrap().wait_with_output().unwrap();
        assert!(
            !output.status.success(),
            "a killed process must not report success"
        );
    }

    fn finish(mut self) -> (i32, Value) {
        let output = self.child.take().unwrap().wait_with_output().unwrap();
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
}

impl Drop for Stopped {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = Command::new("kill")
                .args(["-KILL", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }
}

fn run_at(program: &Path, args: &[&str]) -> (i32, Value) {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
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

fn run(args: &[&str]) -> (i32, Value) {
    run_at(Path::new(bin()), args)
}

fn wait_stopped(child: &mut std::process::Child, checkpoint: &Path) {
    let start = Instant::now();
    loop {
        if checkpoint.is_file() {
            break;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before checkpoint: {status}");
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("checkpoint timeout");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before stop: {status}");
        }
        if stopped(child.id()) {
            return;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("pid {} was not observed stopped", child.id());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stopped(pid: u32) -> bool {
    let Ok(text) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return false;
    };
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next() == Some("State:") && parts.next() == Some("T")
    })
}

fn resume(pid: u32) {
    assert!(stopped(pid), "refusing SIGCONT before State T");
    assert!(Command::new("kill")
        .args(["-CONT", &pid.to_string()])
        .status()
        .unwrap()
        .success());
}

fn kill_stopped(child: &mut std::process::Child) {
    let pid = child.id();
    assert!(
        stopped(pid),
        "SIGKILL in this test is not a power-loss or hardware fsync proof"
    );
    assert!(Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap()
        .success());
}

fn header_user_version(db: &Path) -> u32 {
    let bytes = fs::read(db).unwrap();
    assert!(bytes.len() >= 64, "database header is short");
    u32::from_be_bytes(bytes[60..64].try_into().unwrap())
}

fn sql_uri(db: &Path, query: &str, statement: &str) -> String {
    let uri = format!("file:{}?{query}", db.display());
    let output = Command::new("sqlite3")
        .arg(&uri)
        .arg(statement)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn sql(vault: &Path, statement: &str) -> String {
    let output = Command::new("sqlite3")
        .arg(vault.join("vault.db"))
        .arg(statement)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn unfinished_init(parent: &Path, body: &Value) -> PathBuf {
    assert_eq!(body["staging_cleanup"], "not_attempted", "{body}");
    assert!(
        body["staging_cleanup_reason"]
            .as_str()
            .unwrap()
            .contains("automatic path deletion is disabled"),
        "{body}"
    );
    assert!(body.get("staging_residue").is_none(), "{body}");
    assert!(body.get("staging_cleanup_error").is_none(), "{body}");
    let path = PathBuf::from(body["staging_path"].as_str().unwrap());
    assert!(path.starts_with(parent), "{}", path.display());
    assert!(path.is_dir(), "{}", path.display());
    path
}

fn scratch_dir(parent: &Path) -> PathBuf {
    let mut found = Vec::new();
    for entry in fs::read_dir(parent).unwrap() {
        let entry = entry.unwrap();
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".subvault-init-")
        {
            found.push(entry.path());
        }
    }
    assert_eq!(found.len(), 1, "{found:?}");
    found.pop().unwrap()
}

fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(current) = dirs.pop() {
        let mut entries: Vec<_> = fs::read_dir(&current)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                dirs.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn vault_files(vault: &Path) -> Vec<(String, Vec<u8>)> {
    let mut all = Vec::new();
    for name in ["vault.db", "vault.db-wal"] {
        let path = vault.join(name);
        if path.is_file() {
            all.push((name.to_string(), fs::read(path).unwrap()));
        }
    }
    for name in ["objects", "journal", "staging", "control"] {
        for (rel, bytes) in tree(&vault.join(name)) {
            all.push((format!("{name}/{rel}"), bytes));
        }
    }
    all
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn note(tmp: &Tmp, name: &str, bytes: &[u8]) -> PathBuf {
    let path = tmp.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn format_1_keeps_t1_and_refuses_the_body_write() {
    let tmp = Tmp::new("v1");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], 1);
    assert_eq!(body["slice"], "T1");
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);

    let refused = tmp.path().join("refused");
    let (code, body) = run(&[
        "init",
        "--vault",
        refused.to_str().unwrap(),
        "--format",
        "1",
        "--stop-at",
        "version_set",
        "--checkpoint",
        tmp.path().join("unused.json").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "format_1_has_no_version_hook");
    assert!(!refused.exists());

    let source = note(&tmp, "plain.txt", b"synthetic-v1\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep",
        "--media-type",
        "text/plain",
        "--source",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    let source_id = imported["source_id"].as_str().unwrap().to_string();
    let out = tmp.path().join("out.bin");
    let (code, read) = run(&[
        "read",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &source_id,
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{read}");
    assert_eq!(fs::read(&out).unwrap(), b"synthetic-v1\n");
    let (code, marker) = run(&[
        "marker",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "mark",
        "--source-id",
        &source_id,
        "--value",
        "synthetic-note",
    ]);
    assert_eq!(code, 0, "{marker}");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["slice"], "T1");
    assert_eq!(opened["vault_format"], 1);
    let (code, verified) = run(&["verify", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{verified}");

    let before = vault_files(&vault);
    let fragment = note(&tmp, "body.txt", b"Amount paid USD 2.00\n");
    let (code, refused) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "body",
        "--fragment",
        fragment.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["outcome"], "NeedsUpgrade");
    assert_eq!(refused["reason"], "needs_upgrade");
    assert_eq!(refused["slice"], "T2a-body");
    assert_eq!(refused["new_effect"], false);
    assert!(refused.get("preservation").is_none());
    let (code, cancel) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "body",
        "--generation",
        "1",
        "--kind",
        "fragment",
    ]);
    assert_eq!(code, 14, "{cancel}");
    assert_eq!(cancel["outcome"], "NeedsUpgrade");
    assert_eq!(cancel["cancel_applied"], false);
    assert_eq!(cancel["slice"], "T2a-body");
    assert_eq!(vault_files(&vault), before);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM sqlite_master WHERE name='candidates';"
        ),
        "0"
    );
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
    let again = note(&tmp, "again.txt", b"synthetic-v1-again\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep-again",
        "--media-type",
        "text/plain",
        "--source",
        again.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
}

#[test]
fn an_unpublished_main_database_does_not_receive_a_fragment() {
    let tmp = Tmp::new("wal");
    let vault = tmp.path().join("vault");
    let checkpoint = tmp.path().join("stop.json");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "2",
        "--stop-at",
        "version_set",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    assert!(!vault.exists());
    let scratch = scratch_dir(tmp.path());
    let db = scratch.join("vault.db");
    let header = header_user_version(&db);
    assert_ne!(
        header, 2,
        "main database already shows 2 before the publish checkpoint"
    );
    let wal_version = sql_uri(&db, "mode=ro", "PRAGMA user_version;");
    assert_eq!(wal_version, "2");
    let journals = tree(&scratch.join("journal"));
    let fragment = note(&tmp, "body.txt", b"Amount paid USD 2.00\n");
    let (code, refused) = run(&[
        "fragment",
        "--vault",
        scratch.to_str().unwrap(),
        "--command-id",
        "early",
        "--fragment",
        fragment.to_str().unwrap(),
    ]);
    assert_ne!(code, 0, "{refused}");
    assert_eq!(tree(&scratch.join("journal")), journals);
    assert!(!vault.exists());
    assert_eq!(header_user_version(&db), header);
    kill_stopped(child.child());
    child.reap();
    assert!(!vault.exists());
}

#[test]
fn checkpoint_or_verify_failure_does_not_publish() {
    let tmp = Tmp::new("fail");
    let vault = tmp.path().join("vault");
    let witness = tmp.path().join("witness.json");
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "2",
        "--inject",
        "enospc:version_checkpoint",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["injected"], true);
    assert_eq!(body["inject_point"], "version_checkpoint");
    assert_eq!(body["created_new_vault"], false);
    assert_eq!(body["outcome"], "NotPreserved");
    assert_eq!(body["reason"], "injected_enospc");
    assert!(!vault.exists());
    let left = unfinished_init(tmp.path(), &body);
    assert!(left.join("vault.db").is_file());
    fs::remove_dir_all(&left).unwrap();
    assert!(witness.is_file());

    let witness = tmp.path().join("verify.json");
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "2",
        "--inject",
        "io_error:version_verify",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["inject_point"], "version_verify");
    assert_eq!(body["outcome"], "NotPreserved");
    assert_eq!(body["reason"], "injected_io_error");
    assert_eq!(body["created_new_vault"], false);
    assert!(!vault.exists());
    let left = unfinished_init(tmp.path(), &body);
    assert!(left.join("vault.db").is_file());
    fs::remove_dir_all(&left).unwrap();

    let checkpoint = tmp.path().join("checked.json");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "2",
        "--stop-at",
        "version_checkpoint",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    assert!(!vault.exists());
    let scratch = scratch_dir(tmp.path());
    let db = scratch.join("vault.db");
    assert_eq!(header_user_version(&db), 2);
    let mut file = OpenOptions::new().write(true).open(&db).unwrap();
    file.seek(SeekFrom::Start(60)).unwrap();
    file.write_all(&1u32.to_be_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert_eq!(header_user_version(&db), 1);
    resume(child.child().id());
    let (code, body) = child.finish();
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "version_not_in_main_db");
    assert_eq!(body["published"], false);
    assert_eq!(body["created_new_vault"], false);
    assert!(!vault.exists());
    assert!(scratch.is_dir());
    assert_eq!(body["staging_path"], scratch.display().to_string());
    assert_eq!(unfinished_init(tmp.path(), &body), scratch);
    fs::remove_dir_all(&scratch).unwrap();

    let checkpoint = tmp.path().join("busy.json");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "2",
        "--stop-at",
        "version_set",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    let scratch = scratch_dir(tmp.path());
    let ready = tmp.path().join("holder.txt");
    let mut holder = Command::new("python3")
        .arg("-c")
        .arg(
            r#"
import sqlite3, sys, time
try:
    con = sqlite3.connect(sys.argv[1], timeout=0)
    con.execute("BEGIN IMMEDIATE")
    open(sys.argv[2], "w").write("ready")
    time.sleep(30)
except Exception as exc:
    open(sys.argv[2], "w").write("fail:" + str(exc))
"#,
        )
        .arg(scratch.join("vault.db"))
        .arg(&ready)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if ready.is_file() {
            break;
        }
        if let Some(status) = holder.try_wait().unwrap() {
            panic!("holder exited: {status}");
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = holder.kill();
            panic!("holder did not lock the scratch database");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let held = fs::read_to_string(&ready).unwrap();
    assert_eq!(held, "ready", "{held}");
    resume(child.child().id());
    let (code, body) = child.finish();
    let _ = holder.kill();
    let _ = holder.wait();
    assert_ne!(code, 0, "{body}");
    assert_eq!(body["created_new_vault"], false);
    assert!(
        body["reason"] == "checkpoint_busy" || body["during"] == "init_checkpoint",
        "{body}"
    );
    assert!(!vault.exists());
}

#[test]
fn publish_points_leave_the_main_database_visible() {
    let tmp = Tmp::new("publish");
    let vault = tmp.path().join("vault");

    let checkpoint = tmp.path().join("before.json");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "2",
        "--stop-at",
        "before_publish",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    assert!(!vault.exists());
    let scratch = scratch_dir(tmp.path());
    assert_eq!(header_user_version(&scratch.join("vault.db")), 2);
    assert_eq!(
        sql_uri(
            &scratch.join("vault.db"),
            "mode=ro&immutable=1",
            "PRAGMA user_version;"
        ),
        "2"
    );
    kill_stopped(child.child());
    child.reap();
    assert!(!vault.exists());

    let checkpoint = tmp.path().join("after.json");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "2",
        "--stop-at",
        "after_publish",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    assert!(vault.is_dir());
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert_eq!(
        sql_uri(
            &vault.join("vault.db"),
            "mode=ro&immutable=1",
            "SELECT value FROM meta WHERE key='vault_format';"
        ),
        "2"
    );
    let (code, seen) = run_at(old_bin(), &["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 14, "{seen}");
    assert_eq!(seen["schema_version_found"], 2);
    kill_stopped(child.child());
    child.reap();
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    let (code, seen) = run_at(old_bin(), &["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 14, "{seen}");
    assert_eq!(seen["outcome"], "VersionUnsupported");
}

#[test]
fn old_program_returns_14_and_leaves_the_vault_unchanged() {
    let tmp = Tmp::new("old");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], 2);
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert_eq!(
        sql_uri(
            &vault.join("vault.db"),
            "mode=ro&immutable=1",
            "PRAGMA user_version;"
        ),
        "2"
    );
    let output = tmp.path().join("already-out.bin");
    fs::write(&output, b"already-there").unwrap();
    let source = note(&tmp, "plain.txt", b"synthetic-old\n");
    let source_id = "src_00000000000000000000000000000000";
    let old = old_bin();
    let hold = tmp.path().join("hold.json");
    let commands: Vec<Vec<&str>> = vec![
        vec!["open", "--vault", vault.to_str().unwrap()],
        vec!["verify", "--vault", vault.to_str().unwrap()],
        vec!["recover", "--vault", vault.to_str().unwrap()],
        vec![
            "import",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "old-import",
            "--media-type",
            "text/plain",
            "--source",
            source.to_str().unwrap(),
        ],
        vec![
            "read",
            "--vault",
            vault.to_str().unwrap(),
            "--source-id",
            source_id,
            "--out",
            output.to_str().unwrap(),
        ],
        vec![
            "marker",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "old-marker",
            "--source-id",
            source_id,
            "--value",
            "synthetic-note",
        ],
        vec![
            "cancel",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "old-cancel",
            "--generation",
            "1",
        ],
        vec![
            "hold",
            "--vault",
            vault.to_str().unwrap(),
            "--stop-at",
            "lock_held",
            "--checkpoint",
            hold.to_str().unwrap(),
        ],
    ];
    for args in commands {
        let before = vault_files(&vault);
        let (code, body) = run_at(old, &args);
        assert_eq!(code, 14, "{} {body}", args[0]);
        assert_eq!(body["outcome"], "VersionUnsupported");
        assert_eq!(body["schema_version_found"], 2);
        assert_eq!(vault_files(&vault), before, "{}", args[0]);
        assert_eq!(fs::read(&output).unwrap(), b"already-there");
        assert!(!tmp.path().join("hold.json").exists());
    }
    let before = vault_files(&vault);
    let (code, body) = run_at(old, &["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(vault_files(&vault), before);
    assert_eq!(fs::read(&source).unwrap(), b"synthetic-old\n");

    let copy = tmp.path().join("unknown");
    copy_tree(&vault, &copy);
    let status = Command::new("sqlite3")
        .arg(copy.join("vault.db"))
        .arg("PRAGMA user_version=3; PRAGMA wal_checkpoint(TRUNCATE);")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(header_user_version(&copy.join("vault.db")), 3);
    let journals = tree(&copy.join("journal"));
    let (code, body) = run(&["open", "--vault", copy.to_str().unwrap()]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "vault_format_unverified");
    assert_eq!(body["schema_version_found"], 3);
    assert_eq!(
        sql(&copy, "SELECT value FROM meta WHERE key='vault_format';"),
        "2"
    );
    assert_eq!(header_user_version(&copy.join("vault.db")), 3);
    assert_eq!(sql(&copy, "PRAGMA user_version;"), "3");
    assert_eq!(tree(&copy.join("journal")), journals);

    let bare = tmp.path().join("bare");
    copy_tree(&vault, &bare);
    sql(&bare, "DELETE FROM meta WHERE key='vault_format';");
    let (code, body) = run(&["open", "--vault", bare.to_str().unwrap()]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "vault_format_unverified");
    assert_eq!(header_user_version(&bare.join("vault.db")), 2);
    assert_eq!(
        sql(&bare, "SELECT count(*) FROM meta WHERE key='vault_format';"),
        "0"
    );
}

#[test]
fn a_main_database_at_1_with_wal_2_gets_no_fragment() {
    let tmp = Tmp::new("split");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
    let ready = tmp.path().join("holder.txt");
    let mut holder = Command::new("python3")
        .arg("-c")
        .arg(
            r#"
import sqlite3, sys, time
con = sqlite3.connect(sys.argv[1])
con.execute("PRAGMA wal_autocheckpoint=0")
con.execute("PRAGMA user_version=2")
open(sys.argv[2], "w").write("ready")
time.sleep(30)
"#,
        )
        .arg(vault.join("vault.db"))
        .arg(&ready)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if ready.is_file() {
            break;
        }
        if let Some(status) = holder.try_wait().unwrap() {
            panic!("holder exited: {status}");
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = holder.kill();
            panic!("holder did not set the WAL version");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
    assert_eq!(
        sql_uri(&vault.join("vault.db"), "mode=ro", "PRAGMA user_version;"),
        "2"
    );
    let journals = tree(&vault.join("journal"));
    let fragment = note(&tmp, "body.txt", b"Amount paid USD 2.00\n");
    let (code, refused) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "split",
        "--fragment",
        fragment.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["outcome"], "VersionUnsupported");
    assert_eq!(refused["reason"], "format_inconsistent");
    assert_eq!(refused["header_user_version"], 1);
    assert_eq!(refused["schema_version_found"], 2);
    assert_eq!(tree(&vault.join("journal")), journals);
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
    let _ = holder.kill();
    let _ = holder.wait();
}

#[test]
fn format_2_body_succeeds_cancels_rolls_back_and_reopens() {
    let tmp = Tmp::new("v2");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    let rolled = note(&tmp, "rolled.txt", b"Amount paid USD 4.00\n");
    let witness = tmp.path().join("witness.json");
    let (code, failed) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "rolled",
        "--fragment",
        rolled.to_str().unwrap(),
        "--inject",
        "enospc:body_schema",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 15, "{failed}");
    assert_eq!(failed["preservation"], "not_preserved");
    assert_eq!(failed["injected"], true);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM sqlite_master WHERE name='candidates';"
        ),
        "0"
    );
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["slice"], "T1");
    assert_eq!(opened["vault_format"], 2);
    let (code, again) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "rolled",
        "--fragment",
        rolled.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{again}");
    assert_eq!(again["candidate_count"], 1);
    assert_eq!(again["slice"], "T2a-body");
    let paid = note(&tmp, "paid.txt", b"Amount paid USD 2.00\n");
    let (code, published) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "paid",
        "--fragment",
        paid.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{published}");
    assert_eq!(published["slice"], "T2a-body");
    assert_eq!(published["candidate_count"], 1);
    assert_eq!(published["preservation"], "preserved");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["slice"], "T1");
    assert_eq!(opened["vault_format"], 2);
    assert_eq!(opened["outcome"], "Ready");

    let late = note(&tmp, "late.txt", b"Amount paid USD 8.00\n");
    let checkpoint = tmp.path().join("late.json");
    let mut child = Stopped::spawn(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "late",
        "--fragment",
        late.to_str().unwrap(),
        "--stop-at",
        "before_publish",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    let (code, cancel) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "late",
        "--generation",
        "1",
        "--kind",
        "fragment",
    ]);
    assert_eq!(code, 0, "{cancel}");
    assert_eq!(cancel["outcome"], "CancelPending");
    resume(child.child().id());
    let (code, writer) = child.finish();
    assert_eq!(code, 0, "{writer}");
    assert_eq!(writer["outcome"], "Late");
    assert_eq!(writer["candidates_published"], false);
    let (code, retried) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "late",
        "--fragment",
        late.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{retried}");
    assert_eq!(retried["candidate_count"], 1);

    let early = note(&tmp, "early.txt", b"Amount paid USD 5.00\n");
    let checkpoint = tmp.path().join("journal.json");
    let journals = tree(&vault.join("journal"));
    let mut child = Stopped::spawn(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "early",
        "--fragment",
        early.to_str().unwrap(),
        "--stop-at",
        "before_journal",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    assert!(!vault.join("journal").join("early.json").exists());
    assert_eq!(tree(&vault.join("journal")), journals);
    kill_stopped(child.child());
    child.reap();
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert!(!vault.join("journal").join("early.json").exists());

    let crash = note(&tmp, "crash.txt", b"Amount paid USD 3.00\n");
    let checkpoint = tmp.path().join("crash.json");
    let candidates_before = sql(&vault, "SELECT count(*) FROM candidates;");
    let mut child = Stopped::spawn(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "crash",
        "--fragment",
        crash.to_str().unwrap(),
        "--stop-at",
        "before_publish",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    kill_stopped(child.child());
    child.reap();
    assert_eq!(
        sql(&vault, "SELECT count(*) FROM candidates;"),
        candidates_before
    );
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    let (code, seen) = run_at(old_bin(), &["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 14, "{seen}");
    assert_eq!(seen["schema_version_found"], 2);
    let (code, recovered) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "crash",
        "--fragment",
        crash.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{recovered}");
    assert_eq!(recovered["candidate_count"], 1);
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["vault_format"], 2);
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
}

#[test]
fn a_real_wal_stays_usable_and_a_split_header_is_not_checkpointed() {
    let tmp = Tmp::new("gate");
    let vault = tmp.path().join("v1");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    let source = note(&tmp, "v1.txt", b"synthetic-v1\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep",
        "--media-type",
        "text/plain",
        "--source",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    leave_wal_frames(&vault.join("vault.db"), "PRAGMA user_version=1;");
    assert!(vault.join("vault.db-wal").metadata().unwrap().len() > 32);
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
    assert_eq!(ro(&vault, "PRAGMA user_version;"), "1");
    assert_eq!(
        ro(&vault, "SELECT value FROM meta WHERE key='schema_version';"),
        "1"
    );
    assert_eq!(
        ro(
            &vault,
            "SELECT count(*) FROM meta WHERE key='vault_format';"
        ),
        "0"
    );
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    assert_eq!(opened["vault_format"], 1);
    assert_eq!(opened["slice"], "T1");
    let again = note(&tmp, "v1-again.txt", b"synthetic-v1-again\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep-again",
        "--media-type",
        "text/plain",
        "--source",
        again.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    let source_id = imported["source_id"].as_str().unwrap();
    let out = tmp.path().join("v1-out.bin");
    let (code, read) = run(&[
        "read",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        source_id,
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{read}");
    assert_eq!(fs::read(&out).unwrap(), b"synthetic-v1-again\n");
    let (code, marked) = run(&[
        "marker",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "mark",
        "--source-id",
        source_id,
        "--value",
        "synthetic-note",
    ]);
    assert_eq!(code, 0, "{marked}");
    let (code, listed) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{listed}");
    assert_eq!(listed["candidates"].as_array().unwrap().len(), 0);
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);

    let checkpoint = tmp.path().join("v1-copy.json");
    let copying = note(&tmp, "v1-copy.txt", b"synthetic-v1-copy\n");
    let mut child = Stopped::spawn(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "copying",
        "--media-type",
        "text/plain",
        "--source",
        copying.to_str().unwrap(),
        "--stop-at",
        "during_copy",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    let (code, queried) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{queried}");
    assert_ne!(queried["outcome"], "Locked");
    let (code, pending) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "copying",
        "--generation",
        "1",
    ]);
    assert_eq!(code, 0, "{pending}");
    assert_eq!(pending["outcome"], "CancelPending");
    assert_eq!(
        fs::read_to_string(vault.join("control").join("copying")).unwrap(),
        "cancel\ngeneration=1\nkind=preservation\n"
    );
    kill_stopped(child.child());
    child.reap();

    let vault = tmp.path().join("v2");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    leave_wal_frames(&vault.join("vault.db"), "PRAGMA user_version=2;");
    assert!(vault.join("vault.db-wal").metadata().unwrap().len() > 32);
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert_eq!(ro(&vault, "PRAGMA user_version;"), "2");
    assert_eq!(
        ro(&vault, "SELECT value FROM meta WHERE key='vault_format';"),
        "2"
    );
    assert_eq!(
        ro(&vault, "SELECT value FROM meta WHERE key='schema_version';"),
        "1"
    );
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    assert_eq!(opened["vault_format"], 2);
    let fragment = note(&tmp, "paid.txt", b"Amount paid USD 2.00\n");
    let (code, published) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "paid",
        "--fragment",
        fragment.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{published}");
    assert_eq!(published["candidate_count"], 1);
    assert_eq!(published["preservation"], "preserved");
    let (code, listed) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{listed}");
    assert_eq!(listed["candidates"].as_array().unwrap().len(), 1);
    let (code, late) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "paid",
        "--generation",
        "1",
    ]);
    assert_eq!(code, 0, "{late}");
    assert_eq!(late["outcome"], "AlreadyCommitted");
    assert_eq!(late["preservation"], "preserved");
    let (code, fragment_late) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "paid",
        "--generation",
        "1",
        "--kind",
        "fragment",
    ]);
    assert_eq!(code, 0, "{fragment_late}");
    assert_eq!(fragment_late["outcome"], "AlreadyCommitted");
    assert_eq!(fragment_late["cancel_applied"], false);
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert_eq!(
        ro(&vault, "SELECT value FROM meta WHERE key='schema_version';"),
        "1"
    );

    let checkpoint = tmp.path().join("v2-late.json");
    let late_file = note(&tmp, "late.txt", b"Amount paid USD 3.00\n");
    let mut child = Stopped::spawn(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "late",
        "--fragment",
        late_file.to_str().unwrap(),
        "--stop-at",
        "before_publish",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    let (code, queried) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{queried}");
    assert_eq!(queried["candidates"].as_array().unwrap().len(), 1);
    let (code, wrong) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "late",
        "--generation",
        "1",
    ]);
    assert_eq!(code, 16, "{wrong}");
    assert_eq!(wrong["reason"], "command_kind_conflict");
    assert!(!vault.join("control").join("late").exists());
    let (code, pending) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "late",
        "--generation",
        "1",
        "--kind",
        "fragment",
    ]);
    assert_eq!(code, 0, "{pending}");
    assert_eq!(pending["outcome"], "CancelPending");
    assert_eq!(
        fs::read_to_string(vault.join("control").join("late")).unwrap(),
        "cancel\ngeneration=1\nkind=fragment\n"
    );
    kill_stopped(child.child());
    child.reap();

    let vault = tmp.path().join("split");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    let source = note(&tmp, "split.txt", b"synthetic-split\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep",
        "--media-type",
        "text/plain",
        "--source",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    fs::write(vault.join("staging").join("keep.bin"), b"stage").unwrap();
    fs::write(vault.join("journal").join(".keep.json.tmp"), b"tmp").unwrap();
    leave_wal_frames(&vault.join("vault.db"), "PRAGMA user_version=2;");
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
    assert_eq!(ro(&vault, "PRAGMA user_version;"), "2");
    assert_eq!(
        ro(
            &vault,
            "SELECT count(*) FROM meta WHERE key='vault_format';"
        ),
        "0"
    );
    let sealed = vault_files(&vault);
    let lock_bytes = fs::read(vault.join("writer.lock")).unwrap();
    let sources = ro(&vault, "SELECT count(*) FROM sources;");
    for command in ["open", "verify", "recover"] {
        let (code, body) = run(&[command, "--vault", vault.to_str().unwrap()]);
        assert_eq!(code, 14, "{command} {body}");
        assert_eq!(body["reason"], "format_inconsistent");
        assert_eq!(body["outcome"], "VersionUnsupported");
        assert_eq!(body["schema_version_found"], 2);
        assert_eq!(body["header_user_version"], 1);
        assert!(body["vault_format_seen"].is_null(), "{body}");
        assert_eq!(body["new_effect"], false);
    }
    for args in [
        vec![
            "cancel",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "keep",
            "--generation",
            "1",
        ],
        vec![
            "cancel",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "keep",
            "--generation",
            "1",
            "--kind",
            "fragment",
        ],
    ] {
        let (code, body) = run(&args);
        assert_eq!(code, 14, "{} {body}", args[5]);
        assert_eq!(body["reason"], "format_inconsistent");
        assert_ne!(body["outcome"], "AlreadyCommitted");
        assert!(!vault.join("control").join("keep").exists());
    }
    assert_eq!(vault_files(&vault), sealed);
    assert_eq!(fs::read(vault.join("writer.lock")).unwrap(), lock_bytes);
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);
    assert_eq!(ro(&vault, "PRAGMA user_version;"), "2");
    assert_eq!(ro(&vault, "SELECT count(*) FROM sources;"), sources);
    assert_eq!(
        fs::read(vault.join("staging").join("keep.bin")).unwrap(),
        b"stage"
    );
    assert_eq!(
        fs::read(vault.join("journal").join(".keep.json.tmp")).unwrap(),
        b"tmp"
    );
    let _shm_is_not_a_domain_write = vault.join("vault.db-shm").exists();

    let _held = hold_flock(&vault.join("writer.lock"));
    let (code, queried) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 14, "{queried}");
    assert_eq!(queried["reason"], "format_inconsistent");
    assert_ne!(code, 11);
    let (code, blocked) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep",
        "--generation",
        "1",
    ]);
    assert_eq!(code, 14, "{blocked}");
    assert_eq!(blocked["reason"], "format_inconsistent");
    assert_ne!(blocked["outcome"], "AlreadyCommitted");
    let (code, blocked) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep",
        "--generation",
        "1",
        "--kind",
        "fragment",
    ]);
    assert_eq!(code, 14, "{blocked}");
    assert_eq!(blocked["reason"], "format_inconsistent");
    assert_eq!(blocked["slice"], "T1");
    assert!(!vault.join("control").join("keep").exists());
    assert_eq!(vault_files(&vault), sealed);
    assert_eq!(fs::read(vault.join("writer.lock")).unwrap(), lock_bytes);
    drop(_held);

    let vault = tmp.path().join("unknown");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    let source = note(&tmp, "unknown.txt", b"synthetic-unknown\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep",
        "--media-type",
        "text/plain",
        "--source",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    leave_wal_frames(&vault.join("vault.db"), "PRAGMA user_version=4;");
    let sealed = vault_files(&vault);
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert_eq!(ro(&vault, "PRAGMA user_version;"), "4");
    let (code, body) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "newer_schema");
    assert_eq!(body["schema_version_found"], 4);
    assert_eq!(vault_files(&vault), sealed);
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert_eq!(ro(&vault, "PRAGMA user_version;"), "4");
    assert_eq!(ro(&vault, "SELECT count(*) FROM sources;"), "1");

    let vault = tmp.path().join("meta");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    let source = note(&tmp, "meta.txt", b"synthetic-meta\n");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "keep",
        "--media-type",
        "text/plain",
        "--source",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    fs::write(vault.join("staging").join("keep.bin"), b"stage").unwrap();
    leave_wal_frames(
        &vault.join("vault.db"),
        "UPDATE meta SET value='9' WHERE key='vault_format';",
    );
    let sealed = vault_files(&vault);
    assert_eq!(header_user_version(&vault.join("vault.db")), 2);
    assert_eq!(ro(&vault, "PRAGMA user_version;"), "2");
    assert_eq!(
        ro(&vault, "SELECT value FROM meta WHERE key='vault_format';"),
        "9"
    );
    assert_eq!(
        sql_uri(
            &vault.join("vault.db"),
            "mode=ro&immutable=1",
            "SELECT value FROM meta WHERE key='vault_format';"
        ),
        "2"
    );
    assert_eq!(
        ro(&vault, "SELECT value FROM meta WHERE key='schema_version';"),
        "1"
    );
    let (code, body) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "vault_format_unverified");
    assert_eq!(vault_files(&vault), sealed);
    assert_eq!(
        ro(&vault, "SELECT value FROM meta WHERE key='vault_format';"),
        "9"
    );
    assert_eq!(
        sql_uri(
            &vault.join("vault.db"),
            "mode=ro&immutable=1",
            "SELECT value FROM meta WHERE key='vault_format';"
        ),
        "2"
    );
    assert_eq!(
        fs::read(vault.join("staging").join("keep.bin")).unwrap(),
        b"stage"
    );
    assert_eq!(ro(&vault, "SELECT count(*) FROM sources;"), "1");
}

fn ro(vault: &Path, statement: &str) -> String {
    sql_uri(&vault.join("vault.db"), "mode=ro", statement)
}

fn leave_wal_frames(db: &Path, sql: &str) {
    let ready = std::env::temp_dir().join(format!(
        "subvault-wal-ready-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(
            r#"
import sqlite3, sys, time
db, ready, sql = sys.argv[1:]
con = sqlite3.connect(db)
con.execute("PRAGMA wal_autocheckpoint=0")
con.executescript(sql)
con.commit()
open(ready, "w").write("ready")
time.sleep(30)
"#,
        )
        .arg(db)
        .arg(&ready)
        .arg(sql)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if ready.is_file() {
            break;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("external wal helper exited before its frames were visible: {status}");
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            panic!("external wal helper did not commit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        Command::new("kill")
            .args(["-KILL", &child.id().to_string()])
            .status()
            .unwrap()
            .success(),
        "stopping the helper keeps it from checkpointing on close; this is not a power-loss proof"
    );
    let _ = child.wait();
    let _ = fs::remove_file(&ready);
}

struct Flock {
    child: Option<std::process::Child>,
}

impl Flock {
    fn hold(lock: &Path) -> Self {
        let ready = std::env::temp_dir().join(format!(
            "subvault-flock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut child = Command::new("python3")
            .arg("-c")
            .arg(
                r#"
import fcntl, sys, time
handle = open(sys.argv[1], "a+")
fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
open(sys.argv[2], "w").write("ready")
time.sleep(30)
"#,
            )
            .arg(lock)
            .arg(&ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        loop {
            if ready.is_file() {
                break;
            }
            if let Some(status) = child.try_wait().unwrap() {
                panic!("flock helper exited: {status}");
            }
            if start.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                panic!("flock helper did not take the lock");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = fs::remove_file(&ready);
        Self { child: Some(child) }
    }
}

impl Drop for Flock {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = Command::new("kill")
                .args(["-KILL", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }
}

fn hold_flock(lock: &Path) -> Flock {
    Flock::hold(lock)
}
