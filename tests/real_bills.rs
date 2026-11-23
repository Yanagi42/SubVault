use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(name: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("subvault-real-{name}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

fn run(args: &[&str]) -> (i32, Value) {
    let out = Command::new(bin())
        .args(args)
        .env_remove("SUBVAULT_PASSPHRASE")
        .env_remove("SUBVAULT_PASSPHRASE_FILE")
        .env_remove("SUBVAULT_RECOVERY_KEY")
        .env_remove("SUBVAULT_RECOVERY_KEY_FILE")
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let body: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.code().unwrap_or(1), body)
}

fn minimal_pdf(stream: &[u8]) -> Vec<u8> {
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    let o1 = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let o2 = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    let o3 = pdf.len();
    pdf.extend_from_slice(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n");
    let o4 = pdf.len();
    pdf.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", stream.len()).as_bytes());
    pdf.extend_from_slice(stream);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in [o1, o2, o3, o4] {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

fn utf16be_hex(text: &str) -> String {
    let mut hex = String::from("<FEFF");
    for unit in text.encode_utf16() {
        hex.push_str(&format!("{unit:04X}"));
    }
    hex.push('>');
    hex
}

fn scan_dir_for_needles(dir: &Path, needles: &[&str]) -> Vec<String> {
    let mut hits = Vec::new();
    fn visit(dir: &Path, needles: &[&str], hits: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                visit(&p, needles, hits);
            } else if p.is_file() {
                let bytes = fs::read(&p).unwrap_or_default();
                let lossy = String::from_utf8_lossy(&bytes);
                for needle in needles {
                    if lossy.contains(needle) {
                        hits.push(format!("{} contains {:?}", p.display(), needle));
                    }
                }
            }
        }
    }
    visit(dir, needles, &mut hits);
    hits
}

#[test]
fn real_bilingual_pdf_styled_html_gbk_csv_manual_entry_and_encrypted_disk_check() {
    let tmp = Tmp::new("bilingual-e2e");
    let vault = tmp.path.join("vault");
    let pass_file = tmp.path.join("pass.txt");
    fs::write(&pass_file, "RealUserVault-2026!#\n").unwrap();
    let rec_out = tmp.path.join("recovery.txt");

    let (code, init) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "passphrase",
        "--passphrase-file",
        pass_file.to_str().unwrap(),
        "--recovery-key-out",
        rec_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{init}");

    // 1. Real Chinese UTF-16BE electronic invoice PDF (¥98.00)
    let zh_pdf_stream = format!(
        "BT\n{} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nET",
        utf16be_hex("增值税电子普通发票"),
        utf16be_hex("销售方名称：金山办公软件股份有限公司"),
        utf16be_hex("商品名称：WPS超级会员年卡"),
        utf16be_hex("服务周期：2026年1月1日 至 2026年12月31日"),
        utf16be_hex("价税合计（小写）¥98.00"),
    );
    let zh_pdf = tmp.path.join("wps-invoice.pdf");
    fs::write(&zh_pdf, minimal_pdf(zh_pdf_stream.as_bytes())).unwrap();
    let (code, r_zh) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-zh-pdf",
        "--source",
        zh_pdf.to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_zh}");
    assert_eq!(r_zh["extraction"], "matched", "{r_zh}");
    assert_eq!(r_zh["facts"].as_array().unwrap().len(), 2, "{r_zh}");

    // 2. Real styled HTML email receipt (OpenAI $20.00 with <style>, <script>, display:none trap)
    let styled_html = tmp.path.join("openai-receipt.html");
    fs::write(
        &styled_html,
        r#"<!doctype html>
<html>
<head>
  <style>.card { font-family: sans-serif; color: #111; }</style>
  <script>var decoy = "Amount paid $999.00";</script>
</head>
<body>
  <div style="display:none">Amount paid $777.00</div>
  <div class="card">
    <h1>OpenAI Receipt</h1>
    <p>From: OpenAI, LLC</p>
    <p>Account email: work.secret.user@company.example</p>
    <p>Date paid: October 5, 2026</p>
    <table style="width:100%">
      <tr><th>Plan</th><td>ChatGPT Plus (Monthly)</td></tr>
      <tr><th>Billing period</th><td>Oct 5, 2026 - Nov 5, 2026</td></tr>
      <tr><th>Amount paid</th><td style="font-weight:bold">$20.00</td></tr>
    </table>
  </div>
</body>
</html>"#,
    )
    .unwrap();
    let (code, r_html) = run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-styled-html",
        "--source",
        styled_html.to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_html}");
    assert_eq!(r_html["extraction"], "matched", "{r_html}");
    assert_eq!(r_html["facts"].as_array().unwrap().len(), 2, "{r_html}");

    // 3. Real Alipay GBK/GB18030 CSV statement
    let alipay_text = "支付宝交易记录明细查询\n账号:[alipay.secret@example.com]\n------------------------------------\n交易时间,交易分类,交易对方,商品说明,收/支,金额,收/付款方式,交易状态\n2026-10-01 09:00:00,云服务,Apple,iCloud+ 2TB包月,支出,68.00,招商银行,交易成功\n2026-10-08 11:00:00,影音娱乐,哔哩哔哩,大会员连续包月,支出,15.00,余额宝,交易成功\n";
    let (gbk_bytes, _, _) = encoding_rs::GB18030.encode(alipay_text);
    let alipay_csv = tmp.path.join("alipay_record.csv");
    fs::write(&alipay_csv, &gbk_bytes).unwrap();
    let (code, r_csv) = run(&[
        "csv",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-alipay-csv",
        "--source",
        alipay_csv.to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_csv}");
    assert_eq!(r_csv["extraction"], "statement", "{r_csv}");
    assert_eq!(r_csv["encoding"], "gb18030", "{r_csv}");
    assert_eq!(r_csv["provider"], "alipay", "{r_csv}");
    assert_eq!(r_csv["total_rows"], 2, "{r_csv}");

    // 4. Screenshot PNG receipt (raw binary import + annotate + manual record-fact + multi-currency correct + undo)
    let png_path = tmp.path.join("receipt-screenshot.png");
    fs::write(
        &png_path,
        b"\x89PNG\r\n\x1a\nFAKE_SCREENSHOT_RECEIPT_BYTES_PRIVATE_NOTE_XYZ",
    )
    .unwrap();
    let (code, r_png) = run(&[
        "import",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-png-1",
        "--media-type",
        "image/png",
        "--source",
        png_path.to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_png}");
    let png_source_id = r_png["source_id"].as_str().unwrap().to_string();

    let (code, r_ann) = run(&[
        "annotate",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &png_source_id,
        "--vendor",
        "JetBrains SuperSecretVendor",
        "--plan",
        "All Products Pack",
        "--account",
        "公司报销账号",
        "--cycle",
        "yearly",
        "--category",
        "开发工具",
        "--bill-date",
        "2026-10-12",
        "--notes",
        "UltraPrivateNote9981",
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_ann}");
    assert_eq!(r_ann["annotation"]["vendor"], "JetBrains SuperSecretVendor");
    assert_eq!(r_ann["annotation"]["billing_cycle"], "yearly");

    let (code, r_man) = run(&[
        "record-fact",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-man-1",
        "--source-id",
        &png_source_id,
        "--currency",
        "EUR",
        "--amount-text",
        "249.00",
        "--period-start",
        "2026-10-12",
        "--period-end",
        "2027-10-12",
        "--actor-id",
        "act_11111111111111111111111111111111",
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_man}");
    let manual_amt_fact_id = r_man["facts"][0]["fact_id"].as_str().unwrap().to_string();

    // Correct EUR amount to CNY 1980.00, then undo back to EUR 249.00!
    let (code, r_corr) = run(&[
        "correct",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-corr-eur",
        "--fact-id",
        &manual_amt_fact_id,
        "--expected-revision",
        "1",
        "--currency",
        "CNY",
        "--amount-text",
        "1980.00",
        "--actor-id",
        "act_11111111111111111111111111111111",
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_corr}");
    assert_eq!(r_corr["currency"], "CNY");
    let corr_dec_id = r_corr["decision_id"].as_str().unwrap().to_string();

    let (code, r_undo) = run(&[
        "undo",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-undo-eur",
        "--decision-id",
        &corr_dec_id,
        "--expected-revision",
        "2",
        "--actor-id",
        "act_11111111111111111111111111111111",
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_undo}");

    // 5. Verify ZERO plaintext leaked onto disk across the ENTIRE vault directory!
    let leaks = scan_dir_for_needles(
        &vault,
        &[
            "work.secret.user@company.example",
            "JetBrains SuperSecretVendor",
            "UltraPrivateNote9981",
            "alipay.secret@example.com",
            "FAKE_SCREENSHOT_RECEIPT_BYTES_PRIVATE_NOTE_XYZ",
            "WPS超级会员年卡",
            "ChatGPT Plus",
        ],
    );
    assert!(leaks.is_empty(), "plaintext leaked on disk: {leaks:?}");

    // 6. Export with plaintext ack + independently verify-package (proves human_record + multi-currency chains!)
    let export_dir = tmp.path.join("exit-pack");
    let (code, r_exp) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        export_dir.to_str().unwrap(),
        "--acknowledge-plaintext",
        "yes",
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_exp}");

    let (code, r_vpkg) = run(&["verify-package", "--package", export_dir.to_str().unwrap()]);
    assert_eq!(code, 0, "{r_vpkg}");

    let records: Value =
        serde_json::from_str(&fs::read_to_string(export_dir.join("records.json")).unwrap())
            .unwrap();
    assert!(
        records["annotations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["vendor"] == "JetBrains SuperSecretVendor"),
        "{records}"
    );
    let facts_csv = fs::read_to_string(export_dir.join("facts.csv")).unwrap();
    assert!(facts_csv.contains(",CNY,98.00,"), "{facts_csv}");
    assert!(facts_csv.contains(",USD,20.00,"), "{facts_csv}");
    assert!(facts_csv.contains(",EUR,249.00,"), "{facts_csv}");
}

#[test]
fn format6_bilingual_merge_long_pdf_line_deep_pages_year_crossing_and_cli_flags() {
    // 1. CLI --version / --help / no-args human UX vs legacy `version` / `help` JSON contracts
    let ver_out = Command::new(bin()).arg("--version").output().unwrap();
    assert_eq!(ver_out.status.code(), Some(0));
    let ver_str = String::from_utf8_lossy(&ver_out.stdout);
    assert!(ver_str.starts_with("subvault 0.1.0"), "{ver_str}");

    let help_out = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(help_out.status.code(), Some(0));
    let help_str = String::from_utf8_lossy(&help_out.stdout);
    assert!(
        help_str.contains("Usage:") && help_str.contains("subvault app"),
        "{help_str}"
    );

    let (code, bogus) = run(&["bogus-cmd", "--help"]);
    assert_eq!(code, 2, "{bogus}");
    assert_eq!(bogus["reason"], "unknown_command");

    let (code, legacy_ver) = run(&["version"]);
    assert_eq!(code, 0, "{legacy_ver}");
    assert_eq!(legacy_ver["user_ready"], false);

    let tmp = Tmp::new("wave5-real");
    let vault = tmp.path.join("vault");
    let (code, init) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "os-disk",
    ]);
    assert_eq!(code, 0, "{init}");

    // 2. Bilingual HTML where strict matches ASCII period and real matches ¥ amount -> BOTH adopted
    let mixed_html = tmp.path.join("mixed-bilingual.html");
    fs::write(
        &mixed_html,
        "<!doctype html><html><body><p>Service period: 2026-01-01 through 2026-01-31</p><p>实付金额：¥98.00</p></body></html>",
    )
    .unwrap();
    let (code, r_html) = run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-mixed-html",
        "--source",
        mixed_html.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_html}");
    let html_facts = r_html["facts"].as_array().unwrap();
    assert_eq!(
        html_facts.len(),
        2,
        "must adopt both period and amount: {r_html}"
    );

    // 3. Bilingual PDF where strict matches ASCII period and real matches ¥ amount -> BOTH adopted
    let mixed_pdf_stream = format!(
        "BT\n(Service period: 2026-02-01 through 2026-02-28) Tj\nT* {} Tj\nET",
        utf16be_hex("实付金额：¥128.00"),
    );
    let mixed_pdf = tmp.path.join("mixed-bilingual.pdf");
    fs::write(&mixed_pdf, minimal_pdf(mixed_pdf_stream.as_bytes())).unwrap();
    let (code, r_pdf) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-mixed-pdf",
        "--source",
        mixed_pdf.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_pdf}");
    let pdf_facts = r_pdf["facts"].as_array().unwrap();
    assert_eq!(
        pdf_facts.len(),
        2,
        "must adopt both period and amount: {r_pdf}"
    );

    // 4. Format 6 PDF with >1024-byte Chinese remark line still extracts amount cleanly
    let long_remark = format!("备注：{}", "项目明细说明文字".repeat(60));
    assert!(long_remark.len() > 1024);
    let long_pdf_stream = format!(
        "BT\n{} Tj\nT* {} Tj\nET",
        utf16be_hex(&long_remark),
        utf16be_hex("价税合计（小写）¥256.00"),
    );
    let long_pdf = tmp.path.join("long-remark.pdf");
    fs::write(&long_pdf, minimal_pdf(long_pdf_stream.as_bytes())).unwrap();
    let (code, r_long) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-long-pdf",
        "--source",
        long_pdf.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_long}");
    assert_eq!(r_long["extraction"], "matched", "{r_long}");
    assert_eq!(r_long["facts"].as_array().unwrap().len(), 1, "{r_long}");

    // 5. Deeply nested /Pages tree PDF (>150 levels) must not stack-overflow
    let mut deep_pdf =
        Vec::from(&b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"[..]);
    let depth = 160u32;
    for i in 2..=depth {
        deep_pdf.extend_from_slice(
            format!(
                "{i} 0 obj\n<< /Type /Pages /Kids [{} 0 R] /Count 0 >>\nendobj\n",
                i + 1
            )
            .as_bytes(),
        );
    }
    deep_pdf.extend_from_slice(
        format!("{} 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n", depth + 1)
            .as_bytes(),
    );
    let deep_pdf_path = tmp.path.join("deep-pages.pdf");
    fs::write(&deep_pdf_path, &deep_pdf).unwrap();
    let (code, r_deep) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-deep-pdf",
        "--source",
        deep_pdf_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_deep}");

    // 6. Year-crossing period `Dec 15 - Jan 15, 2026` resolves to 2025-12-15..2026-01-15
    let ycross_html = tmp.path.join("year-cross.html");
    fs::write(
        &ycross_html,
        "<html><body><p>Billing period: Dec 15 - Jan 15, 2026</p><p>Amount paid: $15.00</p></body></html>",
    )
    .unwrap();
    let (code, r_yc) = run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-year-cross",
        "--source",
        ycross_html.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_yc}");
    let yc_sid = r_yc["source_id"].as_str().unwrap();
    let (code, yc_facts) = run(&[
        "facts",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        yc_sid,
    ]);
    assert_eq!(code, 0, "{yc_facts}");
    let per_fact = yc_facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["fact_type"] == "stated_service_period")
        .expect("year-crossing period fact");
    assert_eq!(per_fact["value"]["period_start"], "2025-12-15");
    assert_eq!(per_fact["value"]["period_end"], "2026-01-15");

    // 7. 1-row CSV with unparseable `10/01/2026` date must not poison source_annotations.bill_date
    let us_csv_path = tmp.path.join("us-date.csv");
    fs::write(
        &us_csv_path,
        "Date,Merchant,Amount (USD),Direction\n10/01/2026,Acme,20.00,expense\n",
    )
    .unwrap();
    let (code, r_us) = run(&[
        "csv",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd-us-csv",
        "--source",
        us_csv_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{r_us}");
    let us_sid = r_us["source_id"].as_str().unwrap();
    let (code, r_us_ann) = run(&[
        "annotate",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        us_sid,
        "--category",
        "云服务",
    ]);
    assert_eq!(code, 0, "{r_us_ann}");
    assert!(r_us_ann["annotation"]["bill_date"].is_null(), "{r_us_ann}");
}
