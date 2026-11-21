use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

fn bin() -> PathBuf {
    if let Ok(path) = std::env::var("SUBVAULT_TEST_BIN") {
        return PathBuf::from(path);
    }
    let compiled = PathBuf::from(env!("CARGO_BIN_EXE_subvault"));
    if compiled.exists() {
        return compiled;
    }
    if let Ok(exe) = std::env::current_exe() {
        let name = if cfg!(windows) {
            "subvault.exe"
        } else {
            "subvault"
        };
        if let Some(candidate) = exe.parent().and_then(|p| p.parent()).map(|p| p.join(name)) {
            if candidate.exists() {
                return candidate;
            }
        }
    }
    compiled
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-verify-pkg-{label}-{}-{}",
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
        if !std::thread::panicking() {
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

fn build_exported_format4_package(tmp: &Tmp) -> PathBuf {
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "4"]);
    assert_eq!(code, 0, "{body}");

    let bill = tmp.path().join("bill.html");
    fs::write(
        &bill,
        "<!doctype html><html><body><table><tr><th>Amount paid</th><td>USD 15.99</td></tr><tr><th>Service period:</th><td>2026-11-01 through 2026-11-30</td></tr></table></body></html>\n",
    )
    .unwrap();

    let (code, body) = run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-html-1",
        "--source",
        bill.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");

    let pack = tmp.path().join("exit-pack");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        pack.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    pack
}

#[test]
fn verify_package_passes_for_clean_format4_export_and_format3_export() {
    let tmp = Tmp::new("clean");
    let pack4 = build_exported_format4_package(&tmp);

    let (code, body) = run(&["verify-package", "--package", pack4.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Verified");
    assert_eq!(body["read_only"], true);
    assert_eq!(body["new_effect"], false);
    assert_eq!(body["completeness"], "complete");
    assert_eq!(body["vault_format"], 4);
    assert_eq!(body["verified_originals"], 1);
    assert_eq!(body["facts_csv_checked"], true);
    assert_eq!(body["defects"].as_array().unwrap().len(), 0);

    // Format 3 fragment + human correction export also verifies cleanly
    let vault3 = tmp.path().join("vault3");
    let (code, body) = run(&["init", "--vault", vault3.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    let note = tmp.path().join("note.txt");
    fs::write(
        &note,
        "Amount paid USD 48.00\nService period: 2023-01-01 through 2023-12-31\n",
    )
    .unwrap();
    let (code, body) = run(&[
        "fragment",
        "--vault",
        vault3.to_str().unwrap(),
        "--command-id",
        "cmd-frag-1",
        "--fragment",
        note.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    let pack3 = tmp.path().join("exit-pack-3");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault3.to_str().unwrap(),
        "--out",
        pack3.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    let (code, body) = run(&["verify-package", "--package", pack3.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["outcome"], "Verified");
    assert_eq!(body["vault_format"], 3);
    assert_eq!(body["verified_originals"], 1);
}

#[test]
fn verify_package_catches_tampered_original_missing_file_extra_file_and_symlink() {
    let tmp = Tmp::new("tamper");
    let pack = build_exported_format4_package(&tmp);

    // Find the original file path from manifest.json
    let manifest: Value =
        serde_json::from_slice(&fs::read(pack.join("manifest.json")).unwrap()).unwrap();
    let orig_rel = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|f| {
            let p = f["path"].as_str()?;
            p.starts_with("originals/").then(|| p.to_string())
        })
        .unwrap();
    let orig_path = pack.join(&orig_rel);
    let orig_bytes = fs::read(&orig_path).unwrap();

    // 1. Flip one byte in original -> hash_mismatch
    let mut tampered = orig_bytes.clone();
    tampered[0] ^= 0xff;
    fs::write(&orig_path, &tampered).unwrap();
    let (code, body) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["outcome"], "PackageInvalid");
    assert_eq!(body["reason"], "hash_mismatch");

    // 2. Delete original -> missing_file
    fs::remove_file(&orig_path).unwrap();
    let (code, body) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["reason"], "missing_file");

    // 3. Replace original with a symlink (Unix) or drop a directory junction (Windows)
    #[cfg(unix)]
    {
        let outside = tmp.path().join("outside.bin");
        fs::write(&outside, &orig_bytes).unwrap();
        std::os::unix::fs::symlink(&outside, &orig_path).unwrap();
        let (code, body) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
        assert_eq!(code, 12, "{body}");
        assert_eq!(body["reason"], "symlink_in_package");
        fs::remove_file(&orig_path).unwrap();
    }
    #[cfg(windows)]
    {
        let outside_dir = tmp.path().join("outside_dir");
        fs::create_dir(&outside_dir).unwrap();
        let junction = pack.join("sneaky_junction");
        let status = Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                junction.to_str().unwrap(),
                outside_dir.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
        fs::write(&orig_path, &orig_bytes).unwrap();
        let (code, body) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
        assert_eq!(code, 12, "{body}");
        assert_eq!(body["reason"], "symlink_in_package");
        fs::remove_dir(&junction).unwrap();
        fs::remove_file(&orig_path).unwrap();
    }

    // Restore clean original
    fs::write(&orig_path, &orig_bytes).unwrap();
    let (code, body) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");

    // 4. Unexpected extra file in package -> unexpected_file
    fs::write(pack.join("injected.txt"), b"sneaky").unwrap();
    let (code, body) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["reason"], "unexpected_file");
    fs::remove_file(pack.join("injected.txt")).unwrap();
}

#[test]
fn verify_package_catches_snapshot_and_csv_mismatch_and_bad_paths() {
    let tmp = Tmp::new("csv");
    let pack = build_exported_format4_package(&tmp);

    // Tamper records.json -> manifest hash / snapshot_id fails
    let orig_records = fs::read(pack.join("records.json")).unwrap();
    let mut altered = orig_records.clone();
    altered.push(b'\n');
    fs::write(pack.join("records.json"), &altered).unwrap();
    let (code, body) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["outcome"], "PackageInvalid");
    fs::write(pack.join("records.json"), &orig_records).unwrap();

    // Missing path -> Rejected (16)
    let missing = tmp.path().join("does-not-exist");
    let (code, body) = run(&["verify-package", "--package", missing.to_str().unwrap()]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "package_missing");

    // Help lists verify-package
    let (code, help) = run(&["help"]);
    assert_eq!(code, 0, "{help}");
    assert!(help["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == "verify-package --package PATH"));
}
