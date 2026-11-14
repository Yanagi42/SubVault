use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("synthetic/corpus")
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-body-{label}-{}-{}",
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

fn init(tmp: &Tmp) -> PathBuf {
    let vault = tmp.path().join("vault");
    let (code, body) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "2"]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["slice"], "T1");
    assert_eq!(body["vault_format"], 2);
    assert_eq!(body["capabilities"]["extraction"], "not_available");
    assert_eq!(body["capabilities"]["pdf_extraction"], "not_available");
    vault
}

fn write_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
}

fn fragment(vault: &Path, command: &str, file: &Path) -> (i32, Value) {
    run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--fragment",
        file.to_str().unwrap(),
    ])
}

fn candidates(vault: &Path) -> Value {
    let (code, body) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["read_only"], true);
    assert_eq!(body["new_effect"], false);
    assert_eq!(body["slice"], "T2a-body");
    body
}

fn honest_body(body: &Value) {
    assert_eq!(body["slice"], "T2a-body");
    assert_eq!(body["user_ready"], false);
    assert_eq!(body["security_complete"], false);
    assert_eq!(body["synthetic_only"], true);
    assert!(body["purchase_id"].is_null());
    assert!(body["entitlement_id"].is_null());
    assert!(body["eml"].is_null());
    assert_eq!(body["adopted"], false);
    assert_eq!(body["pdf_extraction"], "not_available");
    assert_eq!(body["ocr"], "not_available");
}

fn kind<'a>(body: &'a Value, kind: &str) -> Vec<&'a Value> {
    body["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|candidate| candidate["kind"] == kind)
        .collect()
}

fn points_at(file: &[u8], candidate: &Value) {
    let locator = &candidate["locator"];
    let start = locator["byte_start"].as_u64().unwrap() as usize;
    let end = locator["byte_end"].as_u64().unwrap() as usize;
    let excerpt = candidate["excerpt"].as_str().unwrap();
    assert_eq!(&file[start..end], excerpt.as_bytes(), "{locator}");
    let text = std::str::from_utf8(file).unwrap();
    let scalar_start = locator["unicode_scalar_start"].as_u64().unwrap() as usize;
    let scalar_end = locator["unicode_scalar_end"].as_u64().unwrap() as usize;
    let slice: String = text
        .chars()
        .skip(scalar_start)
        .take(scalar_end - scalar_start)
        .collect();
    assert_eq!(slice, excerpt);
    assert_eq!(
        locator["offset_unit"],
        "unicode_scalar_in_versioned_extracted_string"
    );
    assert_eq!(locator["normalization"], "identity-utf8-scalars-1");
    assert_eq!(locator["join"], "single-fragment-1");
    assert!(locator["eml"].is_null());
    assert!(locator["pdf_page"].is_null());
    assert!(locator["coordinates"].is_null());
    assert!(candidate["value"]["purchase_id"].is_null());
    assert!(candidate["value"]["account_id"].is_null());
    assert!(candidate["value"]["relationship"].is_null());
    assert!(candidate["value"]["observed_at"].is_null());
    assert_eq!(candidate["value"]["time_status"], "unknown");
    assert_eq!(candidate["value"]["effective"], false);
    assert_eq!(candidate["value"]["adopted"], false);
    assert_eq!(candidate["value"]["payment_authorization"], Value::Null);
    assert_eq!(candidate["adopted"], false);
}

