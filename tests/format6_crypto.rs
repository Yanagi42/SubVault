use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
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
            "subvault-f6-{label}-{}-{}",
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

fn run_with(args: &[&str], envs: &[(&str, &str)], stdin_data: Option<&str>) -> (i32, Value) {
    let mut cmd = Command::new(bin());
    cmd.args(args)
        .env_remove("SUBVAULT_PASSPHRASE")
        .env_remove("SUBVAULT_PASSPHRASE_FILE")
        .env_remove("SUBVAULT_RECOVERY_KEY")
        .env_remove("SUBVAULT_RECOVERY_KEY_FILE")
        .env_remove("SUBVAULT_NEW_PASSPHRASE")
        .env_remove("SUBVAULT_NEW_PASSPHRASE_FILE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|err| panic!("failed to spawn {}: {err}", bin().display()));
    if let Some(input) = stdin_data {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
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
    run_with(args, &[], None)
}

fn extract_recovery_key(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    text.lines()
        .find(|line| line.starts_with("SVRK-"))
        .expect("SVRK recovery key line")
        .trim()
        .to_string()
}

#[test]
fn format6_dual_protection_init_isolation_and_cli_secret_guard() {
    let tmp = Tmp::new("init");

    // 1. Direct --passphrase CLI arg (both space and `=` forms) is explicitly forbidden and never echoed
    let (code, bad_arg) = run(&[
        "init",
        "--vault",
        tmp.path.join("bad").to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "passphrase",
        "--passphrase",
        "leak-in-ps",
    ]);
    assert_eq!(code, 2, "{bad_arg}");
    assert_eq!(bad_arg["reason"], "passphrase_cli_arg_forbidden");

    let (code, bad_eq) = run(&[
        "init",
        "--vault",
        tmp.path.join("badeq").to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "passphrase",
        "--passphrase=SecretNeverEchoed999",
    ]);
    assert_eq!(code, 2, "{bad_eq}");
    assert_eq!(bad_eq["reason"], "passphrase_cli_arg_forbidden");
    assert_eq!(bad_eq["flag"], "--passphrase");
    assert!(
        !bad_eq.to_string().contains("SecretNeverEchoed999"),
        "forbidden CLI secret must never be echoed: {bad_eq}"
    );

    // 2. Format 6 requires explicit --protection
    let (code, missing_prot) = run(&[
        "init",
        "--vault",
        tmp.path.join("noprot").to_str().unwrap(),
        "--format",
        "6",
    ]);
    assert_eq!(code, 2, "{missing_prot}");
    assert_eq!(missing_prot["reason"], "protection_required_for_format_6");

    // 3. Format 5 refuses --protection
    let (code, old_prot) = run(&[
        "init",
        "--vault",
        tmp.path.join("oldprot").to_str().unwrap(),
        "--format",
        "5",
        "--protection",
        "os-disk",
    ]);
    assert_eq!(code, 2, "{old_prot}");
    assert_eq!(old_prot["reason"], "protection_requires_format_6");

    // 4. os-disk Format 6 vault creates SUBVAULT_VAULT (no SYNTHETIC_ONLY), opens without key
    let os_vault = tmp.path.join("os-vault");
    let (code, init_os) = run(&[
        "init",
        "--vault",
        os_vault.to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "os-disk",
    ]);
    assert_eq!(code, 0, "{init_os}");
    assert_eq!(init_os["vault_format"], 6);
    assert_eq!(init_os["synthetic_only"], false);
    assert_eq!(init_os["protection"], "os_disk_assumed");
    assert_eq!(init_os["security_complete"], false);
    assert_eq!(init_os["user_ready"], true);
    assert_eq!(init_os["capabilities"]["real_data_import"], "available");
    assert!(os_vault.join("SUBVAULT_VAULT").is_file());
    assert!(!os_vault.join("SYNTHETIC_ONLY").exists());

    let (code, open_os) = run(&["open", "--vault", os_vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{open_os}");
    assert_eq!(open_os["outcome"], "Ready");
    assert_eq!(open_os["vault_format"], 6);
    assert_eq!(open_os["synthetic_only"], false);
    assert_eq!(open_os["disposable"], false);
    assert_eq!(open_os["protection"], "os_disk_assumed");

    // Dropping SYNTHETIC_ONLY alongside SUBVAULT_VAULT is refused as mixed_vault_banners
    fs::write(os_vault.join("SYNTHETIC_ONLY"), b"extra").unwrap();
    let (code, mixed) = run(&["open", "--vault", os_vault.to_str().unwrap()]);
    assert_eq!(code, 13, "{mixed}");
    assert_eq!(mixed["reason"], "mixed_vault_banners");
    fs::remove_file(os_vault.join("SYNTHETIC_ONLY")).unwrap();

    // Tampering SUBVAULT_VAULT banner is refused cleanly
    let orig_banner = fs::read(os_vault.join("SUBVAULT_VAULT")).unwrap();
    fs::write(os_vault.join("SUBVAULT_VAULT"), b"tampered\n").unwrap();
    let (code, tampered) = run(&["open", "--vault", os_vault.to_str().unwrap()]);
    assert_eq!(code, 13, "{tampered}");
    assert_eq!(tampered["reason"], "vault_banner_mismatch");
    fs::write(os_vault.join("SUBVAULT_VAULT"), orig_banner).unwrap();

    let (code, open_again) = run(&["open", "--vault", os_vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{open_again}");
}

#[test]
fn format6_passphrase_auth_recovery_key_and_rotate_passphrase() {
    let tmp = Tmp::new("auth");
    let vault = tmp.path.join("enc-vault");
    let pass1_file = tmp.path.join("pass1.txt");
    let pass2_file = tmp.path.join("pass2.txt");
    let pass3_file = tmp.path.join("pass3.txt");
    let rec1_out = tmp.path.join("recovery1.key");
    let rec2_out = tmp.path.join("recovery2.key");

    fs::write(&pass1_file, "initial-strong-passphrase\n").unwrap();
    fs::write(&pass2_file, "rotated-strong-passphrase\n").unwrap();
    fs::write(&pass3_file, "recovered-strong-passphrase\n").unwrap();

    // 1. Missing passphrase on init -> EXIT_AUTH (22), no directory created
    let (code, no_pass) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "passphrase",
    ]);
    assert_eq!(code, 22, "{no_pass}");
    assert_eq!(no_pass["outcome"], "KeyRequired");
    assert_eq!(no_pass["reason"], "passphrase_required");
    assert!(!vault.exists());

    // 2. Init with passphrase file + --recovery-key-out (omits raw recovery key from stdout!)
    let (code, init_enc) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "passphrase",
        "--passphrase-file",
        pass1_file.to_str().unwrap(),
        "--recovery-key-out",
        rec1_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{init_enc}");
    assert_eq!(init_enc["vault_format"], 6);
    assert_eq!(init_enc["synthetic_only"], false);
    assert_eq!(init_enc["protection"], "passphrase_aead_v1");
    assert_eq!(init_enc["security_complete"], true);
    assert!(
        init_enc.get("recovery_key").is_none(),
        "must not echo recovery_key to stdout when --recovery-key-out is used"
    );
    assert!(rec1_out.is_file());
    let rec1 = extract_recovery_key(&rec1_out);
    assert!(rec1.starts_with("SVRK-"));

    // 3. Open without key -> EXIT_AUTH (22), KeyRequired
    let (code, locked) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 22, "{locked}");
    assert_eq!(locked["outcome"], "KeyRequired");
    assert_eq!(locked["reason"], "passphrase_required");
    assert_eq!(locked["locked_at_rest"], true);

    // 4. Open with wrong passphrase via stdin -> EXIT_AUTH (22), AuthFailed
    let (code, wrong) = run_with(
        &[
            "open",
            "--vault",
            vault.to_str().unwrap(),
            "--passphrase-stdin",
        ],
        &[],
        Some("definitely-wrong-pass\n"),
    );
    assert_eq!(code, 22, "{wrong}");
    assert_eq!(wrong["outcome"], "AuthFailed");
    assert_eq!(wrong["reason"], "invalid_passphrase");

    // 5. Open with right passphrase via --passphrase-file -> Ready
    let (code, ok_pass) = run(&[
        "open",
        "--vault",
        vault.to_str().unwrap(),
        "--passphrase-file",
        pass1_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{ok_pass}");
    assert_eq!(ok_pass["outcome"], "Ready");
    assert_eq!(ok_pass["protection"], "passphrase_aead_v1");
    assert_eq!(ok_pass["synthetic_only"], false);

    // 6. Open with recovery key via env -> Ready
    let (code, ok_rec) = run_with(
        &["verify", "--vault", vault.to_str().unwrap()],
        &[("SUBVAULT_RECOVERY_KEY", &rec1)],
        None,
    );
    assert_eq!(code, 0, "{ok_rec}");
    assert_eq!(ok_rec["outcome"], "Ready");

    // 7. Rotate passphrase using current passphrase -> old fails, new works
    let (code, rot1) = run(&[
        "rotate-passphrase",
        "--vault",
        vault.to_str().unwrap(),
        "--passphrase-file",
        pass1_file.to_str().unwrap(),
        "--new-passphrase-file",
        pass2_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{rot1}");
    assert_eq!(rot1["outcome"], "PassphraseRotated");
    assert_eq!(rot1["recovery_key_rotated"], false);

    let (code, old_fails) = run(&[
        "open",
        "--vault",
        vault.to_str().unwrap(),
        "--passphrase-file",
        pass1_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 22, "{old_fails}");
    assert_eq!(old_fails["reason"], "invalid_passphrase");

    let (code, new_works) = run(&[
        "open",
        "--vault",
        vault.to_str().unwrap(),
        "--passphrase-file",
        pass2_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{new_works}");

    // 8. Rotate passphrase AND recovery key using original recovery key -> old rec fails, new pass & rec work
    let (code, rot2) = run_with(
        &[
            "rotate-passphrase",
            "--vault",
            vault.to_str().unwrap(),
            "--new-passphrase-file",
            pass3_file.to_str().unwrap(),
            "--rotate-recovery-key",
            "--recovery-key-out",
            rec2_out.to_str().unwrap(),
        ],
        &[("SUBVAULT_RECOVERY_KEY", &rec1)],
        None,
    );
    assert_eq!(code, 0, "{rot2}");
    assert_eq!(rot2["recovery_key_rotated"], true);
    let rec2 = extract_recovery_key(&rec2_out);
    assert_ne!(rec1, rec2);

    let (code, old_rec_fails) = run_with(
        &["open", "--vault", vault.to_str().unwrap()],
        &[("SUBVAULT_RECOVERY_KEY", &rec1)],
        None,
    );
    assert_eq!(code, 22, "{old_rec_fails}");
    assert_eq!(old_rec_fails["reason"], "invalid_recovery_key");

    let (code, new_rec_ok) = run_with(
        &["open", "--vault", vault.to_str().unwrap()],
        &[("SUBVAULT_RECOVERY_KEY", &rec2)],
        None,
    );
    assert_eq!(code, 0, "{new_rec_ok}");

    let (code, pass3_ok) = run(&[
        "open",
        "--vault",
        vault.to_str().unwrap(),
        "--passphrase-file",
        pass3_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{pass3_ok}");
}

fn minimal_pdf(lines: &[&str], author_secret: Option<&str>) -> Vec<u8> {
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
    let info_obj = if let Some(author) = author_secret {
        format!("6 0 obj\n<< /Author ({author}) /Title (Private Receipt) >>\nendobj\n")
    } else {
        String::new()
    };
    let info_ref = if author_secret.is_some() {
        " /Info 6 0 R"
    } else {
        ""
    };
    let doc = format!(
        "%PDF-1.4\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
{info_obj}\
trailer\n<< /Root 1 0 R{info_ref} >>\n\
%%EOF\n",
        stream.len()
    );
    doc.into_bytes()
}

fn collect_all_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_all_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn format6_passphrase_e2e_zero_plaintext_disk_export_review_and_tamper_proofs() {
    let tmp = Tmp::new("e2e-enc");
    let vault = tmp.path.join("enc-vault");
    let pass1_file = tmp.path.join("pass1.txt");
    let pass2_file = tmp.path.join("pass2.txt");
    let rec_out = tmp.path.join("rec.key");
    fs::write(&pass1_file, "vault-secret-pass-one!\n").unwrap();
    fs::write(&pass2_file, "vault-secret-pass-two!\n").unwrap();

    // Init Format 6 passphrase vault
    let (code, init) = run(&[
        "init",
        "--vault",
        vault.to_str().unwrap(),
        "--format",
        "6",
        "--protection",
        "passphrase",
        "--passphrase-file",
        pass1_file.to_str().unwrap(),
        "--recovery-key-out",
        rec_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{init}");

    // Prepare 4 distinct realistic inputs with unmistakable secret tokens
    let raw_src = tmp.path.join("super_secret_raw_filename.bin");
    let raw_bytes = b"RAW_SECRET_PAYLOAD_ALPHA_9876543210_alice.secret@example.net\n";
    fs::write(&raw_src, raw_bytes).unwrap();

    let pdf_src = tmp.path.join("cloud_bill_secret.pdf");
    let pdf_bytes = minimal_pdf(
        &[
            "Bill to: Alice Confidential",
            "Email: alice.pdf@secretmail.org",
            "Amount paid USD 49.99",
            "Service period: 2026-09-01 through 2026-09-30",
        ],
        Some("Alice PDF Author Secret"),
    );
    fs::write(&pdf_src, &pdf_bytes).unwrap();

    let html_src = tmp.path.join("saas_invoice_secret.html");
    let html_bytes = concat!(
        "<!doctype html><html><body><table>",
        "<tr><th>Customer</th><td>Bob Confidential</td></tr>",
        "<tr><th>Amount paid</th><td>USD <b>19&#46;95</b></td></tr>",
        "<tr><th>Service period</th><td>2026-10-01 through 2026-10-31</td></tr>",
        "</table></body></html>"
    )
    .as_bytes();
    fs::write(&html_src, html_bytes).unwrap();

    let frag_src = tmp.path.join("fragment_secret.txt");
    let frag_bytes = concat!(
        "Customer: Charlie Confidential\n",
        "Amount paid USD 29.50\n",
        "Service period: 2026-11-01 through 2026-11-30\n"
    )
    .as_bytes();
    fs::write(&frag_src, frag_bytes).unwrap();

    let pf = pass1_file.to_str().unwrap();
    let v = vault.to_str().unwrap();

    // 1. import + marker
    let (code, imp) = run(&[
        "import",
        "--vault",
        v,
        "--command-id",
        "cmd_imp_01",
        "--media-type",
        "application/octet-stream",
        "--source",
        raw_src.to_str().unwrap(),
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{imp}");
    assert_eq!(imp["synthetic_only"], false);
    assert_eq!(imp["protection"], "passphrase_aead_v1");
    let raw_src_id = imp["source_id"].as_str().unwrap().to_string();
    let raw_obj_id = imp["object_id"].as_str().unwrap().to_string();

    let (code, mrk) = run(&[
        "marker",
        "--vault",
        v,
        "--command-id",
        "cmd_mrk_01",
        "--source-id",
        &raw_src_id,
        "--value",
        "SECRET_MARKER_TAG_XYZ",
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{mrk}");
    assert_eq!(mrk["user_marker"], "SECRET_MARKER_TAG_XYZ");

    // 2. pdf
    let (code, pdf_res) = run(&[
        "pdf",
        "--vault",
        v,
        "--command-id",
        "cmd_pdf_01",
        "--source",
        pdf_src.to_str().unwrap(),
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{pdf_res}");
    assert_eq!(pdf_res["slice"], "M4-real");
    assert_eq!(pdf_res["synthetic_only"], false);
    assert_eq!(pdf_res["extraction"], "matched");
    let pdf_obj_id = pdf_res["object_id"].as_str().unwrap().to_string();

    // 3. html
    let (code, html_res) = run(&[
        "html",
        "--vault",
        v,
        "--command-id",
        "cmd_html_01",
        "--source",
        html_src.to_str().unwrap(),
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{html_res}");
    assert_eq!(html_res["slice"], "M4-real");
    assert_eq!(html_res["synthetic_only"], false);
    assert_eq!(html_res["extraction"], "matched");

    // 4. fragment
    let (code, frag_res) = run(&[
        "fragment",
        "--vault",
        v,
        "--command-id",
        "cmd_frag_01",
        "--fragment",
        frag_src.to_str().unwrap(),
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{frag_res}");
    assert_eq!(frag_res["slice"], "M4-real");
    assert_eq!(frag_res["synthetic_only"], false);
    assert_eq!(frag_res["facts_published"], true);

    // 5. facts + correct + undo + candidates
    let (code, facts_before) = run(&["facts", "--vault", v, "--passphrase-file", pf]);
    assert_eq!(code, 0, "{facts_before}");
    assert_eq!(facts_before["slice"], "M4-real");
    assert_eq!(facts_before["synthetic_only"], false);
    let facts_arr = facts_before["facts"].as_array().unwrap();
    assert_eq!(facts_arr.len(), 6, "3 sources x (amount + period)");

    let amount_fact = facts_arr
        .iter()
        .find(|f| f["fact_type"] == "stated_amount_paid" && f["value"]["amount_text"] == "49.99")
        .expect("pdf amount fact");
    let fact_id = amount_fact["fact_id"].as_str().unwrap().to_string();

    let (code, corrected) = run(&[
        "correct",
        "--vault",
        v,
        "--command-id",
        "cmd_cor_01",
        "--fact-id",
        &fact_id,
        "--expected-revision",
        "1",
        "--amount-text",
        "88.88",
        "--actor-id",
        "act_11111111111111111111111111111111",
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{corrected}");
    assert_eq!(corrected["amount_text"], "88.88");
    let dec_id = corrected["decision_id"].as_str().unwrap().to_string();

    let (code, undone) = run(&[
        "undo",
        "--vault",
        v,
        "--command-id",
        "cmd_undo_01",
        "--decision-id",
        &dec_id,
        "--expected-revision",
        "2",
        "--actor-id",
        "act_11111111111111111111111111111111",
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{undone}");
    assert_eq!(undone["amount_text"], "49.99");

    let (code, cands) = run(&["candidates", "--vault", v, "--passphrase-file", pf]);
    assert_eq!(code, 0, "{cands}");
    assert_eq!(cands["slice"], "M4-real");
    assert_eq!(cands["synthetic_only"], false);
    assert!(cands["candidates"].as_array().unwrap().len() >= 6);

    // 6. read decrypts exact original bytes
    let read_out = tmp.path.join("read-back.bin");
    let (code, read_res) = run(&[
        "read",
        "--vault",
        v,
        "--source-id",
        &raw_src_id,
        "--out",
        read_out.to_str().unwrap(),
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{read_res}");
    assert_eq!(fs::read(&read_out).unwrap(), raw_bytes);

    // 7. open checkpoint + ZERO-PLAINTEXT DISK SCAN across the entire vault directory!
    let (code, open_res) = run(&["open", "--vault", v, "--passphrase-file", pf]);
    assert_eq!(code, 0, "{open_res}");
    assert_eq!(open_res["outcome"], "Ready");

    let mut vault_files = Vec::new();
    collect_all_files(&vault, &mut vault_files);
    assert!(!vault_files.is_empty());
    let forbidden_needles: &[&[u8]] = &[
        b"RAW_SECRET_PAYLOAD_ALPHA",
        b"super_secret_raw_filename",
        b"SECRET_MARKER_TAG_XYZ",
        b"Alice Confidential",
        b"alice.pdf@secretmail.org",
        b"Bob Confidential",
        b"Charlie Confidential",
        b"49.99",
        b"19.95",
        b"29.50",
        b"88.88",
        b"Amount paid",
        b"Service period",
        b"%PDF-1.4",
        b"<!doctype html>",
    ];
    for file in &vault_files {
        let data = fs::read(file).unwrap();
        for needle in forbidden_needles {
            assert!(
                !contains_subslice(&data, needle),
                "PLAINTEXT LEAK: found {:?} in vault file {}",
                String::from_utf8_lossy(needle),
                file.display()
            );
        }
    }

    // 8. Export refuses without --acknowledge-plaintext yes
    let pkg_dir = tmp.path.join("exit-pack");
    let (code, no_ack) = run(&[
        "export",
        "--vault",
        v,
        "--out",
        pkg_dir.to_str().unwrap(),
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 16, "{no_ack}");
    assert_eq!(no_ack["reason"], "plaintext_export_ack_required");
    assert_eq!(no_ack["output_written"], false);
    assert_eq!(no_ack["output_cleanup"], "not_created");
    assert!(!pkg_dir.exists());

    // 9. Export succeeds with --acknowledge-plaintext yes, passes verify-package & review --package
    let (code, exp) = run(&[
        "export",
        "--vault",
        v,
        "--out",
        pkg_dir.to_str().unwrap(),
        "--acknowledge-plaintext",
        "yes",
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{exp}");
    assert_eq!(exp["outcome"], "Exported");
    assert_eq!(exp["vault_format"], 6);
    assert_eq!(exp["synthetic_only"], false);
    assert_eq!(exp["protection"], "plaintext_exit_from_passphrase_aead_v1");
    assert_eq!(exp["plaintext_acknowledged"], true);

    let (code, vpkg) = run(&["verify-package", "--package", pkg_dir.to_str().unwrap()]);
    assert_eq!(code, 0, "{vpkg}");
    assert_eq!(vpkg["outcome"], "Verified");
    assert_eq!(vpkg["vault_format"], 6);
    assert_eq!(vpkg["verified_originals"], 4);
    assert_eq!(vpkg["protection"], "plaintext_exit_from_passphrase_aead_v1");
    assert_eq!(vpkg["synthetic_only"], false);

    let review_out = tmp.path.join("review.txt");
    let (code, rev) = run(&[
        "review",
        "--vault",
        v,
        "--out",
        review_out.to_str().unwrap(),
        "--package",
        pkg_dir.to_str().unwrap(),
        "--passphrase-file",
        pf,
    ]);
    assert_eq!(code, 0, "{rev}");
    assert_eq!(rev["outcome"], "Reviewed");
    assert_eq!(rev["package_associated"], true);
    assert_eq!(rev["originals_verified"], 4);
    assert_eq!(rev["synthetic_only"], false);
    let rev_text = fs::read_to_string(&review_out).unwrap();
    assert!(rev_text.contains("真实资料库只读审阅单（格式 6）"));
    assert!(rev_text.contains("49.99"));
    assert!(rev_text.contains("19.95"));
    assert!(rev_text.contains("29.50"));

    // 10. Rotate passphrase keeps all existing encrypted objects/facts readable
    let (code, rot) = run(&[
        "rotate-passphrase",
        "--vault",
        v,
        "--passphrase-file",
        pf,
        "--new-passphrase-file",
        pass2_file.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{rot}");
    let pf2 = pass2_file.to_str().unwrap();
    let (code, facts_after_rot) = run(&["facts", "--vault", v, "--passphrase-file", pf2]);
    assert_eq!(code, 0, "{facts_after_rot}");
    assert_eq!(facts_after_rot["facts"].as_array().unwrap().len(), 6);

    // 11. Tamper Proof A: swap two encrypted objects -> fails object_id AAD authentication!
    let obj_a_path = vault.join("objects").join(&raw_obj_id);
    let obj_b_path = vault.join("objects").join(&pdf_obj_id);
    let obj_a_orig = fs::read(&obj_a_path).unwrap();
    let obj_b_orig = fs::read(&obj_b_path).unwrap();
    fs::write(&obj_a_path, &obj_b_orig).unwrap();
    fs::write(&obj_b_path, &obj_a_orig).unwrap();

    let (code, swap_read) = run(&[
        "read",
        "--vault",
        v,
        "--source-id",
        &raw_src_id,
        "--out",
        tmp.path.join("swap-read.bin").to_str().unwrap(),
        "--passphrase-file",
        pf2,
    ]);
    assert_eq!(code, 12, "{swap_read}");
    assert_eq!(swap_read["reason"], "ciphertext_auth_failed");

    let (code, swap_open) = run(&["open", "--vault", v, "--passphrase-file", pf2]);
    assert_eq!(code, 12, "{swap_open}");
    assert_eq!(swap_open["outcome"], "RecoveryRequired");

    // Restore objects, then Tamper Proof B: flip a single bit in encrypted object -> auth fails!
    fs::write(&obj_a_path, &obj_a_orig).unwrap();
    fs::write(&obj_b_path, &obj_b_orig).unwrap();
    let mut flipped = obj_a_orig.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0x01;
    fs::write(&obj_a_path, &flipped).unwrap();

    let (code, flip_read) = run(&[
        "read",
        "--vault",
        v,
        "--source-id",
        &raw_src_id,
        "--out",
        tmp.path.join("flip-read.bin").to_str().unwrap(),
        "--passphrase-file",
        pf2,
    ]);
    assert_eq!(code, 12, "{flip_read}");
    assert_eq!(flip_read["reason"], "ciphertext_auth_failed");
    fs::write(&obj_a_path, &obj_a_orig).unwrap();

    // Tamper Proof C: cross-row enc:v1 cell swap in SQLite `facts` -> fails row_id AAD!
    let conn = rusqlite::Connection::open(vault.join("vault.db")).unwrap();
    let mut stmt = conn
        .prepare("SELECT fact_id, value_json FROM facts ORDER BY fact_id LIMIT 2")
        .unwrap();
    let pair: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(pair.len(), 2);
    assert!(pair[0].1.starts_with("enc:v1:"));
    assert!(pair[1].1.starts_with("enc:v1:"));
    conn.execute(
        "UPDATE facts SET value_json = ?2 WHERE fact_id = ?1",
        rusqlite::params![pair[0].0, pair[1].1],
    )
    .unwrap();
    drop(stmt);
    drop(conn);

    let (code, cell_swap) = run(&["facts", "--vault", v, "--passphrase-file", pf2]);
    assert_eq!(code, 12, "{cell_swap}");
    assert_eq!(cell_swap["reason"], "ciphertext_auth_failed");
}

#[test]
fn format6_os_disk_e2e_without_passphrase_or_plaintext_ack() {
    let tmp = Tmp::new("e2e-osdisk");
    let vault = tmp.path.join("os-vault");
    let v = vault.to_str().unwrap();

    let (code, init) = run(&[
        "init",
        "--vault",
        v,
        "--format",
        "6",
        "--protection",
        "os-disk",
    ]);
    assert_eq!(code, 0, "{init}");

    let pdf_src = tmp.path.join("sample.pdf");
    fs::write(
        &pdf_src,
        minimal_pdf(
            &[
                "Amount paid USD 12.00",
                "Service period: 2026-08-01 through 2026-08-31",
            ],
            None,
        ),
    )
    .unwrap();

    let (code, pdf_res) = run(&[
        "pdf",
        "--vault",
        v,
        "--command-id",
        "cmd_os_pdf",
        "--source",
        pdf_src.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{pdf_res}");
    assert_eq!(pdf_res["protection"], "os_disk_assumed");
    assert_eq!(pdf_res["synthetic_only"], false);

    // os-disk export succeeds without --acknowledge-plaintext yes
    let pkg_dir = tmp.path.join("os-exit");
    let (code, exp) = run(&["export", "--vault", v, "--out", pkg_dir.to_str().unwrap()]);
    assert_eq!(code, 0, "{exp}");
    assert_eq!(exp["protection"], "plaintext_exit_from_os_disk_assumed");
    assert_eq!(exp["synthetic_only"], false);

    let (code, vpkg) = run(&["verify-package", "--package", pkg_dir.to_str().unwrap()]);
    assert_eq!(code, 0, "{vpkg}");
    assert_eq!(vpkg["outcome"], "Verified");
    assert_eq!(vpkg["protection"], "plaintext_exit_from_os_disk_assumed");

    let rev_out = tmp.path.join("os-review.txt");
    let (code, rev) = run(&[
        "review",
        "--vault",
        v,
        "--out",
        rev_out.to_str().unwrap(),
        "--package",
        pkg_dir.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{rev}");
    assert_eq!(rev["package_associated"], true);
    assert_eq!(rev["originals_verified"], 1);
}
