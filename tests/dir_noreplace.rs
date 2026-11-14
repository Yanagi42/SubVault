use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
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

#[derive(Clone, Copy)]
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

const PATH_CLEANUP_REASON: &str = "automatic path deletion is disabled because a path is not ownership; this is the original staging path, and its current location and current owner are unknown";

fn assert_init_cleanup(body: &Value, stage: &Path) {
    assert_eq!(body["staging_cleanup"], "not_attempted", "{body}");
    assert_eq!(
        body["staging_cleanup_reason"], PATH_CLEANUP_REASON,
        "{body}"
    );
    assert_eq!(body["staging_path"], stage.display().to_string(), "{body}");
    assert!(body.get("staging_residue").is_none(), "{body}");
    assert!(body.get("staging_cleanup_error").is_none(), "{body}");
}

fn assert_export_cleanup(body: &Value, stage: &Path) {
    assert_eq!(body["output_cleanup"], "not_attempted", "{body}");
    assert_eq!(body["output_cleanup_reason"], PATH_CLEANUP_REASON, "{body}");
    assert_eq!(
        body["output_staging_path"],
        stage.display().to_string(),
        "{body}"
    );
    assert_eq!(body["output_written"], false, "{body}");
    assert_eq!(body["completeness"], "incomplete", "{body}");
    assert!(body.get("output_cleanup_error").is_none(), "{body}");
}

fn file_snapshot(root: &Path) -> Vec<(String, (u64, u64), &'static str, String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let meta = fs::symlink_metadata(&path).unwrap();
            assert!(
                !meta.file_type().is_symlink(),
                "unexpected symlink {}",
                path.display()
            );
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let ino = (meta.dev(), meta.ino());
            if meta.is_dir() {
                out.push((rel, ino, "dir", String::new(), Vec::new()));
                dirs.push(path);
                continue;
            }
            let bytes = fs::read(&path).unwrap();
            out.push((rel, ino, "file", sha256(&path), bytes));
        }
    }
    out.sort();
    out
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
            let staged = dev_ino(&staging);
            let files = file_snapshot(&staging);
            assert!(files
                .iter()
                .any(|(rel, _, kind, _, _)| rel == "SYNTHETIC_ONLY" && *kind == "file"));
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
            assert_init_cleanup(&body, &staging);
            assert_mark(&vault, &mark);
            assert_eq!(dev_ino(&staging), staged);
            assert_eq!(file_snapshot(&staging), files);
            assert!(!vault.join("SYNTHETIC_ONLY").exists());
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
        let staging = one_staging(tmp.path(), ".export-");
        let staged = dev_ino(&staging);
        let files = file_snapshot(&staging);
        assert!(files
            .iter()
            .any(|(rel, _, kind, _, _)| rel == "manifest.json" && *kind == "file"));
        let mark = plant(&out, competitor);
        let (code, body) = child.resume();
        assert_eq!(code, 16, "{body}");
        assert_eq!(body["outcome"], "Rejected");
        assert_eq!(body["reason"], "output_exists");
        assert_eq!(body["during"], "export_publish");
        assert_eq!(body["injected"], false);
        assert_export_cleanup(&body, &staging);
        assert_mark(&out, &mark);
        assert_eq!(dev_ino(&staging), staged);
        assert_eq!(file_snapshot(&staging), files);
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
    assert!(body.get("staging_cleanup").is_none(), "{body}");
    assert!(body.get("staging_path").is_none(), "{body}");
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
        assert!(body.get("staging_cleanup").is_none(), "{body}");
        assert!(body.get("staging_path").is_none(), "{body}");
        assert!(body.get("staging_cleanup_reason").is_none(), "{body}");
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
    assert!(body.get("output_cleanup_reason").is_none(), "{body}");
    assert!(body.get("output_staging_path").is_none(), "{body}");
    assert_eq!(dev_ino(&out), staged);
    assert!(names(tmp.path(), ".export-").is_empty());
    assert!(out.join("manifest.json").is_file());
    assert!(!out.join("vault.db").exists());
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
}

