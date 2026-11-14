use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const BILL: &str = r#"<!DOCTYPE html>
<html>
<body>
<div hidden><p>Amount paid USD 99.99</p></div>
<table>
<tr><th>Total</th><td>USD 40.00</td></tr>
<tr><th>Amount due</th><td>USD 40.00</td></tr>
<tr><th>Amount paid</th><td>USD <b>15&#46;99</b></td></tr>
<tr><th>Service period</th><td><span>2026-09-01</span> through <span>2026-09-30</span></td></tr>
</table>
</body>
</html>
"#;

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-html-row-{label}-{}-{}",
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

fn sha256(bytes: &[u8]) -> String {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn init(tmp: &Tmp, name: &str, format: &str) -> PathBuf {
    let vault = tmp.path.join(name);
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        format,
    ]);
    assert_eq!(code, 0, "{body}");
    vault
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

fn fact_of<'a>(facts: &'a Value, fact_type: &str) -> &'a Value {
    facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == fact_type)
        .unwrap_or_else(|| panic!("missing {fact_type}"))
}

fn assert_slice(fact: &Value, source: &str) {
    let locator = &fact["locator"]["locator"];
    let start = locator["byte_start"].as_u64().unwrap() as usize;
    let end = locator["byte_end"].as_u64().unwrap() as usize;
    let excerpt = locator["excerpt"].as_str().unwrap();
    assert_eq!(&source.as_bytes()[start..end], excerpt.as_bytes());
    assert_eq!(
        locator["unicode_scalar_start"].as_u64().unwrap() as usize,
        source[..start].chars().count()
    );
    assert_eq!(
        locator["unicode_scalar_end"].as_u64().unwrap() as usize,
        source[..end].chars().count()
    );
    assert_eq!(fact["locator"]["excerpt"], excerpt);
    assert_ne!(
        excerpt,
        fact["locator"]["display_excerpt"].as_str().unwrap()
    );
}

