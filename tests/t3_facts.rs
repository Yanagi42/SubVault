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

fn baseline_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let src = PathBuf::from("/tmp/subvault-42364745");
        if !src.join("Cargo.toml").is_file() {
            let status = Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "--detach",
                    "/tmp/subvault-42364745",
                    "42364745aa00910c3ec47ee4fd63c3dac831e11a",
                ])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("git worktree");
            assert!(status.success(), "baseline worktree was not created");
        }
        let status = Command::new("cargo")
            .args([
                "build",
                "--offline",
                "--locked",
                "--manifest-path",
                "/tmp/subvault-42364745/Cargo.toml",
                "--target-dir",
                "/tmp/subvault-42364745-target",
            ])
            .env(
                "SUBVAULT_GIT_SHA",
                "42364745aa00910c3ec47ee4fd63c3dac831e11a",
            )
            .status()
            .expect("baseline cargo");
        assert!(status.success(), "baseline 42364745 binary did not build");
        PathBuf::from("/tmp/subvault-42364745-target/debug/subvault")
    })
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-t3-{label}-{}-{}",
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
    for name in ["vault.db", "vault.db-wal", "vault.db-shm"] {
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
    all.sort();
    all
}

fn note(tmp: &Tmp, name: &str, bytes: &[u8]) -> PathBuf {
    let path = tmp.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn format_3_publishes_only_after_the_main_header_matches() {
    let tmp = Tmp::new("init3");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], 1);
    assert_eq!(body["slice"], "T1");
    assert_eq!(header_user_version(&vault.join("vault.db")), 1);

    let v2 = tmp.path().join("v2");
    let (code, body) = run(&["init", "--vault", v2.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], 2);
    assert_eq!(header_user_version(&v2.join("vault.db")), 2);
    assert_eq!(
        sql(&v2, "SELECT value FROM meta WHERE key='schema_version';"),
        "1"
    );
    assert_eq!(
        sql(
            &v2,
            "SELECT count(*) FROM sqlite_master WHERE name='facts';"
        ),
        "0"
    );

    let unpublished = tmp.path().join("unpublished");
    let witness = tmp.path().join("witness.json");
    let (code, body) = run(&[
        "init",
        "--vault",
        unpublished.to_str().unwrap(),
        "--format",
        "3",
        "--inject",
        "enospc:version_checkpoint",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["injected"], true);
    assert_eq!(body["inject_point"], "version_checkpoint");
    assert_eq!(body["reason"], "injected_enospc");
    assert_eq!(body["outcome"], "NotPreserved");
    assert_eq!(body["created_new_vault"], false);
    assert_eq!(body["staging_cleanup"], "not_attempted");
    assert!(!unpublished.exists());
    let left = PathBuf::from(body["staging_path"].as_str().unwrap());
    assert!(left.join("vault.db").is_file(), "{body}");
    fs::remove_dir_all(&left).unwrap();

    let vault3 = tmp.path().join("vault3");
    let checkpoint = tmp.path().join("before.json");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault3.to_str().unwrap(),
        "--format",
        "3",
        "--stop-at",
        "before_publish",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    assert!(!vault3.exists());
    let scratch = scratch_dir(tmp.path());
    let db = scratch.join("vault.db");
    assert_eq!(header_user_version(&db), 3);
    assert_eq!(
        sql_uri(&db, "mode=ro&immutable=1", "PRAGMA user_version;"),
        "3"
    );
    assert_eq!(
        sql_uri(
            &db,
            "mode=ro&immutable=1",
            "SELECT value FROM meta WHERE key='vault_format';"
        ),
        "3"
    );
    assert_eq!(
        sql_uri(
            &db,
            "mode=ro&immutable=1",
            "SELECT value FROM meta WHERE key='schema_version';"
        ),
        "1"
    );
    kill_stopped(child.child());
    child.reap();
    assert!(
        !vault3.exists(),
        "SIGKILL before rename is not a published vault and is not a power-loss proof"
    );
    let _ = fs::remove_dir_all(&scratch);

    let checkpoint = tmp.path().join("verify.json");
    let mut child = Stopped::spawn(&[
        "init",
        "--vault",
        vault3.to_str().unwrap(),
        "--format",
        "3",
        "--stop-at",
        "version_checkpoint",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    wait_stopped(child.child(), &checkpoint);
    let scratch = scratch_dir(tmp.path());
    let db = scratch.join("vault.db");
    assert_eq!(header_user_version(&db), 3);
    let mut file = OpenOptions::new().write(true).open(&db).unwrap();
    file.seek(SeekFrom::Start(60)).unwrap();
    file.write_all(&2u32.to_be_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    resume(child.child().id());
    let (code, body) = child.finish();
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "version_not_in_main_db");
    assert_eq!(body["published"], false);
    assert_eq!(body["created_new_vault"], false);
    assert_eq!(body["staging_cleanup"], "not_attempted");
    assert!(!vault3.exists());
    let left = PathBuf::from(body["staging_path"].as_str().unwrap());
    assert!(left.is_dir(), "{body}");
    assert_eq!(left, scratch);

    let (code, body) = run(&["init", "--vault", vault3.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], 3);
    assert_eq!(body["created_new_vault"], true);
    assert_eq!(body["slice"], "T1");
    assert_eq!(body["user_ready"], false);
    assert_eq!(header_user_version(&vault3.join("vault.db")), 3);
    assert_eq!(
        sql_uri(
            &vault3.join("vault.db"),
            "mode=ro&immutable=1",
            "PRAGMA user_version;"
        ),
        "3"
    );
    assert_eq!(
        sql(&vault3, "SELECT value FROM meta WHERE key='vault_format';"),
        "3"
    );
    assert_eq!(
        sql(
            &vault3,
            "SELECT value FROM meta WHERE key='schema_version';"
        ),
        "1"
    );
    let (code, opened) = run(&["open", "--vault", vault3.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["slice"], "T1");
    assert_eq!(opened["vault_format"], 3);

    let (code, body) = run(&[
        "init",
        "--vault",
        tmp.path().join("nope").to_str().unwrap(),
        "--format",
        "7",
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "bad_format");
    assert!(!tmp.path().join("nope").exists());
}

#[test]
fn baseline_42364745_refuses_format_3_before_any_write() {
    let tmp = Tmp::new("old3");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    let source = note(&tmp, "plain.txt", b"Amount paid USD 15.99\n");
    let fragment = note(&tmp, "body.txt", b"Amount paid USD 15.99\n");
    let output = tmp.path().join("out.bin");
    fs::write(&output, b"already-there").unwrap();
    let hold = tmp.path().join("hold.json");
    let old = baseline_bin();
    let source_id = "src_00000000000000000000000000000000";
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
            "fragment",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "old-fragment",
            "--fragment",
            fragment.to_str().unwrap(),
        ],
        vec!["candidates", "--vault", vault.to_str().unwrap()],
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
        assert_eq!(body["reason"], "newer_schema");
        assert_eq!(body["schema_version_found"], 3);
        assert_eq!(vault_files(&vault), before, "{}", args[0]);
        assert_eq!(fs::read(&output).unwrap(), b"already-there");
        assert!(!hold.exists());
    }
    assert_eq!(header_user_version(&vault.join("vault.db")), 3);
    assert_eq!(
        tree(&vault.join("journal")),
        Vec::<(String, Vec<u8>)>::new()
    );
}

#[test]
fn a_newer_than_3_header_is_still_refused_without_a_rewrite() {
    let tmp = Tmp::new("newer");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    let status = Command::new("sqlite3")
        .arg(vault.join("vault.db"))
        .arg("PRAGMA user_version=4; PRAGMA wal_checkpoint(TRUNCATE);")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(header_user_version(&vault.join("vault.db")), 4);
    let before = fs::read(vault.join("vault.db")).unwrap();
    let (code, body) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "newer_schema");
    assert_eq!(body["schema_version_found"], 4);
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), before);
}

fn init3(tmp: &Tmp) -> PathBuf {
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], 3);
    assert_eq!(header_user_version(&vault.join("vault.db")), 3);
    vault
}

fn write_note(tmp: &Tmp, name: &str, text: &str) -> PathBuf {
    note(tmp, name, text.as_bytes())
}

fn fragment(vault: &Path, command: &str, file: &Path) -> (i32, Value) {
    run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--fragment",
        file.to_str().unwrap(),
    ])
}

fn facts(vault: &Path) -> Value {
    let (code, body) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["slice"], "T3-facts");
    assert_eq!(body["read_only"], true);
    assert_eq!(body["new_effect"], false);
    assert_eq!(body["user_ready"], false);
    body
}

fn of_type<'a>(body: &'a Value, fact_type: &str) -> Vec<&'a Value> {
    body["facts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fact| fact["fact_type"] == fact_type)
        .collect()
}

