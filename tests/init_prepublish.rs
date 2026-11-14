//! Behavior of init formats 1, 2, and 3 before rename.
//!
//! Actual: a new process creates and reopens a vault; chmod makes a later
//! `open`/`sync_all` fail; another process's `BEGIN IMMEDIATE` blocks
//! checkpoint; SQL edits change identity or meta; extra or missing names are
//! real directory entries.
//! Injection: `--inject` at `checkpoint_unexpected`, `writer_close`, and
//! `verify_close`. Those receipts are not SQLite close failures and not a
//! checkpoint row SQLite returned.
//! Static row shapes live in `store::checkpoint_contract` and are not rerun here.
//! Nothing here is a power loss or a failed disk.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-prepublish-{label}-{}-{}",
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

    fn finish(mut self) -> (i32, Value) {
        let output = self.child.take().unwrap().wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
            panic!("stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{stderr}")
        });
        assert!(stderr.is_empty(), "stderr:\n{stderr}");
        (output.status.code().unwrap_or(101), body)
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

fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{stderr}")
    });
    assert!(stderr.is_empty(), "stderr:\n{stderr}");
    (output.status.code().unwrap_or(101), body)
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

fn names(dir: &Path) -> Vec<String> {
    let mut found = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    found.sort();
    found
}

fn meta(db: &Path, key: &str) -> String {
    let uri = format!("file:{}?mode=ro&immutable=1", db.display());
    let output = Command::new("sqlite3")
        .arg(uri)
        .arg(format!("SELECT value FROM meta WHERE key='{key}';"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn expected_names() -> Vec<String> {
    [
        "SYNTHETIC_ONLY",
        "control",
        "journal",
        "objects",
        "staging",
        "vault.db",
        "writer.lock",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn assert_exact_init_tree(vault: &Path) {
    assert_eq!(names(vault), expected_names());
    for dir in ["objects", "staging", "journal", "control"] {
        assert_eq!(names(&vault.join(dir)), Vec::<String>::new(), "{dir}");
    }
}

fn assert_unpublished(body: &Value) {
    assert_ne!(body["outcome"], "Ready", "{body}");
    assert_ne!(body["outcome"], "Initialized", "{body}");
    assert_eq!(body["created_new_vault"], false, "{body}");
    assert_ne!(body.get("published"), Some(&json!(true)), "{body}");
    assert_eq!(body["staging_cleanup"], "not_attempted", "{body}");
}

fn stop_init<'a>(tmp: &Tmp, format: &str, point: &str, checkpoint: &'a Path) -> Stopped {
    let vault = tmp.path().join("vault");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        format,
        "--stop-at",
        point,
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), checkpoint);
    assert!(!vault.exists());
    child
}

fn kept_stage(tmp: &Tmp, body: &Value) -> PathBuf {
    let stage = PathBuf::from(body["staging_path"].as_str().unwrap());
    assert_eq!(stage, scratch_dir(tmp.path()));
    assert!(stage.is_dir(), "{body}");
    assert!(!tmp.path().join("vault").exists(), "{body}");
    stage
}

fn sql_rewrite(db: &Path, statement: &str) {
    let output = Command::new("python3")
        .arg("-c")
        .arg(
            r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute(sys.argv[2])
con.commit()
con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
con.close()
"#,
        )
        .arg(db)
        .arg(statement)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "python sqlite rewrite failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!companion(db, "-wal").exists(), "rewrite left -wal");
    assert!(!companion(db, "-shm").exists(), "rewrite left -shm");
}

fn companion(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

#[test]
fn a_new_vault_reopens_with_only_the_init_names() {
    for format in ["1", "2", "3"] {
        let tmp = Tmp::new(&format!("new{format}"));
        let vault = tmp.path().join("vault");
        let (code, body) = run(&[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            format,
        ]);
        assert_eq!(code, 0, "{format}: {body}");
        assert_eq!(body["outcome"], "Initialized", "{body}");
        assert_eq!(
            body["vault_format"],
            format.parse::<u8>().unwrap(),
            "{body}"
        );
        assert_eq!(body["created_new_vault"], true, "{body}");
        assert_eq!(body["user_ready"], false, "{body}");
        assert_eq!(body["slice"], "T1", "{body}");
        assert!(body.get("published").is_none(), "{body}");
        assert!(body.get("staging_cleanup").is_none(), "{body}");
        assert!(body.get("staging_path").is_none(), "{body}");
        assert!(names(tmp.path())
            .iter()
            .all(|name| !name.starts_with(".subvault-init-")));
        assert_exact_init_tree(&vault);
        let db = vault.join("vault.db");
        assert_eq!(meta(&db, "schema_version"), "1");
        assert_eq!(meta(&db, "slice"), "T1");
        assert!(!body["vault_id"].as_str().unwrap().is_empty());
        assert_eq!(meta(&db, "vault_id"), body["vault_id"].as_str().unwrap());
        match format {
            "1" => {
                assert_eq!(meta(&db, "vault_format"), "");
                assert_eq!(meta(&db, "body_schema"), "");
            }
            "2" => {
                assert_eq!(meta(&db, "vault_format"), "2");
                assert_eq!(meta(&db, "body_schema"), "");
            }
            "3" => {
                assert_eq!(meta(&db, "vault_format"), "3");
                assert_eq!(meta(&db, "body_schema"), "1");
            }
            _ => unreachable!(),
        }
        let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
        assert_eq!(code, 0, "{format} reopen: {opened}");
        assert_eq!(opened["outcome"], "Ready", "{opened}");
        assert_eq!(
            opened["vault_format"],
            format.parse::<u8>().unwrap(),
            "{opened}"
        );
        assert_exact_init_tree(&vault);
    }
}

#[test]
fn a_sync_failure_keeps_the_unrenamed_stage() {
    // Actual OS error: the stopped process has not opened the sync fd yet.
    // chmod 000 makes that open fail. This is not an injection and not a power loss.
    let cases = [
        ("before_sync_vault_db", "sync_vault_db", "vault.db"),
        (
            "before_sync_synthetic_only",
            "sync_synthetic_only",
            "SYNTHETIC_ONLY",
        ),
        ("before_sync_writer_lock", "sync_writer_lock", "writer.lock"),
        ("before_sync_objects", "sync_objects", "objects"),
        ("before_sync_staging", "sync_staging", "staging"),
        ("before_sync_journal", "sync_journal", "journal"),
        ("before_sync_control", "sync_control", "control"),
        ("before_sync_stage", "sync_stage", ""),
    ];
    for format in ["1", "2", "3"] {
        for (point, during, name) in cases {
            let tmp = Tmp::new(&format!(
                "sync{format}-{}",
                during.trim_start_matches("sync_")
            ));
            let checkpoint = tmp.path().join("stop.json");
            let mut child = stop_init(&tmp, format, point, &checkpoint);
            let scratch = scratch_dir(tmp.path());
            let target = if name.is_empty() {
                scratch.clone()
            } else {
                scratch.join(name)
            };
            let before = target.is_file().then(|| fs::read(&target).unwrap());
            fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).unwrap();
            resume(child.child().id());
            let (code, body) = child.finish();
            let directory =
                name.is_empty() || matches!(name, "objects" | "staging" | "journal" | "control");
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if directory { 0o755 } else { 0o644 }),
            )
            .unwrap();
            assert_eq!(code, 15, "{format} {during}: {body}");
            assert_eq!(body["outcome"], "NotPreserved", "{body}");
            assert_eq!(body["reason"], "io_error", "{body}");
            assert_eq!(body["during"], during, "{body}");
            assert_eq!(body["injected"], false, "{body}");
            assert_unpublished(&body);
            let stage = kept_stage(&tmp, &body);
            assert_eq!(stage, scratch);
            assert!(target.exists(), "{format} {during}");
            if let Some(bytes) = before {
                assert_eq!(fs::read(&target).unwrap(), bytes, "{format} {during}");
            }
        }
    }
}

