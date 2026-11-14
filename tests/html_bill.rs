use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::{json, Value};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OLD: &str = "4276033b22a81f05dfe456224ee33c6996a0a68e";

const BILL: &str = r#"<!DOCTYPE html>
<html>
<head><title>Synthetic bill</title></head>
<body>
<div hidden><p>Amount paid USD 99.99</p></div>
<!-- Amount paid USD 99.99 -->
<script>Amount paid USD 99.99</script>
<p>Amount paid USD <b>15&#46;99</b></p>
<p>Service period: <span>2026-09-01</span> through <span>2026-09-30</span></p>
</body>
</html>
"#;

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-html-{label}-{}-{}",
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

fn run_at(program: &Path, args: &[&str]) -> (i32, Value) {
    let output = Command::new(program)
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

fn run(args: &[&str]) -> (i32, Value) {
    run_at(Path::new(bin()), args)
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
    let vault = tmp.path().join(name);
    let (code, body) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        format,
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["vault_format"], format.parse::<i64>().unwrap());
    vault
}

fn write_source(tmp: &Tmp, name: &str, bytes: &[u8]) -> PathBuf {
    let path = tmp.path().join(name);
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

fn object_bytes(vault: &Path, object_id: &str) -> Vec<u8> {
    fs::read(vault.join("objects").join(object_id)).unwrap()
}

fn fact_of<'a>(facts: &'a Value, fact_type: &str) -> &'a Value {
    facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fact_type"] == fact_type)
        .unwrap_or_else(|| panic!("missing {fact_type} in {facts}"))
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
fn one_local_html_bill_round_trips_through_correct_undo_and_two_exports() {
    let tmp = Tmp::new("bill");
    let vault = init(&tmp, "vault", "4");
    let source = write_source(&tmp, "bill.html", BILL.as_bytes());
    let source_hash = sha256(BILL.as_bytes());
    let (code, imported) = html(&vault, "bill", &source);
    assert_eq!(code, 0, "{imported}");
    assert_eq!(imported["outcome"], "Committed");
    assert_eq!(imported["slice"], "T4-html");
    assert_eq!(imported["new_effect"], true);
    assert_eq!(imported["preservation"], "preserved");
    assert_eq!(imported["extraction"], "matched");
    assert_eq!(imported["media_type"], "text/html;charset=utf-8");
    assert_eq!(imported["role"], "user_provided_html");
    assert_eq!(imported["content_sha256"], source_hash);
    assert_eq!(imported["mailbox"], false);
    assert_eq!(imported["clipboard"], false);
    assert_eq!(imported["network"], false);
    assert_eq!(imported["rendered"], false);
    assert_eq!(imported["external_references"], "not_loaded");
    assert_eq!(imported["payment_verified"], false);
    let object_id = imported["object_id"].as_str().unwrap();
    assert_eq!(object_bytes(&vault, object_id), BILL.as_bytes());
    assert_eq!(
        sql(&vault, "SELECT role FROM source_versions;"),
        "user_provided_html"
    );
    let config = sql(&vault, "SELECT config_json FROM extraction_runs;");
    assert!(config.contains("\"parser\":\"tl\""), "{config}");
    assert!(config.contains("\"parser_version\":\"0.7.8\""), "{config}");
    assert!(
        config.contains("\"entity_decoder\":\"html-escape\""),
        "{config}"
    );
    assert!(
        config.contains("\"entity_decoder_version\":\"0.2.13\""),
        "{config}"
    );
    assert!(!config.contains("identity-utf8-scalars-1"), "{config}");
    assert!(!config.contains("stated-lines-1"), "{config}");
    assert!(!config.contains("user_provided_fragment"), "{config}");

    let (code, again) = html(&vault, "bill", &source);
    assert_eq!(code, 0, "{again}");
    assert_eq!(again["new_effect"], false);
    assert_eq!(again["payload_matches"], true);
    assert_eq!(again["preservation"], "preserved");
    assert_eq!(object_bytes(&vault, object_id), BILL.as_bytes());

    let (code, facts) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert_eq!(facts["slice"], "T4-html");
    assert_eq!(facts["facts"].as_array().unwrap().len(), 2);
    let amount = fact_of(&facts, "stated_amount_paid");
    let period = fact_of(&facts, "stated_service_period");
    assert_eq!(amount["value"]["amount_text"], "15.99");
    assert_eq!(amount["value"]["currency"], "USD");
    assert_eq!(amount["value"]["statement"], "source_stated");
    assert_eq!(amount["value"]["payment_verified"], false);
    assert!(amount["value"]["observed_at"].is_null());
    assert_eq!(amount["value"]["current_status"], "unknown");
    assert_eq!(amount["value"]["time_status"], "unknown");
    assert_eq!(amount["revision"], 1);
    assert_eq!(period["value"]["period_start"], "2026-09-01");
    assert_eq!(period["value"]["period_end"], "2026-09-30");
    assert!(period["value"]["observed_at"].is_null());
    assert_eq!(period["revision"], 1);
    assert_span(amount, BILL);
    assert_span(period, BILL);
    assert!(!BILL.contains("Amount paid USD 15.99"));
    let encoded = serde_json::to_string(&facts).unwrap();
    assert!(!encoded.contains("99.99"), "{encoded}");

    let (code, candidates) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{candidates}");
    assert_eq!(candidates["slice"], "T4-html");
    assert_eq!(candidates["html_body_schema"], "1");
    assert!(candidates["body_schema"].is_null());
    assert_eq!(candidates["candidates"].as_array().unwrap().len(), 2);
    let listed = serde_json::to_string(&candidates).unwrap();
    assert!(!listed.contains("99.99"), "{listed}");
    assert!(listed.contains("&#46;"), "{listed}");

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
    assert_eq!(corrected["new_effect"], true);
    assert_eq!(corrected["revision"], 2);
    assert_eq!(corrected["amount_text"], "10.00");
    assert_eq!(corrected["slice"], "T3-facts");
    assert_eq!(corrected["payload_version"], "t3-payload-1");
    assert_eq!(object_bytes(&vault, object_id), BILL.as_bytes());
    let decision_id = corrected["decision_id"].as_str().unwrap().to_string();

    let (code, undone) = run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "undo-bill",
        "--decision-id",
        &decision_id,
        "--expected-revision",
        "2",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["revision"], 3);
    assert_eq!(undone["amount_text"], "15.99");
    assert_eq!(object_bytes(&vault, object_id), BILL.as_bytes());

    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    assert_eq!(opened["vault_format"], 4);

    let (code, current) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{current}");
    assert_eq!(fact_of(&current, "stated_amount_paid")["revision"], 3);
    assert_eq!(
        fact_of(&current, "stated_amount_paid")["value"]["amount_text"],
        "15.99"
    );
    assert_eq!(fact_of(&current, "stated_service_period")["revision"], 1);
    assert_span(fact_of(&current, "stated_amount_paid"), BILL);

    let package = tmp.path().join("package-1");
    let (code, exported) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exported}");
    assert_eq!(exported["outcome"], "Exported");
    assert_eq!(exported["output_written"], true);
    assert_eq!(exported["completeness"], "complete");
    let records_bytes = fs::read(package.join("records.json")).unwrap();
    check_package(&package, object_id, BILL.as_bytes(), "15.99", 3);
    let kinds = decision_kinds(&package);
    assert!(kinds.contains(&"rule_adopt".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"human_correct".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"undo".to_string()), "{kinds:?}");

    let (code, clash) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{clash}");
    assert_eq!(clash["reason"], "output_exists");
    assert_eq!(
        fs::read(package.join("records.json")).unwrap(),
        records_bytes
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
    assert_eq!(edited["amount_text"], "10.00");
    assert_eq!(object_bytes(&vault, object_id), BILL.as_bytes());
    let package_2 = tmp.path().join("package-2");
    let (code, second) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        package_2.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{second}");
    check_package(&package_2, object_id, BILL.as_bytes(), "10.00", 4);
    assert_eq!(
        fs::read(package.join("records.json")).unwrap(),
        records_bytes
    );
    assert_eq!(
        fs::read(package.join("originals").join(object_id)).unwrap(),
        BILL.as_bytes()
    );
    assert_ne!(
        fs::read(package.join("facts.csv")).unwrap(),
        fs::read(package_2.join("facts.csv")).unwrap()
    );
}

