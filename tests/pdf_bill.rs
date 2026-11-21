use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

const ACTOR: &str = "act_00000000000000000000000000000002";

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

fn corpus() -> PathBuf {
    if let Ok(dir) = std::env::var("SUBVAULT_CORPUS_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("synthetic/corpus")
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-pdf-{label}-{}-{}",
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

fn init_format(tmp: &Tmp, name: &str, format: &str) -> PathBuf {
    let vault = tmp.path().join(name);
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

#[test]
fn format_5_ingests_full_synthetic_pdf_corpus_preserves_edits_and_deduplicates() {
    let tmp = Tmp::new("corpus");
    let vault = init_format(&tmp, "vault5", "5");

    // 1. September receipt PDF -> adopts USD 15.99 + 2026-09-01..2026-09-30 on page 1
    let sep = corpus().join("originals/v-2026-09-receipt.pdf");
    let (code, sep_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-sep",
        "--source",
        sep.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{sep_res}");
    assert_eq!(sep_res["slice"], "T5-pdf");
    assert_eq!(sep_res["preservation"], "preserved");
    assert_eq!(sep_res["extraction"], "matched");
    assert_eq!(sep_res["page_count"], 1);
    assert_eq!(
        sep_res["content_sha256"],
        "c69d395d4e1abf23a82ef91c5b50220bdb821c4de53daf03a1f060d49acec8f3"
    );
    assert_eq!(sep_res["facts"].as_array().unwrap().len(), 2);

    // Apply human correction to September receipt amount
    let amt_fact_id = sep_res["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["fact_type"] == "stated_amount_paid")
        .unwrap()["fact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, corrected) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "fix-sep",
        "--fact-id",
        &amt_fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "14.99",
        "--actor-id",
        ACTOR,
    ]);
    assert_eq!(code, 0, "{corrected}");

    // 2. Renamed byte-identical duplicate -> AlreadyPreserved, zero new facts, human edit intact!
    let renamed = corpus().join("originals/renamed/NLV-september-statement-copy.pdf");
    let (code, dup_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-sep-renamed",
        "--source",
        renamed.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{dup_res}");
    assert_eq!(dup_res["outcome"], "AlreadyPreserved");
    assert_eq!(dup_res["facts_created"], false);
    assert_eq!(dup_res["source_id"], sep_res["source_id"]);
    assert_eq!(dup_res["object_id"], sep_res["object_id"]);

    let (code, sep_facts) = run(&[
        "facts",
        "--vault",
        vault.to_str().unwrap(),
        "--fact-id",
        &amt_fact_id,
    ]);
    assert_eq!(code, 0, "{sep_facts}");
    assert_eq!(sep_facts["facts"][0]["value"]["amount_text"], "14.99");
    assert_eq!(sep_facts["facts"][0]["locator"]["locator"]["pdf_page"], 1);

    // 3. Credit note PDF -> Total USD 15.99 MUST NOT become a stated_amount_paid fact
    let credit = corpus().join("originals/v-2026-10-credit-note.pdf");
    let (code, cn_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-credit",
        "--source",
        credit.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{cn_res}");
    assert_eq!(cn_res["preservation"], "preserved");
    assert_eq!(cn_res["facts"].as_array().unwrap().len(), 0);
    let (code, cn_cands) = run(&[
        "candidates",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        cn_res["source_id"].as_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{cn_cands}");
    assert!(cn_cands["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["kind"] == "credit_note_total" && c["usable"] == false));

    // 4. Order confirmation PDF -> Total USD 99.00 MUST NOT become a paid fact
    let order = corpus().join("originals/c-work-order-confirm.pdf");
    let (code, oc_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-order",
        "--source",
        order.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{oc_res}");
    assert_eq!(oc_res["facts"].as_array().unwrap().len(), 0);

    // 5. Masked cancellation notice -> unassigned masked email + usable_through, zero paid facts
    let cancel = corpus().join("originals/c-cancel-notice-masked.pdf");
    let (code, can_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-cancel",
        "--source",
        cancel.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{can_res}");
    assert_eq!(can_res["facts"].as_array().unwrap().len(), 0);
    let (code, can_cands) = run(&[
        "candidates",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        can_res["source_id"].as_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{can_cands}");
    let can_list = can_cands["candidates"].as_array().unwrap();
    assert!(can_list
        .iter()
        .any(|c| c["kind"] == "masked_account_email" && c["value"]["attributed"] == false));
    assert!(can_list
        .iter()
        .any(|c| c["kind"] == "cancellation_usable_through"
            && c["value"]["usable_through"] == "2027-02-28"));

    // 6. One-time perpetual license PDF -> adopts USD 79.00 only, no invented service period
    let onetime = corpus().join("originals/lumen-draft-onetime.pdf");
    let (code, ot_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-onetime",
        "--source",
        onetime.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{ot_res}");
    let ot_facts = ot_res["facts"].as_array().unwrap();
    assert_eq!(ot_facts.len(), 1);
    assert_eq!(ot_facts[0]["fact_type"], "stated_amount_paid");

    // 7. Personal & Work Paperlantern invoices stay distinct sources even with identical USD 107.91
    let personal = corpus().join("originals/c-personal-2026-invoice.pdf");
    let work = corpus().join("originals/c-work-2026-invoice.pdf");
    let (c1, p_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-personal",
        "--source",
        personal.to_str().unwrap(),
    ]);
    let (c2, w_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-work",
        "--source",
        work.to_str().unwrap(),
    ]);
    assert_eq!(c1, 0, "{p_res}");
    assert_eq!(c2, 0, "{w_res}");
    assert_ne!(p_res["source_id"], w_res["source_id"]);
}

#[test]
fn format_5_supports_mixed_pdf_and_html_flate_encrypted_and_export_verify_review() {
    let tmp = Tmp::new("mixed");
    let vault = init_format(&tmp, "vault5", "5");

    // 1. Compressed FlateDecode PDF
    let stream = b"BT\n(Amount paid USD 35.50) Tj\nT* (Service period: 2026-05-01 through 2026-05-31) Tj\nET\n";
    let deflated = miniz_oxide::deflate::compress_to_vec_zlib(stream, 6);
    let mut flate_pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n".to_vec();
    flate_pdf.extend_from_slice(
        format!(
            "4 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            deflated.len()
        )
        .as_bytes(),
    );
    flate_pdf.extend_from_slice(&deflated);
    flate_pdf.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
    let flate_path = tmp.path().join("compressed.pdf");
    fs::write(&flate_path, &flate_pdf).unwrap();

    let (code, flate_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-flate",
        "--source",
        flate_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{flate_res}");
    assert_eq!(flate_res["extraction"], "matched");
    assert_eq!(flate_res["facts"].as_array().unwrap().len(), 2);

    // 2. Encrypted PDF -> preserved verbatim, extraction unsupported
    let enc_path = tmp.path().join("encrypted.pdf");
    fs::write(
        &enc_path,
        b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R /Encrypt 9 0 R >>\n%%EOF\n",
    )
    .unwrap();
    let (code, enc_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-enc",
        "--source",
        enc_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{enc_res}");
    assert_eq!(enc_res["preservation"], "preserved");
    assert_eq!(enc_res["extraction"], "unsupported");
    assert_eq!(enc_res["extraction_reason"], "encrypted_pdf");

    // 3. Static HTML bill in the exact same Format 5 vault!
    let html_path = tmp.path().join("bill.html");
    fs::write(
        &html_path,
        "<!doctype html><html><body><table><tr><th>Amount paid</th><td>USD 12.00</td></tr><tr><th>Service period:</th><td>2026-06-01 through 2026-06-30</td></tr></table></body></html>\n",
    )
    .unwrap();
    let (code, html_res) = run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "html-1",
        "--source",
        html_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{html_res}");
    assert_eq!(html_res["preservation"], "preserved");
    assert_eq!(html_res["facts"].as_array().unwrap().len(), 2);

    // 4. Renamed non-PDF text file -> rejected before preservation
    let fake_pdf = tmp.path().join("fake.pdf");
    fs::write(&fake_pdf, b"Amount paid USD 99.00\n").unwrap();
    let (code, fake_res) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "pdf-fake",
        "--source",
        fake_pdf.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{fake_res}");
    assert_eq!(fake_res["reason"], "invalid_pdf_header");
    assert_eq!(fake_res["preservation"], "not_preserved");

    // 5. Export -> verify-package -> review --package all succeed cleanly on Format 5!
    let pack = tmp.path().join("exit-pack-5");
    let (code, exp) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        pack.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exp}");
    assert_eq!(exp["vault_format"], 5);
    assert_eq!(exp["completeness"], "complete");

    let (code, verified) = run(&["verify-package", "--package", pack.to_str().unwrap()]);
    assert_eq!(code, 0, "{verified}");
    assert_eq!(verified["outcome"], "Verified");
    assert_eq!(verified["vault_format"], 5);
    assert_eq!(verified["verified_originals"], 3);
    assert_eq!(verified["facts_csv_checked"], true);

    let review_out = tmp.path().join("review5.txt");
    let (code, rev) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        review_out.to_str().unwrap(),
        "--package",
        pack.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{rev}");
    assert_eq!(rev["package_associated"], true);
    let sheet = fs::read_to_string(&review_out).unwrap();
    assert!(sheet.contains("USD 35.50"));
    assert!(sheet.contains("USD 12.00"));
    assert!(sheet.contains("格式 5 收录静态 PDF 与 HTML 原件"));
}

#[test]
fn older_vault_formats_refuse_pdf_without_modifying_vault() {
    let tmp = Tmp::new("gate");
    let sep = corpus().join("originals/v-2026-09-receipt.pdf");
    for fmt in ["1", "2", "3", "4"] {
        let vault = init_format(&tmp, &format!("v{fmt}"), fmt);
        let before = fs::read(vault.join("vault.db")).unwrap();
        let (code, body) = run(&[
            "pdf",
            "--vault",
            vault.to_str().unwrap(),
            "--command-id",
            "pdf-nope",
            "--source",
            sep.to_str().unwrap(),
        ]);
        assert_eq!(code, 14, "format {fmt}: {body}");
        assert_eq!(body["reason"], "pdf_requires_format_5");
        assert_eq!(body["new_effect"], false);
        assert_eq!(fs::read(vault.join("vault.db")).unwrap(), before);
    }

    // Format 5 refuses fragment & plain import
    let vault5 = init_format(&tmp, "v5", "5");
    let note = tmp.path().join("note.txt");
    fs::write(&note, b"Amount paid USD 10.00\n").unwrap();
    let (code, body) = run(&[
        "fragment",
        "--vault",
        vault5.to_str().unwrap(),
        "--command-id",
        "frag-nope",
        "--fragment",
        note.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "format_5_refuses_fragment");

    let (code, body) = run(&[
        "import",
        "--vault",
        vault5.to_str().unwrap(),
        "--command-id",
        "imp-nope",
        "--media-type",
        "text/plain",
        "--source",
        note.to_str().unwrap(),
    ]);
    assert_eq!(code, 14, "{body}");
    assert_eq!(body["reason"], "format_5_refuses_import");
}
