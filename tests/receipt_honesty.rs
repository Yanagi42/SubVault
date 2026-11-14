use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NOTE: &str = "\
Amount paid USD 15.99
Service period: 2026-09-01 through 2026-09-30
";
const STDOUT_LINE: &str = "stdout_undelivered; result unknown; retry the same command id\n";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-receipt-{label}-{}-{}",
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

fn run_fd(args: &[&str], stdout: Stdio) -> (i32, Vec<u8>, Vec<u8>) {
    let output = Command::new(bin())
        .args(args)
        .stdout(stdout)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(101),
        output.stdout,
        output.stderr,
    )
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

fn assert_unknown_stderr(stderr: &[u8]) {
    let text = String::from_utf8_lossy(stderr);
    assert_eq!(text, STDOUT_LINE, "{text}");
    assert!(!text.contains("NotPreserved"));
    assert!(!text.contains("failed printing"));
    assert!(!text.contains("panicked"));
    assert!(!text.contains("new command"));
    assert!(!text.contains("change the id"));
}

fn dev_full() -> Stdio {
    Stdio::from(File::options().write(true).open("/dev/full").unwrap())
}

#[test]
fn stdout_write_failure_is_unknown_and_the_same_command_is_not_redone() {
    let (help_ok, help_body) = run(&["help"]);
    assert_eq!(help_ok, 0, "{help_body}");
    let help_bytes = serde_json::to_string_pretty(&help_body).unwrap().len() + 1;
    assert!(
        help_bytes < 8192,
        "help is the short receipt, {help_bytes} bytes"
    );
    let (help_code, _, help_err) = run_fd(&["help"], dev_full());
    assert_eq!(
        help_code, 21,
        "short receipt flush to /dev/full must not exit 0"
    );
    assert_ne!(help_code, 0);
    assert_ne!(help_code, 15);
    assert_ne!(help_code, 101);
    assert_unknown_stderr(&help_err);

    let tmp = Tmp::new("full");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    let note = tmp.path().join("stated.txt");
    fs::write(&note, NOTE).unwrap();
    let (code, published) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "a",
        "--fragment",
        note.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{published}");
    let (code, listed) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{listed}");
    let paid = listed["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap();
    let fact_id = paid["fact_id"].as_str().unwrap();
    assert_eq!(paid["value"]["amount_text"], "15.99");

    let correct = [
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "f",
        "--fact-id",
        fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "13.00",
        "--actor-id",
        ACTOR,
    ];
    let (code, _, err) = run_fd(&correct, dev_full());
    assert_eq!(
        code, 21,
        "/dev/full is a write error after the command, not a COMMIT fault"
    );
    assert_ne!(code, 0);
    assert_ne!(code, 15);
    assert_ne!(code, 101);
    assert_unknown_stderr(&err);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='f';"
        ),
        "1"
    );
    assert_eq!(
        sql(
            &vault,
            &format!(
                "SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';"
            ),
        ),
        "13.00"
    );
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='f';"
        ),
        "1"
    );

    let (code, replay) = run(&correct);
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["outcome"], "AlreadyCommitted");
    assert_eq!(replay["new_effect"], false);
    assert_eq!(replay["amount_text"], "13.00");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='f';"
        ),
        "1"
    );
    assert_eq!(
        sql(
            &vault,
            &format!(
                "SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';"
            ),
        ),
        "13.00"
    );
}

#[test]
fn a_long_receipt_fails_in_the_write_and_does_not_panic() {
    let tmp = Tmp::new("wide");
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "1"]);
    assert_eq!(code, 0, "{body}");
    for index in 0..12 {
        let source = tmp.path().join(format!("f{index}.txt"));
        fs::write(&source, format!("synthetic-{index}\n")).unwrap();
        let (code, imported) = run(&[
            "import",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            &format!("c{index}"),
            "--media-type",
            "text/plain",
            "--source",
            source.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "{imported}");
    }
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    let open_bytes = serde_json::to_string_pretty(&opened).unwrap().len() + 1;
    assert!(
        open_bytes > 8192,
        "open receipt must cross the stdout buffer, {open_bytes} bytes"
    );
    let sources = sql(&vault, "SELECT count(*) FROM sources;");
    assert_eq!(sources, "12");
    let (failed, _, err) = run_fd(&["open", "--vault", vault.to_str().unwrap()], dev_full());
    assert_eq!(
        failed, 21,
        "long receipt write to /dev/full must not exit 0"
    );
    assert_ne!(failed, 0);
    assert_ne!(failed, 15);
    assert_ne!(failed, 101);
    assert_unknown_stderr(&err);
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "12");
    let (again, seen) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(again, 0, "{seen}");
    assert_eq!(seen["outcome"], "Ready");
}

#[test]
fn injected_stdout_write_is_transport_not_a_commit_fault() {
    let tmp = Tmp::new("preload");
    let source = tmp.path().join("failwrite.c");
    fs::write(
        &source,
        r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <unistd.h>
#include <sys/uio.h>
static ssize_t (*real_write)(int, const void *, size_t);
static ssize_t (*real_writev)(int, const struct iovec *, int);
ssize_t write(int fd, const void *buf, size_t count) {
    if (!real_write) real_write = dlsym(RTLD_NEXT, "write");
    if (fd == 1) { errno = ENOSPC; return -1; }
    return real_write(fd, buf, count);
}
ssize_t writev(int fd, const struct iovec *iov, int count) {
    if (!real_writev) real_writev = dlsym(RTLD_NEXT, "writev");
    if (fd == 1) { errno = ENOSPC; return -1; }
    return real_writev(fd, iov, count);
}
"#,
    )
    .unwrap();
    let library = tmp.path().join("failwrite.so");
    let compiled = Command::new("gcc")
        .args([
            "-shared",
            "-fPIC",
            "-o",
            library.to_str().unwrap(),
            source.to_str().unwrap(),
            "-ldl",
        ])
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "3"]);
    assert_eq!(code, 0, "{body}");
    let note = tmp.path().join("stated.txt");
    fs::write(&note, NOTE).unwrap();
    let (code, published) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "a",
        "--fragment",
        note.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{published}");
    let (code, listed) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{listed}");
    let fact_id = listed["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap()["fact_id"]
        .as_str()
        .unwrap()
        .to_string();

    let output = Command::new(bin())
        .args([
            "correct",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "g",
            "--fact-id",
            &fact_id,
            "--expected-revision",
            "1",
            "--amount-text",
            "14.00",
            "--actor-id",
            ACTOR,
        ])
        .env("LD_PRELOAD", &library)
        .output()
        .unwrap();
    let code = output.status.code().unwrap_or(101);
    assert_eq!(code, 21);
    assert_ne!(code, 0);
    assert_ne!(code, 15);
    assert_ne!(code, 101);
    assert!(output.stdout.is_empty());
    assert_unknown_stderr(&output.stderr);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM t3_receipts WHERE command_id='g';"
        ),
        "1"
    );
    assert_eq!(
        sql(
            &vault,
            &format!(
                "SELECT json_extract(value_json, '$.amount_text') FROM facts WHERE fact_id='{fact_id}';"
            ),
        ),
        "14.00"
    );

    let (code, replay) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "g",
        "--fact-id",
        &fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "14.00",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["outcome"], "AlreadyCommitted");
    assert_eq!(replay["new_effect"], false);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM decisions WHERE command_id='g';"
        ),
        "1"
    );
}