#[test]
fn a_blocked_checkpoint_does_not_publish() {
    // Actual SQLite busy: a second process holds BEGIN IMMEDIATE.
    // Not an injection and not a power loss.
    let tmp = Tmp::new("busy");
    let checkpoint = tmp.path().join("stop.json");
    let mut child = stop_init(&tmp, "1", "before_checkpoint", &checkpoint);
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
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["reason"], "checkpoint_busy", "{body}");
    assert_eq!(body["during"], "init_checkpoint", "{body}");
    assert_eq!(body["injected"], false, "{body}");
    assert_ne!(body["checkpoint_busy"], 0, "{body}");
    assert_unpublished(&body);
    let stage = kept_stage(&tmp, &body);
    assert!(stage.join("vault.db").is_file());
}

#[test]
fn an_injected_checkpoint_row_is_not_a_sqlite_return() {
    for format in ["1", "2", "3"] {
        let tmp = Tmp::new(&format!("row{format}"));
        let vault = tmp.path().join("vault");
        let witness = tmp.path().join("witness.json");
        let (code, body) = run(&[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            format,
            "--inject",
            "io_error:checkpoint_unexpected",
            "--witness",
            witness.to_str().unwrap(),
        ]);
        assert_eq!(code, 15, "{format}: {body}");
        assert_eq!(body["reason"], "checkpoint_status_unexpected", "{body}");
        assert_eq!(body["during"], "init_checkpoint", "{body}");
        assert_eq!(body["injected"], true, "{body}");
        assert_eq!(body["inject_point"], "checkpoint_unexpected", "{body}");
        assert_eq!(body["checkpoint_busy"], 0, "{body}");
        assert_eq!(body["checkpoint_log"], 1, "{body}");
        assert_eq!(body["checkpoint_checkpointed"], 0, "{body}");
        assert!(
            body["note"]
                .as_str()
                .unwrap()
                .contains("not a return from SQLite"),
            "{body}"
        );
        assert_unpublished(&body);
        assert!(witness.is_file());
        let stage = kept_stage(&tmp, &body);
        assert!(stage.join("vault.db").is_file());
        fs::remove_dir_all(stage).unwrap();
    }
}