#[test]
fn one_command_records_paid_and_period_without_making_a_purchase() {
    let tmp = Tmp::new("stated");
    let vault = init3(&tmp);
    assert_eq!(
        sql(
            &vault,
            "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('facts','decisions','t3_receipts','source_versions') ORDER BY name;"
        ),
        "decisions\nfacts\nsource_versions\nt3_receipts"
    );
    let decisions_sql = sql(
        &vault,
        "SELECT sql FROM sqlite_master WHERE name='decisions';",
    );
    assert!(!decisions_sql.contains("UNIQUE"), "{decisions_sql}");
    let index_sql = sql(
        &vault,
        "SELECT sql FROM sqlite_master WHERE name='decisions_command_candidate_slot';",
    );
    assert!(index_sql.contains("command_id"), "{index_sql}");
    assert!(index_sql.contains("candidate_id"), "{index_sql}");
    assert!(index_sql.contains("slot"), "{index_sql}");

    let file = write_note(
        &tmp,
        "stated.txt",
        "Amount paid USD 15.99\nAmount paid USD 4.00\nTotal USD 15.99\nThis Total is an order total whose payment nature is not stated.\nService period: 2020-01-01 through 2020-01-31\nThis document states the service period above and no status after 2020-02-01.\nAccount email: one@example.test\nAccount email: two@example.test\n",
    );
    let (code, body) = fragment(&vault, "stated", &file);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["slice"], "T3-facts");
    assert_eq!(body["facts_published"], true);
    assert_eq!(body["candidates_published"], true);
    assert_eq!(body["payment_verified"], false);
    assert!(body["observed_at"].is_null());
    assert_eq!(body["time_status"], "unknown");
    assert_eq!(body["current_status"], "unknown");
    assert!(body["purchase_id"].is_null());
    let anchors = body["fact_anchors"].as_array().unwrap();
    assert_eq!(anchors.len(), 3, "{body}");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='stated';"
        ),
        "3"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT count(DISTINCT command_id) FROM decisions WHERE command_id='stated';"
        ),
        "1"
    );
    assert_eq!(sql(&vault, "SELECT count(*) FROM t3_receipts;"), "1");
    assert_eq!(sql(&vault, "SELECT count(*) FROM commands;"), "1");
    assert_eq!(sql(&vault, "SELECT DISTINCT adopted FROM candidates;"), "0");

    let listed = facts(&vault);
    let paid = of_type(&listed, "stated_amount_paid");
    let period = of_type(&listed, "stated_service_period");
    assert_eq!(paid.len(), 2, "{listed}");
    assert_eq!(period.len(), 1, "{listed}");
    assert_eq!(listed["facts"].as_array().unwrap().len(), 3);
    let amounts: Vec<_> = paid
        .iter()
        .map(|fact| fact["value"]["amount_text"].as_str().unwrap())
        .collect();
    assert!(amounts.contains(&"15.99"), "{amounts:?}");
    assert!(amounts.contains(&"4.00"), "{amounts:?}");
    for fact in listed["facts"].as_array().unwrap() {
        assert_eq!(fact["revision"], 1, "{fact}");
        assert!(fact["observed_at"].is_null(), "{fact}");
        assert_eq!(fact["time_status"], "unknown");
        assert_eq!(fact["current_status"], "unknown");
        assert_eq!(fact["payment_verified"], false);
        assert!(fact["purchase_id"].is_null());
        assert!(fact["account_id"].is_null());
        assert!(fact["value"]["account_id"].is_null());
        assert_eq!(fact["value"]["statement"], "source_stated");
        let decision = &fact["decisions"][0];
        assert_eq!(decision["kind"], "rule_adopt");
        assert_eq!(decision["actor_kind"], "rule");
        assert_eq!(decision["actor_id"], "act_00000000000000000000000000000001");
        assert_eq!(decision["rule_version"], "stated-lines-1");
        assert_eq!(decision["command_id"], "stated");
        assert!(decision["target_decision_id"].is_null());
        assert!(decision["prior_value"].is_null());
        assert_ne!(decision["recorded_at"], fact["observed_at"]);
        assert!(decision["recorded_at"].as_i64().unwrap() > 0);
        assert_eq!(fact["candidate_id"], decision["candidate_id"]);
        assert_eq!(
            fact["locator"]["excerpt"],
            fact["locator"]["locator"]["excerpt"]
        );
    }
    assert_eq!(period[0]["value"]["period_start"], "2020-01-01");
    assert_eq!(period[0]["value"]["period_end"], "2020-01-31");
    let (code, again) = fragment(&vault, "stated", &file);
    assert_eq!(code, 0, "{again}");
    assert_eq!(again["outcome"], "AlreadyCommitted");
    assert_eq!(again["new_effect"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "3");
    assert_eq!(sql(&vault, "SELECT count(*) FROM decisions;"), "3");
    assert_eq!(sql(&vault, "SELECT count(*) FROM t3_receipts;"), "1");

    let other = write_note(
        &tmp,
        "other.txt",
        "Amount paid USD 15.99\nService period: 2026-10-01 through 2026-10-31\nA different source.\n",
    );
    let (code, second) = fragment(&vault, "other", &other);
    assert_eq!(code, 0, "{second}");
    assert_ne!(second["source_id"], body["source_id"]);
    let listed = facts(&vault);
    assert_eq!(listed["facts"].as_array().unwrap().len(), 5, "{listed}");
    let first_paid = of_type(&facts(&vault), "stated_amount_paid")
        .into_iter()
        .find(|fact| {
            fact["source_id"] == body["source_id"] && fact["value"]["amount_text"] == "15.99"
        })
        .unwrap()
        .clone();
    let other_paid = of_type(&listed, "stated_amount_paid")
        .into_iter()
        .find(|fact| fact["source_id"] == second["source_id"])
        .unwrap()
        .clone();
    assert_ne!(first_paid["fact_id"], other_paid["fact_id"]);
    assert_eq!(first_paid["revision"], 1);
    assert_eq!(other_paid["revision"], 1);
    assert_eq!(first_paid["value"]["amount_text"], "15.99");
    assert_eq!(other_paid["value"]["amount_text"], "15.99");
    assert!(first_paid["purchase_id"].is_null());
    assert!(other_paid["purchase_id"].is_null());

    let credit = write_note(
        &tmp,
        "credit.txt",
        "Total USD 15.99\nThis Total is a credit-note total, not an amount paid.\nReceipt of the refund is not stated.\n",
    );
    let (code, credit_body) = fragment(&vault, "credit", &credit);
    assert_eq!(code, 0, "{credit_body}");
    assert_eq!(credit_body["facts_published"], true);
    assert!(credit_body["fact_ids"].as_array().unwrap().is_empty());
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "5");
    let (code, candidates) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{candidates}");
    assert!(candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["kind"] == "stated_order_total" && row["value"]["amount_text"] == "15.99"));
    assert!(candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["kind"] == "stated_credit_note_total"));
    assert!(candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["kind"] == "stated_account_label"));
    assert!(candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| {
            row["kind"] == "stated_status_absent" && row["value"]["current_status"] == "unknown"
        }));
}

