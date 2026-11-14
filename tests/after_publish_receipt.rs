//! Receipt after `renameat2` when the `after_publish` checkpoint cannot be written.
//!
//! Actual: formats 2 and 3 point `--checkpoint` at a directory whose mode is
//! `000`. The process reaches that write only after the vault directory has
//! been renamed and the parent `fsync` call has returned. The error is
//! `EACCES` from that checkpoint write. It is not an inject, not a full disk,
//! and not a parent `fsync` error. `enospc` is only the existing `io_fail`
//! classification of `ENOSPC`; this file does not produce `ENOSPC`.
//! `directory_durable=false` here means final directory durability is
//! unconfirmed. `parent_fsync_returned_ok=true` records the earlier call's
//! `Ok`. Those two fields had no old definition. This does not show that the
//! rename fd and the later path-reopened fd are the same handle.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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
            "subvault-afterpub-{label}-{}-{}",
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

fn stages(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(".subvault-init-"))
        .collect()
}

#[test]
fn format_1_still_rejects_after_publish_before_a_directory_exists() {
    let tmp = Tmp::new("fmt1");
    let vault = tmp.path().join("vault");
    let checkpoint = tmp.path().join("point.json");
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "1",
        "--stop-at",
        "after_publish",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "format_1_has_no_version_hook");
    assert_eq!(body["created_new_vault"], false);
    assert_ne!(body.get("published"), Some(&serde_json::json!(true)));
    assert!(body.get("preservation").is_none(), "{body}");
    assert!(body.get("staging_cleanup").is_none(), "{body}");
    assert!(!vault.exists());
    assert!(stages(tmp.path()).is_empty());
    assert!(!checkpoint.exists());
}

#[test]
fn checkpoint_eacces_after_rename_stays_published() {
    for format in ["2", "3"] {
        let tmp = Tmp::new(&format!("eacces{format}"));
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
            "after_publish",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ]);
        assert_eq!(code, 3, "{body}");
        assert_ne!(code, 15, "{body}");
        assert_eq!(body["outcome"], "Initialized", "{body}");
        assert_ne!(body["outcome"], "NotPreserved", "{body}");
        assert_eq!(body["reason"], "io_error", "{body}");
        assert_ne!(body["reason"], "directory_durability_unknown", "{body}");
        assert_eq!(body["during"], "checkpoint", "{body}");
        assert_ne!(body["during"], "publish_dir_fsync", "{body}");
        assert_eq!(body["injected"], false, "{body}");
        assert!(body.get("inject_point").is_none(), "{body}");
        let os_error = body["os_error"].as_str().unwrap();
        assert!(
            os_error.contains("Permission denied"),
            "expected EACCES text, got {os_error}"
        );
        assert_eq!(body["published"], true, "{body}");
        assert_eq!(body["created_new_vault"], true, "{body}");
        assert_eq!(body["new_effect"], true, "{body}");
        assert_eq!(body["directory_durable"], false, "{body}");
        assert_eq!(body["parent_fsync_returned_ok"], true, "{body}");
        assert_eq!(body["output"], requested, "{body}");
        assert_eq!(body["vault_format"], format.parse::<i64>().unwrap());
        assert!(body.get("preservation").is_none(), "{body}");
        assert!(body.get("staging_cleanup").is_none(), "{body}");
        assert!(body.get("staging_path").is_none(), "{body}");
        assert!(vault.join("vault.db").is_file(), "{body}");
        assert!(vault.join("SYNTHETIC_ONLY").is_file());
        assert!(vault.join("writer.lock").is_file());
        assert!(stages(tmp.path()).is_empty(), "{:?}", stages(tmp.path()));
        assert!(!checkpoint.exists());
        let bytes = fs::read(vault.join("vault.db")).unwrap();

        let (code, opened) = run(&["open", "--vault", requested]);
        assert_eq!(code, 0, "{opened}");
        assert_eq!(opened["outcome"], "Ready");
        assert_eq!(opened["vault_id"], body["vault_id"]);

        let (code, again) = run(&["init", "--vault", requested, "--format", format]);
        assert_eq!(code, 16, "{again}");
        assert_eq!(again["reason"], "vault_path_exists");
        assert_eq!(again["created_new_vault"], false);
        assert_ne!(again["outcome"], "Initialized");
        assert_eq!(fs::read(vault.join("vault.db")).unwrap(), bytes);
        assert!(stages(tmp.path()).is_empty(), "{:?}", stages(tmp.path()));
    }
}
