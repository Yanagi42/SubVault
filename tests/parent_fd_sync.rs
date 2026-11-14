//! The publish parent file stays open across rename and is the file `sync_all` uses.
//!
//! `before_parent_sync` stops after `renameat2` returns and before that sync.
//! While the process is in state T, the test moves the parent directory and
//! puts a different directory at the old path. `/proc/<pid>/fd` is the open
//! file. Its device and inode stay the moved parent, not the replacement.
//! The next call after SIGCONT is `sync_all` on that same file. This file
//! does not trace the syscall. Exit 0 by itself is not the check.
//!
//! A real `sync_all` error on an already-open directory fd was not produced:
//! chmod 000 and a replaced path both left `fsync` returning success. The
//! sync-failure receipt is the static mapping in the library tests. The
//! checkpoint-write error below is EACCES on that write, not an fsync failure.
//! The inject tests are a witness after sync has already returned Ok.

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
            "subvault-parentfd-{label}-{}-{}",
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

struct RestoredDir {
    path: PathBuf,
}

impl Drop for RestoredDir {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o755));
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

fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(bin()).args(args).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{stderr}")
    });
    assert!(stderr.is_empty(), "stderr:\n{stderr}");
    (output.status.code().unwrap_or(101), body)
}

fn dev_ino(path: &Path) -> (u64, u64) {
    let meta = fs::symlink_metadata(path).unwrap();
    (meta.dev(), meta.ino())
}

fn fd_ids(pid: u32) -> Vec<(u64, u64)> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fd")).unwrap() {
        let path = entry.unwrap().path();
        if let Ok(meta) = fs::metadata(&path) {
            ids.push((meta.dev(), meta.ino()));
        }
    }
    ids
}

fn stages(dir: &Path, prefix: &str) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(prefix))
        .collect()
}

fn id_u64(value: &Value, key: &str) -> u64 {
    value[key]
        .as_u64()
        .unwrap_or_else(|| panic!("{key} in {value}"))
}

#[test]
fn init_syncs_the_moved_parent_file_not_the_replacement_directory() {
    for format in ["1", "2", "3"] {
        let tmp = Tmp::new(&format!("init{format}"));
        let parent = tmp.path().join("publish-parent");
        fs::create_dir(&parent).unwrap();
        let vault = parent.join("vault");
        let held = tmp.path().join("held-parent");
        let checkpoint = tmp.path().join("point.json");
        let requested = vault.to_str().unwrap();
        let mut child = spawn(
            &[
                "init",
                "--vault",
                requested,
                "--format",
                format,
                "--stop-at",
                "before_parent_sync",
                "--checkpoint",
                checkpoint.to_str().unwrap(),
            ],
            &checkpoint,
        );
        let seen: Value = serde_json::from_str(&fs::read_to_string(&checkpoint).unwrap()).unwrap();
        assert_eq!(seen["point"], "before_parent_sync");
        assert_eq!(seen["published"], true);
        assert_eq!(seen["vault_format"], format.parse::<i64>().unwrap());
        let handle = (id_u64(&seen, "parent_dev"), id_u64(&seen, "parent_ino"));
        assert_eq!(
            dev_ino(&parent),
            handle,
            "checkpoint fstat is not the publish parent"
        );
        let before = fd_ids(child.pid());
        assert!(
            before.contains(&handle),
            "open fd list does not contain the publish parent {handle:?}\n{before:?}"
        );
        fs::rename(&parent, &held).unwrap();
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join("MARKER"), b"REPLACEMENT\n").unwrap();
        let replacement = dev_ino(&parent);
        assert_ne!(replacement, handle);
        assert_eq!(dev_ino(&held), handle);
        let after = fd_ids(child.pid());
        assert!(
            after.contains(&handle),
            "after the path was replaced, the open fd is no longer the moved parent\n{after:?}"
        );
        assert!(
            !after.contains(&replacement),
            "the open fd table includes the replacement directory {replacement:?}\n{after:?}"
        );
        let db = held.join("vault").join("vault.db");
        let bytes = fs::read(&db).unwrap();
        assert!(bytes.starts_with(b"SQLite format 3\0"));
        let banner = fs::read(held.join("vault").join("SYNTHETIC_ONLY")).unwrap();

        let (code, body) = child.resume();
        assert_eq!(code, 0, "{body}");
        assert_eq!(body["outcome"], "Initialized");
        assert_eq!(body["vault_format"], format.parse::<i64>().unwrap());
        assert!(body.get("output").is_none(), "{body}");
        assert!(body.get("parent_dev").is_none(), "{body}");
        assert!(body.get("parent_ino").is_none(), "{body}");
        assert!(body.get("staging_cleanup").is_none(), "{body}");
        let rendered = serde_json::to_string(&body).unwrap();
        assert!(
            !rendered.contains("held-parent"),
            "receipt names the moved parent: {rendered}"
        );
        assert_eq!(fs::read(&db).unwrap(), bytes);
        assert_eq!(
            fs::read(held.join("vault").join("SYNTHETIC_ONLY")).unwrap(),
            banner
        );
        assert_eq!(fs::read(parent.join("MARKER")).unwrap(), b"REPLACEMENT\n");
        assert!(!parent.join("vault").exists());
        assert!(stages(tmp.path(), ".subvault-init-").is_empty());
        let (code, opened) = run(&["open", "--vault", held.join("vault").to_str().unwrap()]);
        assert_eq!(code, 0, "{opened}");
        assert_eq!(opened["outcome"], "Ready");
        assert_eq!(opened["vault_id"], body["vault_id"]);
        let (code, stale) = run(&["open", "--vault", requested]);
        assert_ne!(code, 0, "{stale}");
        assert_ne!(stale["outcome"], "Ready");
    }
}