#[test]
fn a_late_attempt_does_not_record_a_fact() {
    let tmp = Tmp::new("late");
    let vault = init3(&tmp);
    let file = write_note(
        &tmp,
        "late.txt",
        "Amount paid USD 8.00\nService period: 2026-08-01 through 2026-08-31\n",
    );
    let checkpoint = tmp.path().join("late.json");
    let mut child = Command::new(bin())
        .args([
            "fragment",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "late",
            "--fragment",
            file.to_str().unwrap(),
            "--stop-at",
            "before_publish",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_stopped(&mut child, &checkpoint);
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
    resume(child.id());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let writer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(writer["outcome"], "Late");
    assert_eq!(writer["late"], true);
    assert_eq!(writer["candidates_published"], false);
    assert_eq!(writer["preservation"], "preserved");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "0");
    assert_eq!(sql(&vault, "SELECT count(*) FROM t3_receipts;"), "0");
    let (code, after) = fragment(&vault, "late", &file);
    assert_eq!(code, 0, "{after}");
    assert_eq!(after["fact_ids"].as_array().unwrap().len(), 2);
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");
}

#[test]
fn format_2_and_format_1_do_not_gain_fact_tables() {
    let tmp = Tmp::new("nofacts");
    let v2 = tmp.path().join("v2");
    let (code, body) = run(&["init", "--vault", v2.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    let file = write_note(
        &tmp,
        "v2.txt",
        "Amount paid USD 15.99\nService period: 2026-09-01 through 2026-09-30\n",
    );
    let (code, body) = fragment(&v2, "v2", &file);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["slice"], "T2a-body");
    assert!(body.get("facts_published").is_none(), "{body}");
    assert_eq!(
        sql(
            &v2,
            "SELECT count(*) FROM sqlite_master WHERE name='facts';"
        ),
        "0"
    );
    assert_eq!(
        sql(
            &v2,
            "SELECT count(*) FROM sqlite_master WHERE name='t3_receipts';"
        ),
        "0"
    );
    let (code, refused) = run(&["facts", "--vault", v2.to_str().unwrap()]);
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["reason"], "facts_require_format_3");
    assert_eq!(refused["new_effect"], false);
    assert_eq!(
        sql(
            &v2,
            "SELECT count(*) FROM sqlite_master WHERE name='facts';"
        ),
        "0"
    );

    let v1 = tmp.path().join("v1");
    let (code, body) = run(&["init", "--vault", v1.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    let (code, refused) = run(&["facts", "--vault", v1.to_str().unwrap()]);
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["reason"], "needs_upgrade");
    assert_eq!(
        sql(
            &v1,
            "SELECT count(*) FROM sqlite_master WHERE name='facts';"
        ),
        "0"
    );
}

#[test]
fn source_can_be_preserved_before_the_fact_transaction_commits() {
    let tmp = Tmp::new("split-commit");
    let vault = init3(&tmp);
    let file = write_note(
        &tmp,
        "both.txt",
        "Amount paid USD 15.99\nService period: 2026-09-01 through 2026-09-30\n",
    );
    let witness = tmp.path().join("witness.json");
    let (code, failed) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "held",
        "--fragment",
        file.to_str().unwrap(),
        "--inject",
        "enospc:publish_tx",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 15, "{failed}");
    assert_eq!(failed["injected"], true);
    assert_eq!(failed["preservation"], "preserved");
    assert_eq!(failed["candidates_published"], false);
    assert_eq!(failed["new_effect"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "1");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "0");
    assert_eq!(sql(&vault, "SELECT count(*) FROM decisions;"), "0");
    assert_eq!(sql(&vault, "SELECT count(*) FROM t3_receipts;"), "0");
    assert_eq!(sql(&vault, "SELECT count(*) FROM commands;"), "0");
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "0");
    let original = fs::read(&file).unwrap();

    let (code, committed) = fragment(&vault, "held", &file);
    assert_eq!(code, 0, "{committed}");
    assert_eq!(committed["facts_published"], true);
    assert_eq!(committed["fact_ids"].as_array().unwrap().len(), 2);
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");
    assert_eq!(sql(&vault, "SELECT count(*) FROM t3_receipts;"), "1");
    assert_eq!(sql(&vault, "SELECT count(*) FROM commands;"), "1");
    assert_eq!(fs::read(&file).unwrap(), original);

    let crash = write_note(
        &tmp,
        "crash.txt",
        "Amount paid USD 3.00\nService period: 2026-11-01 through 2026-11-30\n",
    );
    let checkpoint = tmp.path().join("crash.json");
    let mut child = Command::new(bin())
        .args([
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
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_stopped(&mut child, &checkpoint);
    kill_stopped(&mut child);
    let killed = child.wait_with_output().unwrap();
    assert_eq!(killed.status.code(), None);
    assert!(killed.stdout.is_empty());
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "2");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM commands WHERE command_id='crash';"
        ),
        "0"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='crash';"
        ),
        "0"
    );
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    let (code, retried) = fragment(&vault, "crash", &crash);
    assert_eq!(code, 0, "{retried}");
    assert_eq!(retried["fact_ids"].as_array().unwrap().len(), 2);
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "4");

    let listed = facts(&vault);
    let bad = sql(
        &vault,
        "UPDATE facts SET value_json='{' WHERE fact_id=(SELECT fact_id FROM facts ORDER BY fact_id LIMIT 1);",
    );
    assert!(bad.is_empty(), "{bad}");
    let (code, broken) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{broken}");
    assert_eq!(broken["reason"], "fact_value_unreadable");
    assert_eq!(broken["new_effect"], false);
    let _ = listed;
}

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACTOR_B: &str = "act_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// Public CLI only. SIGKILL is not a power cut or an fsync proof.
// `decision_tx` and `db_commit` return before COMMIT, so they are not COMMIT-visibility failures.
// `commit_error_*` probes simulate commit() returning an error. They are not disk COMMIT faults.
// Power loss, a real full disk, a real SQLite COMMIT error, Windows, and PDF are not covered.

fn correct_amount(
    vault: &Path,
    command: &str,
    fact_id: &str,
    revision: &str,
    amount: &str,
    actor: &str,
) -> (i32, Value) {
    run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--fact-id",
        fact_id,
        "--expected-revision",
        revision,
        "--amount-text",
        amount,
        "--actor-id",
        actor,
    ])
}

fn paid_id(vault: &Path, source_id: &str) -> String {
    paid_for(&facts(vault), source_id)["fact_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn correct_probe(
    vault: &Path,
    command: &str,
    fact_id: &str,
    revision: &str,
    amount: &str,
    action: &str,
    point: &str,
    witness: &Path,
) -> (i32, Value) {
    run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--fact-id",
        fact_id,
        "--expected-revision",
        revision,
        "--amount-text",
        amount,
        "--actor-id",
        ACTOR,
        "--inject",
        &format!("{action}:{point}"),
        "--witness",
        witness.to_str().unwrap(),
    ])
}

fn undo_probe(
    vault: &Path,
    command: &str,
    decision_id: &str,
    revision: &str,
    action: &str,
    point: &str,
    witness: &Path,
) -> (i32, Value) {
    run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--decision-id",
        decision_id,
        "--expected-revision",
        revision,
        "--actor-id",
        ACTOR,
        "--inject",
        &format!("{action}:{point}"),
        "--witness",
        witness.to_str().unwrap(),
    ])
}

fn assert_not_disk(body: &Value) {
    let note = body["note"].as_str().unwrap();
    assert!(note.contains("simulated commit() error"), "{note}");
    assert!(note.contains("Not a disk COMMIT fault"), "{note}");
}

fn assert_no_command_row(vault: &Path, command: &str) {
    assert_eq!(
        sql(
            vault,
            &format!("SELECT count(*) FROM commands WHERE command_id='{command}';")
        ),
        "0",
        "{command} must not be settled through the commands table"
    );
}

fn undo_at(
    vault: &Path,
    command: &str,
    decision_id: &str,
    revision: &str,
    actor: &str,
) -> (i32, Value) {
    run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--decision-id",
        decision_id,
        "--expected-revision",
        revision,
        "--actor-id",
        actor,
    ])
}

fn journal_file(vault: &Path, command: &str) -> PathBuf {
    vault.join("journal").join(format!("{command}.json"))
}

fn journal_value(vault: &Path, command: &str) -> Value {
    serde_json::from_slice(&fs::read(journal_file(vault, command)).unwrap()).unwrap()
}

fn paid_for<'a>(listed: &'a Value, source_id: &str) -> &'a Value {
    of_type(listed, "stated_amount_paid")
        .into_iter()
        .find(|fact| fact["source_id"] == source_id)
        .unwrap()
}

fn stated_file(tmp: &Tmp) -> (PathBuf, PathBuf) {
    let vault = init3(tmp);
    let file = write_note(
        tmp,
        "stated.txt",
        "Amount paid USD 15.99\nService period: 2026-09-01 through 2026-09-30\n",
    );
    (vault, file)
}

