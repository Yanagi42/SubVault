use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};

use subvault::workbench::{WorkbenchOptions, WorkbenchServer};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(name: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "subvault-wbtest-{name}-{}-{id}",
            std::process::id()
        ));
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

fn http_call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    origin: Option<&str>,
    body: Option<&[u8]>,
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).unwrap();
    let payload = body.unwrap_or(&[]);
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n",
        addr.port()
    );
    if let Some(tok) = token {
        req.push_str(&format!("X-SubVault-Token: {tok}\r\n"));
    }
    if let Some(orig) = origin {
        req.push_str(&format!("Origin: {orig}\r\n"));
    }
    if !payload.is_empty() {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    req.push_str("Connection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    if !payload.is_empty() {
        stream.write_all(payload).unwrap();
    }
    stream.flush().unwrap();

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("valid HTTP response");
    let head_str = String::from_utf8_lossy(&raw[..sep]).to_string();
    let status: u16 = head_str
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, head_str, raw[sep + 4..].to_vec())
}

fn api_json(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let bytes = body.map(|v| serde_json::to_vec(&v).unwrap());
    let (status, _, resp) = http_call(addr, method, path, Some(token), None, bytes.as_deref());
    let parsed: Value = serde_json::from_slice(&resp).unwrap_or_else(|e| {
        panic!(
            "response not JSON ({e}), status={status}, body={}",
            String::from_utf8_lossy(&resp)
        )
    });
    (status, parsed)
}

