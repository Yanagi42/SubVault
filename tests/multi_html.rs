use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const DOC_A: &str = r#"<!DOCTYPE html>
<html><body>
<p>Amount paid USD <b>15&#46;99</b></p>
<p>Service period: <span>2026-09-01</span> through <span>2026-09-30</span></p>
</body></html>
"#;

const DOC_B: &str = r#"<!DOCTYPE html>
<html><body>
<p>Amount paid USD 15.99</p>
<p>Service period: 2024-01-02 through 2024-03-04</p>
<table><tr><th>Amount paid</th><td>USD 8.00</td><td>USD 9.00</td></tr></table>
</body></html>
"#;

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-multi-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
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

fn init(tmp: &Tmp) -> PathBuf {
    let vault = tmp.path.join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "4"]);
    assert_eq!(code, 0, "{body}");
    vault
}

fn write_file(tmp: &Tmp, name: &str, bytes: &[u8]) -> PathBuf {
    let path = tmp.path.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn html(vault: &Path, command: &str, source: &Path) -> (i32, Value) {
    run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--source",
        source.to_str().unwrap(),
    ])
}

fn facts(vault: &Path) -> Value {
    let (code, body) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    body
}

fn amount_of<'a>(rows: &'a Value, source_id: &str) -> &'a Value {
    rows["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source_id"] == source_id && row["fact_type"] == "stated_amount_paid")
        .unwrap_or_else(|| panic!("missing amount for {source_id} in {rows}"))
}

fn period_of<'a>(rows: &'a Value, source_id: &str) -> &'a Value {
    rows["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source_id"] == source_id && row["fact_type"] == "stated_service_period")
        .unwrap_or_else(|| panic!("missing period for {source_id} in {rows}"))
}

fn object_bytes(vault: &Path, object_id: &str) -> Vec<u8> {
    fs::read(vault.join("objects").join(object_id)).unwrap()
}

#[test]
fn two_documents_stay_separate_through_correction_and_a_new_file() {
    let tmp = Tmp::new("two");
    let vault = init(&tmp);
    let path_a = write_file(&tmp, "a.html", DOC_A.as_bytes());
    let (code, first) = html(&vault, "cmd-a", &path_a);
    assert_eq!(code, 0, "{first}");
    assert_eq!(first["new_effect"], true);
    let object_a = first["object_id"].as_str().unwrap().to_string();
    let source_a = first["source_id"].as_str().unwrap().to_string();
    let bytes_a = object_bytes(&vault, &object_a);

    let bad = write_file(&tmp, "bad.html", &[0xff, 0xfe]);
    let before = fs::read(vault.join("vault.db")).unwrap();
    let (code, failed) = html(&vault, "cmd-bad", &bad);
    assert_eq!(code, 16, "{failed}");
    assert_eq!(failed["new_effect"], false);
    assert_eq!(failed["preservation"], "not_preserved");
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "1");
    assert_eq!(object_bytes(&vault, &object_a), bytes_a);
    assert_eq!(fs::read(vault.join("vault.db")).unwrap(), before);

    let path_b = write_file(&tmp, "b.html", DOC_B.as_bytes());
    let (code, second) = html(&vault, "cmd-b", &path_b);
    assert_eq!(code, 0, "{second}");
    assert_eq!(second["new_effect"], true);
    let object_b = second["object_id"].as_str().unwrap().to_string();
    let source_b = second["source_id"].as_str().unwrap().to_string();
    assert_ne!(source_a, source_b);
    assert_ne!(object_a, object_b);
    assert_eq!(object_bytes(&vault, &object_a), bytes_a);
    assert_eq!(object_bytes(&vault, &object_b), DOC_B.as_bytes());

    let listed = facts(&vault);
    assert_eq!(
        amount_of(&listed, &source_a)["value"]["amount_text"],
        "15.99"
    );
    assert_eq!(
        amount_of(&listed, &source_b)["value"]["amount_text"],
        "15.99"
    );
    assert_eq!(amount_of(&listed, &source_a)["revision"], 1);
    assert_eq!(amount_of(&listed, &source_b)["revision"], 1);
    assert_eq!(
        period_of(&listed, &source_a)["value"]["period_start"],
        "2026-09-01"
    );
    assert_eq!(
        period_of(&listed, &source_b)["value"]["period_start"],
        "2024-01-02"
    );
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "2");
    let (code, candidates) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{candidates}");
    let ambiguous = candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["usable"] == false)
        .count();
    assert_eq!(ambiguous, 1, "{candidates}");

    let fact_a = amount_of(&listed, &source_a)["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let fact_b = amount_of(&listed, &source_b)["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, corrected) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "fix-a",
        "--fact-id",
        &fact_a,
        "--expected-revision",
        "1",
        "--amount-text",
        "12.00",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{corrected}");
    let decision = corrected["decision_id"].as_str().unwrap().to_string();
    let after_correct = facts(&vault);
    assert_eq!(
        amount_of(&after_correct, &source_a)["value"]["amount_text"],
        "12.00"
    );
    assert_eq!(amount_of(&after_correct, &source_a)["revision"], 2);
    assert_eq!(
        amount_of(&after_correct, &source_b)["value"]["amount_text"],
        "15.99"
    );
    assert_eq!(amount_of(&after_correct, &source_b)["revision"], 1);
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT count(*) FROM decisions WHERE fact_id = '{fact_b}';")
        ),
        "1"
    );

    let copy = write_file(&tmp, "a-copy.html", DOC_A.as_bytes());
    let (code, reused) = html(&vault, "cmd-a-copy", &copy);
    assert_eq!(code, 0, "{reused}");
    assert_eq!(reused["outcome"], "AlreadyPreserved");
    assert_eq!(reused["facts_created"], false);
    assert_eq!(reused["source_id"], source_a);
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "2");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "4");

    let (code, undone) = run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "undo-a",
        "--decision-id",
        &decision,
        "--expected-revision",
        "2",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{undone}");
    let after_undo = facts(&vault);
    assert_eq!(
        amount_of(&after_undo, &source_a)["value"]["amount_text"],
        "15.99"
    );
    assert_eq!(amount_of(&after_undo, &source_a)["revision"], 3);
    assert_eq!(
        amount_of(&after_undo, &source_b)["value"]["amount_text"],
        "15.99"
    );
    assert_eq!(amount_of(&after_undo, &source_b)["revision"], 1);
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT count(*) FROM decisions WHERE fact_id = '{fact_b}';")
        ),
        "1"
    );
    assert_eq!(object_bytes(&vault, &object_a), bytes_a);
    assert_eq!(object_bytes(&vault, &object_b), DOC_B.as_bytes());

    fs::write(&path_a, b"<p>Amount paid USD 7.00</p>").unwrap();
    let (code, replaced) = html(&vault, "cmd-a-new", &path_a);
    assert_eq!(code, 0, "{replaced}");
    assert_eq!(replaced["new_effect"], true);
    assert_ne!(replaced["source_id"], source_a);
    assert_ne!(replaced["source_id"], source_b);
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "3");
    assert_eq!(object_bytes(&vault, &object_a), bytes_a);
    assert_eq!(object_bytes(&vault, &object_b), DOC_B.as_bytes());

    let (code, retry) = html(&vault, "cmd-b", &path_b);
    assert_eq!(code, 0, "{retry}");
    assert_eq!(retry["new_effect"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "3");
    let other = write_file(&tmp, "b-other.html", b"<p>Amount paid USD 1.00</p>");
    let (code, clash) = html(&vault, "cmd-b", &other);
    assert_eq!(code, 16, "{clash}");
    assert_eq!(clash["reason"], "payload_conflict");
    assert_eq!(clash["new_effect"], false);
    assert_eq!(object_bytes(&vault, &object_b), DOC_B.as_bytes());
    assert_eq!(amount_of(&facts(&vault), &source_b)["revision"], 1);
}