#[test]
fn correct_then_a_new_source_then_undo_restores_the_first_stated_amount() {
    let tmp = Tmp::new("abc");
    let (vault, file) = stated_file(&tmp);
    let original = fs::read(&file).unwrap();
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let source_a = published["source_id"].as_str().unwrap().to_string();
    let listed = facts(&vault);
    let paid = paid_for(&listed, &source_a);
    assert_eq!(paid["revision"], 1);
    assert_eq!(paid["value"]["amount_text"], "15.99");
    assert!(paid["value"]["amount_text"].is_string());
    assert_eq!(
        sql(
            &vault,
            &format!(
                "SELECT typeof(json_extract(value_json, '$.amount_text')) FROM facts WHERE fact_id='{}';",
                paid["fact_id"].as_str().unwrap()
            )
        ),
        "text"
    );
    let period = of_type(&listed, "stated_service_period");
    assert_eq!(period.len(), 1);
    assert_eq!(period[0]["revision"], 1);
    assert_eq!(paid["locator"]["excerpt"], "Amount paid USD 15.99");
    assert_eq!(sql(&vault, "SELECT DISTINCT adopted FROM candidates;"), "0");
    let fact_id = paid["fact_id"].as_str().unwrap().to_string();
    let candidate_id = paid["candidate_id"].as_str().unwrap().to_string();

    let (code, corrected) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{corrected}");
    assert_eq!(corrected["outcome"], "Committed");
    assert_eq!(corrected["new_effect"], true);
    assert_eq!(corrected["revision"], 2);
    assert_eq!(corrected["amount_text"], "10.00");
    assert!(corrected["amount_text"].is_string());
    assert_eq!(corrected["actor_kind"], "human");
    assert_eq!(corrected["actor_id"], ACTOR);
    assert!(corrected["rule_version"].is_null());
    assert!(corrected["target_decision_id"].is_null());
    assert!(corrected["observed_at"].is_null());
    assert_eq!(corrected["current_status"], "unknown");
    assert_eq!(corrected["time_status"], "unknown");
    assert_eq!(corrected["payment_verified"], false);
    assert!(corrected["purchase_id"].is_null());
    assert!(corrected["account_id"].is_null());
    assert_eq!(corrected["payload_version"], "t3-payload-1");
    let decision_b = corrected["decision_id"].as_str().unwrap();
    let canonical = sql(
        &vault,
        "SELECT canonical_payload FROM t3_receipts WHERE command_id='b';",
    );
    assert!(
        canonical.starts_with("t3-payload-1\ncorrect\n"),
        "{canonical}"
    );
    assert!(
        canonical.contains(&format!("fact_id={fact_id}\n")),
        "{canonical}"
    );
    assert!(canonical.contains("expected_revision=1\n"), "{canonical}");
    assert!(canonical.contains("amount_text=10.00\n"), "{canonical}");
    assert!(
        canonical.ends_with(&format!("actor_id={ACTOR}")),
        "{canonical}"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT payload_version FROM t3_receipts WHERE command_id='b';"
        ),
        "t3-payload-1"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM commands WHERE command_id='b';"
        ),
        "0"
    );
    let listed = facts(&vault);
    let paid = paid_for(&listed, published["source_id"].as_str().unwrap());
    assert_eq!(paid["revision"], 2);
    assert_eq!(paid["value"]["amount_text"], "10.00");
    assert!(paid["observed_at"].is_null());
    assert_eq!(paid["current_status"], "unknown");
    assert!(paid["account_id"].is_null());
    assert_eq!(paid["locator"]["excerpt"], "Amount paid USD 15.99");
    assert_eq!(paid["decisions"].as_array().unwrap().len(), 2);
    let human = &paid["decisions"][1];
    assert_eq!(human["decision_id"], decision_b);
    assert_eq!(human["kind"], "human_correct");
    assert_eq!(human["actor_kind"], "human");
    assert_eq!(human["actor_id"], ACTOR);
    assert!(human["rule_version"].is_null());
    assert!(human["target_decision_id"].is_null());
    assert_eq!(human["prior_value"]["amount_text"], "15.99");
    assert_eq!(human["new_value"]["amount_text"], "10.00");
    assert!(human["recorded_at"].as_i64().unwrap() > 0);
    assert!(human["recorded_at"] != paid["observed_at"]);
    assert_eq!(of_type(&listed, "stated_service_period")[0]["revision"], 1);
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["has_incomplete"], false, "{opened}");
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT excerpt FROM candidates WHERE candidate_id='{candidate_id}';")
        ),
        "Amount paid USD 15.99"
    );

    let later = write_note(
        &tmp,
        "later.txt",
        "Amount paid USD 15.99\nService period: 2026-10-01 through 2026-10-31\nA later source, not a new purchase.\n",
    );
    let (code, source_c) = fragment(&vault, "c", &later);
    assert_eq!(code, 0, "{source_c}");
    let source_c_id = source_c["source_id"].as_str().unwrap().to_string();
    assert_ne!(source_c_id, source_a);
    let listed = facts(&vault);
    let kept = paid_for(&listed, &source_a);
    let independent = paid_for(&listed, &source_c_id);
    assert_eq!(kept["revision"], 2);
    assert_eq!(kept["value"]["amount_text"], "10.00");
    assert_eq!(kept["fact_id"], fact_id);
    assert_eq!(independent["revision"], 1);
    assert_eq!(independent["value"]["amount_text"], "15.99");
    assert_ne!(independent["fact_id"], fact_id);
    assert!(independent["purchase_id"].is_null());
    assert_eq!(independent["decisions"].as_array().unwrap().len(), 1);
    assert_eq!(independent["decisions"][0]["kind"], "rule_adopt");

    let (code, undone) = undo_at(&vault, "undo-b", decision_b, "2", ACTOR);
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["outcome"], "Committed");
    assert_eq!(undone["new_effect"], true);
    assert_eq!(undone["revision"], 3);
    assert_eq!(undone["amount_text"], "15.99");
    assert_eq!(undone["target_decision_id"], decision_b);
    assert_eq!(undone["actor_kind"], "human");
    assert!(undone["observed_at"].is_null());
    assert_eq!(undone["current_status"], "unknown");
    assert!(undone["account_id"].is_null());
    assert_eq!(undone["payment_verified"], false);
    let listed = facts(&vault);
    let restored = paid_for(&listed, &source_a);
    let still_c = paid_for(&listed, &source_c_id);
    assert_eq!(restored["revision"], 3);
    assert_eq!(restored["value"]["amount_text"], "15.99");
    assert_eq!(restored["locator"]["excerpt"], "Amount paid USD 15.99");
    assert_eq!(restored["decisions"].as_array().unwrap().len(), 3);
    assert_eq!(restored["decisions"][1]["decision_id"], decision_b);
    assert_eq!(restored["decisions"][2]["kind"], "undo");
    assert_eq!(restored["decisions"][2]["target_decision_id"], decision_b);
    assert_eq!(
        restored["decisions"][2]["prior_value"]["amount_text"],
        "10.00"
    );
    assert_eq!(
        restored["decisions"][2]["new_value"]["amount_text"],
        "15.99"
    );
    assert!(restored["decisions"][2].get("supersedes").is_none());
    assert_eq!(still_c["revision"], 1);
    assert_eq!(still_c["value"]["amount_text"], "15.99");
    assert_eq!(still_c["fact_id"], independent["fact_id"]);
    assert_eq!(of_type(&listed, "stated_service_period").len(), 2);
    assert_eq!(sql(&vault, "SELECT count(*) FROM commands;"), "2");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM commands WHERE command_id='b';"
        ),
        "0"
    );
    assert_eq!(sql(&vault, "SELECT count(*) FROM t3_receipts;"), "4");
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(sql(&vault, "SELECT DISTINCT adopted FROM candidates;"), "0");
    let decisions_sql = sql(
        &vault,
        "SELECT sql FROM sqlite_master WHERE name='decisions';",
    );
    assert!(
        decisions_sql.contains("target_decision_id"),
        "{decisions_sql}"
    );
    assert!(!decisions_sql.contains("supersedes"), "{decisions_sql}");
    assert!(!decisions_sql.contains("keep"), "{decisions_sql}");

    let (code, replay) = undo_at(&vault, "undo-b", decision_b, "2", ACTOR);
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["outcome"], "AlreadyCommitted");
    assert_eq!(replay["new_effect"], false);
    assert_eq!(replay["decision_id"], undone["decision_id"]);
    assert_eq!(replay["revision"], 3);
    assert_eq!(
        sql(&vault, "SELECT count(*) FROM decisions WHERE kind='undo';"),
        "1"
    );
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
        ),
        "3"
    );
}

#[test]
fn strict_amount_unknown_status_and_local_actor_are_refused_before_a_journal() {
    let tmp = Tmp::new("strict");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let listed = facts(&vault);
    let paid = paid_for(&listed, published["source_id"].as_str().unwrap());
    let fact_id = paid["fact_id"].as_str().unwrap();
    let period_id = of_type(&listed, "stated_service_period")[0]["fact_id"]
        .as_str()
        .unwrap();
    for (command, amount) in [
        ("bad-whole", "10"),
        ("bad-tenth", "10.0"),
        ("bad-milli", "10.000"),
        ("bad-comma", "10,00"),
        ("bad-symbol", "$10.00"),
        ("bad-suffix", "10.00 USD"),
        ("bad-space", " 10.00"),
        ("bad-long", "15.990"),
    ] {
        let (code, refused) = correct_amount(&vault, command, fact_id, "1", amount, ACTOR);
        assert_eq!(code, 16, "{amount} {refused}");
        assert_eq!(refused["reason"], "amount_text_rejected");
        assert_eq!(refused["new_effect"], false);
        assert_eq!(refused["decision_committed"], false);
        assert!(!journal_file(&vault, command).exists(), "{command}");
    }
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
        ),
        "1"
    );
    let (code, rule) = correct_amount(
        &vault,
        "rule",
        fact_id,
        "1",
        "10.00",
        "act_00000000000000000000000000000001",
    );
    assert_eq!(code, 16, "{rule}");
    assert_eq!(rule["reason"], "actor_not_human");
    assert!(!journal_file(&vault, "rule").exists());
    let (code, period) = correct_amount(&vault, "period", period_id, "1", "10.00", ACTOR);
    assert_eq!(code, 16, "{period}");
    assert_eq!(period["reason"], "not_amount_fact");
    assert!(!journal_file(&vault, "period").exists());
    let (code, missing) = correct_amount(
        &vault,
        "missing",
        "fact_00000000000000000000000000000000",
        "1",
        "10.00",
        ACTOR,
    );
    assert_eq!(code, 16, "{missing}");
    assert_eq!(missing["reason"], "unknown_fact");
    assert!(!journal_file(&vault, "missing").exists());
    for revision in ["0", "02", "-1", "1.0"] {
        let (code, bad) = correct_amount(&vault, "rev", fact_id, revision, "10.00", ACTOR);
        assert_eq!(code, 2, "{revision} {bad}");
        assert_eq!(bad["reason"], "bad_revision");
    }
    assert!(!journal_file(&vault, "rev").exists());
    assert_eq!(sql(&vault, "SELECT count(*) FROM decisions;"), "2");
}

