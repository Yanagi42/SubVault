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
<tr><td>Amount paid USD <b>15&#46;99</b></td></tr>
<tr><td>Service period: <span>2026-09-01</span> through <span>2026-09-30</span></td></tr>
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
            "subvault-html-table-{label}-{}-{}",
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

fn init(tmp: &Tmp) -> PathBuf {
    let vault = tmp.path.join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "4"]);
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

#[test]
fn a_table_cell_bill_round_trips_without_joining_neighbor_cells() {
    let tmp = Tmp::new("bill");
    let vault = init(&tmp);
    let source = tmp.path.join("bill.html");
    fs::write(&source, BILL.as_bytes()).unwrap();
    let (code, imported) = html(&vault, "bill", &source);
    assert_eq!(code, 0, "{imported}");
    assert_eq!(imported["extraction"], "matched");
    assert_eq!(imported["preservation"], "preserved");
    assert_eq!(imported["network"], false);
    let object_id = imported["object_id"].as_str().unwrap();
    assert_eq!(
        fs::read(vault.join("objects").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    let config = sql(&vault, "SELECT config_json FROM extraction_runs;");
    assert!(config.contains("stated-html-blocks-3"), "{config}");
    assert!(config.contains("td,th"), "{config}");

    let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let amount = fact_of(&facts, "stated_amount_paid");
    let period = fact_of(&facts, "stated_service_period");
    assert_eq!(amount["value"]["amount_text"], "15.99");
    assert_eq!(amount["value"]["payment_verified"], false);
    assert!(amount["value"]["observed_at"].is_null());
    assert_eq!(amount["value"]["current_status"], "unknown");
    assert_eq!(period["value"]["period_start"], "2026-09-01");
    assert_eq!(period["value"]["period_end"], "2026-09-30");
    let excerpt = amount["locator"]["locator"]["excerpt"].as_str().unwrap();
    let start = amount["locator"]["locator"]["byte_start"].as_u64().unwrap() as usize;
    let end = amount["locator"]["locator"]["byte_end"].as_u64().unwrap() as usize;
    assert_eq!(&BILL.as_bytes()[start..end], excerpt.as_bytes());
    assert!(excerpt.starts_with("<td>"), "{excerpt}");
    assert!(excerpt.contains("&#46;"), "{excerpt}");
    assert_eq!(
        amount["locator"]["display_excerpt"],
        "Amount paid USD 15.99"
    );
    assert_eq!(amount["locator"]["display_version"], "html-collapse-1");
    assert!(!serde_json::to_string(&facts).unwrap().contains("99.99"));

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
    assert_eq!(
        fs::read(package.join("originals").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    assert_eq!(
        sha256(&fs::read(package.join("originals").join(object_id)).unwrap()),
        sha256(BILL.as_bytes())
    );

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
    assert_eq!(
        fs::read(package.join("records.json")).unwrap(),
        records_bytes
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
fn table_limits_stay_closed() {
    let tmp = Tmp::new("limits");
    let styled = init(&tmp);
    let source = tmp.path.join("styled.html");
    fs::write(
        &source,
        br#"<table><tr><td style="color:red">Amount paid USD 15.99</td></tr></table>"#,
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

    let clash_vault = tmp.path.join("clash");
    let (code, body) = run(&[
        "init",
        "--vault",
        clash_vault.to_str().unwrap(),
        "--format",
        "4",
    ]);
    assert_eq!(code, 0, "{body}");
    let clash = tmp.path.join("clash.html");
    fs::write(
        &clash,
        b"<table><tr><td>Amount paid USD 15.99</td></tr><tr><td>Amount paid USD 10.00</td></tr><tr><td>Service period: 2026-09-01 through 2026-09-30</td></tr></table>",
    )
    .unwrap();
    let (code, body) = html(&clash_vault, "clash", &clash);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "conflict");
    let (code, facts) = run(&["facts", "--vault", clash_vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert_eq!(facts["facts"].as_array().unwrap().len(), 1);
    assert_eq!(facts["facts"][0]["fact_type"], "stated_service_period");

    let split_vault = tmp.path.join("split");
    let (code, body) = run(&[
        "init",
        "--vault",
        split_vault.to_str().unwrap(),
        "--format",
        "4",
    ]);
    assert_eq!(code, 0, "{body}");
    let split = tmp.path.join("split.html");
    fs::write(
        &split,
        b"<table><tr><th>Amount paid</th><td>USD 15.99</td></tr></table>",
    )
    .unwrap();
    let (code, body) = html(&split_vault, "split", &split);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "matched");
    assert_eq!(body["preservation"], "preserved");
    let (code, facts) = run(&["facts", "--vault", split_vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    let paid = &facts["facts"][0];
    assert_eq!(paid["fact_type"], "stated_amount_paid");
    assert_eq!(paid["value"]["amount_text"], "15.99");
    assert_eq!(paid["value"]["payment_verified"], false);
    assert_eq!(paid["locator"]["display_version"], "html-row-stated-1");
    let excerpt = paid["locator"]["locator"]["excerpt"].as_str().unwrap();
    let bytes = fs::read(&split).unwrap();
    let start = paid["locator"]["locator"]["byte_start"].as_u64().unwrap() as usize;
    let end = paid["locator"]["locator"]["byte_end"].as_u64().unwrap() as usize;
    assert_eq!(&bytes[start..end], excerpt.as_bytes());
    assert_eq!(excerpt, "<th>Amount paid</th><td>USD 15.99</td>");
    assert_ne!(
        excerpt,
        paid["locator"]["display_excerpt"].as_str().unwrap()
    );

    let caption_vault = tmp.path.join("caption");
    let (code, body) = run(&[
        "init",
        "--vault",
        caption_vault.to_str().unwrap(),
        "--format",
        "4",
    ]);
    assert_eq!(code, 0, "{body}");
    let caption = tmp.path.join("caption.html");
    fs::write(
        &caption,
        b"<table><caption>Amount paid USD 15.99</caption><tr><td>Amount paid USD 15.99</td></tr></table>",
    )
    .unwrap();
    let (code, body) = html(&caption_vault, "caption", &caption);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "out_of_scope");
    assert_eq!(body["extraction_reason"], "unsupported_element:caption");
    assert_eq!(body["preservation"], "preserved");
    let (code, facts) = run(&["facts", "--vault", caption_vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert!(facts["facts"].as_array().unwrap().is_empty());
    assert_eq!(
        fs::read(
            caption_vault
                .join("objects")
                .join(body["object_id"].as_str().unwrap())
        )
        .unwrap(),
        fs::read(&caption).unwrap()
    );
}