#[test]
fn an_injected_close_does_not_publish() {
    // Injection only. sqlite3_close was not observed to fail.
    for format in ["1", "2", "3"] {
        for (point, during) in [
            ("writer_close", "writer_close"),
            ("verify_close", "verify_close"),
        ] {
            let tmp = Tmp::new(&format!("close{format}-{during}"));
            let vault = tmp.path().join("vault");
            let witness = tmp.path().join("witness.json");
            let (code, body) = run(&[
                "init",
                "--vault",
                vault.to_str().unwrap(),
                "--format",
                format,
                "--inject",
                &format!("io_error:{point}"),
                "--witness",
                witness.to_str().unwrap(),
            ]);
            assert_eq!(code, 15, "{format} {during}: {body}");
            assert_eq!(body["outcome"], "NotPreserved", "{body}");
            assert_eq!(body["reason"], "injected_io_error", "{body}");
            assert_eq!(body["during"], during, "{body}");
            assert_eq!(body["injected"], true, "{body}");
            assert_eq!(body["inject_point"], point, "{body}");
            assert!(body.get("published").is_none(), "{body}");
            assert_unpublished(&body);
            let stage = kept_stage(&tmp, &body);
            assert!(stage.join("vault.db").is_file());
            fs::remove_dir_all(stage).unwrap();
        }
    }
}

#[test]
fn a_different_vault_id_does_not_publish() {
    for format in ["1", "2", "3"] {
        let tmp = Tmp::new(&format!("id{format}"));
        let checkpoint = tmp.path().join("stop.json");
        let mut child = stop_init(&tmp, format, "before_main_verify", &checkpoint);
        let scratch = scratch_dir(tmp.path());
        let db = scratch.join("vault.db");
        sql_rewrite(
            &db,
            "UPDATE meta SET value='not-the-vault' WHERE key='vault_id'",
        );
        resume(child.child().id());
        let (code, body) = child.finish();
        assert_eq!(code, 15, "{format}: {body}");
        assert_eq!(body["reason"], "vault_id_mismatch", "{body}");
        assert_eq!(body["during"], "init_verify", "{body}");
        assert_eq!(body["meta_key"], "vault_id", "{body}");
        assert_eq!(body["published"], false, "{body}");
        assert!(body.get("injected").is_none(), "{body}");
        assert_unpublished(&body);
        let stage = kept_stage(&tmp, &body);
        assert_eq!(meta(&stage.join("vault.db"), "vault_id"), "not-the-vault");
    }
}