fn sql(vault: &Path, statement: &str) -> String {
    let output = Command::new("sqlite3")
        .arg(vault.join("vault.db"))
        .arg(statement)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn object_bytes(vault: &Path, body: &Value) -> Vec<u8> {
    fs::read(
        vault
            .join("objects")
            .join(body["object_id"].as_str().unwrap()),
    )
    .unwrap()
}

#[test]
fn submitted_fragment_keeps_bytes_and_points_at_them() {
    let tmp = Tmp::new("locate");
    let vault = init(&tmp);
    let file = tmp.path().join("note.txt");
    let mut text = String::from("café\nAmount paid USD 15.99\n");
    text.push_str("cafe\u{0301}\n\u{FB01}\n\u{1F4B0}\n");
    write_file(&file, text.as_bytes());
    let before = fs::read(&file).unwrap();
    let (code, body) = fragment(&vault, "note", &file);
    assert_eq!(code, 0, "{body}");
    honest_body(&body);
    assert_eq!(body["outcome"], "Committed");
    assert_eq!(body["new_effect"], true);
    assert_eq!(body["preservation"], "preserved");
    assert_eq!(body["arrangement"], "not_arranged");
    assert_eq!(body["searchability"], "not_searchable");
    assert_eq!(body["role"], "user_provided_fragment");
    assert_eq!(body["complete_message"], false);
    assert_eq!(body["mailbox"], false);
    assert_eq!(body["clipboard"], false);
    assert_eq!(body["media_type"], "text/plain;charset=utf-8");
    assert_eq!(body["config"]["normalization"], "identity-utf8-scalars-1");
    assert_eq!(body["config"]["join"], "single-fragment-1");
    assert_eq!(body["config"]["rules_version"], "stated-lines-1");
    assert_eq!(body["config"]["extractor"], "subvault-body");
    assert_eq!(body["config"]["extractor_version"], "1");
    assert_eq!(fs::read(&file).unwrap(), before);
    assert_eq!(object_bytes(&vault, &body), before);
    let listed = candidates(&vault);
    let paid = kind(&listed, "stated_amount_paid");
    assert_eq!(paid.len(), 1);
    assert_eq!(paid[0]["excerpt"], "Amount paid USD 15.99");
    assert_eq!(paid[0]["usable"], true);
    assert_eq!(paid[0]["value"]["semantic"], "amount_paid");
    assert_eq!(paid[0]["value"]["amount_text"], "15.99");
    assert_eq!(paid[0]["value"]["currency"], "USD");
    points_at(&before, paid[0]);
    assert!(listed["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .all(|candidate| candidate["excerpt"] != "fi" && candidate["excerpt"] != "café"));
    assert!(
        before
            .windows("cafe\u{0301}".len())
            .any(|window| window == "cafe\u{0301}".as_bytes())
            || before
                .windows(6)
                .any(|window| window == "cafe\u{0301}".as_bytes())
    );
    assert!(std::str::from_utf8(&before).unwrap().contains('\u{0301}'));
    assert!(std::str::from_utf8(&before).unwrap().contains('\u{FB01}'));
    assert!(std::str::from_utf8(&before).unwrap().contains('\u{1F4B0}'));

    let (again_code, again) = fragment(&vault, "note-again", &file);
    assert_eq!(again_code, 0, "{again}");
    assert_eq!(again["outcome"], "AlreadyPublished");
    assert_eq!(again["new_effect"], false);
    assert_eq!(again["attempt_created"], false);
    assert_eq!(again["candidate_ids"], body["candidate_ids"]);
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "1");

    let (same_code, same) = fragment(&vault, "note", &file);
    assert_eq!(same_code, 0, "{same}");
    assert_eq!(same["outcome"], "AlreadyCommitted");
    assert_eq!(same["new_effect"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "1");
}

#[test]
fn same_amount_keeps_paid_order_and_refund_apart() {
    let tmp = Tmp::new("amounts");
    let vault = init(&tmp);
    let paid = tmp.path().join("paid.txt");
    let order = tmp.path().join("order.txt");
    let refund = tmp.path().join("refund.txt");
    let bare = tmp.path().join("bare.txt");
    let both = tmp.path().join("both.txt");
    write_file(&paid, b"Amount paid USD 15.99\n");
    write_file(
        &order,
        b"Total USD 15.99\nThis Total is an order total whose payment nature is not stated.\n",
    );
    write_file(
        &refund,
        b"Total USD 15.99\nThis Total is a credit-note total, not an amount paid.\nReceipt of the refund is not stated.\n",
    );
    write_file(&bare, b"Total USD 15.99\n");
    write_file(
        &both,
        b"Total USD 15.99\nTotal USD 15.99\nThis Total is a credit-note total, not an amount paid.\n",
    );
    for (id, path) in [
        ("paid", &paid),
        ("order", &order),
        ("refund", &refund),
        ("bare", &bare),
        ("both", &both),
    ] {
        let (code, body) = fragment(&vault, id, path);
        assert_eq!(code, 0, "{id} {body}");
        honest_body(&body);
    }
    let listed = candidates(&vault);
    let paid_rows = kind(&listed, "stated_amount_paid");
    let order_rows = kind(&listed, "stated_order_total");
    let refund_rows = kind(&listed, "stated_credit_note_total");
    let unresolved = kind(&listed, "unresolved_total");
    assert_eq!(paid_rows.len(), 1);
    assert_eq!(order_rows.len(), 1);
    assert_eq!(refund_rows.len(), 1);
    assert_eq!(paid_rows[0]["value"]["amount_text"], "15.99");
    assert_eq!(order_rows[0]["value"]["amount_text"], "15.99");
    assert_eq!(refund_rows[0]["value"]["amount_text"], "15.99");
    assert_eq!(order_rows[0]["value"]["semantic"], "order_total");
    assert_eq!(order_rows[0]["value"]["payment_nature"], "not_stated");
    assert_eq!(refund_rows[0]["value"]["semantic"], "credit_note_total");
    assert_eq!(refund_rows[0]["value"]["payment_nature"], "not_amount_paid");
    assert_eq!(refund_rows[0]["value"]["refund_receipt"], "not_stated");
    assert_ne!(paid_rows[0]["source_id"], order_rows[0]["source_id"]);
    assert_ne!(paid_rows[0]["source_id"], refund_rows[0]["source_id"]);
    assert!(unresolved.iter().all(|row| row["usable"] == false));
    assert!(unresolved
        .iter()
        .any(|row| row["value"]["reason"] == "payment_nature_not_stated"));
    assert!(unresolved
        .iter()
        .any(|row| row["value"]["reason"] == "multiple_totals_not_paired"));
    assert!(unresolved.len() >= 3);
    for row in listed["candidates"].as_array().unwrap() {
        assert!(row["purchase_id"].is_null());
        assert!(row["relationship"].is_null());
        assert_eq!(row["effective"], false);
    }
}

#[test]
fn missing_time_and_two_accounts_are_not_decided() {
    let tmp = Tmp::new("decide");
    let vault = init(&tmp);
    let period = tmp.path().join("period.txt");
    write_file(
        &period,
        b"Amount paid USD 48.00\nService period: 2023-01-01 through 2023-12-31\nThis document states the service period above and no status after 2023-12-31.\n",
    );
    let accounts = tmp.path().join("accounts.txt");
    write_file(
        &accounts,
        b"Amount paid USD 107.91\nAccount email: mira.ellison.example@paperlantern.example\nAccount email: jordan.hale.example@paperlantern.example\nAccount email: j***@paperlantern.example\nThis notice does not say which account owns the masked email.\nMerchant: Paperlantern\n",
    );
    assert_eq!(fragment(&vault, "period", &period).0, 0);
    assert_eq!(fragment(&vault, "accounts", &accounts).0, 0);
    let listed = candidates(&vault);
    let period_file = fs::read(&period).unwrap();
    let account_file = fs::read(&accounts).unwrap();
    for candidate in listed["candidates"].as_array().unwrap() {
        if candidate["excerpt"].as_str().unwrap().contains("48.00")
            || candidate["excerpt"].as_str().unwrap().contains("2023")
        {
            points_at(&period_file, candidate);
        }
        if candidate["kind"] == "stated_account_label"
            || candidate["excerpt"].as_str().unwrap().contains("107.91")
        {
            points_at(&account_file, candidate);
        }
        assert!(candidate["value"]["observed_at"].is_null());
        assert_eq!(candidate["value"]["current_status"], "unknown");
        assert_ne!(candidate["value"]["current_status"], "expired");
    }
    let periods = kind(&listed, "stated_service_period");
    assert_eq!(periods.len(), 1);
    assert_eq!(periods[0]["value"]["period_start"], "2023-01-01");
    assert_eq!(periods[0]["value"]["period_end"], "2023-12-31");
    let absent = kind(&listed, "stated_status_absent");
    assert_eq!(absent.len(), 1);
    assert_eq!(absent[0]["value"]["after"], "2023-12-31");
    let labels = kind(&listed, "stated_account_label");
    assert_eq!(labels.len(), 3);
    assert!(labels.iter().all(|row| row["value"]["attributed"] == false));
    assert_eq!(
        labels
            .iter()
            .filter(|row| row["value"]["masked"] == true)
            .count(),
        1
    );
    let texts: Vec<&str> = labels
        .iter()
        .map(|row| row["value"]["label"].as_str().unwrap())
        .collect();
    assert!(texts.contains(&"mira.ellison.example@paperlantern.example"));
    assert!(texts.contains(&"jordan.hale.example@paperlantern.example"));
    assert!(texts.contains(&"j***@paperlantern.example"));
    assert!(listed["candidates"].as_array().unwrap().iter().all(|row| {
        row["excerpt"] != "Merchant: Paperlantern"
            && row["excerpt"] != "This notice does not say which account owns the masked email."
    }));
    let opened = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(opened.0, 0);
    let recorded: Vec<i64> = opened.1["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["recorded_at"].as_i64().unwrap())
        .collect();
    assert!(!recorded.is_empty());
    for candidate in listed["candidates"].as_array().unwrap() {
        assert!(candidate["value"]["observed_at"].is_null());
        for stamp in &recorded {
            assert_ne!(candidate["value"]["observed_at"], serde_json::json!(stamp));
        }
    }
}

#[test]
fn bad_fragments_and_old_imports_do_not_become_candidates() {
    let tmp = Tmp::new("reject");
    let vault = init(&tmp);
    let pdf = corpus().join("originals/v-2026-09-receipt.pdf");
    let pdf_bytes = fs::read(&pdf).unwrap();
    let (import_code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf",
        "--media-type",
        "application/pdf",
        "--source",
        pdf.to_str().unwrap(),
    ]);
    assert_eq!(import_code, 0, "{imported}");
    let pdf_hash = imported["content_sha256"].as_str().unwrap().to_string();
    let (query_code, queried) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(query_code, 0, "{queried}");
    assert!(queried["body_schema"].is_null());
    assert_eq!(queried["candidates"].as_array().unwrap().len(), 0);
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM sqlite_master WHERE name='extraction_runs';"
        ),
        "0"
    );

    let empty = tmp.path().join("empty.txt");
    write_file(&empty, b"");
    let (code, body) = fragment(&vault, "empty", &empty);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "empty_fragment");
    assert_eq!(body["bytes_read"], 0);

    let big = tmp.path().join("big.bin");
    write_file(&big, &vec![b'a'; 65_537]);
    let (code, body) = fragment(&vault, "big", &big);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "over_input_budget");
    assert_eq!(body["bytes_read"], 0);
    assert_eq!(body["stat_size"], 65_537);

    let bad = tmp.path().join("bad.bin");
    let bad_bytes = [0xff, 0xfe];
    write_file(&bad, &bad_bytes);
    let (code, body) = fragment(&vault, "bad", &bad);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "invalid_utf8");
    assert_eq!(body["bytes_read"], 2);
    assert_eq!(fs::read(&bad).unwrap(), bad_bytes);

    let line = tmp.path().join("line.txt");
    write_file(&line, &vec![b'a'; 1025]);
    let (code, body) = fragment(&vault, "line", &line);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "over_line");
    assert_eq!(body["preservation"], "preserved");
    assert_eq!(body["candidates_published"], false);
    let (code, again) = fragment(&vault, "line-2", &line);
    assert_eq!(code, 16, "{again}");
    assert_eq!(again["reason"], "over_line");
    let listed = candidates(&vault);
    assert!(kind(&listed, "stated_amount_paid").is_empty());
    let attempts = listed["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["source_id"] == body["source_id"])
        .unwrap()["attempts"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(attempts, 2);

    let many = tmp.path().join("many.txt");
    let mut lines = String::new();
    for _ in 0..65 {
        lines.push_str("Amount paid USD 15.99\n");
    }
    write_file(&many, lines.as_bytes());
    let (code, body) = fragment(&vault, "many", &many);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "too_many_candidates");
    assert_eq!(body["candidates_published"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "0");

    let same = tmp.path().join("same.txt");
    write_file(&same, b"Amount paid USD 15.99\n");
    let (code, imported_text) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "text-import",
        "--media-type",
        "text/plain;charset=utf-8",
        "--source",
        same.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported_text}");
    let (code, refused) = fragment(&vault, "not-submitted", &same);
    assert_eq!(code, 16, "{refused}");
    assert_eq!(refused["reason"], "not_a_submitted_fragment");
    assert_eq!(refused["preservation"], "preserved");
    assert_eq!(refused["candidates_published"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "0");
    assert_eq!(sql(&vault, "PRAGMA user_version;"), "2");
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT content_sha256 FROM objects WHERE content_sha256='{pdf_hash}';")
        ),
        pdf_hash
    );
    let opened = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(opened.0, 0, "{}", opened.1);
    assert_eq!(opened.1["slice"], "T1");
    assert_eq!(opened.1["schema_version"], 1);
    assert!(opened.1["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["content_sha256"] == pdf_hash
                && source["preservation"] == "preserved"
                && source["arrangement"] == "not_arranged"
                && source["searchability"] == "not_searchable"
                && source["purchase_id"].is_null()
        }));
    assert_eq!(
        fs::read(
            vault
                .join("objects")
                .join(imported["object_id"].as_str().unwrap())
        )
        .unwrap(),
        pdf_bytes
    );
}

