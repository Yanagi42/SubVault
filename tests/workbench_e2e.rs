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

    // 5. Verify raw sandbox preview CSP on HTML and redaction API
    let (status, headers, raw_html) = http_call(
        addr,
        "GET",
        &format!("/api/sources/{html_sid}/raw?token={token}"),
        None,
        None,
        None,
    );
    assert_eq!(status, 200);
    assert!(headers.contains("sandbox; default-src 'none'"), "{headers}");
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

    // 8. Demo pack import endpoint works cleanly on an unlocked vault
    let (status, demo_snap) = api_json(
        addr,
        "POST",
        "/api/import-demo-pack",
        &token,
        Some(json!({})),
    );
    assert_eq!(status, 200, "{demo_snap}");
    assert!(demo_snap["vault"]["source_count"].as_u64().unwrap() >= 7);

    server.stop();
}
