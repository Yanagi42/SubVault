use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

fn bin() -> PathBuf {
    if let Ok(path) = std::env::var("SUBVAULT_TEST_BIN") {
        return PathBuf::from(path);
    }
    let compiled = PathBuf::from(env!("CARGO_BIN_EXE_subvault"));
    if compiled.exists() {
        return compiled;
    }
    compiled
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-redact-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
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

fn run(args: &[&str]) -> (i32, Value, String) {
    let output = Command::new(bin())
        .args(args)
        .env_remove("SUBVAULT_PASSPHRASE")
        .env_remove("SUBVAULT_PASSPHRASE_FILE")
        .env_remove("SUBVAULT_RECOVERY_KEY")
        .env_remove("SUBVAULT_RECOVERY_KEY_FILE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(1), body, stdout)
}

fn make_pdf(lines: &[&str], author: &str) -> Vec<u8> {
    let mut stream = String::from("BT\n/F1 12 Tf\n72 720 Td\n");
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            stream.push_str("0 -18 Td\n");
        }
        let escaped = line
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        stream.push_str(&format!("({escaped}) Tj\n"));
    }
    stream.push_str("ET\n");
    let doc = format!(
        "%PDF-1.4\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
6 0 obj\n<< /Author ({author}) /Title (Confidential Personal Invoice) >>\nendobj\n\
trailer\n<< /Root 1 0 R /Info 6 0 R >>\n\
%%EOF\n",
        stream.len()
    );
    doc.into_bytes()
}

