use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-noreplace-{label}-{}-{}",
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
    child: Option<Child>,
}

impl Stopped {
    fn wait(mut child: Child, checkpoint: &Path) -> Self {
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
                return Self { child: Some(child) };
            }
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                panic!("pid {} was not observed stopped", child.id());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn pid(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    fn resume(&mut self) -> (i32, Value) {
        let pid = self.pid();
        assert!(stopped(pid), "refusing SIGCONT before State T");
        assert!(Command::new("kill")
            .args(["-CONT", &pid.to_string()])
            .status()
            .unwrap()
            .success());
        let child = self.child.take().unwrap();
        let output = child.wait_with_output().unwrap();
        let code = output.status.code().unwrap_or(101);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
            panic!(
                "stdout is not one JSON object ({err})\nstdout:\n{stdout}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert!(
            output.stderr.is_empty(),
            "stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        (code, body)
    }
}

impl Drop for Stopped {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = Command::new("kill")
                .args(["-CONT", &child.id().to_string()])
                .status();
            let _ = child.kill();
            let _ = child.wait();
        }
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

fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(bin()).args(args).output().unwrap();
    let code = output.status.code().unwrap_or(101);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not one JSON object ({err})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (code, body)
}

fn dev_ino(path: &Path) -> (u64, u64) {
    let meta = fs::symlink_metadata(path).unwrap();
    (meta.dev(), meta.ino())
}

fn sha256(path: &Path) -> String {
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

fn names(dir: &Path, prefix: &str) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(prefix))
        .collect()
}

fn one_staging(dir: &Path, prefix: &str) -> PathBuf {
    let found = names(dir, prefix);
    assert_eq!(found.len(), 1, "{found:?}");
    dir.join(&found[0])
}

fn spawn(args: &[&str], checkpoint: &Path) -> Stopped {
    let child = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    Stopped::wait(child, checkpoint)
}

enum Competitor {
    Empty,
    NonEmpty,
    File,
    Symlink,
}

struct Mark {
    kind: &'static str,
    entry: (u64, u64),
    keep_ino: Option<(u64, u64)>,
    keep_sha: Option<String>,
    link: Option<PathBuf>,
    target_ino: Option<(u64, u64)>,
    target_sha: Option<String>,
}

fn plant(dir: &Path, competitor: Competitor) -> Mark {
    match competitor {
        Competitor::Empty => {
            fs::create_dir(dir).unwrap();
            Mark {
                kind: "empty",
                entry: dev_ino(dir),
                keep_ino: None,
                keep_sha: None,
                link: None,
                target_ino: None,
                target_sha: None,
            }
        }
        Competitor::NonEmpty => {
            fs::create_dir(dir).unwrap();
            fs::write(dir.join("KEEP"), b"COMPETITOR-KEEP\n").unwrap();
            Mark {
                kind: "nonempty",
                entry: dev_ino(dir),
                keep_ino: Some(dev_ino(&dir.join("KEEP"))),
                keep_sha: Some(sha256(&dir.join("KEEP"))),
                link: None,
                target_ino: None,
                target_sha: None,
            }
        }
        Competitor::File => {
            fs::write(dir, b"FILE-KEEP\n").unwrap();
            Mark {
                kind: "file",
                entry: dev_ino(dir),
                keep_ino: None,
                keep_sha: Some(sha256(dir)),
                link: None,
                target_ino: None,
                target_sha: None,
            }
        }
        Competitor::Symlink => {
            fs::write(dir.with_file_name("elsewhere"), b"SYMLINK-TARGET\n").unwrap();
            std::os::unix::fs::symlink("elsewhere", dir).unwrap();
            Mark {
                kind: "symlink",
                entry: dev_ino(dir),
                keep_ino: None,
                keep_sha: None,
                link: Some(fs::read_link(dir).unwrap()),
                target_ino: Some(dev_ino(&dir.with_file_name("elsewhere"))),
                target_sha: Some(sha256(&dir.with_file_name("elsewhere"))),
            }
        }
    }
}

