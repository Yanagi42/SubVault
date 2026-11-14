use std::fs;
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
            "subvault-initdur-{label}-{}-{}",
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

fn sql(vault: &Path, query: &str) -> String {
    let output = Command::new("sqlite3")
        .arg(vault.join("vault.db"))
        .arg(query)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn init_parent_fsync_after_publish_does_not_say_not_created() {
    for (format, action) in [("1", "io_error"), ("3", "enospc")] {
        let tmp = Tmp::new(&format!("init{format}"));
        let vault = tmp.path().join("vault");
        let witness = tmp.path().join("witness.json");
        let (code, body) = run(&[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            format,
            "--inject",
            &format!("{action}:publish_dir_fsync"),
            "--witness",
            witness.to_str().unwrap(),
        ]);
        assert_eq!(code, 3, "{body}");
        assert_ne!(code, 15);
        assert_eq!(body["outcome"], "Initialized");
        assert_ne!(body["outcome"], "NotPreserved");
        assert!(body.get("preservation").is_none(), "{body}");
        assert_eq!(body["created_new_vault"], true);
        assert_eq!(body["published"], true);
        assert_eq!(body["directory_durable"], false);
        assert_eq!(body["reason"], "directory_durability_unknown");
        assert_eq!(body["during"], "publish_dir_fsync");
        assert_eq!(body["new_effect"], true);
        assert_eq!(body["injected"], true);
        assert_eq!(body["inject_action"], action);
        assert_eq!(body["vault_format"], format.parse::<i64>().unwrap());
        let seen = serde_json::from_str::<Value>(&fs::read_to_string(&witness).unwrap()).unwrap();
        assert_eq!(seen["point"], "publish_dir_fsync");
        assert_eq!(seen["reached"], true);
        assert_eq!(seen["published"], true);
        assert!(vault.join("SYNTHETIC_ONLY").is_file());
        assert!(vault.join("writer.lock").is_file());
        let header = fs::read(vault.join("vault.db")).unwrap();
        assert!(header.starts_with(b"SQLite format 3\0"), "{}", header.len());
        assert_eq!(sql(&vault, "PRAGMA user_version;"), format);
        let scratch: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".subvault-init-"))
            .collect();
        assert!(scratch.is_empty(), "{scratch:?}");
        let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
        assert_eq!(code, 0, "{opened}");
        assert_eq!(opened["outcome"], "Ready");
        let (code, again) = run(&[
            "init",
            "--vault",
            vault.to_str().unwrap(),
            "--format",
            format,
        ]);
        assert_eq!(code, 16, "{again}");
        assert_eq!(again["reason"], "vault_path_exists");
        assert_eq!(again["created_new_vault"], false);
    }
}