#[test]
fn a_failed_build_keeps_the_stage_and_the_main_error() {
    for (format, inject, reason, point) in [
        (
            "2",
            "enospc:version_checkpoint",
            "injected_enospc",
            "version_checkpoint",
        ),
        (
            "3",
            "io_error:version_verify",
            "injected_io_error",
            "version_verify",
        ),
    ] {
        let tmp = Tmp::new(&format!("build{format}"));
        let vault = tmp.path().join("vault");
        let witness = tmp.path().join("witness.json");
        let (code, body) = run(&[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            format,
            "--inject",
            inject,
            "--witness",
            witness.to_str().unwrap(),
        ]);
        assert_eq!(code, 15, "{body}");
        assert_eq!(body["outcome"], "NotPreserved");
        assert_eq!(body["reason"], reason);
        assert_eq!(body["injected"], true);
        assert_eq!(body["inject_point"], point);
        assert_eq!(body["preservation"], "not_preserved");
        assert_eq!(body["created_new_vault"], false);
        assert!(body.get("published").is_none(), "{body}");
        assert!(!vault.exists());
        let stage = PathBuf::from(body["staging_path"].as_str().unwrap());
        assert_init_cleanup(&body, &stage);
        assert!(stage.join("SYNTHETIC_ONLY").is_file());
        assert!(stage.join("vault.db").is_file());
        let seen: Value = serde_json::from_str(&fs::read_to_string(&witness).unwrap()).unwrap();
        assert_eq!(seen["reached"], true);
        assert_eq!(seen["point"], point);
    }

    let tmp = Tmp::new("export-build");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    let vault_db = fs::read(vault.join("vault.db")).unwrap();
    let mut seen_stages = Vec::new();
    for (action, reason) in [
        ("io_error", "injected_io_error"),
        ("enospc", "injected_enospc"),
    ] {
        let out = tmp.path().join(action);
        let witness = tmp.path().join(format!("{action}.json"));
        let (code, body) = run(&[
            "export",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--inject",
            &format!("{action}:export_write"),
            "--witness",
            witness.to_str().unwrap(),
        ]);
        assert_eq!(code, 3, "{body}");
        assert_eq!(body["outcome"], "Incomplete");
        assert_eq!(body["reason"], reason);
        assert_eq!(body["during"], "export_write");
        assert_eq!(body["injected"], true);
        assert!(!out.exists());
        let stage = PathBuf::from(body["output_staging_path"].as_str().unwrap());
        assert_export_cleanup(&body, &stage);
        assert!(stage.is_dir());
        assert_eq!(fs::symlink_metadata(&stage).unwrap().is_dir(), true);
        seen_stages.push(stage);
        let seen: Value = serde_json::from_str(&fs::read_to_string(&witness).unwrap()).unwrap();
        assert_eq!(seen["point"], "export_write");
        assert_eq!(seen["action"], action);
    }
    assert_eq!(seen_stages.len(), 2);
    assert_ne!(seen_stages[0], seen_stages[1]);
    assert!(seen_stages.iter().all(|stage| stage.is_dir()));
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
}

fn plant_replacement(stage: &Path) -> ((u64, u64), (u64, u64), String) {
    fs::create_dir(stage).unwrap();
    let mine = stage.join("ONLY-MINE");
    fs::write(&mine, b"ONLY-MINE\n").unwrap();
    (dev_ino(stage), dev_ino(&mine), sha256(&mine))
}