#[test]
fn a_format_3_body_schema_mismatch_does_not_publish() {
    let tmp = Tmp::new("body");
    let checkpoint = tmp.path().join("stop.json");
    let mut child = stop_init(&tmp, "3", "before_main_verify", &checkpoint);
    let scratch = scratch_dir(tmp.path());
    sql_rewrite(
        &scratch.join("vault.db"),
        "UPDATE meta SET value='9' WHERE key='body_schema'",
    );
    resume(child.child().id());
    let (code, body) = child.finish();
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["reason"], "body_schema_mismatch", "{body}");
    assert_eq!(body["during"], "init_verify", "{body}");
    assert_eq!(body["published"], false, "{body}");
    assert_unpublished(&body);
    let stage = kept_stage(&tmp, &body);
    assert_eq!(meta(&stage.join("vault.db"), "body_schema"), "9");
}

#[test]
fn a_changed_meta_value_does_not_publish() {
    let tmp = Tmp::new("meta");
    let checkpoint = tmp.path().join("stop.json");
    let mut child = stop_init(&tmp, "1", "before_main_verify", &checkpoint);
    let scratch = scratch_dir(tmp.path());
    sql_rewrite(
        &scratch.join("vault.db"),
        "UPDATE meta SET value='other' WHERE key='protection'",
    );
    resume(child.child().id());
    let (code, body) = child.finish();
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["reason"], "meta_mismatch", "{body}");
    assert_eq!(body["during"], "init_verify", "{body}");
    assert_eq!(body["meta_key"], "protection", "{body}");
    assert_eq!(body["published"], false, "{body}");
    assert_unpublished(&body);
    kept_stage(&tmp, &body);
}

#[test]
fn an_extra_stage_name_stays_and_blocks_publish() {
    // Actual directory entries planted while stopped. The product must not delete them.
    for format in ["1", "2", "3"] {
        for name in ["vault.db-wal", "vault.db-shm", "stray"] {
            let tmp = Tmp::new(&format!("extra{format}-{name}"));
            let checkpoint = tmp.path().join("stop.json");
            let mut child = stop_init(&tmp, format, "before_final_set", &checkpoint);
            let scratch = scratch_dir(tmp.path());
            let planted = scratch.join(name);
            fs::write(&planted, b"ONLY-MINE\n").unwrap();
            resume(child.child().id());
            let (code, body) = child.finish();
            assert_eq!(code, 15, "{format} {name}: {body}");
            assert_eq!(body["reason"], "stage_set_mismatch", "{body}");
            assert_eq!(body["during"], "final_set", "{body}");
            assert_eq!(body["published"], false, "{body}");
            assert_eq!(body["injected"], false, "{body}");
            assert_unpublished(&body);
            assert!(
                body["unexpected"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(name)),
                "{body}"
            );
            let stage = kept_stage(&tmp, &body);
            assert_eq!(fs::read(stage.join(name)).unwrap(), b"ONLY-MINE\n");
        }
    }
}

#[test]
fn a_missing_init_file_blocks_publish() {
    let tmp = Tmp::new("missing");
    let checkpoint = tmp.path().join("stop.json");
    let mut child = stop_init(&tmp, "2", "before_final_set", &checkpoint);
    let scratch = scratch_dir(tmp.path());
    fs::remove_file(scratch.join("SYNTHETIC_ONLY")).unwrap();
    resume(child.child().id());
    let (code, body) = child.finish();
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["reason"], "stage_set_mismatch", "{body}");
    assert_eq!(body["during"], "final_set", "{body}");
    assert_eq!(body["published"], false, "{body}");
    assert_unpublished(&body);
    assert!(body["missing"]
        .as_array()
        .unwrap()
        .contains(&json!("SYNTHETIC_ONLY")));
    let stage = kept_stage(&tmp, &body);
    assert!(!stage.join("SYNTHETIC_ONLY").exists());
}