#[test]
fn export_syncs_the_moved_parent_file_not_the_replacement_directory() {
    let tmp = Tmp::new("export");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    let parent = tmp.path().join("package-parent");
    fs::create_dir(&parent).unwrap();
    let out = parent.join("pkg");
    let held = tmp.path().join("held-package-parent");
    let checkpoint = tmp.path().join("point.json");
    let requested = out.to_str().unwrap();
    let mut child = spawn(
        &[
            "export",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            requested,
            "--stop-at",
            "before_parent_sync",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ],
        &checkpoint,
    );
    let seen: Value = serde_json::from_str(&fs::read_to_string(&checkpoint).unwrap()).unwrap();
    assert_eq!(seen["point"], "before_parent_sync");
    assert_eq!(seen["published"], true);
    let handle = (id_u64(&seen, "parent_dev"), id_u64(&seen, "parent_ino"));
    assert_eq!(dev_ino(&parent), handle);
    assert!(fd_ids(child.pid()).contains(&handle));
    fs::rename(&parent, &held).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::write(parent.join("MARKER"), b"REPLACEMENT\n").unwrap();
    let replacement = dev_ino(&parent);
    assert_ne!(replacement, handle);
    assert_eq!(dev_ino(&held), handle);
    let after = fd_ids(child.pid());
    assert!(after.contains(&handle), "{after:?}");
    assert!(!after.contains(&replacement), "{after:?}");
    let records = fs::read(held.join("pkg").join("records.json")).unwrap();
    let manifest = fs::read(held.join("pkg").join("manifest.json")).unwrap();

    let (code, body) = child.resume();
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Exported");
    assert_eq!(body["output_written"], true);
    assert_eq!(body["output"], requested);
    assert!(body.get("parent_dev").is_none(), "{body}");
    assert!(body.get("output_cleanup").is_none(), "{body}");
    let rendered = serde_json::to_string(&body).unwrap();
    assert!(
        !rendered.contains("held-package-parent"),
        "receipt tracks the moved parent: {rendered}"
    );
    assert_eq!(
        fs::read(held.join("pkg").join("records.json")).unwrap(),
        records
    );
    assert_eq!(
        fs::read(held.join("pkg").join("manifest.json")).unwrap(),
        manifest
    );
    assert!(!held.join("pkg").join("vault.db").exists());
    assert_eq!(fs::read(parent.join("MARKER")).unwrap(), b"REPLACEMENT\n");
    assert!(!parent.join("pkg").exists());
    assert!(stages(tmp.path(), ".export-").is_empty());
}

#[test]
fn checkpoint_write_before_parent_sync_stays_published_and_is_not_an_fsync_error() {
    for format in ["1", "2", "3"] {
        let tmp = Tmp::new(&format!("point{format}"));
        let vault = tmp.path().join("vault");
        let blocked = tmp.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        let _restore = RestoredDir {
            path: blocked.clone(),
        };
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        let checkpoint = blocked.join("point.json");
        let requested = vault.to_str().unwrap();
        let (code, body) = run(&[
            "init",
            "--vault",
            requested,
            "--format",
            format,
            "--stop-at",
            "before_parent_sync",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ]);
        assert_eq!(code, 3, "{body}");
        assert_ne!(code, 15, "{body}");
        assert_eq!(body["outcome"], "Initialized");
        assert_eq!(body["reason"], "io_error", "{body}");
        assert_ne!(body["reason"], "directory_durability_unknown");
        assert_eq!(body["during"], "checkpoint", "{body}");
        assert_ne!(body["during"], "publish_dir_fsync", "{body}");
        assert_eq!(body["injected"], false, "{body}");
        let os_error = body["os_error"].as_str().unwrap();
        assert!(
            os_error.contains("Permission denied"),
            "checkpoint write EACCES, not an fsync errno: {os_error}"
        );
        assert_eq!(body["published"], true, "{body}");
        assert_eq!(body["created_new_vault"], true, "{body}");
        assert_eq!(body["new_effect"], true, "{body}");
        assert_eq!(body["directory_durable"], false, "{body}");
        assert_eq!(body["parent_fsync_returned_ok"], false, "{body}");
        assert_eq!(body["output"], requested, "{body}");
        assert!(body.get("preservation").is_none(), "{body}");
        assert!(body.get("staging_cleanup").is_none(), "{body}");
        assert!(vault.join("vault.db").is_file(), "{body}");
        assert!(stages(tmp.path(), ".subvault-init-").is_empty());
        assert!(!checkpoint.exists());
        let (code, opened) = run(&["open", "--vault", requested]);
        assert_eq!(code, 0, "{opened}");
        assert_eq!(opened["outcome"], "Ready");
        assert_eq!(opened["vault_id"], body["vault_id"]);
    }
}