#[test]
fn workbench_security_and_complete_end_to_end_user_workflow() {
    std::env::set_var("SUBVAULT_NO_RECENT", "1");
    let tmp = Tmp::new("e2e");
    let server = WorkbenchServer::start(WorkbenchOptions {
        port: 0,
        open_browser: false,
        initial_vault: None,
    })
    .unwrap();
    let addr = server.addr();
    let token = server.token().to_string();

    // 1. Static shell loads cleanly, while API rejects missing token (401) and foreign Origin (403)
    let (status, _, html) = http_call(addr, "GET", "/", None, None, None);
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&html).contains("SubVault"));

    let (status, _, _) = http_call(addr, "GET", "/api/bootstrap", None, None, None);
    assert_eq!(status, 401);

    let (status, _, _) = http_call(
        addr,
        "GET",
        "/api/bootstrap",
        Some(&token),
        Some("https://evil.example"),
        None,
    );
    assert_eq!(status, 403);

    // 2. Create encrypted Format 6 vault via Workbench API (returns recovery key, unlocks in RAM)
    let vault_dir = tmp.path.join("user-vault");
    let (status, init_res) = api_json(
        addr,
        "POST",
        "/api/vault/init",
        &token,
        Some(json!({
            "path": vault_dir.display().to_string(),
            "protection": "passphrase",
            "passphrase": "MyUserPass2026!"
        })),
    );
    assert_eq!(status, 200, "{init_res}");
    let recovery_key = init_res["recovery_key"].as_str().unwrap().to_string();
    assert!(recovery_key.starts_with("SVRK-"));

    // 3. Upload real styled HTML email ($20.00), Alipay GBK CSV, and a PNG receipt screenshot
    let openai_html = r#"<html><head><style>body{color:#111}</style></head><body><h1>OpenAI Receipt</h1><p>From: OpenAI</p><p>Account email: user@company.example</p><table><tr><th>Plan</th><td>ChatGPT Plus Monthly</td></tr><tr><th>Amount paid</th><td>$20.00</td></tr><tr><th>Billing period</th><td>Oct 1, 2026 - Nov 1, 2026</td></tr></table></body></html>"#;
    let (status, imp1) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "openai-oct.html",
            "content_base64": B64.encode(openai_html.as_bytes())
        })),
    );
    assert_eq!(status, 200, "{imp1}");
    let html_sid = imp1["result"]["source_id"].as_str().unwrap().to_string();

    // Re-upload exact same bytes with a renamed filename -> automatic SHA-256 dedup!
    let (status, imp1_dup) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "openai-oct-copy-renamed.html",
            "content_base64": B64.encode(openai_html.as_bytes())
        })),
    );
    assert_eq!(status, 200, "{imp1_dup}");
    assert_eq!(imp1_dup["reused"], true, "{imp1_dup}");

    // Upload Alipay GBK CSV
    let alipay_str = "支付宝交易记录明细查询\n交易时间,交易对方,商品说明,收/支,金额,交易状态\n2026-10-02,Apple,iCloud+ 2TB包月,支出,68.00,交易成功\n2026-10-04,金山办公,WPS年卡,支出,148.00,交易成功\n";
    let (gbk_bytes, _, _) = encoding_rs::GB18030.encode(alipay_str);
    let (status, imp_csv) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "alipay.csv",
            "content_base64": B64.encode(gbk_bytes.as_ref())
        })),
    );
    assert_eq!(status, 200, "{imp_csv}");
    let csv_sid = imp_csv["result"]["source_id"].as_str().unwrap().to_string();

    // Promote first Alipay CSV row into a standalone verifiable subscription entry!
    let (status, prom) = api_json(
        addr,
        "POST",
        "/api/csv/promote",
        &token,
        Some(json!({
            "parent_source_id": csv_sid,
            "row_number": 3,
            "merchant": "Apple",
            "description": "iCloud+ 2TB包月",
            "date": "2026-10-02",
            "currency": "CNY",
            "amount_text": "68.00",
            "billing_cycle": "monthly",
            "account_label": "个人",
            "raw_line": "2026-10-02,Apple,iCloud+ 2TB包月,支出,68.00,交易成功"
        })),
    );
    assert_eq!(status, 200, "{prom}");
    assert_eq!(prom["storage_reused"], false);

    // Upload PNG receipt screenshot and manually record vendor + EUR amount via unified save API
    let png_bytes = b"\x89PNG\r\n\x1a\nSCREENSHOT_RECEIPT_01";
    let (status, imp_png) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "jetbrains-shot.png",
            "content_base64": B64.encode(png_bytes)
        })),
    );
    assert_eq!(status, 200, "{imp_png}");
    let png_sid = imp_png["result"]["source_id"].as_str().unwrap().to_string();

    let (status, saved) = api_json(
        addr,
        "POST",
        "/api/sources/save",
        &token,
        Some(json!({
            "source_id": png_sid,
            "vendor": "JetBrains",
            "plan_name": "All Products Pack",
            "account_label": "公司工作",
            "billing_cycle": "yearly",
            "currency": "EUR",
            "amount_text": "249.00",
            "category": "开发工具",
            "bill_date": "2026-10-10",
            "notes": "可报销"
        })),
    );
    assert_eq!(status, 200, "{saved}");

    // 4. Verify multi-currency summary (CNY 68.00, EUR 249.00, USD 20.00 — deduped!)
    let summary = saved["summary_by_currency"].as_array().unwrap();
    assert_eq!(summary.len(), 3, "{summary:?}");
    let find_cur = |code: &str| {
        summary.iter().find(|c| c["currency"] == code).unwrap()["total_recorded"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(find_cur("CNY"), "68.00");
    assert_eq!(find_cur("EUR"), "249.00");
    assert_eq!(find_cur("USD"), "20.00");
    assert_eq!(saved["vault"]["duplicate_count"], 1);

    // 5. Verify raw sandbox preview CSP on HTML (via header token, query ?token= rejected!) and redaction API
    let (status, _, _) = http_call(
        addr,
        "GET",
        &format!("/api/sources/{html_sid}/raw?token={token}"),
        None,
        None,
        None,
    );
    assert_eq!(status, 401, "query-string ?token= must be rejected");

    let (status, headers, raw_html) = http_call(
        addr,
        "GET",
        &format!("/api/sources/{html_sid}/raw"),
        Some(&token),
        None,
        None,
    );
    assert_eq!(status, 200);
    assert!(headers.contains("sandbox; default-src 'none'"), "{headers}");
    assert!(headers.contains("filename*=UTF-8''"), "{headers}");
    assert_eq!(raw_html, openai_html.as_bytes());

    let (status, red) = api_json(
        addr,
        "POST",
        "/api/sources/redact",
        &token,
        Some(json!({
            "source_id": html_sid,
            "redact_terms": ["OpenAI"]
        })),
    );
    assert_eq!(status, 200, "{red}");
    assert!(
        red["total_redactions"].as_u64().unwrap_or(0) > 0,
        "total_redactions must reflect actual redaction count: {red}"
    );
    let red_text = red["redacted_text"].as_str().unwrap();
    assert!(!red_text.contains("user@company.example"));
    assert!(!red_text.contains("OpenAI"));

    // 6. Excel UTF-8 BOM CSV export + ZIP plaintext ack gate + verified directory export
    let (status, _, excel_bytes) =
        http_call(addr, "GET", "/api/export/csv", Some(&token), None, None);
    assert_eq!(status, 200);
    assert!(excel_bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    let excel_str = String::from_utf8_lossy(&excel_bytes);
    assert!(excel_str.contains("JetBrains"));
    assert!(excel_str.contains("68.00"));

    // Encrypted ZIP refused without acknowledge_plaintext=yes
    let (status, _, _) = http_call(addr, "GET", "/api/export/zip", Some(&token), None, None);
    assert_eq!(status, 400);

    let (status, _, zip_bytes) = http_call(
        addr,
        "GET",
        "/api/export/zip?acknowledge_plaintext=yes",
        Some(&token),
        None,
        None,
    );
    assert_eq!(status, 200);
    assert!(zip_bytes.starts_with(b"PK\x03\x04"));

    // Native directory export + independent verification
    let export_dir = tmp.path.join("exit-pack-dir");
    let (status, exp) = api_json(
        addr,
        "POST",
        "/api/export/package",
        &token,
        Some(json!({
            "out_dir": export_dir.display().to_string(),
            "acknowledge_plaintext": true
        })),
    );
    assert_eq!(status, 200, "{exp}");
    assert_eq!(exp["verified"], true, "{exp}");

    // 7. Lock vault -> subsequent workspace call returns 423 Locked; unlock with recovery key succeeds!
    let (status, _) = api_json(addr, "POST", "/api/vault/lock", &token, Some(json!({})));
    assert_eq!(status, 200);

    let (status, locked) = api_json(addr, "GET", "/api/workspace", &token, None);
    assert_eq!(status, 423, "{locked}");

    let (status, unlocked_again) = api_json(
        addr,
        "POST",
        "/api/vault/unlock",
        &token,
        Some(json!({
            "path": vault_dir.display().to_string(),
            "recovery_key": recovery_key
        })),
    );
    assert_eq!(status, 200, "{unlocked_again}");
    assert_eq!(unlocked_again["vault"]["source_count"], 4);

    // 8. Demo pack import endpoint works cleanly on an unlocked vault (full asset portfolio + analytics)
    let (status, demo_snap) = api_json(
        addr,
        "POST",
        "/api/import-demo-pack",
        &token,
        Some(json!({})),
    );
    assert_eq!(status, 200, "{demo_snap}");
    assert!(
        demo_snap["vault"]["source_count"].as_u64().unwrap() >= 13,
        "{demo_snap}"
    );
    assert!(demo_snap["analytics"]["categories"].is_object());
    assert!(demo_snap["analytics"]["accounts"].is_object());
    assert!(demo_snap["analytics"]["monthly_trend"].is_object());

    // Asset counts cover subscriptions, licenses, hardware warranties, and reimbursable invoices
    assert!(demo_snap["asset_counts"]["subscription"].as_u64().unwrap() >= 4);
    assert!(demo_snap["asset_counts"]["license"].as_u64().unwrap() >= 1);
    assert!(demo_snap["asset_counts"]["hardware"].as_u64().unwrap() >= 1);
    assert!(demo_snap["asset_counts"]["reimbursable"].as_u64().unwrap() >= 1);

    // Vendor portfolio detects OpenAI price hike ($18.00 -> $20.00, +11%) and aggregates cumulative spend ($38.00)
    let portfolio = demo_snap["vendor_portfolio"].as_array().unwrap();
    let openai_vp = portfolio
        .iter()
        .find(|p| p["vendor"] == "OpenAI, LLC" && p["currency"] == "USD")
        .expect("OpenAI vendor portfolio entry");
    assert_eq!(openai_vp["bill_count"], 2, "{openai_vp}");
    assert_eq!(openai_vp["total_text"], "38.00", "{openai_vp}");
    assert_eq!(openai_vp["price_drift"]["direction"], "up", "{openai_vp}");
    assert_eq!(openai_vp["price_drift"]["pct_change"], 11, "{openai_vp}");

    // Asset radar lights up with price hike, expiring warranty, and pending reimbursement
    let radar = demo_snap["asset_radar"].as_array().unwrap();
    assert!(radar.iter().any(|r| r["kind"] == "price_hike"), "{radar:?}");
    assert!(
        radar.iter().any(|r| r["kind"] == "warranty_expiring"),
        "{radar:?}"
    );
    assert!(
        radar.iter().any(|r| r["kind"] == "reimburse_pending"),
        "{radar:?}"
    );

    // 1-click reimbursement status workflow transitions pending -> submitted -> reimbursed cleanly
    let tencent_sid = demo_snap["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["annotation"]["vendor"] == "腾讯云")
        .expect("reimbursable demo item")["source_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, sub_snap) = api_json(
        addr,
        "POST",
        "/api/sources/reimburse",
        &token,
        Some(json!({
            "source_id": tencent_sid,
            "reimburse_status": "submitted"
        })),
    );
    assert_eq!(status, 200, "{sub_snap}");
    let cny_reimb = sub_snap["reimburse_summary"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["currency"] == "CNY")
        .expect("CNY reimburse summary");
    assert_eq!(cny_reimb["submitted_text"], "420.00", "{cny_reimb}");
    assert_eq!(cny_reimb["pending_count"], 0, "{cny_reimb}");

    // 9. Self-declared manual entry supports encrypted perpetual license keys without leaking plaintext on disk or CSV
    let secret_key = "ULTRA-SECRET-KEY-9941-SubVault";
    let (status, man_snap) = api_json(
        addr,
        "POST",
        "/api/manual-entry",
        &token,
        Some(json!({
            "vendor": "CleanShot X",
            "plan_name": "Pro Perpetual",
            "account_label": "个人",
            "billing_cycle": "one_time",
            "category": "办公效率",
            "currency": "USD",
            "amount_text": "29.00",
            "bill_date": "2026-11-20",
            "asset_kind": "license",
            "license_key": secret_key,
            "license_scope": "终身买断 · 单机激活",
            "notes": "官网直购"
        })),
    );
    assert_eq!(status, 200, "{man_snap}");
    assert!(man_snap["created_source_id"]
        .as_str()
        .unwrap()
        .starts_with("src_"));
    assert!(
        man_snap["items"].as_array().unwrap().iter().any(|i| {
            i["annotation"]["vendor"] == "CleanShot X"
                && i["annotation"]["asset_kind"] == "license"
                && i["annotation"]["license_key"] == secret_key
                && i["amount_fact"]["value"]["amount_text"] == "29.00"
        }),
        "{man_snap}"
    );

    // Raw license keys MUST NEVER appear in plain Excel CSV exports OR on disk inside vault.db!
    let (status, _, excel_bytes_2) =
        http_call(addr, "GET", "/api/export/csv", Some(&token), None, None);
    assert_eq!(status, 200);
    let excel_str_2 = String::from_utf8_lossy(&excel_bytes_2);
    assert!(excel_str_2.contains("资产类型"));
    assert!(excel_str_2.contains("买断授权"));
    assert!(!excel_str_2.contains(secret_key));
    assert!(!excel_str_2.contains("RCST-DEMO-8842-K9X2"));

    let db_bytes = fs::read(vault_dir.join("vault.db")).unwrap();
    let db_lossy = String::from_utf8_lossy(&db_bytes);
    assert!(
        !db_lossy.contains(secret_key),
        "raw manual license key leaked into vault.db"
    );
    assert!(
        !db_lossy.contains("RCST-DEMO-8842-K9X2"),
        "raw demo license key leaked into vault.db"
    );

    // 10. One-click clear-demo-pack removes ONLY demo items & orphaned object files, preserving all real user bills!
    assert!(man_snap["vault"]["demo_count"].as_u64().unwrap() >= 10);
    let (status, cleared_snap) = api_json(
        addr,
        "POST",
        "/api/clear-demo-pack",
        &token,
        Some(json!({})),
    );
    assert_eq!(status, 200, "{cleared_snap}");
    assert!(cleared_snap["removed_count"].as_u64().unwrap() >= 10);
    assert_eq!(cleared_snap["vault"]["demo_count"], 0);
    assert_eq!(cleared_snap["vault"]["source_count"], 5);
    assert_eq!(
        cleared_snap["vault"]["alerts"].as_array().unwrap().len(),
        0,
        "clearing demo pack must leave zero integrity alerts: {cleared_snap}"
    );

    // 11. Single-source delete removes a mistaken manual entry cleanly and passes full export+verify!
    let created_sid = man_snap["created_source_id"].as_str().unwrap().to_string();
    let (status, after_del) = api_json(
        addr,
        "POST",
        "/api/sources/delete",
        &token,
        Some(json!({ "source_id": created_sid })),
    );
    assert_eq!(status, 200, "{after_del}");
    assert_eq!(after_del["removed_count"], 1);
    assert_eq!(after_del["vault"]["source_count"], 4);
    assert_eq!(after_del["vault"]["alerts"].as_array().unwrap().len(), 0);

    let post_del_export = tmp.path.join("exit-pack-after-clean");
    let (status, exp2) = api_json(
        addr,
        "POST",
        "/api/export/package",
        &token,
        Some(json!({
            "out_dir": post_del_export.display().to_string(),
            "acknowledge_plaintext": true
        })),
    );
    assert_eq!(status, 200, "{exp2}");
    assert_eq!(exp2["verified"], true, "{exp2}");

    server.stop();
}

#[test]
fn workbench_wave1_security_zip_verify_conflict_and_rotate_hardening() {
    std::env::set_var("SUBVAULT_NO_RECENT", "1");
    let tmp = Tmp::new("wave1");
    let server = WorkbenchServer::start(WorkbenchOptions {
        port: 0,
        open_browser: false,
        initial_vault: None,
    })
    .unwrap();
    let addr = server.addr();
    let token = server.token().to_string();

    // 1. DNS-rebinding Host header rejected (403)
    let mut bad_host_stream = TcpStream::connect(addr).unwrap();
    bad_host_stream
        .write_all(
            format!(
                "GET /api/bootstrap HTTP/1.1\r\nHost: evil.example:{}\r\nX-SubVault-Token: {token}\r\nConnection: close\r\n\r\n",
                addr.port()
            )
            .as_bytes(),
        )
        .unwrap();
    let mut raw = Vec::new();
    bad_host_stream.read_to_end(&mut raw).unwrap();
    assert!(
        String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 403"),
        "{}",
        String::from_utf8_lossy(&raw)
    );

    // 2. Cross-site Sec-Fetch-Site rejected (403)
    let mut cross_stream = TcpStream::connect(addr).unwrap();
    cross_stream
        .write_all(
            format!(
                "GET /api/bootstrap HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-SubVault-Token: {token}\r\nSec-Fetch-Site: cross-site\r\nConnection: close\r\n\r\n",
                addr.port()
            )
            .as_bytes(),
        )
        .unwrap();
    let mut raw = Vec::new();
    cross_stream.read_to_end(&mut raw).unwrap();
    assert!(
        String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 403"),
        "{}",
        String::from_utf8_lossy(&raw)
    );

    // 3. Vault path containing `#` and `%` works cleanly via percent-encoded SQLite URI
    let vault_dir = tmp.path.join("vault#1%20test");
    let (status, init_res) = api_json(
        addr,
        "POST",
        "/api/vault/init",
        &token,
        Some(json!({
            "path": vault_dir.display().to_string(),
            "protection": "passphrase",
            "passphrase": "Wave1Pass!2026"
        })),
    );
    assert_eq!(status, 200, "{init_res}");

    // 4. Conflicting amounts bill -> has_conflict true, then resolved after manual save -> has_conflict false
    let conflict_html =
        "<html><body><p>Amount paid: $19.99</p><p>Amount paid: $29.99</p></body></html>";
    let (status, imp_conf) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "conflict-bill.html",
            "content_base64": B64.encode(conflict_html.as_bytes())
        })),
    );
    assert_eq!(status, 200, "{imp_conf}");
    let conf_sid = imp_conf["result"]["source_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, ws1) = api_json(addr, "GET", "/api/workspace", &token, None);
    assert_eq!(status, 200, "{ws1}");
    let conf_item = ws1["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["source_id"] == conf_sid)
        .unwrap();
    assert_eq!(
        conf_item["status_flags"]["has_conflict"], true,
        "{conf_item}"
    );

    // Save resolved amount AND a formula-injection vendor name to test CSV neutralization
    let (status, ws2) = api_json(
        addr,
        "POST",
        "/api/sources/save",
        &token,
        Some(json!({
            "source_id": conf_sid,
            "vendor": "=CMD|'/C calc'!A0",
            "billing_cycle": "monthly",
            "currency": "USD",
            "amount_text": "19.99"
        })),
    );
    assert_eq!(status, 200, "{ws2}");
    let resolved_item = ws2["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["source_id"] == conf_sid)
        .unwrap();
    assert_eq!(
        resolved_item["status_flags"]["has_conflict"], false,
        "resolving amount must clear conflict flag: {resolved_item}"
    );

    // 5. Excel CSV neutralizes formula injection
    let (status, _, csv_bytes) =
        http_call(addr, "GET", "/api/export/csv", Some(&token), None, None);
    assert_eq!(status, 200);
    let csv_text = String::from_utf8_lossy(&csv_bytes);
    assert!(
        csv_text.contains("'=CMD|'/C calc'!A0"),
        "CSV formula must be neutralized: {csv_text}"
    );

    // 6. Export inside vault does NOT create a stray directory inside the vault!
    let inside_target = vault_dir.join("nested-bad").join("pkg");
    let (status, inside_res) = api_json(
        addr,
        "POST",
        "/api/export/package",
        &token,
        Some(json!({
            "out_dir": inside_target.display().to_string(),
            "acknowledge_plaintext": true
        })),
    );
    assert_eq!(status, 400, "{inside_res}");
    assert!(
        !vault_dir.join("nested-bad").exists(),
        "export inside vault must never pre-create directories inside vault"
    );

    // 7. In-memory ZIP export puts verifiable package under `package/` and passes verify-package!
    let (status, _, zip_bytes) = http_call(
        addr,
        "GET",
        "/api/export/zip?acknowledge_plaintext=yes",
        Some(&token),
        None,
        None,
    );
    assert_eq!(status, 200);
    let zip_extract = tmp.path.join("unzipped");
    extract_store_zip(&zip_bytes, &zip_extract);
    assert!(zip_extract.join("ledger-excel.csv").is_file());
    assert!(zip_extract.join("review-sheet.txt").is_file());
    assert!(zip_extract.join("README.txt").is_file());
    let pkg_sub = zip_extract.join("package");
    let (status, ver_ok) = api_json(
        addr,
        "POST",
        "/api/verify-package",
        &token,
        Some(json!({ "package_dir": pkg_sub.display().to_string() })),
    );
    assert_eq!(status, 200, "{ver_ok}");
    assert_eq!(ver_ok["ok"], true, "{ver_ok}");
    assert_eq!(ver_ok["verified"], true, "{ver_ok}");
    assert!(ver_ok["report"]["verified_files"].as_u64().unwrap() >= 3);

    // Tamper extracted package -> /api/verify-package returns HTTP 200 with ok: true, verified: false
    fs::write(pkg_sub.join("extra-tamper.txt"), b"tamper").unwrap();
    let (status, ver_bad) = api_json(
        addr,
        "POST",
        "/api/verify-package",
        &token,
        Some(json!({ "package_dir": pkg_sub.display().to_string() })),
    );
    assert_eq!(status, 200, "{ver_bad}");
    assert_eq!(ver_bad["ok"], true, "{ver_bad}");
    assert_eq!(ver_bad["verified"], false, "{ver_bad}");
    assert_eq!(ver_bad["report"]["reason"], "unexpected_file");

    // 8. Rotate passphrase requires current_passphrase or recovery_key (never falls back to RAM DEK!)
    let (status, rot_missing) = api_json(
        addr,
        "POST",
        "/api/vault/rotate-passphrase",
        &token,
        Some(json!({ "new_passphrase": "NewStrongPass!2026" })),
    );
    assert_eq!(status, 400, "{rot_missing}");
    assert_eq!(rot_missing["error"], "current_credential_required");

    let (status, rot_ok) = api_json(
        addr,
        "POST",
        "/api/vault/rotate-passphrase",
        &token,
        Some(json!({
            "current_passphrase": "Wave1Pass!2026",
            "new_passphrase": "NewStrongPass!2026"
        })),
    );
    assert_eq!(status, 200, "{rot_ok}");

    // 9. Deleting source removes its command & journal file completely
    let journals_before: usize = fs::read_dir(vault_dir.join("journal")).unwrap().count();
    assert!(journals_before > 0);
    let (status, del_res) = api_json(
        addr,
        "POST",
        "/api/sources/delete",
        &token,
        Some(json!({ "source_id": conf_sid })),
    );
    assert_eq!(status, 200, "{del_res}");
    assert_eq!(del_res["vault"]["source_count"], 0);
    let journals_after: usize = fs::read_dir(vault_dir.join("journal")).unwrap().count();
    assert_eq!(journals_after, 0, "deleted source journals must be pruned");

    server.stop();
}

#[test]
fn workbench_wave2_validation_csv_guard_dedup_monthly_and_price_drift_isolation() {
    std::env::set_var("SUBVAULT_NO_RECENT", "1");
    let tmp = Tmp::new("wave2");
    let server = WorkbenchServer::start(WorkbenchOptions {
        port: 0,
        open_browser: false,
        initial_vault: None,
    })
    .unwrap();
    let addr = server.addr();
    let token = server.token().to_string();

    let vault_dir = tmp.path.join("wave2-vault");
    let (status, init_res) = api_json(
        addr,
        "POST",
        "/api/vault/init",
        &token,
        Some(json!({
            "path": vault_dir.display().to_string(),
            "protection": "os-disk"
        })),
    );
    assert_eq!(status, 200, "{init_res}");

    // 1. Upload 2 monthly bills for the SAME subscription (OpenAI Plus personal: $18 then $20)
    //    + 1 monthly bill for a DIFFERENT account (OpenAI Plus work: $30)
    //    Verify:
    //    - summary_by_currency total_recorded = 68.00, monthly_equiv_recurring = 50.00 (20 + 30, NOT 68!)
    //    - vendor_portfolio detects +11% hike ONLY on the personal track ($18 -> $20), NOT $20 -> $30 across accounts!
    let mk_html = |acct: &str, amt: &str, period: &str| {
        format!("<html><body><h1>OpenAI Receipt</h1><p>From: OpenAI, LLC</p><p>Account email: {acct}</p><table><tr><th>Plan</th><td>ChatGPT Plus</td></tr><tr><th>Amount paid</th><td>${amt}</td></tr><tr><th>Billing period</th><td>{period}</td></tr></table></body></html>")
    };
    let h1 = mk_html("me@personal.example", "18.00", "2026-08-01 to 2026-09-01");
    let h2 = mk_html("me@personal.example", "20.00", "2026-09-01 to 2026-10-01");
    let h3 = mk_html("me@work.example", "30.00", "2026-09-05 to 2026-10-05");

    let mut sids = Vec::new();
    for (name, body, acct, date) in [
        ("oa-p1.html", &h1, "个人", "2026-08-01"),
        ("oa-p2.html", &h2, "个人", "2026-09-01"),
        ("oa-w1.html", &h3, "公司工作", "2026-09-05"),
    ] {
        let (st, imp) = api_json(
            addr,
            "POST",
            "/api/import",
            &token,
            Some(json!({
                "filename": name,
                "content_base64": B64.encode(body.as_bytes())
            })),
        );
        assert_eq!(st, 200, "{imp}");
        let sid = imp["result"]["source_id"].as_str().unwrap().to_string();
        let (st, _) = api_json(
            addr,
            "POST",
            "/api/sources/save",
            &token,
            Some(json!({
                "source_id": sid,
                "vendor": "OpenAI, LLC",
                "plan_name": "ChatGPT Plus",
                "account_label": acct,
                "billing_cycle": "monthly",
                "bill_date": date
            })),
        );
        assert_eq!(st, 200);
        sids.push(sid);
    }

    let (st, ws) = api_json(addr, "GET", "/api/workspace", &token, None);
    assert_eq!(st, 200, "{ws}");
    let usd = ws["summary_by_currency"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["currency"] == "USD")
        .expect("USD summary");
    assert_eq!(usd["total_recorded"], "68.00", "{usd}");
    assert_eq!(
        usd["monthly_equiv_recurring"], "50.00",
        "recurring monthly equivalent must deduplicate multiple bills of same subscription: {usd}"
    );
    assert_eq!(usd["recurring_count"], 2, "{usd}");

    let vp = ws["vendor_portfolio"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["vendor"] == "OpenAI, LLC")
        .expect("OpenAI portfolio");
    assert_eq!(vp["price_drift"]["prev_text"], "18.00", "{vp}");
    assert_eq!(vp["price_drift"]["curr_text"], "20.00", "{vp}");
    assert_eq!(vp["price_drift"]["pct_change"], 11, "{vp}");

    // 2. api_source_save validates invalid amount/date UPFRONT without mutating vendor/notes
    let (st, bad_save) = api_json(
        addr,
        "POST",
        "/api/sources/save",
        &token,
        Some(json!({
            "source_id": sids[0],
            "vendor": "SHOULD_NOT_BE_SAVED",
            "currency": "USD",
            "amount_text": "not-a-number"
        })),
    );
    assert_eq!(st, 400, "{bad_save}");
    let (st, bad_date) = api_json(
        addr,
        "POST",
        "/api/sources/save",
        &token,
        Some(json!({
            "source_id": sids[0],
            "vendor": "SHOULD_NOT_BE_SAVED",
            "period_start": "2026-02-31",
            "period_end": "2026-03-01"
        })),
    );
    assert_eq!(st, 400, "{bad_date}");

    let (st, ws_after) = api_json(addr, "GET", "/api/workspace", &token, None);
    assert_eq!(st, 200);
    let item0 = ws_after["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["source_id"] == sids[0])
        .unwrap();
    assert_eq!(
        item0["annotation"]["vendor"], "OpenAI, LLC",
        "failed validation must not partially mutate annotations"
    );

    // 3. api_csv_promote refuses refund / income / neutral rows
    let csv_body = "支付宝交易记录明细查询\n交易时间,交易对方,商品说明,收/支,金额,交易状态\n2026-10-02,Apple,退款,收入,68.00,退款成功\n";
    let (st, imp_csv) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "refunds.csv",
            "content_base64": B64.encode(csv_body.as_bytes())
        })),
    );
    assert_eq!(st, 200, "{imp_csv}");
    let csv_sid = imp_csv["result"]["source_id"].as_str().unwrap().to_string();

    for dir in ["refund", "income", "neutral"] {
        let (st, rej) = api_json(
            addr,
            "POST",
            "/api/csv/promote",
            &token,
            Some(json!({
                "parent_source_id": csv_sid,
                "row_number": 3,
                "merchant": "Apple",
                "description": "退款",
                "date": "2026-10-02",
                "currency": "CNY",
                "amount_text": "68.00",
                "direction": dir,
                "raw_line": "2026-10-02,Apple,退款,收入,68.00,退款成功"
            })),
        );
        assert_eq!(st, 400, "direction={dir} must be rejected: {rej}");
        assert_eq!(rej["error"], "csv_row_not_expense", "{rej}");
    }

    // 4. Demo pack generates crisp visual Chinese e-Fapiao PDF + real PNG receipt (not 1x1 pixel!)
    let (st, demo) = api_json(
        addr,
        "POST",
        "/api/import-demo-pack",
        &token,
        Some(json!({})),
    );
    assert_eq!(st, 200, "{demo}");
    let items = demo["items"].as_array().unwrap();
    let wps = items
        .iter()
        .find(|i| i["display_name"] == "示例-WPS年卡电子发票.pdf")
        .expect("WPS demo fapiao");
    assert_eq!(wps["amount_fact"]["value"]["currency"], "CNY", "{wps}");
    assert_eq!(wps["amount_fact"]["value"]["amount_text"], "98.00", "{wps}");
    assert!(
        wps["annotation"]["vendor"]
            .as_str()
            .unwrap_or("")
            .contains("金山办公"),
        "WPS vendor should extract cleanly from UTF-16BE layer: {wps}"
    );
    let png_item = items
        .iter()
        .find(|i| i["display_name"].as_str().unwrap_or("").ends_with(".png"))
        .expect("demo PNG receipt");
    let png_sid = png_item["source_id"].as_str().unwrap();
    let (st, _, png_raw) = http_call(
        addr,
        "GET",
        &format!("/api/sources/{png_sid}/raw"),
        Some(&token),
        None,
        None,
    );
    assert_eq!(st, 200);
    assert!(png_raw.starts_with(b"\x89PNG\r\n\x1a\n"));
    // Check PNG IHDR width/height is 360x420 (not 1x1!)
    let width = u32::from_be_bytes([png_raw[16], png_raw[17], png_raw[18], png_raw[19]]);
    let height = u32::from_be_bytes([png_raw[20], png_raw[21], png_raw[22], png_raw[23]]);
    assert!(
        width >= 300 && height >= 300,
        "demo PNG should be a readable receipt, got {width}x{height}"
    );

    server.stop();
}