#[test]
fn a_replaced_stage_name_stays_with_the_moved_stage_and_the_destination() {
    for format in ["1", "2", "3"] {
        for competitor in [
            Competitor::Empty,
            Competitor::NonEmpty,
            Competitor::File,
            Competitor::Symlink,
        ] {
            let tmp = Tmp::new(&format!("swap{format}"));
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
            assert!(!vault.exists());
            let staging = one_staging(tmp.path(), ".subvault-init-");
            let staged = dev_ino(&staging);
            let files = file_snapshot(&staging);
            let moved = tmp.path().join(format!(
                "moved-{}",
                staging.file_name().unwrap().to_string_lossy()
            ));
            fs::rename(&staging, &moved).unwrap();
            let (replacement, mine_ino, mine_sha) = plant_replacement(&staging);
            let mark = plant(&vault, competitor);
            let (code, body) = child.resume();
            assert_eq!(code, 16, "format {format} {}: {body}", mark.kind);
            assert_eq!(body["outcome"], "Rejected");
            assert_eq!(body["reason"], "vault_path_exists");
            assert_eq!(body["during"], "init_rename");
            assert_eq!(body["created_new_vault"], false);
            assert_eq!(body["injected"], false);
            assert!(body.get("preservation").is_none(), "{body}");
            assert_init_cleanup(&body, &staging);
            assert_eq!(dev_ino(&moved), staged, "moved stage inode");
            assert_eq!(file_snapshot(&moved), files, "moved stage bytes");
            assert_eq!(dev_ino(&staging), replacement, "replacement inode");
            assert_eq!(dev_ino(&staging.join("ONLY-MINE")), mine_ino);
            assert_eq!(sha256(&staging.join("ONLY-MINE")), mine_sha);
            assert_eq!(fs::read(staging.join("ONLY-MINE")).unwrap(), b"ONLY-MINE\n");
            assert_mark(&vault, &mark);
        }
    }

    for competitor in [
        Competitor::Empty,
        Competitor::NonEmpty,
        Competitor::File,
        Competitor::Symlink,
    ] {
        let tmp = Tmp::new(&format!("swap-export-{}", competitor_label(competitor)));
        let vault = tmp.path().join("vault");
        let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
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
        let seen: Value = serde_json::from_str(&fs::read_to_string(&checkpoint).unwrap()).unwrap();
        assert_eq!(seen["point"], "before_dir_publish");
        assert_eq!(seen["published"], false);
        assert!(!out.exists());
        let staging = one_staging(tmp.path(), ".export-");
        let staged = dev_ino(&staging);
        let files = file_snapshot(&staging);
        let moved = tmp.path().join(format!(
            "moved-{}",
            staging.file_name().unwrap().to_string_lossy()
        ));
        fs::rename(&staging, &moved).unwrap();
        let (replacement, mine_ino, mine_sha) = plant_replacement(&staging);
        let mark = plant(&out, competitor);
        let (code, body) = child.resume();
        assert_eq!(code, 16, "{}: {body}", mark.kind);
        assert_eq!(body["outcome"], "Rejected");
        assert_eq!(body["reason"], "output_exists");
        assert_eq!(body["during"], "export_publish");
        assert_eq!(body["injected"], false);
        assert_export_cleanup(&body, &staging);
        assert_eq!(dev_ino(&moved), staged, "moved export stage inode");
        assert_eq!(file_snapshot(&moved), files, "moved export stage bytes");
        assert_eq!(dev_ino(&staging), replacement, "replacement inode");
        assert_eq!(dev_ino(&staging.join("ONLY-MINE")), mine_ino);
        assert_eq!(sha256(&staging.join("ONLY-MINE")), mine_sha);
        assert_eq!(fs::read(staging.join("ONLY-MINE")).unwrap(), b"ONLY-MINE\n");
        assert_mark(&out, &mark);
        assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
    }
}

fn competitor_label(competitor: Competitor) -> &'static str {
    match competitor {
        Competitor::Empty => "empty",
        Competitor::NonEmpty => "nonempty",
        Competitor::File => "file",
        Competitor::Symlink => "symlink",
    }
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
        assert!(body.get("staging_cleanup").is_none(), "{body}");
        assert!(body.get("staging_path").is_none(), "{body}");
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
    assert!(body.get("output_cleanup").is_none(), "{body}");
    assert!(body.get("output_staging_path").is_none(), "{body}");
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
    assert!(body.get("staging_path").is_none(), "{body}");
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
    assert!(body.get("output_staging_path").is_none(), "{body}");
    assert!(names(tmp.path(), ".export-").is_empty());
    assert!(out.join("records.json").is_file());
    let seen: Value = serde_json::from_str(&fs::read_to_string(&witness).unwrap()).unwrap();
    assert_eq!(seen["published"], true);
}

fn linux_backslash_name(parent: &Path, name: &str) -> PathBuf {
    assert!(name.contains('\\'), "{name}");
    assert!(!name.contains('/'), "{name}");
    let path = parent.join(name);
    assert_eq!(path.file_name(), Some(OsStr::new(name)));
    path
}

#[test]
fn a_backslash_is_one_linux_name_for_a_new_init_and_export() {
    for format in ["1", "2", "3"] {
        let tmp = Tmp::new(&format!("bs{format}"));
        let vault = linux_backslash_name(tmp.path(), "a\\b");
        let (code, body) = run(&[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            format,
        ]);
        assert_eq!(code, 0, "{body}");
        assert_eq!(body["outcome"], "Initialized");
        assert_eq!(body["created_new_vault"], true);
        assert_eq!(body["vault_format"], format.parse::<i64>().unwrap());
        assert!(body.get("staging_cleanup").is_none(), "{body}");
        assert!(body.get("staging_path").is_none(), "{body}");
        assert!(vault.join("vault.db").is_file(), "{body}");
        assert!(vault.join("SYNTHETIC_ONLY").is_file());
        assert!(!tmp.path().join("a").exists());
        assert!(!tmp.path().join("b").exists());
        let (opened, ready) = run(&["open", "--vault", vault.to_str().unwrap()]);
        assert_eq!(opened, 0, "{ready}");
        assert_eq!(ready["outcome"], "Ready");
    }

    let tmp = Tmp::new("bs-export");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    let vault_db = fs::read(vault.join("vault.db")).unwrap();
    let out = linux_backslash_name(tmp.path(), "p\\k");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Exported");
    assert_eq!(body["output_written"], true);
    assert!(body.get("output_cleanup").is_none(), "{body}");
    assert!(body.get("output_staging_path").is_none(), "{body}");
    assert!(out.join("manifest.json").is_file(), "{body}");
    assert!(out.join("records.json").is_file());
    assert!(!out.join("vault.db").exists());
    assert!(!tmp.path().join("p").exists());
    assert!(!tmp.path().join("k").exists());
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
}

