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

fn run_cmd(command: &mut Command) -> (i32, Value) {
    let output = command.output().unwrap();
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

fn run(args: &[&str]) -> (i32, Value) {
    run_cmd(Command::new(bin()).args(args))
}

fn run_exact(args: &[String]) -> (i32, Value) {
    run_cmd(Command::new(bin()).args(args))
}

fn with_slash(path: &Path) -> String {
    let text = path.to_str().unwrap();
    assert!(!text.ends_with('/'), "{text}");
    format!("{text}/")
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

fn refused_spelling(vault: &str) {
    assert!(vault.ends_with('/'), "{vault}");
    let (code, body) = run_exact(&[
        "init".to_string(),
        "--vault".to_string(),
        vault.to_string(),
        "--format".to_string(),
        "1".to_string(),
    ]);
    assert_eq!(code, 16, "{vault} {body}");
    assert_eq!(body["outcome"], "Rejected");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(body["created_new_vault"], false);
    assert_eq!(body["new_effect"], false);
    assert!(body.get("preservation").is_none(), "{body}");
    assert!(body.get("during").is_none(), "{body}");
}

#[test]
fn init_keeps_a_trailing_slash_on_the_same_final_entry() {
    let tmp = Tmp::new("slash");
    let root = tmp.path();
    let plain = root.join("plain.txt");
    fs::write(&plain, b"plain-file\n").unwrap();
    let dir = root.join("plaindir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("keep.txt"), b"dir-keep\n").unwrap();
    let empty = root.join("emptydir");
    fs::create_dir(&empty).unwrap();
    let vault = root.join("origvault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    let db = fs::read(vault.join("vault.db")).unwrap();
    let dangling = root.join("link-dangling");
    symlink("missing-target", &dangling).unwrap();
    let to_file = root.join("link-file");
    symlink("plain.txt", &to_file).unwrap();
    let to_dir = root.join("link-dir");
    symlink("plaindir", &to_dir).unwrap();
    let to_vault = root.join("link-vault");
    symlink("origvault", &to_vault).unwrap();
    let looping = root.join("link-loop");
    symlink("link-loop", &looping).unwrap();
    let link_a = root.join("link-a");
    let link_b = root.join("link-b");
    symlink("link-b", &link_a).unwrap();
    symlink("link-a", &link_b).unwrap();

    let occupied = [
        &dangling, &to_file, &to_dir, &to_vault, &looping, &link_a, &link_b, &plain, &dir, &empty,
        &vault,
    ];
    let before: Vec<_> = occupied.iter().map(|path| inode(path)).collect();
    for path in &occupied {
        refused_spelling(&with_slash(path));
    }
    refused_spelling(&format!("{}//", plain.to_str().unwrap()));
    for (path, ino) in occupied.iter().zip(before) {
        assert_eq!(inode(path), ino);
    }
    assert_eq!(
        fs::read_link(&dangling).unwrap(),
        Path::new("missing-target")
    );
    assert_eq!(fs::read_link(&looping).unwrap(), Path::new("link-loop"));
    assert_eq!(fs::read_link(&link_a).unwrap(), Path::new("link-b"));
    assert_eq!(fs::read(&plain).unwrap(), b"plain-file\n");
    assert_eq!(fs::read(dir.join("keep.txt")).unwrap(), b"dir-keep\n");
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), db);
    assert!(!root.join("missing-target").exists());
    assert!(scratch_names(root).is_empty());

    let (code, body) = run_cmd(Command::new(bin()).current_dir(root).args([
        "init",
        "--vault",
        "link-dangling/",
        "--format",
        "1",
    ]));
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert!(body.get("preservation").is_none(), "{body}");
    assert_eq!(
        fs::read_link(&dangling).unwrap(),
        Path::new("missing-target")
    );

    let nested = format!("{}/child/", plain.to_str().unwrap());
    let (code, body) = run_exact(&[
        "init".into(),
        "--vault".into(),
        nested,
        "--format".into(),
        "1".into(),
    ]);
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["during"], "init_stat");
    assert_ne!(body["reason"], "vault_path_exists");
    assert_eq!(fs::read(&plain).unwrap(), b"plain-file\n");
    let ancestor = format!("{}/child/", dangling.to_str().unwrap());
    let (code, body) = run_exact(&[
        "init".into(),
        "--vault".into(),
        ancestor,
        "--format".into(),
        "1".into(),
    ]);
    assert_eq!(code, 10, "{body}");
    assert_eq!(body["reason"], "parent_missing");
    assert_ne!(body["reason"], "vault_path_exists");
    assert!(!root.join("no-parent").exists());
    let missing_parent = format!("{}/no-parent/vault/", root.to_str().unwrap());
    let (code, body) = run_exact(&[
        "init".into(),
        "--vault".into(),
        missing_parent,
        "--format".into(),
        "1".into(),
    ]);
    assert_eq!(code, 10, "{body}");
    assert_eq!(body["reason"], "parent_missing");
    assert!(!root.join("no-parent").exists());
    assert!(scratch_names(root).is_empty());

    let fresh = with_slash(&root.join("fresh-slash"));
    let (code, body) = run_exact(&[
        "init".into(),
        "--vault".into(),
        fresh.clone(),
        "--format".into(),
        "1".into(),
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Initialized");
    assert_eq!(body["created_new_vault"], true);
    let (code, opened) = run_exact(&["open".into(), "--vault".into(), fresh]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    assert!(scratch_names(root).is_empty());

    let (code, body) = run_cmd(
        Command::new(bin())
            .current_dir(root)
            .args(["init", "--vault", "/", "--format", "1"]),
    );
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(body["created_new_vault"], false);
    assert!(scratch_names(root).is_empty());
}