#[test]
fn payload_is_checked_before_revision_and_a_conflict_does_not_write() {
    let tmp = Tmp::new("payload");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_for(&facts(&vault), published["source_id"].as_str().unwrap())["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, corrected) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{corrected}");
    let journal_before = fs::read(journal_file(&vault, "b")).unwrap();
    let (code, again) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{again}");
    assert_eq!(again["outcome"], "AlreadyCommitted");
    assert_eq!(again["new_effect"], false);
    assert_eq!(again["decision_id"], corrected["decision_id"]);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE kind='human_correct';"
        ),
        "1"
    );
    assert!(sql(
        &vault,
        &format!("UPDATE facts SET revision=9 WHERE fact_id='{fact_id}';")
    )
    .is_empty());
    let (code, replay) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["outcome"], "AlreadyCommitted");
    assert_eq!(replay["new_effect"], false);
    assert_eq!(replay["revision"], 2);
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
        ),
        "9"
    );
    assert_eq!(
        sql(&vault, &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")),
        "10.00"
    );
    let (code, conflict) = correct_amount(&vault, "b", &fact_id, "9", "11.00", ACTOR);
    assert_eq!(code, 16, "{conflict}");
    assert_eq!(conflict["reason"], "payload_conflict");
    assert_eq!(conflict["new_effect"], false);
    assert_eq!(fs::read(journal_file(&vault, "b")).unwrap(), journal_before);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE kind='human_correct';"
        ),
        "1"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='b';"
        ),
        "1"
    );
    let (code, revision) = correct_amount(&vault, "other", &fact_id, "2", "11.00", ACTOR);
    assert_eq!(code, 16, "{revision}");
    assert_eq!(revision["reason"], "revision_conflict");
    assert!(!journal_file(&vault, "other").exists());
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
        ),
        "9"
    );
}

#[test]
fn undo_of_an_earlier_decision_is_a_conflict_when_a_later_human_edit_exists() {
    let tmp = Tmp::new("successor");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_for(&facts(&vault), published["source_id"].as_str().unwrap())["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, first) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{first}");
    let (code, second) = correct_amount(&vault, "later", &fact_id, "2", "11.00", ACTOR_B);
    assert_eq!(code, 0, "{second}");
    assert_eq!(second["actor_id"], ACTOR_B);
    let before = sql(&vault, "SELECT count(*) FROM decisions;");
    let (code, blocked) = undo_at(
        &vault,
        "undo-b",
        first["decision_id"].as_str().unwrap(),
        "3",
        ACTOR,
    );
    assert_eq!(code, 16, "{blocked}");
    assert_eq!(blocked["reason"], "successor_conflict");
    assert_eq!(blocked["new_effect"], false);
    assert_eq!(blocked["facts_changed"], false);
    assert!(!journal_file(&vault, "undo-b").exists());
    assert_eq!(sql(&vault, "SELECT count(*) FROM decisions;"), before);
    assert_eq!(
        sql(&vault, &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")),
        "11.00"
    );
    let (code, undone) = undo_at(
        &vault,
        "undo-later",
        second["decision_id"].as_str().unwrap(),
        "3",
        ACTOR,
    );
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["revision"], 4);
    assert_eq!(undone["amount_text"], "10.00");
    assert_eq!(undone["target_decision_id"], second["decision_id"]);
    let listed = facts(&vault);
    let paid = paid_for(&listed, published["source_id"].as_str().unwrap());
    assert_eq!(paid["revision"], 4);
    assert_eq!(paid["value"]["amount_text"], "10.00");
    assert_eq!(paid["decisions"].as_array().unwrap().len(), 4);
    assert_eq!(paid["decisions"][1]["decision_id"], first["decision_id"]);
}

#[test]
fn format_1_and_format_2_refuse_correct_and_undo_without_fact_tables() {
    let tmp = Tmp::new("old-format");
    let v2 = tmp.path().join("v2");
    let (code, body) = run(&["init", "--vault", v2.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    let (code, refused) = correct_amount(
        &v2,
        "b",
        "fact_00000000000000000000000000000000",
        "1",
        "10.00",
        ACTOR,
    );
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["reason"], "facts_require_format_3");
    assert_eq!(refused["new_effect"], false);
    let (code, undo) = undo_at(&v2, "u", "dec_00000000000000000000000000000000", "1", ACTOR);
    assert_eq!(code, 14, "{undo}");
    assert_eq!(undo["reason"], "facts_require_format_3");
    assert!(!journal_file(&v2, "b").exists());
    assert!(!journal_file(&v2, "u").exists());
    assert_eq!(
        sql(
            &v2,
            "SELECT count(*) FROM sqlite_master WHERE name IN ('facts','decisions','t3_receipts');"
        ),
        "0"
    );
    let v1 = tmp.path().join("v1");
    let (code, body) = run(&["init", "--vault", v1.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    let (code, refused) = correct_amount(
        &v1,
        "b",
        "fact_00000000000000000000000000000000",
        "1",
        "10.00",
        ACTOR,
    );
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["outcome"], "NeedsUpgrade");
    assert_eq!(refused["reason"], "needs_upgrade");
    assert!(!journal_file(&v1, "b").exists());
    assert_eq!(
        sql(
            &v1,
            "SELECT count(*) FROM sqlite_master WHERE name='facts';"
        ),
        "0"
    );
}

#[test]
fn an_injected_receipt_read_does_not_decide_that_the_command_failed() {
    let tmp = Tmp::new("receipt-read");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_for(&facts(&vault), published["source_id"].as_str().unwrap())["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let witness = tmp.path().join("witness.json");
    let (code, unseen) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "b",
        "--fact-id",
        &fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "10.00",
        "--actor-id",
        ACTOR,
        "--inject",
        "io_error:receipt_read",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 12, "{unseen}");
    assert_eq!(unseen["reason"], "t3_receipt_unseen");
    assert_eq!(unseen["injected"], true);
    assert_eq!(unseen["new_effect"], false);
    assert_eq!(unseen["decision_committed"], false);
    assert!(unseen["note"]
        .as_str()
        .unwrap()
        .contains("not a failed read"));
    assert!(!journal_file(&vault, "b").exists());
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
        ),
        "1"
    );
    let (code, corrected) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{corrected}");
    assert_eq!(corrected["revision"], 2);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE kind='human_correct';"
        ),
        "1"
    );
}

#[test]
fn injected_decision_faults_roll_back_and_are_not_commit_visibility() {
    let tmp = Tmp::new("decision-inject");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_for(&facts(&vault), published["source_id"].as_str().unwrap())["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    for (command, point) in [("tx", "decision_tx"), ("commit", "db_commit")] {
        let witness = tmp.path().join(format!("{command}.json"));
        let revision = sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';"),
        );
        let (code, failed) = run(&[
            "correct",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            command,
            "--fact-id",
            &fact_id,
            "--expected-revision",
            &revision,
            "--amount-text",
            "10.00",
            "--actor-id",
            ACTOR,
            "--inject",
            &format!("io_error:{point}"),
            "--witness",
            witness.to_str().unwrap(),
        ]);
        assert_eq!(code, 15, "{point} {failed}");
        assert_eq!(failed["injected"], true);
        assert_eq!(failed["decision_committed"], false);
        assert_eq!(failed["facts_changed"], false);
        assert!(failed["note"]
            .as_str()
            .unwrap()
            .contains("not a COMMIT visibility result"));
        assert_eq!(journal_value(&vault, command)["state"], "started");
        assert_eq!(
            sql(
                &vault,
                &format!("SELECT count(*) FROM t3_receipts WHERE command_id='{command}';")
            ),
            "0"
        );
        assert_eq!(
            sql(
                &vault,
                &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
            ),
            revision
        );
        let (code, opened) = run(&["recover", "--vault", vault.to_str().unwrap()]);
        assert_eq!(code, 0, "{opened}");
        let journal = journal_value(&vault, command);
        assert_eq!(journal["state"], "aborted");
        assert_eq!(journal["reason"], "uncommitted");
        let (code, retried) = correct_amount(&vault, command, &fact_id, &revision, "10.00", ACTOR);
        assert_eq!(code, 0, "{retried}");
        assert_eq!(retried["new_effect"], true);
        assert_eq!(
            sql(
                &vault,
                &format!("SELECT count(*) FROM decisions WHERE command_id='{command}';")
            ),
            "1"
        );
    }
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE kind='human_correct';"
        ),
        "2"
    );
}

#[test]
fn sigkill_before_the_decision_transaction_is_not_power_loss() {
    let tmp = Tmp::new("decision-kill");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_for(&facts(&vault), published["source_id"].as_str().unwrap())["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let checkpoint = tmp.path().join("stop.json");
    let mut child = Command::new(bin())
        .args([
            "correct",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "b",
            "--fact-id",
            &fact_id,
            "--expected-revision",
            "1",
            "--amount-text",
            "10.00",
            "--actor-id",
            ACTOR,
            "--stop-at",
            "before_decision_commit",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_stopped(&mut child, &checkpoint);
    kill_stopped(&mut child);
    let killed = child.wait_with_output().unwrap();
    assert_eq!(killed.status.code(), None);
    assert!(killed.stdout.is_empty());
    assert_eq!(journal_value(&vault, "b")["state"], "started");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='b';"
        ),
        "0"
    );
    assert_eq!(
        sql(&vault, &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")),
        "15.99"
    );
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    let journal = journal_value(&vault, "b");
    assert_eq!(journal["state"], "aborted");
    assert_eq!(journal["reason"], "uncommitted");
    let (code, retried) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{retried}");
    assert_eq!(retried["revision"], 2);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='b';"
        ),
        "1"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='b';"
        ),
        "1"
    );
}