fn assert_span(fact: &Value, source: &str) {
    let anchor = &fact["locator"];
    let locator = &anchor["locator"];
    let start = locator["byte_start"].as_u64().unwrap() as usize;
    let end = locator["byte_end"].as_u64().unwrap() as usize;
    let excerpt = locator["excerpt"].as_str().unwrap();
    assert_eq!(anchor["excerpt"], excerpt);
    assert_eq!(&source.as_bytes()[start..end], excerpt.as_bytes());
    assert_eq!(
        locator["unicode_scalar_start"].as_u64().unwrap() as usize,
        source[..start].chars().count()
    );
    assert_eq!(
        locator["unicode_scalar_end"].as_u64().unwrap() as usize,
        source[..end].chars().count()
    );
    assert_eq!(
        locator["offset_unit"],
        "unicode_scalar_in_original_html_utf8"
    );
    assert_eq!(anchor["display_excerpt"], locator["display_excerpt"]);
    let display = locator["display_excerpt"].as_str().unwrap();
    assert_ne!(display, excerpt);
    if fact["fact_type"] == "stated_amount_paid" {
        assert!(excerpt.contains("<b>"), "{excerpt}");
        assert!(excerpt.contains("&#46;"), "{excerpt}");
        assert_eq!(display, "Amount paid USD 15.99");
    } else {
        assert!(excerpt.contains("<span>"), "{excerpt}");
    }
    assert!(!excerpt.contains("99.99"), "{excerpt}");
}