#[test]
fn schema_failure_does_not_raise_user_version() {
    let tmp = Tmp::new("schema");
    let vault = init(&tmp);
    let pdf = corpus().join("originals/v-2026-10-receipt.pdf");
    let (code, imported) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf",
        "--media-type",
        "application/pdf",
        "--source",
        pdf.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{imported}");
    let object = fs::read(
        vault
            .join("objects")
            .join(imported["object_id"].as_str().unwrap()),
    )
    .unwrap();
    let note = tmp.path().join("note.txt");
    write_file(&note, b"Amount paid USD 15.99\n");
    let witness = tmp.path().join("witness.json");
    let (code, body) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "schema",
        "--fragment",
        note.to_str().unwrap(),
        "--inject",
        "enospc:body_schema",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 15, "{body}");
    assert_eq!(body["reason"], "injected_enospc");
    assert_eq!(body["injected"], true);
    assert_eq!(body["preservation"], "not_preserved");
    assert_eq!(sql(&vault, "PRAGMA user_version;"), "2");
    assert_eq!(
        sql(
            &vault,
            "SELECT count(*) FROM sqlite_master WHERE name='candidates';"
        ),
        "0"
    );
    assert_eq!(
        fs::read(
            vault
                .join("objects")
                .join(imported["object_id"].as_str().unwrap())
        )
        .unwrap(),
        object
    );
    let (code, published) = fragment(&vault, "schema", &note);
    assert_eq!(code, 0, "{published}");
    assert_eq!(published["candidate_count"], 1);
    assert_eq!(sql(&vault, "PRAGMA user_version;"), "2");
    assert_eq!(
        fs::read(
            vault
                .join("objects")
                .join(imported["object_id"].as_str().unwrap())
        )
        .unwrap(),
        object
    );

    let fresh = Tmp::new("schema-bad");
    let other = init(&fresh);
    let marker = fresh.path().join("note.txt");
    write_file(&marker, b"Amount paid USD 4.00\n");
    assert_eq!(fragment(&other, "first", &marker).0, 0);
    sql(&other, "UPDATE meta SET value='9' WHERE key='body_schema';");
    let next = fresh.path().join("next.txt");
    write_file(&next, b"Amount paid USD 5.00\n");
    let before_sources = sql(&other, "SELECT count(*) FROM sources;");
    let (code, refused) = fragment(&other, "next", &next);
    assert_eq!(code, 16, "{refused}");
    assert_eq!(refused["reason"], "body_schema_unsupported");
    assert_eq!(sql(&other, "PRAGMA user_version;"), "2");
    assert_eq!(
        sql(&other, "SELECT value FROM meta WHERE key='body_schema';"),
        "9"
    );
    assert_eq!(sql(&other, "SELECT count(*) FROM sources;"), before_sources);
    assert_eq!(sql(&other, "SELECT count(*) FROM candidates;"), "1");
}