#[test]
fn an_unreadable_t3_receipt_stays_in_recovery_without_aborting_the_journal() {
    let tmp = Tmp::new("unreadable");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_for(&facts(&vault), published["source_id"].as_str().unwrap())["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, corrected) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{corrected}");
    let journal_before = fs::read(journal_file(&vault, "b")).unwrap();
    assert!(sql(
        &vault,
        "UPDATE t3_receipts SET result_json='{' WHERE command_id='b';"
    )
    .is_empty());
    let (code, broken) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 12, "{broken}");
    assert_eq!(broken["reason"], "t3_receipt_unreadable");
    assert_eq!(broken["new_effect"], false);
    assert_eq!(broken["decision_committed"], false);
    assert_eq!(fs::read(journal_file(&vault, "b")).unwrap(), journal_before);
    assert_eq!(journal_value(&vault, "b")["state"], "committed");
    assert_eq!(
        sql(&vault, &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")),
        "10.00"
    );
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{opened}");
    assert_eq!(opened["reason"], "t3_receipt_unreadable");
    assert_eq!(fs::read(journal_file(&vault, "b")).unwrap(), journal_before);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE kind='human_correct';"
        ),
        "1"
    );
}

#[test]
fn cancel_and_marker_do_not_rewrite_a_decision_journal() {
    let tmp = Tmp::new("guard");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_for(&facts(&vault), published["source_id"].as_str().unwrap())["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, corrected) = correct_amount(&vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{corrected}");
    let before = fs::read(journal_file(&vault, "b")).unwrap();
    let (code, cancel) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "b",
        "--generation",
        "1",
        "--kind",
        "preservation",
    ]);
    assert_eq!(code, 16, "{cancel}");
    assert_eq!(cancel["reason"], "command_kind_conflict");
    assert_eq!(cancel["cancel_applied"], false);
    assert_eq!(fs::read(journal_file(&vault, "b")).unwrap(), before);
    let (code, marker) = run(&[
        "marker",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "b",
        "--source-id",
        published["source_id"].as_str().unwrap(),
        "--value",
        "not a decision",
    ]);
    assert_eq!(code, 16, "{marker}");
    assert_eq!(marker["reason"], "command_kind_conflict");
    assert_eq!(fs::read(journal_file(&vault, "b")).unwrap(), before);
    assert_eq!(
        sql(&vault, &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")),
        "10.00"
    );
    let _ = corrected;
}

#[test]
fn the_previous_binary_has_no_correct_command_and_does_not_write_a_format_3_vault() {
    let (code, version) = run_at(baseline_bin(), &["version"]);
    assert_eq!(code, 0, "{version}");
    assert_eq!(
        version["git_sha"],
        "42364745aa00910c3ec47ee4fd63c3dac831e11a"
    );
    assert_eq!(
        version["supported_vault_formats"],
        serde_json::json!([1, 2])
    );
    let tmp = Tmp::new("old-bin");
    let vault = init3(&tmp);
    let before = fs::read(vault.join("vault.db")).unwrap();
    let (code, bare) = run_at(baseline_bin(), &["correct"]);
    assert_eq!(code, 2, "{bare}");
    assert_eq!(bare["reason"], "unknown_command");
    let (code, flagged) = run_at(
        baseline_bin(),
        &[
            "correct",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "b",
            "--fact-id",
            "fact_00000000000000000000000000000000",
            "--expected-revision",
            "1",
            "--amount-text",
            "10.00",
            "--actor-id",
            ACTOR,
        ],
    );
    assert_eq!(code, 2, "{flagged}");
    assert_eq!(flagged["reason"], "unknown_flag");
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), before);
    assert!(!journal_file(&vault, "b").exists());
}

#[test]
fn commit_error_with_a_visible_t3_receipt_returns_the_stored_result() {
    // The probe commits, then uses the same settlement as a commit() error.
    // It is not a disk COMMIT fault. `db_commit` remains the before-COMMIT case.
    let tmp = Tmp::new("commit-visible");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_id(&vault, published["source_id"].as_str().unwrap());
    let witness = tmp.path().join("visible.json");
    let (code, corrected) = correct_probe(
        &vault,
        "vis",
        &fact_id,
        "1",
        "10.00",
        "io_error",
        "commit_error_visible",
        &witness,
    );
    assert_eq!(code, 0, "{corrected}");
    assert_eq!(corrected["outcome"], "Committed");
    assert_eq!(corrected["revision"], 2);
    assert_eq!(corrected["amount_text"], "10.00");
    assert_eq!(corrected["new_effect"], true);
    assert_eq!(corrected["visible_despite_commit_error"], true);
    assert_eq!(corrected["injected"], true);
    assert_eq!(corrected["commit_error"], "simulated commit() error");
    assert_not_disk(&corrected);
    assert_eq!(journal_value(&vault, "vis")["state"], "committed");
    assert!(journal_value(&vault, "vis")["reason"].is_null());
    assert_no_command_row(&vault, "vis");
    let stored: Value = serde_json::from_str(&sql(
        &vault,
        "SELECT result_json FROM t3_receipts WHERE command_id='vis';",
    ))
    .unwrap();
    assert_eq!(stored["decision_id"], corrected["decision_id"]);
    assert_eq!(stored["outcome"], "Committed");
    assert_eq!(stored["new_effect"], true);
    assert!(stored.get("visible_despite_commit_error").is_none());
    assert!(sql(
        &vault,
        "SELECT canonical_payload FROM t3_receipts WHERE command_id='vis';"
    )
    .contains("amount_text=10.00"));
    let (code, again) = correct_amount(&vault, "vis", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{again}");
    assert_eq!(again["outcome"], "AlreadyCommitted");
    assert_eq!(again["new_effect"], false);
    assert_eq!(again["decision_id"], corrected["decision_id"]);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='vis';"
        ),
        "1"
    );

    let target = corrected["decision_id"].as_str().unwrap();
    let witness = tmp.path().join("undo-visible.json");
    let (code, undone) = undo_probe(
        &vault,
        "uvis",
        target,
        "2",
        "io_error",
        "commit_error_visible",
        &witness,
    );
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["outcome"], "Committed");
    assert_eq!(undone["revision"], 3);
    assert_eq!(undone["amount_text"], "15.99");
    assert_eq!(undone["target_decision_id"], target);
    assert_eq!(undone["new_effect"], true);
    assert_eq!(undone["visible_despite_commit_error"], true);
    assert_eq!(undone["injected"], true);
    assert_not_disk(&undone);
    assert_eq!(journal_value(&vault, "uvis")["state"], "committed");
    assert_no_command_row(&vault, "uvis");
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")
        ),
        "15.99"
    );
    let (code, replay) = undo_at(&vault, "uvis", target, "2", ACTOR);
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["outcome"], "AlreadyCommitted");
    assert_eq!(replay["new_effect"], false);
    assert_eq!(replay["decision_id"], undone["decision_id"]);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='uvis';"
        ),
        "1"
    );
}

#[test]
fn commit_error_with_no_t3_receipt_is_not_preserved() {
    let tmp = Tmp::new("commit-absent");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_id(&vault, published["source_id"].as_str().unwrap());
    for (command, action) in [("disk", "enospc"), ("abs", "io_error")] {
        let witness = tmp.path().join(format!("{command}.json"));
        let (code, failed) = correct_probe(
            &vault,
            command,
            &fact_id,
            "1",
            "10.00",
            action,
            "commit_error_absent",
            &witness,
        );
        assert_eq!(code, 15, "{action} {failed}");
        assert_eq!(failed["outcome"], "NotPreserved");
        assert_eq!(failed["reason"], "db_error");
        assert_ne!(failed["reason"], "enospc");
        assert_eq!(failed["injected"], true);
        assert_eq!(failed["decision_committed"], false);
        assert_eq!(failed["facts_changed"], false);
        assert_eq!(failed["new_effect"], false);
        assert_not_disk(&failed);
        assert!(failed["note"].as_str().unwrap().contains("no t3 receipt"));
        let journal = journal_value(&vault, command);
        assert_eq!(journal["state"], "started");
        assert!(journal["reason"].is_null());
        assert_eq!(
            sql(
                &vault,
                &format!("SELECT count(*) FROM t3_receipts WHERE command_id='{command}';")
            ),
            "0"
        );
        assert_no_command_row(&vault, command);
    }
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
        ),
        "1"
    );
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    for command in ["disk", "abs"] {
        let journal = journal_value(&vault, command);
        assert_eq!(journal["state"], "aborted");
        assert_eq!(journal["reason"], "uncommitted");
    }
    let (code, retried) = correct_amount(&vault, "abs", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{retried}");
    assert_eq!(retried["revision"], 2);
    let target = retried["decision_id"].as_str().unwrap().to_string();
    let witness = tmp.path().join("undo-absent.json");
    let (code, failed) = undo_probe(
        &vault,
        "uabs",
        &target,
        "2",
        "io_error",
        "commit_error_absent",
        &witness,
    );
    assert_eq!(code, 15, "{failed}");
    assert_eq!(failed["outcome"], "NotPreserved");
    assert_eq!(failed["reason"], "db_error");
    assert_eq!(failed["decision_committed"], false);
    assert_eq!(failed["facts_changed"], false);
    assert_not_disk(&failed);
    assert_eq!(journal_value(&vault, "uabs")["state"], "started");
    assert_eq!(journal_value(&vault, "abs")["state"], "committed");
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT revision FROM facts WHERE fact_id='{fact_id}';")
        ),
        "2"
    );
    assert_eq!(
        sql(&vault, "SELECT count(*) FROM decisions WHERE kind='undo';"),
        "0"
    );
    assert_no_command_row(&vault, "uabs");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(journal_value(&vault, "uabs")["state"], "aborted");
    assert_eq!(journal_value(&vault, "uabs")["reason"], "uncommitted");
    let (code, undone) = undo_at(&vault, "uabs", &target, "2", ACTOR);
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["revision"], 3);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='uabs';"
        ),
        "1"
    );
}

