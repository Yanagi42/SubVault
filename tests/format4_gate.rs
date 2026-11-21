use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
            "subvault-format4-{label}-{}-{}",
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
    let output = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(1), body)
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

fn vault_files(vault: &Path) -> Vec<(String, Vec<u8>)> {
    let mut all = Vec::new();
    for name in ["vault.db", "vault.db-wal", "vault.db-shm"] {
        let path = vault.join(name);
        if path.is_file() {
            all.push((name.to_string(), fs::read(&path).unwrap()));
        }
    }
    for name in ["objects", "journal", "staging", "control"] {
        let dir = vault.join(name);
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            if entry.path().is_file() {
                all.push((
                    format!("{name}/{}", entry.file_name().to_string_lossy()),
                    fs::read(entry.path()).unwrap(),
                ));
            }
        }
    }
    all.sort();
    all
}

#[test]
fn format_4_is_a_new_vault_and_does_not_accept_plain_import_or_fragment() {
    let tmp = Tmp::new("gate");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "4"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], 4);
    assert_eq!(body["created_new_vault"], true);
    assert_eq!(body["slice"], "T1");
    assert_eq!(sql(&vault, "PRAGMA user_version;"), "4");
    assert_eq!(
        sql(&vault, "SELECT value FROM meta WHERE key='vault_format';"),
        "4"
    );
    assert_eq!(
        sql(&vault, "SELECT value FROM meta WHERE key='schema_version';"),
        "1"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT value FROM meta WHERE key='html_body_schema';"
        ),
        "1"
    );
    assert_eq!(
        sql(&vault, "SELECT value FROM meta WHERE key='body_schema';"),
        ""
    );
    let role_sql = sql(
        &vault,
        "SELECT sql FROM sqlite_master WHERE name='source_versions';",
    );
    assert!(role_sql.contains("user_provided_html"), "{role_sql}");
    assert!(!role_sql.contains("user_provided_fragment"), "{role_sql}");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    assert_eq!(opened["vault_format"], 4);

    let sample = tmp.path().join("note.txt");
    fs::write(&sample, b"Amount paid USD 15.99\n").unwrap();
    let before = vault_files(&vault);
    let (code, refused) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "plain",
        "--fragment",
        sample.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{refused}");
    assert_eq!(refused["reason"], "format_4_refuses_fragment");
    assert_eq!(refused["new_effect"], false);
    assert_eq!(vault_files(&vault), before);
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "plain-import",
        "--media-type",
        "text/plain",
        "--source",
        sample.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{imported}");
    assert_eq!(imported["reason"], "format_4_refuses_import");
    assert_eq!(imported["new_effect"], false);
    assert_eq!(vault_files(&vault), before);
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "0");

    let missing = tmp.path().join("nope");
    let (code, bad) = run(&[
        "init",
        "--vault",
        missing.to_str().unwrap(),
        "--format",
        "7",
    ]);
    assert_eq!(code, 2, "{bad}");
    assert_eq!(bad["reason"], "bad_format");
    assert!(!missing.exists());

    let older = tmp.path().join("old");
    let (code, body) = run(&["init", "--vault", older.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(
        sql(&older, "SELECT value FROM meta WHERE key='body_schema';"),
        "1"
    );
    assert_eq!(
        sql(
            &older,
            "SELECT value FROM meta WHERE key='html_body_schema';"
        ),
        ""
    );
    let plain_sql = sql(
        &older,
        "SELECT sql FROM sqlite_master WHERE name='source_versions';",
    );
    assert!(plain_sql.contains("user_provided_fragment"), "{plain_sql}");
    assert!(!plain_sql.contains("user_provided_html"), "{plain_sql}");
}
