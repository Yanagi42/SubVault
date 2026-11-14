use std::fs;
use std::os::unix::fs::{symlink, MetadataExt};
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
            "subvault-initentry-{label}-{}-{}",
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

fn inode(path: &Path) -> u64 {
    fs::symlink_metadata(path).unwrap().ino()
}

fn refused(path: &Path) {
    let (code, body) = run(&["init", "--vault", path.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["outcome"], "Rejected");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(body["created_new_vault"], false);
    assert_eq!(body["new_effect"], false);
    assert!(body.get("preservation").is_none(), "{body}");
    assert_ne!(body["outcome"], "NotPreserved");
}

fn scratch_names(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(".subvault-init-"))
        .collect()
}

#[test]
fn init_refuses_an_existing_final_entry_and_still_creates_a_new_path() {
    let tmp = Tmp::new("entries");
    let root = tmp.path();
    let plain = root.join("plain.txt");
    fs::write(&plain, b"plain-file\n").unwrap();
    let dir = root.join("plaindir");
    fs::create_dir(&dir).unwrap();
    let kept = dir.join("keep.txt");
    fs::write(&kept, b"dir-keep\n").unwrap();
    let vault = root.join("origvault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Initialized");
    assert_eq!(body["created_new_vault"], true);
    let vault_db = fs::read(vault.join("vault.db")).unwrap();
    let banner = fs::read(vault.join("SYNTHETIC_ONLY")).unwrap();

    let dangling = root.join("link-dangling");
    symlink("missing-target", &dangling).unwrap();
    let to_file = root.join("link-file");
    symlink("plain.txt", &to_file).unwrap();
    let to_dir = root.join("link-dir");
    symlink("plaindir", &to_dir).unwrap();
    let to_vault = root.join("link-vault");
    symlink("origvault", &to_vault).unwrap();
    let to_db = root.join("link-vaultdb");
    symlink(vault.join("vault.db"), &to_db).unwrap();
    let looping = root.join("link-loop");
    symlink("link-loop", &looping).unwrap();

    let links = [
        (&dangling, "missing-target"),
        (&to_file, "plain.txt"),
        (&to_dir, "plaindir"),
        (&to_vault, "origvault"),
        (&looping, "link-loop"),
    ];
    let before: Vec<_> = links
        .iter()
        .map(|(path, target)| {
            (
                inode(path),
                fs::read_link(path).unwrap(),
                (*target).to_string(),
            )
        })
        .collect();
    let db_link_ino = inode(&to_db);
    let db_link_target = fs::read_link(&to_db).unwrap();
    let plain_ino = inode(&plain);
    let dir_ino = inode(&dir);
    let vault_ino = inode(&vault);

    for (path, _) in links {
        refused(path);
    }
    refused(&to_db);
    refused(&plain);
    refused(&dir);
    refused(&vault);

    for ((path, _), (ino, target, text)) in links.iter().zip(before) {
        assert_eq!(inode(path), ino);
        assert_eq!(fs::read_link(path).unwrap(), target);
        assert_eq!(target.to_str().unwrap(), text);
        assert!(path.symlink_metadata().unwrap().file_type().is_symlink());
    }
    assert!(!root.join("missing-target").exists());
    assert_eq!(inode(&to_db), db_link_ino);
    assert_eq!(fs::read_link(&to_db).unwrap(), db_link_target);
    assert_eq!(fs::read(&plain).unwrap(), b"plain-file\n");
    assert_eq!(inode(&plain), plain_ino);
    assert_eq!(fs::read(&kept).unwrap(), b"dir-keep\n");
    assert_eq!(inode(&dir), dir_ino);
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), vault_db);
    assert_eq!(fs::read(vault.join("SYNTHETIC_ONLY")).unwrap(), banner);
    assert_eq!(inode(&vault), vault_ino);
    assert!(scratch_names(root).is_empty());

    let fresh = root.join("brand-new");
    let (code, body) = run(&["init", "--vault", fresh.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Initialized");
    assert_eq!(body["created_new_vault"], true);
    assert!(body.get("reason").is_none(), "{body}");
    let (code, opened) = run(&["open", "--vault", fresh.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    assert!(scratch_names(root).is_empty());
}