#[test]
fn commit_error_with_an_unreadable_t3_receipt_stays_started() {
    let tmp = Tmp::new("commit-unreadable");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_id(&vault, published["source_id"].as_str().unwrap());
    let witness = tmp.path().join("bad.json");
    let (code, failed) = correct_probe(
        &vault,
        "bad",
        &fact_id,
        "1",
        "10.00",
        "io_error",
        "commit_error_unreadable",
        &witness,
    );
    assert_eq!(code, 12, "{failed}");
    assert_eq!(failed["outcome"], "RecoveryRequired");
    assert_eq!(failed["reason"], "t3_receipt_unseen");
    assert_eq!(failed["injected"], true);
    assert!(failed["new_effect"].is_null());
    assert!(failed["decision_committed"].is_null());
    assert!(failed["facts_changed"].is_null());
    assert_not_disk(&failed);
    assert!(failed["note"].as_str().unwrap().contains("stays started"));
    assert_eq!(journal_value(&vault, "bad")["state"], "started");
    assert!(journal_value(&vault, "bad")["reason"].is_null());
    assert_eq!(
        sql(
            &vault,
            "SELECT result_json FROM t3_receipts WHERE command_id='bad';"
        ),
        "{"
    );
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")
        ),
        "10.00"
    );
    assert_no_command_row(&vault, "bad");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{opened}");
    assert_eq!(opened["reason"], "t3_receipt_unreadable");
    assert_eq!(journal_value(&vault, "bad")["state"], "started");

    let undo_vault = tmp.path().join("undo");
    let (code, body) = run(&[
        "init",
        "--vault",
        undo_vault.to_str().unwrap(),
        "--format",
        "3",
    ]);
    assert_eq!(code, 0, "{body}");
    let (code, published) = fragment(&undo_vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_id(&undo_vault, published["source_id"].as_str().unwrap());
    let (code, corrected) = correct_amount(&undo_vault, "b", &fact_id, "1", "10.00", ACTOR);
    assert_eq!(code, 0, "{corrected}");
    let target = corrected["decision_id"].as_str().unwrap();
    let witness = tmp.path().join("undo-bad.json");
    let (code, failed) = undo_probe(
        &undo_vault,
        "ubad",
        target,
        "2",
        "io_error",
        "commit_error_unreadable",
        &witness,
    );
    assert_eq!(code, 12, "{failed}");
    assert_eq!(failed["reason"], "t3_receipt_unseen");
    assert!(failed["new_effect"].is_null());
    assert!(failed["decision_committed"].is_null());
    assert_not_disk(&failed);
    assert_eq!(journal_value(&undo_vault, "ubad")["state"], "started");
    assert_eq!(journal_value(&undo_vault, "b")["state"], "committed");
    assert_eq!(
        sql(
            &undo_vault,
            "SELECT result_json FROM t3_receipts WHERE command_id='ubad';"
        ),
        "{"
    );
    assert_eq!(
        sql(
            &undo_vault,
            &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")
        ),
        "15.99"
    );
    let (code, opened) = run(&["open", "--vault", undo_vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{opened}");
    assert_eq!(opened["reason"], "t3_receipt_unreadable");
    assert_eq!(journal_value(&undo_vault, "ubad")["state"], "started");
    assert_eq!(journal_value(&undo_vault, "b")["state"], "committed");
}

#[test]
fn commit_error_with_a_failed_receipt_read_stays_started() {
    // The probe asks the independent reader to open not-a-vault.db, which is not the database.
    let tmp = Tmp::new("commit-unseen");
    let (vault, file) = stated_file(&tmp);
    let (code, published) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{published}");
    let fact_id = paid_id(&vault, published["source_id"].as_str().unwrap());
    let witness = tmp.path().join("miss.json");
    let (code, failed) = correct_probe(
        &vault,
        "miss",
        &fact_id,
        "1",
        "10.00",
        "io_error",
        "commit_error_unseen",
        &witness,
    );
    assert_eq!(code, 12, "{failed}");
    assert_eq!(failed["outcome"], "RecoveryRequired");
    assert_eq!(failed["reason"], "t3_receipt_unseen");
    assert_ne!(failed["outcome"], "NotPreserved");
    assert_eq!(failed["injected"], true);
    assert!(failed["new_effect"].is_null());
    assert!(failed["decision_committed"].is_null());
    assert!(failed["facts_changed"].is_null());
    assert_not_disk(&failed);
    assert!(!vault.join("not-a-vault.db").exists());
    assert_eq!(journal_value(&vault, "miss")["state"], "started");
    assert!(journal_value(&vault, "miss")["reason"].is_null());
    let stored: Value = serde_json::from_str(&sql(
        &vault,
        "SELECT result_json FROM t3_receipts WHERE command_id='miss';",
    ))
    .unwrap();
    assert_eq!(stored["outcome"], "Committed");
    assert_eq!(stored["amount_text"], "10.00");
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")
        ),
        "10.00"
    );
    assert_no_command_row(&vault, "miss");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(journal_value(&vault, "miss")["state"], "committed");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='miss';"
        ),
        "1"
    );

    let target = stored["decision_id"].as_str().unwrap();
    let witness = tmp.path().join("undo-miss.json");
    let (code, failed) = undo_probe(
        &vault,
        "umiss",
        target,
        "2",
        "io_error",
        "commit_error_unseen",
        &witness,
    );
    assert_eq!(code, 12, "{failed}");
    assert_eq!(failed["reason"], "t3_receipt_unseen");
    assert_ne!(failed["outcome"], "NotPreserved");
    assert!(failed["new_effect"].is_null());
    assert!(failed["decision_committed"].is_null());
    assert_not_disk(&failed);
    assert_eq!(journal_value(&vault, "umiss")["state"], "started");
    assert_eq!(journal_value(&vault, "miss")["state"], "committed");
    let undone: Value = serde_json::from_str(&sql(
        &vault,
        "SELECT result_json FROM t3_receipts WHERE command_id='umiss';",
    ))
    .unwrap();
    assert_eq!(undone["outcome"], "Committed");
    assert_eq!(undone["amount_text"], "15.99");
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';")
        ),
        "15.99"
    );
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(journal_value(&vault, "umiss")["state"], "committed");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='umiss';"
        ),
        "1"
    );
}

fn fragment_probe(
    vault: &Path,
    command: &str,
    file: &Path,
    action: &str,
    point: &str,
    witness: &Path,
) -> (i32, Value) {
    run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--fragment",
        file.to_str().unwrap(),
        "--inject",
        &format!("{action}:{point}"),
        "--witness",
        witness.to_str().unwrap(),
    ])
}

fn other_note(tmp: &Tmp, name: &str, amount: &str) -> PathBuf {
    write_note(
        tmp,
        name,
        &format!("Amount paid USD {amount}\nService period: 2026-10-01 through 2026-10-31\n"),
    )
}