fn assert_mark(dir: &Path, mark: &Mark) {
    assert_eq!(dev_ino(dir), mark.entry, "{}", mark.kind);
    match mark.kind {
        "empty" => {
            assert!(fs::symlink_metadata(dir).unwrap().is_dir());
            assert!(
                fs::read_dir(dir).unwrap().next().is_none(),
                "empty competitor gained entries"
            );
        }
        "nonempty" => {
            assert_eq!(dev_ino(&dir.join("KEEP")), mark.keep_ino.unwrap());
            assert_eq!(sha256(&dir.join("KEEP")), mark.keep_sha.as_deref().unwrap());
            assert_eq!(fs::read(dir.join("KEEP")).unwrap(), b"COMPETITOR-KEEP\n");
        }
        "file" => {
            assert!(fs::symlink_metadata(dir).unwrap().is_file());
            assert_eq!(sha256(dir), mark.keep_sha.as_deref().unwrap());
            assert_eq!(fs::read(dir).unwrap(), b"FILE-KEEP\n");
        }
        "symlink" => {
            assert_eq!(
                fs::read_link(dir).unwrap(),
                mark.link.as_ref().unwrap().as_os_str()
            );
            let target = dir.with_file_name("elsewhere");
            assert_eq!(dev_ino(&target), mark.target_ino.unwrap());
            assert_eq!(sha256(&target), mark.target_sha.as_deref().unwrap());
            assert_eq!(fs::read(&target).unwrap(), b"SYMLINK-TARGET\n");
        }
        other => panic!("unknown mark {other}"),
    }
}

#[test]
fn real_stop_keeps_a_competitor_created_after_the_check() {
    for format in ["1", "2", "3"] {
        for competitor in [
            Competitor::Empty,
            Competitor::NonEmpty,
            Competitor::File,
            Competitor::Symlink,
        ] {
            let tmp = Tmp::new(&format!("init{format}"));
            let vault = tmp.path().join("vault");
            let checkpoint = tmp.path().join("checkpoint.json");
            let mut child = spawn(
                &[
                    "init",
                    "--vault",
                    vault.to_str().unwrap(),
                    "--format",
                    format,
                    "--stop-at",
                    "before_dir_publish",
                    "--checkpoint",
                    checkpoint.to_str().unwrap(),
                ],
                &checkpoint,
            );
            let seen: Value =
                serde_json::from_str(&fs::read_to_string(&checkpoint).unwrap()).unwrap();
            assert_eq!(seen["point"], "before_dir_publish");
            assert_eq!(seen["published"], false);
            assert_eq!(seen["vault_format"], format.parse::<i64>().unwrap());
            assert!(!vault.exists());
            let staging = one_staging(tmp.path(), ".subvault-init-");
            let mark = plant(&vault, competitor);
            let (code, body) = child.resume();
            assert_eq!(code, 16, "format {format} {:?}: {body}", mark.kind);
            assert_eq!(body["outcome"], "Rejected");
            assert_eq!(body["reason"], "vault_path_exists");
            assert_eq!(body["created_new_vault"], false);
            assert_eq!(body["during"], "init_rename");
            assert_eq!(body["injected"], false);
            assert!(body.get("preservation").is_none(), "{body}");
            assert_ne!(body["reason"], "writer_busy");
            assert_mark(&vault, &mark);
            assert!(names(tmp.path(), ".subvault-init-").is_empty(), "{body}");
            assert!(!vault.join("SYNTHETIC_ONLY").exists());
            let _ = staging;
        }
    }
}

#[test]
fn real_export_stop_keeps_the_competitor_and_the_source_vault() {
    for competitor in [
        Competitor::Empty,
        Competitor::NonEmpty,
        Competitor::File,
        Competitor::Symlink,
    ] {
        let tmp = Tmp::new("export");
        let vault = tmp.path().join("vault");
        let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
        assert_eq!(code, 0, "{body}");
        let vault_db = fs::read(vault.join("vault.db")).unwrap();
        let banner = fs::read(vault.join("SYNTHETIC_ONLY")).unwrap();
        let old = tmp.path().join("old-pkg");
        fs::create_dir(&old).unwrap();
        fs::write(old.join("KEEP"), b"OLD-PACKAGE\n").unwrap();
        let old_ino = dev_ino(&old);
        let old_sha = sha256(&old.join("KEEP"));
        let out = tmp.path().join("pkg");
        let checkpoint = tmp.path().join("checkpoint.json");
        let mut child = spawn(
            &[
                "export",
                "--vault",
                vault.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--stop-at",
                "before_dir_publish",
                "--checkpoint",
                checkpoint.to_str().unwrap(),
            ],
            &checkpoint,
        );
        let seen: Value = serde_json::from_str(&fs::read_to_string(&checkpoint).unwrap()).unwrap();
        assert_eq!(seen["point"], "before_dir_publish");
        assert_eq!(seen["published"], false);
        assert!(!out.exists());
        let _staging = one_staging(tmp.path(), ".export-");
        let mark = plant(&out, competitor);
        let (code, body) = child.resume();
        assert_eq!(code, 16, "{body}");
        assert_eq!(body["outcome"], "Rejected");
        assert_eq!(body["reason"], "output_exists");
        assert_eq!(body["output_written"], false);
        assert_eq!(body["during"], "export_publish");
        assert_eq!(body["injected"], false);
        assert_eq!(body["output_cleanup"], "removed");
        assert_mark(&out, &mark);
        assert!(names(tmp.path(), ".export-").is_empty(), "{body}");
        assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
        assert_eq!(fs::read(vault.join("SYNTHETIC_ONLY")).unwrap(), banner);
        assert_eq!(dev_ino(&old), old_ino);
        assert_eq!(sha256(&old.join("KEEP")), old_sha);
        assert_eq!(fs::read(old.join("KEEP")).unwrap(), b"OLD-PACKAGE\n");
    }
}