#[test]
fn a_backslash_name_keeps_a_competitor_that_is_already_there_or_appears_after_the_check() {
    let tmp = Tmp::new("bs-early");
    let vault = linux_backslash_name(tmp.path(), "a\\b");
    fs::create_dir(&vault).unwrap();
    fs::write(vault.join("KEEP"), b"KEEP\n").unwrap();
    let entry = dev_ino(&vault);
    let keep = dev_ino(&vault.join("KEEP"));
    let hash = sha256(&vault.join("KEEP"));
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(body["created_new_vault"], false);
    assert!(body.get("during").is_none(), "{body}");
    assert!(body.get("preservation").is_none(), "{body}");
    assert!(body.get("staging_cleanup").is_none(), "{body}");
    assert!(body.get("staging_path").is_none(), "{body}");
    assert_eq!(dev_ino(&vault), entry);
    assert_eq!(dev_ino(&vault.join("KEEP")), keep);
    assert_eq!(sha256(&vault.join("KEEP")), hash);
    assert_eq!(fs::read(vault.join("KEEP")).unwrap(), b"KEEP\n");
    assert!(!vault.join("vault.db").exists());
    assert!(names(tmp.path(), ".subvault-init-").is_empty());

    for competitor in [
        Competitor::Empty,
        Competitor::NonEmpty,
        Competitor::File,
        Competitor::Symlink,
    ] {
        let tmp = Tmp::new("bs-race");
        let vault = linux_backslash_name(tmp.path(), "a\\b");
        let checkpoint = tmp.path().join("checkpoint.json");
        let mut child = spawn(
            &[
                "init",
                "--vault",
                vault.to_str().unwrap(),
                "--format",
                "1",
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
        assert_eq!(seen["vault_format"], 1);
        assert!(!vault.exists());
        let staging = one_staging(tmp.path(), ".subvault-init-");
        let staged = dev_ino(&staging);
        let files = file_snapshot(&staging);
        let mark = plant(&vault, competitor);
        let (code, body) = child.resume();
        assert_eq!(code, 16, "{}: {body}", mark.kind);
        assert_eq!(body["reason"], "vault_path_exists");
        assert_eq!(body["during"], "init_rename");
        assert_eq!(body["created_new_vault"], false);
        assert_eq!(body["injected"], false);
        assert!(body.get("preservation").is_none(), "{body}");
        assert_init_cleanup(&body, &staging);
        assert_mark(&vault, &mark);
        assert_eq!(dev_ino(&staging), staged);
        assert_eq!(file_snapshot(&staging), files);
        assert!(!tmp.path().join("a").exists());
        assert!(!tmp.path().join("b").exists());
    }

    let tmp = Tmp::new("bs-export-race");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    let vault_db = fs::read(vault.join("vault.db")).unwrap();
    let out = linux_backslash_name(tmp.path(), "p\\k");
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
    let staging = one_staging(tmp.path(), ".export-");
    let staged = dev_ino(&staging);
    let files = file_snapshot(&staging);
    fs::create_dir(&out).unwrap();
    fs::write(out.join("KEEP"), b"EXPORT-KEEP\n").unwrap();
    let entry = dev_ino(&out);
    let keep = dev_ino(&out.join("KEEP"));
    let hash = sha256(&out.join("KEEP"));
    let (code, body) = child.resume();
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_exists");
    assert_eq!(body["during"], "export_publish");
    assert_eq!(body["injected"], false);
    assert_export_cleanup(&body, &staging);
    assert_eq!(dev_ino(&out), entry);
    assert_eq!(dev_ino(&out.join("KEEP")), keep);
    assert_eq!(sha256(&out.join("KEEP")), hash);
    assert_eq!(fs::read(out.join("KEEP")).unwrap(), b"EXPORT-KEEP\n");
    assert!(!out.join("manifest.json").exists());
    assert_eq!(dev_ino(&staging), staged);
    assert_eq!(file_snapshot(&staging), files);
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
    assert!(!tmp.path().join("p").exists());
    assert!(!tmp.path().join("k").exists());
}