#[test]
fn format_3_publish_commit_error_uses_the_t3_receipt() {
    // db_commit still returns before COMMIT. These probes are not disk COMMIT faults.
    // A commands row is not the T3 witness: the unseen case has that row and still
    // refuses to call the publish committed or absent.
    let tmp = Tmp::new("fragment-commit");
    let (vault, file) = stated_file(&tmp);
    let absent_file = other_note(&tmp, "absent.txt", "4.00");
    let witness = tmp.path().join("absent.json");
    let (code, failed) = fragment_probe(
        &vault,
        "abs",
        &absent_file,
        "enospc",
        "commit_error_absent",
        &witness,
    );
    assert_eq!(code, 3, "{failed}");
    assert_eq!(failed["outcome"], "NotPublished");
    assert_eq!(failed["reason"], "db_error");
    assert_ne!(failed["reason"], "enospc");
    assert_eq!(failed["candidates_published"], false);
    assert_eq!(failed["new_effect"], false);
    assert_eq!(failed["injected"], true);
    assert_eq!(failed["preservation"], "preserved");
    assert_not_disk(&failed);
    assert!(failed["note"].as_str().unwrap().contains("no t3 receipt"));
    assert_eq!(journal_value(&vault, "abs")["state"], "aborted");
    assert_eq!(journal_value(&vault, "abs")["reason"], "db_error");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='abs';"
        ),
        "0"
    );
    assert_no_command_row(&vault, "abs");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "0");

    let witness = tmp.path().join("visible.json");
    let (code, published) = fragment_probe(
        &vault,
        "vis",
        &file,
        "io_error",
        "commit_error_visible",
        &witness,
    );
    assert_eq!(code, 0, "{published}");
    assert_eq!(published["outcome"], "Committed");
    assert_eq!(published["facts_published"], true);
    assert_eq!(published["candidates_published"], true);
    assert_eq!(published["new_effect"], true);
    assert_eq!(published["visible_despite_commit_error"], true);
    assert_eq!(published["injected"], true);
    assert_eq!(published["commit_error"], "simulated commit() error");
    assert_not_disk(&published);
    assert_eq!(journal_value(&vault, "vis")["state"], "committed");
    let stored: Value = serde_json::from_str(&sql(
        &vault,
        "SELECT result_json FROM t3_receipts WHERE command_id='vis';",
    ))
    .unwrap();
    assert_eq!(stored["outcome"], "Committed");
    assert_eq!(stored["fact_ids"], published["fact_ids"]);
    assert!(stored.get("visible_despite_commit_error").is_none());
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM commands WHERE command_id='vis';"
        ),
        "1"
    );
    let (code, again) = fragment(&vault, "vis", &file);
    assert_eq!(code, 0, "{again}");
    assert_eq!(again["new_effect"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");

    let miss_file = other_note(&tmp, "miss.txt", "6.00");
    let witness = tmp.path().join("miss.json");
    let (code, missed) = fragment_probe(
        &vault,
        "miss",
        &miss_file,
        "io_error",
        "commit_error_unseen",
        &witness,
    );
    assert_eq!(code, 12, "{missed}");
    assert_eq!(missed["outcome"], "RecoveryRequired");
    assert_eq!(missed["reason"], "t3_receipt_unseen");
    assert_ne!(missed["outcome"], "NotPublished");
    assert_eq!(missed["injected"], true);
    assert!(missed["new_effect"].is_null());
    assert!(missed["candidates_published"].is_null());
    assert!(missed["facts_published"].is_null());
    assert_not_disk(&missed);
    assert!(!vault.join("not-a-vault.db").exists());
    assert_eq!(journal_value(&vault, "miss")["state"], "object_ready");
    assert!(journal_value(&vault, "miss")["reason"].is_null());
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM commands WHERE command_id='miss';"
        ),
        "1"
    );
    let missed_stored: Value = serde_json::from_str(&sql(
        &vault,
        "SELECT result_json FROM t3_receipts WHERE command_id='miss';",
    ))
    .unwrap();
    assert_eq!(missed_stored["outcome"], "Committed");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "4");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(journal_value(&vault, "miss")["state"], "committed");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "4");

    let bad_vault = tmp.path().join("bad");
    let (code, body) = run(&[
        "init",
        "--vault",
        bad_vault.to_str().unwrap(),
        "--format",
        "3",
    ]);
    assert_eq!(code, 0, "{body}");
    let witness = tmp.path().join("bad.json");
    let (code, broken) = fragment_probe(
        &bad_vault,
        "bad",
        &file,
        "io_error",
        "commit_error_unreadable",
        &witness,
    );
    assert_eq!(code, 12, "{broken}");
    assert_eq!(broken["reason"], "t3_receipt_unseen");
    assert_ne!(broken["outcome"], "NotPublished");
    assert!(broken["candidates_published"].is_null());
    assert_not_disk(&broken);
    assert_eq!(journal_value(&bad_vault, "bad")["state"], "object_ready");
    assert_eq!(
        sql(
            &bad_vault,
            "SELECT result_json FROM t3_receipts WHERE command_id='bad';"
        ),
        "{"
    );
    assert_eq!(sql(&bad_vault, "SELECT count(*) FROM facts;"), "2");
    let (code, opened) = run(&["open", "--vault", bad_vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{opened}");
    assert_eq!(opened["reason"], "t3_receipt_unreadable");
    assert_eq!(journal_value(&bad_vault, "bad")["state"], "object_ready");
}

#[test]
fn format_3_replay_commit_error_uses_the_t3_receipt() {
    let tmp = Tmp::new("fragment-replay");
    let (vault, file) = stated_file(&tmp);
    let (code, first) = fragment(&vault, "a", &file);
    assert_eq!(code, 0, "{first}");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");
    let witness = tmp.path().join("visible.json");
    let (code, replayed) = fragment_probe(
        &vault,
        "b",
        &file,
        "io_error",
        "commit_error_visible",
        &witness,
    );
    assert_eq!(code, 0, "{replayed}");
    assert_eq!(replayed["outcome"], "AlreadyPublished");
    assert_eq!(replayed["facts_published"], false);
    assert_eq!(replayed["new_effect"], false);
    assert_eq!(replayed["candidates_published"], true);
    assert_eq!(replayed["visible_despite_commit_error"], true);
    assert_eq!(replayed["injected"], true);
    assert_not_disk(&replayed);
    assert_eq!(journal_value(&vault, "b")["state"], "committed");
    assert_eq!(journal_value(&vault, "a")["state"], "committed");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");
    let stored: Value = serde_json::from_str(&sql(
        &vault,
        "SELECT result_json FROM t3_receipts WHERE command_id='b';",
    ))
    .unwrap();
    assert_eq!(stored["outcome"], "AlreadyPublished");
    assert!(stored.get("visible_despite_commit_error").is_none());

    let witness = tmp.path().join("absent.json");
    let (code, failed) = fragment_probe(
        &vault,
        "c",
        &file,
        "io_error",
        "commit_error_absent",
        &witness,
    );
    assert_eq!(code, 15, "{failed}");
    assert_eq!(failed["outcome"], "NotPublished");
    assert_eq!(failed["reason"], "db_error");
    assert_eq!(failed["candidates_published"], false);
    assert_eq!(failed["new_effect"], false);
    assert_eq!(failed["preservation"], "preserved");
    assert_not_disk(&failed);
    assert!(failed["note"].as_str().unwrap().contains("no t3 receipt"));
    assert_eq!(journal_value(&vault, "c")["state"], "object_ready");
    assert!(journal_value(&vault, "c")["reason"].is_null());
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='c';"
        ),
        "0"
    );
    assert_no_command_row(&vault, "c");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");

    let witness = tmp.path().join("bad.json");
    let (code, broken) = fragment_probe(
        &vault,
        "d",
        &file,
        "io_error",
        "commit_error_unreadable",
        &witness,
    );
    assert_eq!(code, 12, "{broken}");
    assert_eq!(broken["reason"], "t3_receipt_unseen");
    assert_ne!(broken["outcome"], "NotPublished");
    assert!(broken["new_effect"].is_null());
    assert!(broken["candidates_published"].is_null());
    assert_not_disk(&broken);
    assert_eq!(journal_value(&vault, "d")["state"], "object_ready");
    assert_eq!(journal_value(&vault, "b")["state"], "committed");
    assert_eq!(
        sql(
            &vault,
            "SELECT result_json FROM t3_receipts WHERE command_id='d';"
        ),
        "{"
    );
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "2");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{opened}");
    assert_eq!(opened["reason"], "t3_receipt_unreadable");
    assert_eq!(journal_value(&vault, "d")["state"], "object_ready");
    assert_eq!(journal_value(&vault, "a")["state"], "committed");
    assert_eq!(journal_value(&vault, "b")["state"], "committed");
}

#[test]
fn format_2_does_not_gain_the_v3_commit_error_probe() {
    let tmp = Tmp::new("fragment-v2-probe");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    let file = write_note(&tmp, "note.txt", "Amount paid USD 15.99\n");
    let witness = tmp.path().join("witness.json");
    let before = fs::read(vault.join("vault.db")).unwrap();
    let (code, failed) = fragment_probe(
        &vault,
        "v2",
        &file,
        "io_error",
        "commit_error_visible",
        &witness,
    );
    assert_eq!(code, 2, "{failed}");
    assert_eq!(failed["reason"], "unknown_inject_point");
    assert_eq!(failed["point"], "commit_error_visible");
    assert!(!journal_file(&vault, "v2").exists());
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), before);
}

#[test]
fn help_names_correct_and_undo_without_claiming_a_finished_product() {
    let (code, help) = run(&["help"]);
    assert_eq!(code, 0, "{help}");
    let note = help["note"].as_str().unwrap();
    assert!(note.contains("not a user-ready product"));
    assert!(note.contains("Security is not complete"));
    let commands = help["commands"].as_array().unwrap();
    assert!(commands
        .iter()
        .any(|command| command.as_str().unwrap().starts_with("correct ")));
    assert!(commands
        .iter()
        .any(|command| command.as_str().unwrap().starts_with("undo ")));
}