#[test]
fn a_name_that_exists_before_init_stays_on_the_friendly_path() {
    let tmp = Tmp::new("early");
    let vault = tmp.path().join("vault");
    fs::create_dir(&vault).unwrap();
    let ino = dev_ino(&vault);
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(body["created_new_vault"], false);
    assert!(body.get("during").is_none(), "{body}");
    assert!(body.get("preservation").is_none(), "{body}");
    assert_eq!(dev_ino(&vault), ino);
    assert!(names(tmp.path(), ".subvault-init-").is_empty());
}

#[test]
fn successful_publish_uses_the_staging_inode() {
    for format in ["1", "2", "3"] {
        let tmp = Tmp::new(&format!("ok{format}"));
        let vault = tmp.path().join("vault");
        let checkpoint = tmp.path().join("checkpoint.json");
        let mut child = spawn(
            &[
                "init",
                "--vault",
                vault.to_str().unwrap(),
                "--format",
                format,
                "--stop-at",
                "before_dir_publish",
                "--checkpoint",
                checkpoint.to_str().unwrap(),
            ],
            &checkpoint,
        );
        let staging = one_staging(tmp.path(), ".subvault-init-");
        let staged = dev_ino(&staging);
        let (code, body) = child.resume();
        assert_eq!(code, 0, "{body}");
        assert_eq!(body["outcome"], "Initialized");
        assert_eq!(body["created_new_vault"], true);
        assert_eq!(body["vault_format"], format.parse::<i64>().unwrap());
        assert_eq!(dev_ino(&vault), staged);
        assert!(names(tmp.path(), ".subvault-init-").is_empty());
        assert!(vault.join("SYNTHETIC_ONLY").is_file());
        let (opened, ready) = run(&["open", "--vault", vault.to_str().unwrap()]);
        assert_eq!(opened, 0, "{ready}");
        assert_eq!(ready["outcome"], "Ready");
    }

    let tmp = Tmp::new("export-ok");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    let vault_db = fs::read(vault.join("vault.db")).unwrap();
    let out = tmp.path().join("pkg");
    let checkpoint = tmp.path().join("checkpoint.json");
    let mut child = spawn(
        &[
            "export",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--stop-at",
            "before_dir_publish",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ],
        &checkpoint,
    );
    let staging = one_staging(tmp.path(), ".export-");
    let staged = dev_ino(&staging);
    let (code, body) = child.resume();
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Exported");
    assert_eq!(body["output_written"], true);
    assert!(body.get("output_cleanup").is_none(), "{body}");
    assert_eq!(dev_ino(&out), staged);
    assert!(names(tmp.path(), ".export-").is_empty());
    assert!(out.join("manifest.json").is_file());
    assert!(!out.join("vault.db").exists());
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
}