#[test]
fn publish_failure_cancel_and_crash_keep_prior_results() {
    let tmp = Tmp::new("retry");
    let vault = init(&tmp);
    let old = tmp.path().join("old.txt");
    write_file(&old, b"Amount paid USD 15.99\n");
    let (code, first) = fragment(&vault, "old", &old);
    assert_eq!(code, 0, "{first}");
    let old_hash = first["content_sha256"].as_str().unwrap().to_string();
    let old_ids = first["candidate_ids"].clone();
    let old_object = object_bytes(&vault, &first);

    let total = tmp.path().join("total.txt");
    write_file(
        &total,
        b"Total USD 15.99\nThis Total is an order total whose payment nature is not stated.\n",
    );
    let witness = tmp.path().join("witness.json");
    let (code, failed) = run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "disk",
        "--fragment",
        total.to_str().unwrap(),
        "--inject",
        "enospc:db_commit",
        "--witness",
        witness.to_str().unwrap(),
    ]);
    assert_eq!(code, 3, "{failed}");
    assert_eq!(failed["outcome"], "NotPublished");
    assert_eq!(failed["injected"], true);
    assert_eq!(failed["preservation"], "preserved");
    assert_eq!(failed["candidates_published"], false);
    assert_eq!(failed["new_effect"], false);
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "1");
    let (code, retried) = fragment(&vault, "disk", &total);
    assert_eq!(code, 0, "{retried}");
    assert_eq!(retried["outcome"], "Committed");
    assert_eq!(retried["new_effect"], true);
    assert_eq!(retried["attempt_no"], 2);
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "2");
    assert_eq!(object_bytes(&vault, &first), old_object);

    let late_file = tmp.path().join("late.txt");
    write_file(&late_file, b"Amount paid USD 8.00\n");
    let checkpoint = tmp.path().join("late.json");
    let mut child = Command::new(bin())
        .args([
            "fragment",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "late",
            "--fragment",
            late_file.to_str().unwrap(),
            "--stop-at",
            "before_publish",
            "--checkpoint",
            checkpoint.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_stopped(&mut child, &checkpoint);
    let (cancel_code, cancel) = run(&[
        "cancel",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "late",
        "--generation",
        "1",
        "--kind",
        "fragment",
    ]);
    assert_eq!(cancel_code, 0, "{cancel}");
    assert_eq!(cancel["outcome"], "CancelPending");
    assert_eq!(cancel["cancel_applied"], false);
    assert_eq!(cancel["candidates_published"], false);
    assert!(cancel.get("preservation").is_none() || cancel["preservation"].is_null());
    resume(child.id());
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let writer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(writer["outcome"], "Late");
    assert_eq!(writer["late"], true);
    assert_eq!(writer["candidates_published"], false);
    assert_eq!(writer["preservation"], "preserved");
    assert_eq!(writer["new_effect"], false);
    let listed = candidates(&vault);
    assert!(listed["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["excerpt"] != "Amount paid USD 8.00"));
    assert!(listed["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["candidate_id"] == old_ids[0]));
    let (code, after_late) = fragment(&vault, "late", &late_file);
    assert_eq!(code, 0, "{after_late}");
    assert_eq!(after_late["candidate_count"], 1);
    assert_eq!(after_late["attempt_no"], 2);

    let crash_file = tmp.path().join("crash.txt");
    write_file(&crash_file, b"Amount paid USD 3.00\n");
    let crash_point = tmp.path().join("crash.json");
    let mut child = Command::new(bin())
        .args([
            "fragment",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "crash",
            "--fragment",
            crash_file.to_str().unwrap(),
            "--stop-at",
            "before_publish",
            "--checkpoint",
            crash_point.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_stopped(&mut child, &crash_point);
    child.kill().unwrap();
    let killed = child.wait_with_output().unwrap();
    assert_eq!(killed.status.code(), None);
    assert!(killed.stdout.is_empty());
    let opened = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(opened.0, 0, "{}", opened.1);
    assert_eq!(opened.1["slice"], "T1");
    let incomplete = opened.1["incomplete_commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["command_id"] == "crash")
        .unwrap();
    assert_eq!(incomplete["preservation"], "preserved");
    assert_eq!(incomplete["candidates_published"], false);
    assert!(opened.1["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["content_sha256"] == old_hash && source["preservation"] == "preserved"
        }));
    let (code, recovered) = fragment(&vault, "crash-retry", &crash_file);
    assert_eq!(code, 0, "{recovered}");
    assert_eq!(recovered["candidate_count"], 1);
    assert_eq!(recovered["attempt_no"], 2);
    assert_eq!(object_bytes(&vault, &first), old_object);
    assert_eq!(
        sql(
            &vault,
            &format!("SELECT count(*) FROM candidates WHERE excerpt='Amount paid USD 3.00';")
        ),
        "1"
    );
}

#[test]
fn stdin_is_not_read_as_the_fragment() {
    let tmp = Tmp::new("stdin");
    let vault = init(&tmp);
    let file = tmp.path().join("file.txt");
    write_file(&file, b"Amount paid USD 15.99\n");
    let mut child = Command::new(bin())
        .args([
            "fragment",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "stdin",
            "--fragment",
            file.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Amount paid USD 1.00\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    let listed = candidates(&vault);
    assert_eq!(listed["candidates"][0]["excerpt"], "Amount paid USD 15.99");
    assert_eq!(body["candidate_count"], 1);
}

fn inject_fragment(
    vault: &Path,
    command: &str,
    file: &Path,
    point: &str,
    witness: &Path,
) -> (i32, Value) {
    let spec = format!("enospc:{point}");
    run(&[
        "fragment",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--fragment",
        file.to_str().unwrap(),
        "--inject",
        spec.as_str(),
        "--witness",
        witness.to_str().unwrap(),
    ])
}

fn assert_source_kept(body: &Value, hash: &str) {
    assert_eq!(body["preservation"], "preserved", "{body}");
    assert_ne!(body["outcome"], "NotPreserved", "{body}");
    assert_eq!(body["candidates_published"], false, "{body}");
    assert_eq!(body["new_effect"], false, "{body}");
    assert_eq!(body["content_sha256"], hash, "{body}");
}

#[test]
fn failure_after_the_source_commit_does_not_relabel_preservation() {
    let tmp = Tmp::new("kept");
    let vault = init(&tmp);
    let old = tmp.path().join("old.txt");
    write_file(&old, b"Amount paid USD 15.99\n");
    let (code, first) = fragment(&vault, "old", &old);
    assert_eq!(code, 0, "{first}");
    let old_hash = first["content_sha256"].as_str().unwrap().to_string();
    let old_id = first["candidate_ids"][0].as_str().unwrap().to_string();
    let old_object = object_bytes(&vault, &first);

    let next = tmp.path().join("next.txt");
    write_file(&next, b"Amount paid USD 8.00\n");
    let witness = tmp.path().join("witness.json");
    let (code, failed) = inject_fragment(&vault, "later", &next, "publish_tx", &witness);
    assert_eq!(code, 15, "{failed}");
    assert_eq!(failed["reason"], "injected_enospc");
    assert_eq!(failed["inject_point"], "publish_tx");
    let later_hash = failed["content_sha256"].as_str().unwrap().to_string();
    assert_source_kept(&failed, &later_hash);
    assert_eq!(object_bytes(&vault, &first), old_object);
    let later_object = sql(
        &vault,
        &format!("SELECT object_id FROM sources WHERE content_sha256='{later_hash}';"),
    );
    assert_eq!(
        fs::read(vault.join("objects").join(later_object)).unwrap(),
        fs::read(&next).unwrap()
    );
    let listed = candidates(&vault);
    let rows = listed["candidates"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["candidate_id"], old_id);
    assert_eq!(rows[0]["content_sha256"], old_hash);
    let opened = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(opened.0, 0, "{}", opened.1);
    assert!(opened.1["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["content_sha256"] == old_hash && source["preservation"] == "preserved"
        }));
    assert!(opened.1["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["content_sha256"] == later_hash && source["preservation"] == "preserved"
        }));
    let (code, retried) = fragment(&vault, "later", &next);
    assert_eq!(code, 0, "{retried}");
    assert_eq!(retried["outcome"], "Committed");
    assert_eq!(retried["attempt_no"], 2);
    assert_eq!(retried["candidate_count"], 1);
    assert_eq!(object_bytes(&vault, &first), old_object);
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "2");

    let early = Tmp::new("pre-commit");
    let early_vault = init(&early);
    let note = early.path().join("note.txt");
    write_file(&note, b"Amount paid USD 4.00\n");
    let early_witness = early.path().join("witness.json");
    let (code, refused) =
        inject_fragment(&early_vault, "schema", &note, "body_schema", &early_witness);
    assert_eq!(code, 15, "{refused}");
    assert_eq!(refused["reason"], "injected_enospc");
    assert_eq!(refused["preservation"], "not_preserved");
    assert_eq!(
        sql(
            &early_vault,
            "SELECT count(*) FROM sqlite_master WHERE name='candidates';"
        ),
        "0"
    );
    assert!(
        run(&["open", "--vault", early_vault.to_str().unwrap()]).1["sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn publish_stages_keep_the_stored_source_and_the_old_success() {
    let tmp = Tmp::new("stages");
    let vault = init(&tmp);
    let old = tmp.path().join("old.txt");
    write_file(&old, b"Amount paid USD 15.99\n");
    let (code, first) = fragment(&vault, "old", &old);
    assert_eq!(code, 0, "{first}");
    let old_bytes = object_bytes(&vault, &first);
    let old_id = first["candidate_ids"][0].clone();

    let wide = tmp.path().join("wide.txt");
    write_file(&wide, &vec![b'a'; 1025]);
    let witness = tmp.path().join("witness.json");
    let (code, status) = inject_fragment(&vault, "wide", &wide, "extract_status", &witness);
    assert_eq!(code, 15, "{status}");
    assert_eq!(status["inject_point"], "extract_status");
    assert_source_kept(&status, status["content_sha256"].as_str().unwrap());
    assert_eq!(
        sql(
            &vault,
            "SELECT status FROM extraction_attempts WHERE command_id='wide';"
        ),
        "running"
    );
    assert!(status.get("attempt_update").is_none());
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "1");
    let (code, over) = fragment(&vault, "wide", &wide);
    assert_eq!(code, 16, "{over}");
    assert_eq!(over["reason"], "over_line");
    assert_eq!(over["preservation"], "preserved");
    assert_eq!(over["candidates_published"], false);
    assert_eq!(
        sql(
            &vault,
            "SELECT status FROM extraction_attempts WHERE command_id='wide' ORDER BY attempt_no;"
        ),
        "abandoned\nfailed"
    );

    let encoded = tmp.path().join("encoded.txt");
    write_file(&encoded, b"Amount paid USD 6.00\n");
    let (code, encode) = inject_fragment(&vault, "encode", &encoded, "result_encode", &witness);
    assert_eq!(code, 15, "{encode}");
    assert_eq!(encode["inject_point"], "result_encode");
    assert_source_kept(&encode, encode["content_sha256"].as_str().unwrap());
    assert_eq!(
        sql(
            &vault,
            "SELECT status || ':' || published FROM extraction_attempts WHERE command_id='encode';"
        ),
        "failed:0"
    );
    assert!(encode.get("attempt_update").is_none());
    let (code, encode_ok) = fragment(&vault, "encode", &encoded);
    assert_eq!(code, 0, "{encode_ok}");
    assert_eq!(encode_ok["attempt_no"], 2);
    assert_eq!(encode_ok["candidate_count"], 1);

    let (code, replay) = inject_fragment(&vault, "replay", &old, "replay_tx", &witness);
    assert_eq!(code, 15, "{replay}");
    assert_eq!(replay["inject_point"], "replay_tx");
    assert_source_kept(&replay, &first["content_sha256"].as_str().unwrap());
    let (code, replay_encode) =
        inject_fragment(&vault, "replay-encode", &old, "result_encode", &witness);
    assert_eq!(code, 15, "{replay_encode}");
    assert_eq!(replay_encode["inject_point"], "result_encode");
    assert_source_kept(&replay_encode, &first["content_sha256"].as_str().unwrap());
    let listed = candidates(&vault);
    let old_rows: Vec<_> = listed["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["candidate_id"] == old_id)
        .collect();
    assert_eq!(old_rows.len(), 1);
    let (code, again) = fragment(&vault, "replay", &old);
    assert_eq!(code, 0, "{again}");
    assert_eq!(again["outcome"], "AlreadyPublished");
    assert_eq!(again["candidate_ids"][0], old_id);
    assert_eq!(object_bytes(&vault, &first), old_bytes);

    let blocked = tmp.path().join("blocked.txt");
    write_file(&blocked, b"Amount paid USD 7.00\n");
    sql(
        &vault,
        "CREATE TRIGGER block_candidate BEFORE INSERT ON candidates BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
    );
    let (code, db_fail) = fragment(&vault, "blocked", &blocked);
    assert_eq!(code, 15, "{db_fail}");
    assert_eq!(db_fail["reason"], "db_error");
    assert_eq!(db_fail["during"], "insert_candidate");
    assert_eq!(db_fail["injected"], false);
    assert_source_kept(&db_fail, db_fail["content_sha256"].as_str().unwrap());
    assert_eq!(object_bytes(&vault, &first), old_bytes);
    sql(&vault, "DROP TRIGGER block_candidate;");
    let (code, unblocked) = fragment(&vault, "blocked", &blocked);
    assert_eq!(code, 0, "{unblocked}");
    assert_eq!(unblocked["attempt_no"], 2);
    assert_eq!(unblocked["candidate_count"], 1);
    assert_eq!(object_bytes(&vault, &first), old_bytes);
}

#[test]
fn a_failed_status_write_is_not_reported_as_cleanup() {
    let tmp = Tmp::new("cleanup");
    let vault = init(&tmp);
    let old = tmp.path().join("old.txt");
    write_file(&old, b"Amount paid USD 15.99\n");
    let (code, first) = fragment(&vault, "old", &old);
    assert_eq!(code, 0, "{first}");
    let old_bytes = object_bytes(&vault, &first);
    let old_id = first["candidate_ids"][0].clone();

    let total = tmp.path().join("total.txt");
    write_file(
        &total,
        b"Total USD 15.99\nThis Total is an order total whose payment nature is not stated.\n",
    );
    sql(
        &vault,
        "CREATE TRIGGER block_attempt BEFORE UPDATE ON extraction_attempts WHEN NEW.status != 'succeeded' BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
    );
    sql(
        &vault,
        "CREATE TRIGGER block_run BEFORE UPDATE ON extraction_runs WHEN NEW.status != 'succeeded' BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
    );
    let witness = tmp.path().join("witness.json");
    let (code, failed) = inject_fragment(&vault, "disk", &total, "db_commit", &witness);
    assert_eq!(code, 3, "{failed}");
    assert_eq!(failed["outcome"], "NotPublished");
    assert_eq!(failed["injected"], true);
    assert_eq!(failed["inject_point"], "db_commit");
    assert_source_kept(&failed, failed["content_sha256"].as_str().unwrap());
    assert_eq!(failed["attempt_update"], "failed");
    assert_eq!(failed["run_update"], "failed");
    assert_eq!(
        sql(
            &vault,
            "SELECT status FROM extraction_attempts WHERE command_id='disk';"
        ),
        "running"
    );
    assert_eq!(sql(&vault, "SELECT count(*) FROM candidates;"), "1");
    let opened = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(opened.0, 0, "{}", opened.1);
    let incomplete = opened.1["incomplete_commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["command_id"] == "disk")
        .unwrap();
    assert_eq!(incomplete["state"], "object_ready");
    assert_eq!(incomplete["preservation"], "preserved");
    assert_eq!(incomplete["candidates_published"], false);
    assert_eq!(object_bytes(&vault, &first), old_bytes);
    sql(
        &vault,
        "DROP TRIGGER block_attempt; DROP TRIGGER block_run;",
    );
    let (code, retried) = fragment(&vault, "disk", &total);
    assert_eq!(code, 0, "{retried}");
    assert_eq!(retried["attempt_no"], 2);
    assert_eq!(retried["candidate_count"], 1);
    let listed = candidates(&vault);
    assert!(listed["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["candidate_id"] == old_id));
    assert_eq!(object_bytes(&vault, &first), old_bytes);
}

#[test]
fn a_damaged_stored_record_is_not_a_null_candidate() {
    let tmp = Tmp::new("record");
    let vault = init(&tmp);
    let keep = tmp.path().join("keep.txt");
    write_file(&keep, b"Amount paid USD 15.99\n");
    let (code, first) = fragment(&vault, "keep", &keep);
    assert_eq!(code, 0, "{first}");
    let keep_bytes = object_bytes(&vault, &first);
    let other = tmp.path().join("other.txt");
    write_file(&other, b"Amount paid USD 6.00\n");
    let (code, second) = fragment(&vault, "other", &other);
    assert_eq!(code, 0, "{second}");
    let other_bytes = object_bytes(&vault, &second);
    let candidate_id = second["candidate_ids"][0].as_str().unwrap();
    let run_id = second["run_id"].as_str().unwrap();
    let attempt_id = second["attempt_id"].as_str().unwrap();
    sql(
        &vault,
        &format!("UPDATE candidates SET value_json='{{', locator_json='{{' WHERE candidate_id='{candidate_id}';"),
    );
    sql(
        &vault,
        &format!("UPDATE extraction_runs SET config_json='{{' WHERE run_id='{run_id}';"),
    );
    sql(
        &vault,
        &format!(
            "UPDATE extraction_attempts SET detail_json='{{' WHERE attempt_id='{attempt_id}';"
        ),
    );
    let (code, listed) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{listed}");
    assert_eq!(listed["outcome"], "RecoveryRequired");
    assert_eq!(listed["read_only"], true);
    let rows = listed["candidates"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["candidate_id"], first["candidate_ids"][0]);
    assert_eq!(rows[0]["excerpt"], "Amount paid USD 15.99");
    assert!(rows.iter().all(|row| !row["value"].is_null()));
    let durings: Vec<_> = listed["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|alert| alert["during"].as_str().unwrap())
        .collect();
    assert!(durings.contains(&"value_json"));
    assert!(durings.contains(&"locator_json"));
    assert!(durings.contains(&"config_json"));
    assert!(durings.contains(&"detail_json"));
    assert!(listed["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|alert| { alert["code"] == "stored_record_unreadable" }));
    let damaged_run = listed["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["run_id"] == run_id)
        .unwrap();
    assert!(damaged_run.get("config").is_none());
    assert!(damaged_run["attempts"][0].get("detail").is_none());
    assert_eq!(object_bytes(&vault, &first), keep_bytes);
    assert_eq!(object_bytes(&vault, &second), other_bytes);
    let opened = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(opened.0, 0, "{}", opened.1);
    assert_eq!(opened.1["outcome"], "Ready");
}

#[test]
fn a_crlf_line_ending_is_not_part_of_the_sentence() {
    let tmp = Tmp::new("crlf");
    let vault = init(&tmp);
    let file = tmp.path().join("paid.txt");
    let bytes = b"Amount paid USD 2.00\r\n".to_vec();
    write_file(&file, &bytes);
    let (code, body) = fragment(&vault, "paid", &file);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["config"]["line_ending"], "lf-or-crlf-1");
    assert_eq!(body["config"]["normalization"], "identity-utf8-scalars-1");
    assert_eq!(body["config"]["rules_version"], "stated-lines-1");
    assert_eq!(object_bytes(&vault, &body), bytes);
    assert_eq!(fs::read(&file).unwrap(), bytes);
    let listed = candidates(&vault);
    let paid = kind(&listed, "stated_amount_paid");
    assert_eq!(paid.len(), 1);
    assert_eq!(paid[0]["excerpt"], "Amount paid USD 2.00");
    assert_eq!(paid[0]["value"]["amount_text"], "2.00");
    points_at(&bytes, paid[0]);
    assert_eq!(paid[0]["locator"]["byte_end"], 20);
    assert_eq!(bytes[20], b'\r');
    assert_eq!(bytes[21], b'\n');

    let mixed = tmp.path().join("mixed.txt");
    let mixed_bytes = b"Amount paid USD 2.00\rX\n".to_vec();
    write_file(&mixed, &mixed_bytes);
    let (code, mixed_body) = fragment(&vault, "mixed", &mixed);
    assert_eq!(code, 0, "{mixed_body}");
    assert_eq!(mixed_body["candidate_count"], 0);
    assert_eq!(object_bytes(&vault, &mixed_body), mixed_bytes);
}

#[test]
fn fragment_rejections_use_the_body_slice() {
    let tmp = Tmp::new("slice");
    let vault = init(&tmp);
    let empty = tmp.path().join("empty.txt");
    write_file(&empty, b"");
    let (code, body) = fragment(&vault, "empty", &empty);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["slice"], "T2a-body");
    assert_eq!(body["reason"], "empty_fragment");

    let big = tmp.path().join("big.txt");
    write_file(&big, &vec![b'a'; 65537]);
    let (code, body) = fragment(&vault, "big", &big);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["slice"], "T2a-body");
    assert_eq!(body["reason"], "over_input_budget");
    assert_eq!(body["bytes_read"], 0);

    let bad = tmp.path().join("bad.bin");
    write_file(&bad, &[0xff, 0xfe]);
    let (code, body) = fragment(&vault, "bad", &bad);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["slice"], "T2a-body");
    assert_eq!(body["reason"], "invalid_utf8");

    let wide = tmp.path().join("wide.txt");
    write_file(&wide, &vec![b'a'; 1025]);
    let (code, body) = fragment(&vault, "wide", &wide);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["slice"], "T2a-body");
    assert_eq!(body["reason"], "over_line");
    assert_eq!(body["preservation"], "preserved");

    let note = tmp.path().join("note.txt");
    write_file(&note, b"Amount paid USD 4.00\n");
    assert_eq!(fragment(&vault, "ok", &note).0, 0);
    sql(&vault, "UPDATE meta SET value='9' WHERE key='body_schema';");
    let again = tmp.path().join("again.txt");
    write_file(&again, b"Amount paid USD 5.00\n");
    let (code, body) = fragment(&vault, "again", &again);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["slice"], "T2a-body");
    assert_eq!(body["reason"], "body_schema_unsupported");

    sql(&vault, "UPDATE meta SET value='1' WHERE key='body_schema';");
    sql(&vault, "DROP TABLE candidates;");
    let (code, body) = run(&["candidates", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 12, "{body}");
    assert_eq!(body["slice"], "T2a-body");
    assert_eq!(body["reason"], "body_schema_mismatch");
    let opened = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(opened.0, 0, "{}", opened.1);
    assert_eq!(opened.1["slice"], "T1");
}

fn wait_stopped(child: &mut std::process::Child, checkpoint: &Path) {
    let start = Instant::now();
    loop {
        if checkpoint.is_file() {
            break;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before checkpoint: {status}");
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("checkpoint timeout");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before stop: {status}");
        }
        if stopped(child.id()) {
            return;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("pid {} was not observed stopped", child.id());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stopped(pid: u32) -> bool {
    let Ok(text) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return false;
    };
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next() == Some("State:") && parts.next() == Some("T")
    })
}

fn resume(pid: u32) {
    assert!(stopped(pid), "refusing SIGCONT before State T");
    assert!(Command::new("kill")
        .args(["-CONT", &pid.to_string()])
        .status()
        .unwrap()
        .success());
}