#[test]
fn workbench_wave5_hardening_regression_guards() {
    std::env::set_var("SUBVAULT_NO_RECENT", "1");
    let tmp = Tmp::new("wave5");
    let server = WorkbenchServer::start(WorkbenchOptions {
        port: 0,
        open_browser: false,
        initial_vault: None,
    })
    .unwrap();
    let addr = server.addr();
    let token = server.token().to_string();

    let vault_dir = tmp.path.join("wave5-vault");
    let (st, init_res) = api_json(
        addr,
        "POST",
        "/api/vault/init",
        &token,
        Some(json!({
            "path": vault_dir.display().to_string(),
            "protection": "os-disk"
        })),
    );
    assert_eq!(st, 200, "{init_res}");

    // 1. path_is_inside_vault must block `..` traversal AND never pre-create dirs inside vault
    let sneaky_target = vault_dir
        .join("..")
        .join("wave5-vault")
        .join("sneaky-sub")
        .join("pkg");
    let (st, sneaky_res) = api_json(
        addr,
        "POST",
        "/api/export/package",
        &token,
        Some(json!({
            "out_dir": sneaky_target.display().to_string(),
            "acknowledge_plaintext": true
        })),
    );
    assert_eq!(st, 400, "{sneaky_res}");
    assert_eq!(sneaky_res["error"], "output_inside_vault");
    assert!(
        !vault_dir.join("sneaky-sub").exists(),
        ".. traversal must not create directories inside vault"
    );

    // 2. Long Chinese UTF-8 filename (>70 CJK chars = >210 UTF-8 bytes) preserves `.html` extension & imports cleanly
    let long_zh_name = format!("{}.html", "电子发票报销凭证".repeat(12));
    assert!(long_zh_name.len() > 200);
    let simple_html = "<html><body><p>收款方：金山办公</p><p>实付金额：¥98.00</p></body></html>";
    let (st, imp_long) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": long_zh_name,
            "content_base64": B64.encode(simple_html.as_bytes())
        })),
    );
    assert_eq!(st, 200, "{imp_long}");
    let sanitized = imp_long["filename"].as_str().unwrap();
    assert!(sanitized.len() <= 200, "byte len {} > 200", sanitized.len());
    assert!(sanitized.ends_with(".html"), "extension lost: {sanitized}");
    let zh_sid = imp_long["result"]["source_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. api_source_save omitting `currency` preserves existing CNY currency; undoing then correcting restores statement
    let (st, saved_keep_cur) = api_json(
        addr,
        "POST",
        "/api/sources/save",
        &token,
        Some(json!({
            "source_id": zh_sid,
            "amount_text": "108.00"
        })),
    );
    assert_eq!(st, 200, "{saved_keep_cur}");
    let zh_item = saved_keep_cur["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["source_id"] == zh_sid)
        .unwrap();
    assert_eq!(zh_item["amount_fact"]["value"]["currency"], "CNY");
    assert_eq!(zh_item["amount_fact"]["value"]["amount_text"], "108.00");

    // Undo latest decision (rev 2 -> rev 1) then undo initial decision (rev 1 -> unknown), then re-save amount
    let dec2 = zh_item["amount_fact"]["decisions"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["decision_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, u1) = api_json(
        addr,
        "POST",
        "/api/facts/undo",
        &token,
        Some(json!({ "decision_id": dec2, "expected_revision": 2 })),
    );
    assert_eq!(st, 200, "{u1}");
    let zh_item_u1 = u1["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["source_id"] == zh_sid)
        .unwrap();
    let dec3 = zh_item_u1["amount_fact"]["decisions"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["decision_id"]
        .as_str()
        .unwrap()
        .to_string();
    // Re-enter on a fresh source whose initial decision is undone -> statement transitions from unknown to user_entered
    let _ = dec3;
    let fresh_html = "<html><body><p>实付金额：¥50.00</p></body></html>";
    let (st, imp_fresh) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "undo-reset.html",
            "content_base64": B64.encode(fresh_html.as_bytes())
        })),
    );
    assert_eq!(st, 200, "{imp_fresh}");
    let fresh_sid = imp_fresh["result"]["source_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, ws_f) = api_json(addr, "GET", "/api/workspace", &token, None);
    assert_eq!(st, 200);
    let f_item = ws_f["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["source_id"] == fresh_sid)
        .unwrap();
    let init_dec = f_item["amount_fact"]["decisions"][0]["decision_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, ws_undone) = api_json(
        addr,
        "POST",
        "/api/facts/undo",
        &token,
        Some(json!({ "decision_id": init_dec, "expected_revision": 1 })),
    );
    assert_eq!(st, 200, "{ws_undone}");
    let (st, ws_reentered) = api_json(
        addr,
        "POST",
        "/api/sources/save",
        &token,
        Some(json!({
            "source_id": fresh_sid,
            "amount_text": "55.00"
        })),
    );
    assert_eq!(st, 200, "{ws_reentered}");
    let re_item = ws_reentered["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["source_id"] == fresh_sid)
        .unwrap();
    assert_eq!(re_item["amount_fact"]["value"]["statement"], "user_entered");
    assert_eq!(re_item["amount_fact"]["value"]["currency"], "CNY");
    assert_eq!(re_item["amount_fact"]["value"]["amount_text"], "55.00");

    // 4. api_manual_entry rejects invalid date/partial period UPFRONT leaving ZERO orphan sources
    let count_before = ws_reentered["vault"]["source_count"].as_u64().unwrap();
    let (st, bad_man) = api_json(
        addr,
        "POST",
        "/api/manual-entry",
        &token,
        Some(json!({
            "vendor": "OrphanTest",
            "currency": "CNY",
            "amount_text": "12.00",
            "bill_date": "2026-02-31"
        })),
    );
    assert_eq!(st, 400, "{bad_man}");
    assert_eq!(bad_man["error"], "bad_bill_date");
    let (st, bad_per) = api_json(
        addr,
        "POST",
        "/api/manual-entry",
        &token,
        Some(json!({
            "vendor": "OrphanTest2",
            "currency": "CNY",
            "amount_text": "12.00",
            "period_start": "2026-05-01"
        })),
    );
    assert_eq!(st, 400, "{bad_per}");
    assert_eq!(bad_per["error"], "period_requires_both_bounds");
    let (st, ws_check) = api_json(addr, "GET", "/api/workspace", &token, None);
    assert_eq!(st, 200);
    assert_eq!(
        ws_check["vault"]["source_count"].as_u64().unwrap(),
        count_before,
        "failed manual entries must not leave orphan sources"
    );

    // 5. GB18030 HTML inline preview decodes to UTF-8 while ?download=1 stays byte-exact
    let gbk_html_text = "<html><body><h1>中文账单</h1><p>实付金额：¥66.00</p></body></html>";
    let (gbk_html_bytes, _, _) = encoding_rs::GB18030.encode(gbk_html_text);
    let (st, imp_gbk) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "gbk-bill.html",
            "content_base64": B64.encode(gbk_html_bytes.as_ref())
        })),
    );
    assert_eq!(st, 200, "{imp_gbk}");
    let gbk_sid = imp_gbk["result"]["source_id"].as_str().unwrap().to_string();
    let (st, _, inline_raw) = http_call(
        addr,
        "GET",
        &format!("/api/sources/{gbk_sid}/raw"),
        Some(&token),
        None,
        None,
    );
    assert_eq!(st, 200);
    assert!(String::from_utf8(inline_raw)
        .expect("inline HTML preview must be UTF-8")
        .contains("中文账单"));
    let (st, _, dl_raw) = http_call(
        addr,
        "GET",
        &format!("/api/sources/{gbk_sid}/raw?download=1"),
        Some(&token),
        None,
        None,
    );
    assert_eq!(st, 200);
    assert_eq!(dl_raw, gbk_html_bytes.as_ref());

    // 6. Deeply nested HTML (250 nested <div>s) must not stack-overflow
    let deep_html = format!(
        "<html><body>{}<p>Amount paid: $19.99</p>{}</body></html>",
        "<div>".repeat(250),
        "</div>".repeat(250)
    );
    let (st, imp_deep) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "deep-nested.html",
            "content_base64": B64.encode(deep_html.as_bytes())
        })),
    );
    assert_eq!(st, 200, "{imp_deep}");

    // 7. GBK Alipay CSV can be redacted in memory AND US MM/DD/YYYY row promotes without orphan failure
    let gbk_csv_text = "支付宝交易记录明细查询\n账号:[secret.user@example.com]\n交易时间,交易对方,商品说明,收/支,金额,交易状态\n10/01/2026,AcmeCloud,Pro Plan,支出,88.00,交易成功\n";
    let (gbk_csv_bytes, _, _) = encoding_rs::GB18030.encode(gbk_csv_text);
    let (st, imp_csv) = api_json(
        addr,
        "POST",
        "/api/import",
        &token,
        Some(json!({
            "filename": "alipay-gbk.csv",
            "content_base64": B64.encode(gbk_csv_bytes.as_ref())
        })),
    );
    assert_eq!(st, 200, "{imp_csv}");
    let csv_sid = imp_csv["result"]["source_id"].as_str().unwrap().to_string();

    let (st, red_csv) = api_json(
        addr,
        "POST",
        "/api/sources/redact",
        &token,
        Some(json!({ "source_id": csv_sid })),
    );
    assert_eq!(st, 200, "{red_csv}");
    let red_csv_text = red_csv["redacted_text"].as_str().unwrap();
    assert!(
        !red_csv_text.contains("secret.user@example.com"),
        "{red_csv_text}"
    );
    assert!(
        red_csv_text.contains("支付宝交易记录明细查询"),
        "{red_csv_text}"
    );
    assert!(
        red_csv_text.contains("[REDACTED_FIELD_") || red_csv_text.contains("[REDACTED_EMAIL_"),
        "{red_csv_text}"
    );

    let (st, prom_us) = api_json(
        addr,
        "POST",
        "/api/csv/promote",
        &token,
        Some(json!({
            "parent_source_id": csv_sid,
            "row_number": 4,
            "merchant": "AcmeCloud",
            "description": "Pro Plan",
            "date": "10/01/2026",
            "currency": "CNY",
            "amount_text": "88.00",
            "billing_cycle": "monthly",
            "direction": "expense",
            "raw_line": "10/01/2026,AcmeCloud,Pro Plan,支出,88.00,交易成功"
        })),
    );
    assert_eq!(st, 200, "{prom_us}");

    server.stop();
}

fn extract_store_zip(bytes: &[u8], out_dir: &std::path::Path) {
    let mut pos = 0usize;
    while pos + 30 <= bytes.len() {
        let sig = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
        if sig != 0x0403_4b50 {
            break;
        }
        let comp_size = u32::from_le_bytes([
            bytes[pos + 18],
            bytes[pos + 19],
            bytes[pos + 20],
            bytes[pos + 21],
        ]) as usize;
        let name_len = u16::from_le_bytes([bytes[pos + 26], bytes[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[pos + 28], bytes[pos + 29]]) as usize;
        let name_start = pos + 30;
        let data_start = name_start + name_len + extra_len;
        let name = std::str::from_utf8(&bytes[name_start..name_start + name_len]).unwrap();
        let data = &bytes[data_start..data_start + comp_size];
        let full = out_dir.join(name);
        if let Some(p) = full.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(&full, data).unwrap();
        pos = data_start + comp_size;
    }
}