#[test]
fn redact_external_pdf_html_and_text_strips_pii_preserves_layout_and_never_echoes_secrets() {
    let tmp = Tmp::new("external");

    // 1. External PDF -> pure UTF-8 template (zero binary PDF or /Author metadata leak)
    let pdf_in = tmp.path.join("bill.pdf");
    let pdf_out = tmp.path.join("bill.redacted.txt");
    fs::write(
        &pdf_in,
        make_pdf(
            &[
                "AcmeSecretCorp Cloud Receipt",
                "Bill to: Jane Secretperson",
                "Account: jane.secret@example.com",
                "Invoice: INV-99887766",
                "Card: **** **** **** 4242",
                "Portal: https://billing.acmesecret.example/u/12345678",
                "Amount paid USD 49.99",
                "Service period: 2026-09-01 through 2026-09-30",
            ],
            "Jane Secret Author Metadata",
        ),
    )
    .unwrap();

    let (code, pdf_rep, pdf_stdout) = run(&[
        "redact",
        "--source",
        pdf_in.to_str().unwrap(),
        "--out",
        pdf_out.to_str().unwrap(),
        "--redact-term",
        "AcmeSecretCorp",
    ]);
    assert_eq!(code, 0, "{pdf_rep}");
    assert_eq!(pdf_rep["outcome"], "Redacted");
    assert_eq!(pdf_rep["input_kind"], "pdf");
    assert_eq!(pdf_rep["input_mode"], "external_file");
    assert_eq!(pdf_rep["manual_review_required"], true);
    assert!(pdf_rep["counts"]["total_redactions"].as_u64().unwrap() >= 7);

    let redacted_pdf = fs::read_to_string(&pdf_out).unwrap();
    assert!(redacted_pdf.contains("=== SUBVAULT REDACTED TEMPLATE"));
    assert!(redacted_pdf.contains("--- PAGE 1 ---"));
    assert!(redacted_pdf.contains("Amount paid USD [AMOUNT_"));
    assert!(redacted_pdf.contains("Service period: [DATE_ISO_"));
    assert!(redacted_pdf.contains("Bill to: [REDACTED_FIELD_"));
    for secret in [
        "AcmeSecretCorp",
        "Jane Secretperson",
        "Jane Secret Author",
        "jane.secret@example.com",
        "INV-99887766",
        "4242",
        "billing.acmesecret.example",
        "49.99",
        "2026-09-01",
        "2026-09-30",
        "%PDF-1.4",
    ] {
        assert!(
            !redacted_pdf.contains(secret),
            "PDF output leaked {secret:?}:\n{redacted_pdf}"
        );
        assert!(
            !pdf_stdout.contains(secret),
            "PDF stdout JSON leaked {secret:?}:\n{pdf_stdout}"
        );
    }

    // 2. External HTML -> strips script/style/comment, keeps table layout & currency symbol
    let html_in = tmp.path.join("invoice.html");
    let html_out = tmp.path.join("invoice.redacted.html");
    fs::write(
        &html_in,
        concat!(
            "<!doctype html><html><head>",
            "<style>.secret-css { color: red; }</style>",
            "<script>window.token = 'JWT_SUPER_SECRET_123';</script>",
            "<!-- internal-tracking-secret-comment -->",
            "</head><body><table>",
            "<tr><th>Email</th><td>j***@private.org</td></tr>",
            "<tr><th>Phone</th><td>+1-415-555-0199</td></tr>",
            "<tr><th>Link</th><td><a href=\"https://portal.example.com/acct/998877\">View</a></td></tr>",
            "<tr><th>Amount paid</th><td>$125.50 on Sep 15, 2026</td></tr>",
            "</table></body></html>"
        ),
    )
    .unwrap();

    let (code, html_rep, html_stdout) = run(&[
        "redact",
        "--source",
        html_in.to_str().unwrap(),
        "--out",
        html_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{html_rep}");
    assert_eq!(html_rep["input_kind"], "html");
    assert_eq!(html_rep["counts"]["stripped_scripts"], 1);
    assert_eq!(html_rep["counts"]["stripped_styles"], 1);
    assert_eq!(html_rep["counts"]["stripped_comments"], 1);

    let redacted_html = fs::read_to_string(&html_out).unwrap();
    assert!(redacted_html.contains("<table>"));
    assert!(redacted_html.contains("<th>Amount paid</th>"));
    assert!(redacted_html.contains("$[AMOUNT_"));
    assert!(redacted_html.contains("[DATE_TEXT_"));
    for secret in [
        "JWT_SUPER_SECRET_123",
        "secret-css",
        "internal-tracking-secret-comment",
        "j***@private.org",
        "415-555-0199",
        "portal.example.com",
        "125.50",
        "Sep 15, 2026",
    ] {
        assert!(
            !redacted_html.contains(secret),
            "HTML output leaked {secret:?}:\n{redacted_html}"
        );
        assert!(
            !html_stdout.contains(secret),
            "HTML stdout JSON leaked {secret:?}:\n{html_stdout}"
        );
    }

    // 3. External Chinese CSV / text
    let txt_in = tmp.path.join("cn_bill.txt");
    let txt_out = tmp.path.join("cn_bill.redacted.txt");
    fs::write(
        &txt_in,
        "姓名：张三丰\n账单地址：北京市朝阳区幸福路88号\n邮箱：zhangsan@example.cn\n日期：2026年09月15日\n实付金额：¥199.00\n",
    )
    .unwrap();
    let (code, txt_rep, txt_stdout) = run(&[
        "redact",
        "--source",
        txt_in.to_str().unwrap(),
        "--out",
        txt_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{txt_rep}");
    assert_eq!(txt_rep["input_kind"], "text");
    let redacted_txt = fs::read_to_string(&txt_out).unwrap();
    assert!(redacted_txt.contains("姓名：[REDACTED_FIELD_"));
    assert!(redacted_txt.contains("账单地址：[REDACTED_FIELD_"));
    assert!(redacted_txt.contains("¥[AMOUNT_"));
    assert!(redacted_txt.contains("[DATE_CJK_"));
    for secret in [
        "张三丰",
        "北京市朝阳区幸福路88号",
        "zhangsan@example.cn",
        "2026年09月15日",
        "199.00",
    ] {
        assert!(!redacted_txt.contains(secret), "{redacted_txt}");
        assert!(!txt_stdout.contains(secret), "{txt_stdout}");
    }
}

#[test]
fn redact_vault_source_requires_auth_guards_paths_and_decrypts_in_memory() {
    let tmp = Tmp::new("vault");
    let vault = tmp.path.join("enc-vault");
    let pass_file = tmp.path.join("pass.txt");
    let rec_out = tmp.path.join("rec.key");
    fs::write(&pass_file, "redact-vault-pass!\n").unwrap();

    let (code, init, _) = run(&[
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

    let pdf_src = tmp.path.join("stored.pdf");
    fs::write(
        &pdf_src,
        make_pdf(
            &[
                "Customer: Secret Vault Owner",
                "Email: owner@secretvault.test",
                "Amount paid USD 33.44",
                "Service period: 2026-12-01 through 2026-12-31",
            ],
            "Owner Metadata",
        ),
    )
    .unwrap();

    let (code, pdf_res, _) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "cmd_red_pdf",
        "--source",
        pdf_src.to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{pdf_res}");
    let source_id = pdf_res["source_id"].as_str().unwrap().to_string();

    // Conflicting --source and --vault refused
    let (code, conflict, _) = run(&[
        "redact",
        "--source",
        pdf_src.to_str().unwrap(),
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &source_id,
        "--out",
        tmp.path.join("conflict.txt").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{conflict}");
    assert_eq!(conflict["reason"], "redact_conflicting_inputs");

    // Output inside vault refused
    let (code, inside, _) = run(&[
        "redact",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &source_id,
        "--out",
        vault.join("leak.txt").to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{inside}");
    assert_eq!(inside["reason"], "output_inside_vault");

    // Missing passphrase on encrypted vault -> EXIT_AUTH (22)
    let out_file = tmp.path.join("vault-redacted.txt");
    let (code, need_key, _) = run(&[
        "redact",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &source_id,
        "--out",
        out_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 22, "{need_key}");
    assert_eq!(need_key["outcome"], "KeyRequired");
    assert!(!out_file.exists());

    // Succeeds with passphrase
    let (code, ok, ok_stdout) = run(&[
        "redact",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &source_id,
        "--out",
        out_file.to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{ok}");
    assert_eq!(ok["input_mode"], "vault_source");
    assert_eq!(ok["input_kind"], "pdf");
    assert_eq!(ok["vault_format"], 6);
    assert_eq!(ok["synthetic_only"], false);
    assert_eq!(ok["protection"], "passphrase_aead_v1");

    let text = fs::read_to_string(&out_file).unwrap();
    assert!(text.contains("Amount paid USD [AMOUNT_"));
    for secret in [
        "Secret Vault Owner",
        "owner@secretvault.test",
        "33.44",
        "2026-12-01",
        "Owner Metadata",
    ] {
        assert!(!text.contains(secret), "{text}");
        assert!(!ok_stdout.contains(secret), "{ok_stdout}");
    }

    // Re-running to existing output refuses overwrite
    let (code, exists, _) = run(&[
        "redact",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &source_id,
        "--out",
        out_file.to_str().unwrap(),
        "--passphrase-file",
        pass_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{exists}");
    assert_eq!(exists["reason"], "output_exists");
}
