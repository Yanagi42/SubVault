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
    assert_eq!(body["created_new_vault"], false);
    assert!(!unpublished.exists());

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
    assert!(!vault3.exists());

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
        "4",
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "bad_format");
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