#[test]
fn cleanup_failure_reports_the_private_directory_and_keeps_the_competitor() {
    let tmp = Tmp::new("init-clean");
    let vault = tmp.path().join("vault");
    let checkpoint = tmp.path().join("checkpoint.json");
    let mut child = spawn(
        &[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            "2",
            "--stop-at",
            "before_dir_publish",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ],
        &checkpoint,
    );
    let staging = one_staging(tmp.path(), ".subvault-init-");
    let mut perms = fs::metadata(&staging).unwrap().permissions();
    perms.set_mode(0o555);
    fs::set_permissions(&staging, perms).unwrap();
    fs::create_dir(&vault).unwrap();
    let ino = dev_ino(&vault);
    let (code, body) = child.resume();
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(body["during"], "init_rename");
    assert_eq!(body["staging_cleanup"], "failed");
    assert_eq!(body["staging_residue"], staging.display().to_string());
    assert!(
        body["staging_cleanup_error"]
            .as_str()
            .unwrap()
            .contains("os error 13"),
        "{body}"
    );
    assert_eq!(dev_ino(&vault), ino);
    assert!(staging.join("SYNTHETIC_ONLY").is_file());
    let mut perms = fs::metadata(&staging).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&staging, perms).unwrap();

    let tmp = Tmp::new("export-clean");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    let vault_db = fs::read(vault.join("vault.db")).unwrap();
    let out = tmp.path().join("pkg");
    let checkpoint = tmp.path().join("checkpoint.json");
    let mut child = spawn(
        &[
            "export",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--stop-at",
            "before_dir_publish",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ],
        &checkpoint,
    );
    let staging = one_staging(tmp.path(), ".export-");
    let mut perms = fs::metadata(&staging).unwrap().permissions();
    perms.set_mode(0o555);
    fs::set_permissions(&staging, perms).unwrap();
    fs::create_dir(&out).unwrap();
    fs::write(out.join("KEEP"), b"STILL-HERE\n").unwrap();
    let ino = dev_ino(&out.join("KEEP"));
    let hash = sha256(&out.join("KEEP"));
    let (code, body) = child.resume();
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_exists");
    assert_eq!(body["output_written"], false);
    assert_eq!(body["output_cleanup"], "failed");
    assert!(
        body["output_cleanup_error"]
            .as_str()
            .unwrap()
            .contains("os error 13"),
        "{body}"
    );
    assert_eq!(dev_ino(&out.join("KEEP")), ino);
    assert_eq!(sha256(&out.join("KEEP")), hash);
    assert!(staging.join("manifest.json").is_file());
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
    let mut perms = fs::metadata(&staging).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&staging, perms).unwrap();
}

#[test]
fn format_1_does_not_gain_the_other_version_hooks() {
    for point in [
        "version_set",
        "version_checkpoint",
        "before_publish",
        "after_publish",
    ] {
        let tmp = Tmp::new(point);
        let vault = tmp.path().join("vault");
        let (code, body) = run(&[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            "1",
            "--stop-at",
            point,
            "--checkpoint",
            tmp.path().join("unused.json").to_str().unwrap(),
        ]);
        assert_eq!(code, 2, "{point}: {body}");
        assert_eq!(body["reason"], "format_1_has_no_version_hook");
        assert_eq!(body["created_new_vault"], false);
        assert!(!vault.exists());
        assert!(names(tmp.path(), ".subvault-init-").is_empty());
    }

    let tmp = Tmp::new("unknown-export");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    let out = tmp.path().join("pkg");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stop-at",
        "export_before_rename",
        "--checkpoint",
        tmp.path().join("unused.json").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "unknown_stop_point");
    assert_eq!(body["point"], "export_before_rename");
    assert!(!out.exists());
    assert!(names(tmp.path(), ".export-").is_empty());
}

#[test]
fn injected_directory_fsync_keeps_the_published_directory() {
    let tmp = Tmp::new("init-inject");
    let vault = tmp.path().join("vault");
    let witness = tmp.path().join("witness.json");
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "1",
        "--inject",
        "io_error:publish_dir_fsync",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 3, "{body}");
    assert_eq!(body["outcome"], "Initialized");
    assert_eq!(body["published"], true);
    assert_eq!(body["directory_durable"], false);
    assert_eq!(body["created_new_vault"], true);
    assert_eq!(body["injected"], true);
    assert!(body.get("staging_cleanup").is_none(), "{body}");
    assert!(body.get("preservation").is_none(), "{body}");
    assert!(names(tmp.path(), ".subvault-init-").is_empty());
    assert!(vault.join("vault.db").is_file());
    let seen: Value = serde_json::from_str(&fs::read_to_string(&witness).unwrap()).unwrap();
    assert_eq!(seen["published"], true);

    let tmp = Tmp::new("export-inject");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    let out = tmp.path().join("pkg");
    let witness = tmp.path().join("witness.json");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--inject",
        "enospc:export_dir_fsync",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 3, "{body}");
    assert_eq!(body["outcome"], "Exported");
    assert_eq!(body["published"], true);
    assert_eq!(body["directory_durable"], false);
    assert_eq!(body["output_written"], true);
    assert_eq!(body["injected"], true);
    assert!(body.get("output_cleanup").is_none(), "{body}");
    assert!(names(tmp.path(), ".export-").is_empty());
    assert!(out.join("records.json").is_file());
    let seen: Value = serde_json::from_str(&fs::read_to_string(&witness).unwrap()).unwrap();
    assert_eq!(seen["published"], true);
}