fn check_package(package: &Path, object_id: &str, original: &[u8], amount: &str, revision: i64) {
    let packed = fs::read(package.join("originals").join(object_id)).unwrap();
    assert_eq!(packed, original);
    let records: Value =
        serde_json::from_slice(&fs::read(package.join("records.json")).unwrap()).unwrap();
    assert_eq!(records["vault_format"], 4);
    assert_eq!(records["package_version"], "offline-exit-1");
    let note = records["field_notes"]["html_original"].as_str().unwrap();
    assert!(note.contains("不执行"), "{note}");
    assert!(note.contains("不自动加载"), "{note}");
    let facts = records["facts"].as_array().unwrap();
    let paid = facts
        .iter()
        .find(|row| row["fact_type"] == "stated_amount_paid")
        .unwrap();
    assert_eq!(paid["revision"], revision);
    assert_eq!(paid["value"]["amount_text"], amount);
    assert_eq!(paid["value"]["payment_verified"], false);
    let candidates = records["candidates"].as_array().unwrap();
    assert!(candidates.iter().any(|row| {
        row["excerpt"].as_str().unwrap().contains("&#46;")
            && row["locator"]["display_excerpt"] == "Amount paid USD 15.99"
    }));
    let manifest: Value =
        serde_json::from_slice(&fs::read(package.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["completeness"], "complete");
    for item in manifest["files"].as_array().unwrap() {
        let path = package.join(item["path"].as_str().unwrap());
        let data = fs::read(&path).unwrap();
        assert_eq!(sha256(&data), item["sha256"].as_str().unwrap());
        assert_eq!(data.len() as i64, item["byte_length"].as_i64().unwrap());
    }
    let csv = fs::read_to_string(package.join("facts.csv")).unwrap();
    assert!(csv.contains(amount), "{csv}");
    assert!(!csv.contains("99.99"), "{csv}");
}

fn decision_kinds(package: &Path) -> Vec<String> {
    let records: Value =
        serde_json::from_slice(&fs::read(package.join("records.json")).unwrap()).unwrap();
    records["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["kind"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn html_boundaries_do_not_wipe_or_invent_facts() {
    let tmp = Tmp::new("bounds");
    let styled = init(&tmp, "styled", "4");
    let source = write_source(
        &tmp,
        "styled.html",
        br#"<p style="color:red">Amount paid USD <b>15&#46;99</b></p>"#,
    );
    let before = fs::read(&source).unwrap();
    let (code, body) = html(&styled, "styled", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["preservation"], "preserved");
    assert_eq!(body["extraction"], "unsupported");
    assert_eq!(body["extraction_reason"], "style_attribute");
    assert_eq!(
        object_bytes(&styled, body["object_id"].as_str().unwrap()),
        before
    );
    assert_eq!(fs::read(&source).unwrap(), before);
    let (code, facts) = run(&["facts", "--vault", styled.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert_eq!(facts["facts"].as_array().unwrap().len(), 0);

    let sheet = init(&tmp, "sheet", "4");
    let source = write_source(
        &tmp,
        "sheet.html",
        b"<head><link rel=\"stylesheet\" href=\"https://example.invalid/a.css\"></head><p>Amount paid USD 15.99</p>",
    );
    let (code, body) = html(&sheet, "sheet", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "unsupported");
    assert_eq!(body["extraction_reason"], "stylesheet_link");
    assert_eq!(body["network"], false);
    assert_eq!(body["external_references"], "not_loaded");
    assert_eq!(
        object_bytes(&sheet, body["object_id"].as_str().unwrap()),
        fs::read(&source).unwrap()
    );

    let clash = init(&tmp, "clash", "4");
    let source = write_source(
        &tmp,
        "clash.html",
        b"<p>Amount paid USD 15.99</p><p>Amount paid USD 10.00</p><p>Service period: 2026-09-01 through 2026-09-30</p>",
    );
    let (code, body) = html(&clash, "clash", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "conflict");
    assert_eq!(body["preservation"], "preserved");
    let (code, facts) = run(&["facts", "--vault", clash.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert_eq!(facts["facts"].as_array().unwrap().len(), 1);
    assert_eq!(facts["facts"][0]["fact_type"], "stated_service_period");
    let (code, candidates) = run(&["candidates", "--vault", clash.to_str().unwrap()]);
    assert_eq!(code, 0, "{candidates}");
    let amounts: Vec<_> = candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "stated_amount_paid")
        .collect();
    assert_eq!(amounts.len(), 2);
    assert!(amounts.iter().all(|row| row["usable"] == false));

    let none = init(&tmp, "none", "4");
    let source = write_source(&tmp, "none.html", b"<p>Nothing owed</p>");
    let (code, body) = html(&none, "none", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "no_match");
    let (code, facts) = run(&["facts", "--vault", none.to_str().unwrap()]);
    assert_eq!(facts["facts"].as_array().unwrap().len(), 0);
    let _ = code;

    let element = init(&tmp, "style-element", "4");
    let source = write_source(
        &tmp,
        "element.html",
        b"<style>p{color:red}</style><p>Amount paid USD 15.99</p>",
    );
    let (code, body) = html(&element, "element", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "unsupported");
    assert_eq!(body["extraction_reason"], "style_element");
    assert_eq!(body["preservation"], "preserved");
    assert_eq!(
        object_bytes(&element, body["object_id"].as_str().unwrap()),
        fs::read(&source).unwrap()
    );
    let (code, facts) = run(&["facts", "--vault", element.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert!(facts["facts"].as_array().unwrap().is_empty());

    let loose = init(&tmp, "loose", "4");
    let source = write_source(&tmp, "loose.html", b"<body>Amount paid USD 15.99</body>");
    let (code, body) = html(&loose, "loose", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "out_of_scope");
    assert_eq!(body["extraction_reason"], "text_outside_block");
    let (code, facts) = run(&["facts", "--vault", loose.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert!(facts["facts"].as_array().unwrap().is_empty());

    let wide = init(&tmp, "wide", "4");
    let source = write_source(
        &tmp,
        "wide.html",
        b"<p>Amount paid USD 15.99</p><img alt=\"bait\">",
    );
    let (code, body) = html(&wide, "wide", &source);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["extraction"], "out_of_scope");
    assert_eq!(body["extraction_reason"], "unsupported_element:img");
    let (code, facts) = run(&["facts", "--vault", wide.to_str().unwrap()]);
    assert_eq!(code, 0, "{facts}");
    assert!(facts["facts"].as_array().unwrap().is_empty());

    let bad = init(&tmp, "bad", "4");
    let source = write_source(&tmp, "bad.html", &[0xff, 0xfe]);
    let db_before = fs::read(bad.join("vault.db")).unwrap();
    let (code, body) = html(&bad, "bad", &source);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "invalid_utf8");
    assert_eq!(body["preservation"], "not_preserved");
    assert_eq!(body["new_effect"], false);
    assert_eq!(fs::read(bad.join("vault.db")).unwrap(), db_before);
    assert_eq!(sql(&bad, "SELECT count(*) FROM sources;"), "0");
    assert!(!bad
        .join("objects")
        .read_dir()
        .unwrap()
        .any(|entry| { entry.unwrap().path().is_file() }));

    let replay = init(&tmp, "replay", "4");
    let source = write_source(&tmp, "replay.html", BILL.as_bytes());
    let (code, first) = html(&replay, "replay", &source);
    assert_eq!(code, 0, "{first}");
    let object_id = first["object_id"].as_str().unwrap().to_string();
    fs::write(&source, b"<p>Amount paid USD 10.00</p>").unwrap();
    let (code, conflict) = html(&replay, "replay", &source);
    assert_eq!(code, 16, "{conflict}");
    assert_eq!(conflict["reason"], "payload_conflict");
    assert_eq!(conflict["preservation"], "preserved");
    assert_eq!(conflict["new_effect"], false);
    assert_eq!(object_bytes(&replay, &object_id), BILL.as_bytes());
    fs::remove_file(&source).unwrap();
    let (code, missing) = html(&replay, "replay", &source);
    assert_eq!(code, 0, "{missing}");
    assert_eq!(missing["new_effect"], false);
    assert_eq!(missing["external_reread"], false);
    assert!(missing["payload_matches"].is_null());
    assert_eq!(missing["preservation"], "preserved");
    assert_eq!(object_bytes(&replay, &object_id), BILL.as_bytes());

    let other = write_source(&tmp, "other.html", b"<p>Amount paid USD 10.00</p>");
    let (code, second) = html(&replay, "other", &other);
    assert_eq!(code, 16, "{second}");
    assert_eq!(second["reason"], "one_html_document");
    assert_eq!(second["new_effect"], false);
    assert_eq!(sql(&replay, "SELECT count(*) FROM sources;"), "1");
    assert_eq!(object_bytes(&replay, &object_id), BILL.as_bytes());

    let same = write_source(&tmp, "same.html", BILL.as_bytes());
    let (code, preserved) = html(&replay, "same", &same);
    assert_eq!(code, 0, "{preserved}");
    assert_eq!(preserved["outcome"], "AlreadyPreserved");
    assert_eq!(preserved["new_effect"], false);
    assert_eq!(sql(&replay, "SELECT count(*) FROM sources;"), "1");
}

#[test]
fn older_vaults_reject_html_and_keep_plain_fragment_text() {
    let tmp = Tmp::new("old-vaults");
    let source = write_source(&tmp, "bill.html", BILL.as_bytes());
    for format in ["1", "3"] {
        let vault = init(&tmp, format, format);
        let before = vault_files(&vault);
        let (code, body) = html(&vault, "html-try", &source);
        assert_eq!(code, 14, "{format} {body}");
        assert_eq!(body["reason"], "html_requires_format_4");
        assert_eq!(body["new_effect"], false);
        assert_eq!(vault_files(&vault), before, "{format}");
        assert_eq!(sql(&vault, "SELECT count(*) FROM sources;"), "0");
    }
    let vault = init(&tmp, "plain", "3");
    let note = write_source(&tmp, "note.txt", b"Amount paid USD 15.99\n");
    let (code, body) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "plain",
        "--fragment",
        note.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(
        sql(&vault, "SELECT media_type FROM source_versions;"),
        "text/plain;charset=utf-8"
    );
    assert_eq!(
        sql(&vault, "SELECT role FROM source_versions;"),
        "user_provided_fragment"
    );
    assert_eq!(body["slice"], "T3-facts");

    let (code, rejected) = run(&[
        "html",
        "--vault",
        init(&tmp, "typed", "4").to_str().unwrap(),
        "--command-id",
        "typed",
        "--source",
        source.to_str().unwrap(),
        "--media-type",
        "text/plain",
    ]);
    assert_eq!(code, 2, "{rejected}");
    assert_eq!(rejected["reason"], "html_media_type_fixed");
}

#[test]
fn the_4276033_binary_rejects_format_4_before_writing() {
    let old = old_binary();
    let (code, version) = run_at(old, &["version"]);
    assert_eq!(code, 0, "{version}");
    assert_eq!(version["git_sha"], OLD);
    assert_eq!(version["supported_vault_formats"], json!([1, 2, 3]));

    let tmp = Tmp::new("old-bin");
    let missing = tmp.path().join("not-created");
    let (code, body) = run_at(
        old,
        &[
            "init",
            "--vault",
            missing.to_str().unwrap(),
            "--format",
            "4",
        ],
    );
    assert_eq!(code, 2, "{body}");
    assert_eq!(body["reason"], "bad_format");
    assert!(!missing.exists());

    let vault = init(&tmp, "vault", "4");
    let source = write_source(&tmp, "bill.html", BILL.as_bytes());
    let (code, imported) = html(&vault, "bill", &source);
    assert_eq!(code, 0, "{imported}");
    let db = fs::read(vault.join("vault.db")).unwrap();
    let files = vault_files(&vault);
    let commands = [
        vec!["open", "--vault", vault.to_str().unwrap()],
        vec![
            "import",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "old-import",
            "--media-type",
            "text/plain",
            "--source",
            source.to_str().unwrap(),
        ],
        vec![
            "fragment",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "old-fragment",
            "--fragment",
            source.to_str().unwrap(),
        ],
        vec![
            "correct",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "old-correct",
            "--fact-id",
            "fact_00000000000000000000000000000000",
            "--expected-revision",
            "1",
            "--amount-text",
            "10.00",
            "--actor-id",
            ACTOR,
        ],
    ];
    for args in commands {
        let (code, body) = run_at(old, &args);
        assert_eq!(code, 14, "{} {body}", args[0]);
        assert_eq!(body["outcome"], "VersionUnsupported");
        assert_eq!(body["reason"], "newer_schema");
        assert_eq!(body["schema_version_found"], 4);
        assert_eq!(fs::read(vault.join("vault.db")).unwrap(), db, "{}", args[0]);
        assert_eq!(vault_files(&vault), files, "{}", args[0]);
    }
}

fn old_binary() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let src = PathBuf::from("/tmp/subvault-4276033");
        if !src.join("Cargo.toml").is_file() {
            let status = Command::new("git")
                .args(["worktree", "add", "--detach", "/tmp/subvault-4276033", OLD])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("git worktree");
            assert!(status.success(), "4276033 worktree was not created");
        }
        let status = Command::new("cargo")
            .args([
                "build",
                "--offline",
                "--locked",
                "--manifest-path",
                "/tmp/subvault-4276033/Cargo.toml",
                "--target-dir",
                "/tmp/subvault-4276033-target",
            ])
            .env("SUBVAULT_GIT_SHA", OLD)
            .status()
            .expect("old cargo");
        assert!(status.success(), "4276033 binary did not build");
        PathBuf::from("/tmp/subvault-4276033-target/debug/subvault")
    })
}