#[test]
fn concurrent_html_keeps_one_fact_set_per_byte_sequence() {
    let tmp = Tmp::new("race");
    let vault = init(&tmp);
    let left = write_file(&tmp, "left.html", DOC_A.as_bytes());
    let right = write_file(&tmp, "right.html", DOC_B.as_bytes());
    let same = write_file(&tmp, "same.html", DOC_A.as_bytes());
    race(&vault, &left, &right, "race-a", "race-b");
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "2");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "4");
    race(&vault, &left, &same, "race-a2", "race-same");
    assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "2");
    assert_eq!(sql(&vault, "SELECT count(*) FROM facts;"), "4");
}

fn race(vault: &Path, left: &Path, right: &Path, left_id: &str, right_id: &str) {
    let vault_left = vault.to_path_buf();
    let vault_right = vault.to_path_buf();
    let left = left.to_path_buf();
    let right = right.to_path_buf();
    let left_id = left_id.to_string();
    let right_id = right_id.to_string();
    let first = thread::spawn(move || submit_until_stored(&vault_left, &left_id, &left));
    let second = thread::spawn(move || submit_until_stored(&vault_right, &right_id, &right));
    let left_body = first.join().unwrap();
    let right_body = second.join().unwrap();
    assert_ne!(left_body["reason"], "payload_conflict", "{left_body}");
    assert_ne!(right_body["reason"], "payload_conflict", "{right_body}");
}

fn submit_until_stored(vault: &Path, command: &str, source: &Path) -> Value {
    for _ in 0..40 {
        let output = Command::new(bin())
            .args([
                "html",
                "--vault",
                vault.to_str().unwrap(),
                "--command-id",
                command,
                "--source",
                source.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let body: Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
            panic!("stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{stderr}")
        });
        let code = output.status.code().unwrap_or(1);
        if code == 11 {
            assert_eq!(body["reason"], "writer_busy", "{stderr}");
            thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        assert_eq!(code, 0, "{body}\nstderr:\n{stderr}");
        return body;
    }
    panic!("writer stayed busy");
}