#[test]
fn a_same_row_label_bill_round_trips_without_taking_the_total() {
    let tmp = Tmp::new("bill");
    let vault = init(&tmp, "vault", "4");
    let source = tmp.path.join("bill.html");
    fs::write(&source, BILL.as_bytes()).unwrap();
    let (code, imported) = html(&vault, "bill", &source);
    assert_eq!(code, 0, "{imported}");
    assert_eq!(imported["extraction"], "matched");
    assert_eq!(imported["preservation"], "preserved");
    assert_eq!(imported["network"], false);
    assert_eq!(imported["payment_verified"], false);
    let object_id = imported["object_id"].as_str().unwrap();
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    let config = sql(&vault, "SELECT config_json FROM extraction_runs;");
    assert!(config.contains("stated-html-blocks-3"), "{config}");
    assert!(config.contains("same-tr-explicit-1"), "{config}");
    assert!(config.contains("html-row-stated-1"), "{config}");

    let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let amount = fact_of(&facts, "stated_amount_paid");
    let period = fact_of(&facts, "stated_service_period");
    assert_eq!(amount["value"]["amount_text"], "15.99");
    assert_eq!(amount["value"]["payment_verified"], false);
    assert!(amount["value"]["observed_at"].is_null());
    assert_eq!(amount["value"]["current_status"], "unknown");
    assert_eq!(amount["value"]["time_status"], "unknown");
    assert_eq!(period["value"]["period_start"], "2026-09-01");
    assert_eq!(period["value"]["period_end"], "2026-09-30");
    assert_slice(amount, BILL);
    assert_slice(period, BILL);
    let excerpt = amount["locator"]["locator"]["excerpt"].as_str().unwrap();
    assert_eq!(excerpt, "<th>Amount paid</th><td>USD <b>15&#46;99</b></td>");
    assert_eq!(
        amount["locator"]["display_excerpt"],
        "Amount paid USD 15.99"
    );
    assert_eq!(amount["locator"]["display_version"], "html-row-stated-1");
    assert_eq!(
        period["locator"]["display_excerpt"],
        "Service period: 2026-09-01 through 2026-09-30"
    );
    assert_eq!(period["locator"]["display_version"], "html-row-stated-1");
    assert!(period["locator"]["locator"]["excerpt"]
        .as_str()
        .unwrap()
        .starts_with("<th>Service period</th>"));
    let facts_text = serde_json::to_string(&facts).unwrap();
    assert!(!facts_text.contains("99.99"), "{facts_text}");
    assert!(!facts_text.contains("40.00"), "{facts_text}");

    let fact_id = amount["fact_id"].as_str().unwrap();
    let (code, corrected) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "fix",
        "--fact-id",
        fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "10.00",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{corrected}");
    assert_eq!(corrected["amount_text"], "10.00");
    let decision_id = corrected["decision_id"].as_str().unwrap();
    let (code, undone) = run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "undo-bill",
        "--decision-id",
        decision_id,
        "--expected-revision",
        "2",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["amount_text"], "15.99");
    assert_eq!(undone["revision"], 3);
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");

    let package = tmp.path.join("package-1");
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    let records_bytes = fs::read(package.join("records.json")).unwrap();
    let records: Value = serde_json::from_slice(&records_bytes).unwrap();
    let paid = records["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap();
    assert_eq!(paid["value"]["amount_text"], "15.99");
    assert_eq!(paid["revision"], 3);
    let packed = fs::read(package.join("originals").join(object_id)).unwrap();
    assert_eq!(packed, BILL.as_bytes());
    assert_eq!(sha256(&packed), sha256(BILL.as_bytes()));

    let (code, edited) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "fix-again",
        "--fact-id",
        fact_id,
        "--expected-revision",
        "3",
        "--amount-text",
        "10.00",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{edited}");
    let package_2 = tmp.path.join("package-2");
    let (code, second) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package_2.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{second}");
    let records_2: Value =
        serde_json::from_slice(&fs::read(package_2.join("records.json")).unwrap()).unwrap();
    let paid_2 = records_2["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap();
    assert_eq!(paid_2["value"]["amount_text"], "10.00");
    assert_eq!(paid_2["revision"], 4);
    assert_eq!(
        fs::read(package.join("records.json")).unwrap(),
        records_bytes
    );
    assert_eq!(
        fs::read(package.join("originals").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    let (code, clash) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{clash}");
    assert_eq!(clash["reason"], "output_exists");
}

#[test]
fn row_association_limits_and_older_sentences_stay_closed() {
    let tmp = Tmp::new("limits");

    let styled = init(&tmp, "styled", "4");
    let source = tmp.path.join("styled.html");
    fs::write(
        &source,
        br#"<table><tr><th style="color:red">Amount paid</th><td>USD 15.99</td></tr></table>"#,
    )
    .unwrap();
    let (code, body) = html(&styled, "styled", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "unsupported");
    assert_eq!(body["extraction_reason"], "style_attribute");
    assert_eq!(body["preservation"], "preserved");
    assert_eq!(
        fs::read(
            styled
                .join("objects")
                .join(body["object_id"].as_str().unwrap())
        )
        .unwrap(),
        fs::read(&source).unwrap()
    );

    let cases = [
        (
            "wrong-row",
            "<table><tr><th>Amount paid</th></tr><tr><td>USD 15.99</td></tr></table>",
            "no_match",
        ),
        (
            "two-tables",
            "<table><tr><th>Amount paid</th></tr></table><table><tr><td>USD 15.99</td></tr></table>",
            "no_match",
        ),
        (
            "nested",
            "<table><tr><th>Amount paid</th><td><table><tr><td>USD 15.99</td></tr></table></td></tr></table>",
            "no_match",
        ),
        (
            "hidden",
            "<table><tr><th hidden>Amount paid</th><td>USD 15.99</td></tr></table>",
            "no_match",
        ),
    ];
    for (name, html_text, extraction) in cases {
        let vault = init(&tmp, name, "4");
        let path = tmp.path.join(format!("{name}.html"));
        fs::write(&path, html_text.as_bytes()).unwrap();
        let (code, body) = html(&vault, name, &path);
        assert_eq!(code, 0, "{name} {body}");
        assert_eq!(body["extraction"], extraction, "{name}");
        assert_eq!(body["preservation"], "preserved", "{name}");
        let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
        assert_eq!(code, 0, "{facts}");
        assert!(facts["facts"].as_array().unwrap().is_empty(), "{name}");
        assert_eq!(
            fs::read(
                vault
                    .join("objects")
                    .join(body["object_id"].as_str().unwrap())
            )
            .unwrap(),
            html_text.as_bytes()
        );
    }

    let clash = init(&tmp, "clash", "4");
    let clash_path = tmp.path.join("clash.html");
    fs::write(
        &clash_path,
        b"<table><tr><th>Amount paid</th><td>USD 15.99</td></tr><tr><th>Amount paid</th><td>USD 10.00</td></tr><tr><th>Service period</th><td>2026-09-01 through 2026-09-30</td></tr></table>",
    )
    .unwrap();
    let (code, body) = html(&clash, "clash", &clash_path);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "conflict");
    let (code, facts) = run(&["facts", "--vault", clash.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert_eq!(facts["facts"].as_array().unwrap().len(), 1);
    assert_eq!(facts["facts"][0]["fact_type"], "stated_service_period");

    let ambiguous = init(&tmp, "ambiguous", "4");
    let ambiguous_path = tmp.path.join("ambiguous.html");
    fs::write(
        &ambiguous_path,
        b"<table><tr><th>Amount paid</th><td>USD 15.99</td><td>USD 10.00</td></tr></table>",
    )
    .unwrap();
    let (code, body) = html(&ambiguous, "ambiguous", &ambiguous_path);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "conflict");
    assert_eq!(body["extraction_reason"], "ambiguous_association");
    let (code, facts) = run(&["facts", "--vault", ambiguous.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert!(facts["facts"].as_array().unwrap().is_empty());

    let sentence = init(&tmp, "sentence", "4");
    let sentence_path = tmp.path.join("sentence.html");
    let sentence_html = b"<p>Amount paid USD <b>15&#46;99</b></p>";
    fs::write(&sentence_path, sentence_html).unwrap();
    let (code, body) = html(&sentence, "sentence", &sentence_path);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "matched");
    let (code, facts) = run(&["facts", "--vault", sentence.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let paid = &facts["facts"][0];
    assert_eq!(paid["value"]["amount_text"], "15.99");
    assert_eq!(paid["locator"]["display_version"], "html-collapse-1");
    assert_eq!(paid["locator"]["locator"]["element"], "p");
    let excerpt = paid["locator"]["locator"]["excerpt"].as_str().unwrap();
    assert_eq!(excerpt.as_bytes(), sentence_html);

    let plain = init(&tmp, "plain", "3");
    let note = tmp.path.join("note.txt");
    fs::write(&note, b"Amount paid USD 15.99\n").unwrap();
    let (code, body) = run(&[
        "fragment",
        "--vault",
        plain.to_str().unwrap(),
        "--command-id",
        "plain",
        "--fragment",
        note.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["slice"], "T3-facts");
    assert_eq!(
        sql(&plain, "SELECT media_type FROM source_versions;"),
        "text/plain;charset=utf-8"
    );
    let (code, facts) = run(&["facts", "--vault", plain.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert_eq!(facts["facts"][0]["value"]["amount_text"], "15.99");
    assert_eq!(facts["facts"][0]["value"]["payment_verified"], false);
    assert!(facts["facts"][0]["locator"]["display_version"].is_null());
}
