//! Embedded local-only graphical workbench server (`subvault-app` / `subvault app`).
//!
//! Security model:
//! - Strictly binds loopback `127.0.0.1:<port>` (never LAN/WAN).
//! - Requires a 256-bit cryptographic session token on every `/api/*` call
//!   (header `X-SubVault-Token` or `?token=` for same-origin iframe/download links).
//! - Verifies `Origin` header on every request to prevent cross-site / DNS-rebinding CSRF.
//! - Holds unlocked Format 6 `VaultCipher` strictly in process memory (`Zeroizing` DEK)
//!   and drops it immediately on lock or shutdown; passphrases never touch disk.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};

use crate::crypto::{AuthInput, NewPassphraseInput, ProtectionMode, VaultCipher};
use crate::engine::{self, Hooks};
use crate::ids;
use crate::model::{
    Report, EXIT_OK, EXIT_USAGE, PRODUCTION_BANNER_FILE, PRODUCTION_BANNER_OS_DISK,
    PRODUCTION_BANNER_PASSPHRASE,
};

const INDEX_HTML: &str = include_str!("../app/index.html");
const STYLES_CSS: &str = include_str!("../app/styles.css");
const APP_JS: &str = include_str!("../app/app.js");
const ICON_SVG: &str = include_str!("../web/assets/icon.svg");

const MAX_HTTP_BODY: usize = 16 * 1024 * 1024; // 16 MiB (handles 8 MiB file in base64)
const WORKBENCH_ACTOR_ID: &str = "act_776f726b62656e63685f757365723031";

const DEMO_PACK_FILENAMES: &[&str] = &[
    "示例-WPS年卡电子发票.pdf",
    "sample-openai-sep.html",
    "sample-openai-receipt.html",
    "sample-openai-receipt-renamed-copy.html",
    "sample-github-copilot.html",
    "示例-Apple-iCloud月账单.html",
    "sample-netflix-nov.html",
    "示例-MacBook-Air硬件保修发票.pdf",
    "sample-raycast-license.html",
    "示例-腾讯云企业报销发票.pdf",
    "示例-待补录收据截图.png",
    "示例-支付宝账单明细.csv",
];

fn is_demo_source(display_name: &str, occurrences: &[Value]) -> bool {
    if DEMO_PACK_FILENAMES.contains(&display_name) {
        return true;
    }
    occurrences.iter().any(|o| {
        o["display_name"]
            .as_str()
            .is_some_and(|n| DEMO_PACK_FILENAMES.contains(&n))
    })
}

#[derive(Clone, Debug)]
pub struct WorkbenchOptions {
    pub port: u16,
    pub open_browser: bool,
    pub initial_vault: Option<PathBuf>,
}

struct ActiveVault {
    path: PathBuf,
    vault_id: String,
    vault_format: i64,
    protection: Option<ProtectionMode>,
    cipher: VaultCipher,
}

impl ActiveVault {
    fn auth(&self) -> AuthInput {
        AuthInput::from_cipher(self.cipher.clone())
    }
}

struct ServerState {
    port: u16,
    token: String,
    initial_vault: Option<PathBuf>,
    active: Mutex<Option<ActiveVault>>,
    shutdown: AtomicBool,
}

pub struct WorkbenchServer {
    addr: SocketAddr,
    token: String,
    state: Arc<ServerState>,
    handle: Option<JoinHandle<()>>,
}

impl WorkbenchServer {
    pub fn start(opts: WorkbenchOptions) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", opts.port))?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;

        let mut token_bytes = [0u8; 32];
        crate::platform::fill_random(&mut token_bytes)?;
        let token = ids::hex_encode(&token_bytes);

        let state = Arc::new(ServerState {
            port: addr.port(),
            token: token.clone(),
            initial_vault: opts.initial_vault,
            active: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });

        let thread_state = Arc::clone(&state);
        let handle = thread::spawn(move || {
            serve_loop(listener, thread_state);
        });

        Ok(Self {
            addr,
            token,
            state,
            handle: Some(handle),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/#token={}",
            self.addr.port(),
            self.token
        )
    }

    pub fn wait(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    pub fn stop(mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.state.active.lock() {
            *guard = None;
        }
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WorkbenchServer {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.state.active.lock() {
            *guard = None;
        }
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn run_standalone(opts: WorkbenchOptions) -> io::Result<()> {
    let open = opts.open_browser;
    let server = WorkbenchServer::start(opts)?;
    let url = server.url();
    println!("SubVault 本地工作台已启动（仅限本机访问，不联网）：");
    println!("  {url}");
    println!("关闭工作台请在页面右上角点击「安全退出」，或在此窗口按 Ctrl+C。");
    if open {
        open_in_browser(&url);
    }
    server.wait();
    println!("SubVault 已安全锁定并退出。");
    Ok(())
}

pub fn run_cli_app(opts: WorkbenchOptions) -> Report {
    match run_standalone(opts) {
        Ok(()) => Report {
            code: EXIT_OK,
            body: json!({
                "program": "subvault",
                "outcome": "WorkbenchStopped",
                "new_effect": false
            }),
        },
        Err(err) => Report {
            code: EXIT_USAGE,
            body: json!({
                "program": "subvault",
                "outcome": "WorkbenchError",
                "os_error": err.to_string(),
                "new_effect": false
            }),
        },
    }
}

fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

fn serve_loop(listener: TcpListener, state: Arc<ServerState>) {
    while !state.shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => {
                if state.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let st = Arc::clone(&state);
                thread::spawn(move || {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                    handle_connection(stream, &st);
                });
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(35));
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

struct HttpRequest {
    method: String,
    path: String,
    query: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: String,
    extra_headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn ok_bytes(content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type: content_type.to_string(),
            extra_headers: Vec::new(),
            body,
        }
    }

    fn json(status: u16, value: Value) -> Self {
        let reason = match status {
            200 => "OK",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            409 => "Conflict",
            413 => "Payload Too Large",
            423 => "Locked",
            _ => "Internal Server Error",
        };
        let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
        Self {
            status,
            reason,
            content_type: "application/json; charset=utf-8".to_string(),
            extra_headers: Vec::new(),
            body,
        }
    }

    fn error(status: u16, code: &str, message: &str) -> Self {
        Self::json(
            status,
            json!({
                "ok": false,
                "error": code,
                "message": message
            }),
        )
    }

    fn with_header(mut self, k: &str, v: impl Into<String>) -> Self {
        self.extra_headers.push((k.to_string(), v.into()));
        self
    }
}

fn handle_connection(mut stream: TcpStream, state: &ServerState) {
    let req = match read_http_request(&mut stream) {
        Ok(Some(req)) => req,
        Ok(None) => return,
        Err(resp) => {
            let _ = write_http_response(&mut stream, resp);
            return;
        }
    };

    if let Some(origin) = req.headers.get("origin") {
        let allowed_ip = format!("http://127.0.0.1:{}", state.port);
        let allowed_lh = format!("http://localhost:{}", state.port);
        if origin != &allowed_ip && origin != &allowed_lh {
            let _ = write_http_response(
                &mut stream,
                HttpResponse::error(403, "origin_forbidden", "拒绝来自外部页面的访问"),
            );
            return;
        }
    }

    let resp = dispatch(&req, state);
    let _ = write_http_response(&mut stream, resp);
}

fn read_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>, HttpResponse> {
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|_| HttpResponse::error(500, "stream_error", "连接读取失败"))?,
    );
    let mut request_line = String::new();
    let n = reader
        .read_line(&mut request_line)
        .map_err(|_| HttpResponse::error(400, "bad_request", "请求读取失败"))?;
    if n == 0 {
        return Ok(None);
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(HttpResponse::error(400, "bad_request", "请求行无效"));
    }
    let method = parts[0].to_ascii_uppercase();
    let raw_target = parts[1];
    let (path, query) = split_path_query(raw_target);

    let mut headers = BTreeMap::new();
    let mut header_count = 0usize;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|_| HttpResponse::error(400, "bad_header", "请求头读取失败"))?;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        header_count += 1;
        if header_count > 64 {
            return Err(HttpResponse::error(400, "too_many_headers", "请求头过多"));
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_len = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_len > MAX_HTTP_BODY {
        return Err(HttpResponse::error(
            413,
            "payload_too_large",
            "上传文件超过单次 8 MiB 限制",
        ));
    }
    let mut body = vec![0u8; content_len];
    if content_len > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|_| HttpResponse::error(400, "incomplete_body", "请求体读取不完整"))?;
    }

    Ok(Some(HttpRequest {
        method,
        path,
        query,
        headers,
        body,
    }))
}

fn write_http_response(stream: &mut TcpStream, resp: HttpResponse) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: SAMEORIGIN\r\nConnection: close\r\n",
        resp.status,
        resp.reason,
        resp.content_type,
        resp.body.len()
    );
    let has_custom_csp = resp
        .extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-security-policy"));
    if !has_custom_csp {
        head.push_str(
            "Content-Security-Policy: default-src 'self'; img-src 'self' data: blob:; frame-src 'self' blob:; style-src 'self' 'unsafe-inline'\r\n",
        );
    }
    for (k, v) in resp.extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&resp.body)?;
    stream.flush()
}

fn split_path_query(raw: &str) -> (String, BTreeMap<String, String>) {
    let mut query = BTreeMap::new();
    let (path, qstr) = match raw.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (raw, None),
    };
    if let Some(q) = qstr {
        for pair in q.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (url_decode(k), url_decode(v)),
                None => (url_decode(pair), String::new()),
            };
            query.insert(k, v);
        }
    }
    (path.to_string(), query)
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn check_token(req: &HttpRequest, expected: &str) -> bool {
    if let Some(header_tok) = req.headers.get("x-subvault-token") {
        if constant_time_eq(header_tok.as_bytes(), expected.as_bytes()) {
            return true;
        }
    }
    if req.method == "GET" {
        if let Some(q_tok) = req.query.get("token") {
            if constant_time_eq(q_tok.as_bytes(), expected.as_bytes()) {
                return true;
            }
        }
    }
    false
}

fn dispatch(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    // Public static UI shell (never contains user data; APIs require session token)
    if req.method == "GET" {
        match req.path.as_str() {
            "/" | "/index.html" => {
                return HttpResponse::ok_bytes(
                    "text/html; charset=utf-8",
                    INDEX_HTML.as_bytes().to_vec(),
                );
            }
            "/styles.css" => {
                return HttpResponse::ok_bytes(
                    "text/css; charset=utf-8",
                    STYLES_CSS.as_bytes().to_vec(),
                );
            }
            "/app.js" => {
                return HttpResponse::ok_bytes(
                    "application/javascript; charset=utf-8",
                    APP_JS.as_bytes().to_vec(),
                );
            }
            "/icon.svg" => {
                return HttpResponse::ok_bytes("image/svg+xml", ICON_SVG.as_bytes().to_vec());
            }
            _ => {}
        }
    }

    if !req.path.starts_with("/api/") {
        return HttpResponse::error(404, "not_found", "未找到页面");
    }

    if !check_token(req, &state.token) {
        return HttpResponse::error(
            401,
            "unauthorized",
            "本地会话凭证无效，请通过启动后的本地链接访问",
        );
    }

    // Dynamic source preview routes: `/api/sources/<id>/raw` and `/api/sources/<id>/text`
    if req.method == "GET" && req.path.starts_with("/api/sources/") {
        let rest = &req.path["/api/sources/".len()..];
        if let Some(source_id) = rest.strip_suffix("/raw") {
            return api_source_raw(req, state, source_id);
        }
        if let Some(source_id) = rest.strip_suffix("/text") {
            return api_source_text(state, source_id);
        }
    }

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/bootstrap") => api_bootstrap(state),
        ("POST", "/api/pick-folder") => api_pick_folder(req),
        ("POST", "/api/vault/init") => api_vault_init(req, state),
        ("POST", "/api/vault/unlock") => api_vault_unlock(req, state),
        ("POST", "/api/vault/lock") => api_vault_lock(state),
        ("POST", "/api/vault/rotate-passphrase") => api_rotate_passphrase(req, state),
        ("GET", "/api/workspace") => api_workspace(state),
        ("POST", "/api/import") => api_import_file(req, state),
        ("POST", "/api/import-demo-pack") => api_import_demo_pack(state),
        ("POST", "/api/clear-demo-pack") => api_clear_demo_pack(state),
        ("POST", "/api/manual-entry") => api_manual_entry(req, state),
        ("POST", "/api/csv/promote") => api_csv_promote(req, state),
        ("POST", "/api/sources/save") => api_source_save(req, state),
        ("POST", "/api/sources/reimburse") => api_source_reimburse(req, state),
        ("POST", "/api/sources/delete") => api_source_delete(req, state),
        ("POST", "/api/facts/undo") => api_fact_undo(req, state),
        ("POST", "/api/sources/redact") => api_source_redact(req, state),
        ("GET", "/api/export/csv") => api_export_csv(state),
        ("GET", "/api/export/zip") => api_export_zip(req, state),
        ("POST", "/api/export/package") => api_export_package(req, state),
        ("POST", "/api/verify-package") => api_verify_package(req),
        ("POST", "/api/shutdown") => {
            if let Ok(mut guard) = state.active.lock() {
                *guard = None;
            }
            state.shutdown.store(true, Ordering::SeqCst);
            HttpResponse::json(200, json!({ "ok": true, "message": "已安全锁定并退出" }))
        }
        _ => HttpResponse::error(404, "not_found", "未知接口"),
    }
}

fn parse_json_body(req: &HttpRequest) -> Result<Value, HttpResponse> {
    serde_json::from_slice(&req.body)
        .map_err(|_| HttpResponse::error(400, "invalid_json", "请求数据格式不正确"))
}

fn with_active<F>(state: &ServerState, f: F) -> HttpResponse
where
    F: FnOnce(&ActiveVault) -> HttpResponse,
{
    let guard = match state.active.lock() {
        Ok(g) => g,
        Err(_) => return HttpResponse::error(500, "lock_poisoned", "内部状态异常"),
    };
    match guard.as_ref() {
        Some(active) => f(active),
        None => HttpResponse::error(423, "vault_locked", "请先解锁或创建一个本地账本"),
    }
}

// --- Recent Vaults & Default Path Helpers ---

fn default_vault_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile)
                .join("Documents")
                .join("SubVault")
                .join("我的订阅账本");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join("Documents")
            .join("SubVault")
            .join("my-vault");
    }
    PathBuf::from("SubVault-Vault")
}

fn recent_store_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Some(
                PathBuf::from(appdata)
                    .join("SubVault")
                    .join("recent_vaults.json"),
            );
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("subvault")
                .join("recent_vaults.json"),
        );
    }
    None
}

fn load_recent_vaults() -> Vec<String> {
    let Some(path) = recent_store_path() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

fn remember_recent_vault(vault_path: &Path) {
    let Some(store_path) = recent_store_path() else {
        return;
    };
    let canonical_str = vault_path.display().to_string();
    let mut list = load_recent_vaults();
    list.retain(|item| item != &canonical_str);
    list.insert(0, canonical_str);
    list.truncate(8);
    if let Some(parent) = store_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
        let _ = fs::write(&store_path, bytes);
    }
}

fn probe_vault_info(path: &Path) -> Value {
    let banner_path = path.join(PRODUCTION_BANNER_FILE);
    let exists = path.is_dir() && path.join("vault.db").is_file();
    let protection = if let Ok(text) = fs::read_to_string(&banner_path) {
        if text == PRODUCTION_BANNER_PASSPHRASE {
            Some("passphrase_aead_v1")
        } else if text == PRODUCTION_BANNER_OS_DISK {
            Some("os_disk_assumed")
        } else {
            None
        }
    } else {
        None
    };
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("本地账本")
        .to_string();
    json!({
        "name": name,
        "path": path.display().to_string(),
        "exists": exists,
        "protection": protection,
        "needs_passphrase": protection == Some("passphrase_aead_v1"),
    })
}

// --- API Handlers ---

fn api_pick_folder(req: &HttpRequest) -> HttpResponse {
    let body = parse_json_body(req).unwrap_or_else(|_| json!({}));
    let title = body["title"]
        .as_str()
        .unwrap_or("选择文件夹")
        .replace(['\'', '"', '`', '$', ';', '\r', '\n'], "");

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
             Add-Type -AssemblyName System.Windows.Forms; \
             $f = New-Object System.Windows.Forms.Form; $f.TopMost = $true; \
             $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = '{title}'; $d.ShowNewFolderButton = $true; \
             if ($d.ShowDialog($f) -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $d.SelectedPath }}"
        );
        match std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .output()
        {
            Ok(out) if out.status.success() => {
                let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if picked.is_empty() {
                    HttpResponse::json(200, json!({ "ok": true, "cancelled": true, "path": null }))
                } else {
                    HttpResponse::json(
                        200,
                        json!({ "ok": true, "cancelled": false, "path": picked }),
                    )
                }
            }
            _ => HttpResponse::error(
                400,
                "picker_unavailable",
                "无法调起系统文件夹选择框，请直接手动输入或粘贴路径",
            ),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let zenity = std::process::Command::new("zenity")
            .args(["--file-selection", "--directory", "--title", &title])
            .output();
        if let Ok(out) = zenity {
            if out.status.success() {
                let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return HttpResponse::json(
                    200,
                    json!({ "ok": true, "cancelled": picked.is_empty(), "path": if picked.is_empty() { Value::Null } else { json!(picked) } }),
                );
            }
            return HttpResponse::json(200, json!({ "ok": true, "cancelled": true, "path": null }));
        }
        let kdialog = std::process::Command::new("kdialog")
            .args(["--getexistingdirectory", ".", "--title", &title])
            .output();
        if let Ok(out) = kdialog {
            if out.status.success() {
                let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return HttpResponse::json(
                    200,
                    json!({ "ok": true, "cancelled": picked.is_empty(), "path": if picked.is_empty() { Value::Null } else { json!(picked) } }),
                );
            }
            return HttpResponse::json(200, json!({ "ok": true, "cancelled": true, "path": null }));
        }
        HttpResponse::error(
            400,
            "picker_unavailable",
            "当前桌面环境未安装文件夹选择器，请直接手动输入或粘贴路径",
        )
    }
}

fn api_import_demo_pack(state: &ServerState) -> HttpResponse {
    with_active(state, |active| {
        let tmp_dir = std::env::temp_dir().join(format!(
            "subvault-demo-{}",
            ids::random_id("dm_").unwrap_or_else(|_| "tmp".into())
        ));
        if let Err(err) = fs::create_dir_all(&tmp_dir) {
            return HttpResponse::error(500, "temp_failed", &err.to_string());
        }
        let batch_id = ids::random_id("b_").unwrap_or_else(|_| "b_demo".into());
        let auth = active.auth();

        // 0a. Scanned-receipt style image (unextracted, lets user try Quick Triage!)
        let tiny_png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
            0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let shot_path = tmp_dir.join("示例-待补录收据截图.png");
        let _ = fs::write(&shot_path, tiny_png);
        let _ = engine::import_file(engine::ImportRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_img".into()),
            batch_id: Some(batch_id.clone()),
            media_type: "image/png".into(),
            source: shot_path,
            display_name: Some("示例-待补录收据截图.png".into()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 0b. Alipay GBK CSV sample statement
        let alipay_str = "支付宝交易记录明细查询（示例）\n账号:[personal@example.com]\n------------------------------------\n交易时间,交易分类,交易对方,商品说明,收/支,金额,收/付款方式,交易状态\n2026-10-01 09:00:00,云服务,Apple,iCloud+ 2TB 连续包月,支出,68.00,招商银行,交易成功\n2026-10-06 14:20:00,影音娱乐,哔哩哔哩,大会员连续包月,支出,15.00,余额宝,交易成功\n2026-10-12 20:00:00,开发工具,JetBrains,All Products Pack 年付,支出,1980.00,信用卡,交易成功\n";
        let (gbk_bytes, _, _) = encoding_rs::GB18030.encode(alipay_str);
        let csv_path = tmp_dir.join("示例-支付宝账单明细.csv");
        let _ = fs::write(&csv_path, gbk_bytes.as_ref());
        let _ = engine::submit_csv(engine::CsvRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_4".into()),
            source: csv_path,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 1. Chinese electronic invoice PDF (WPS ¥98.00 yearly)
        let zh_stream = format!(
            "BT\n{} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nET",
            utf16be_pdf_hex("增值税电子普通发票（示例）"),
            utf16be_pdf_hex("销售方名称：金山办公软件股份有限公司"),
            utf16be_pdf_hex("商品名称：WPS超级会员年卡"),
            utf16be_pdf_hex("服务周期：2026年1月1日 至 2026年12月31日"),
            utf16be_pdf_hex("价税合计（小写）¥98.00"),
        );
        let zh_pdf = tmp_dir.join("示例-WPS年卡电子发票.pdf");
        let _ = fs::write(&zh_pdf, build_minimal_demo_pdf(zh_stream.as_bytes()));
        let _ = engine::submit_pdf(engine::PdfRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_1".into()),
            source: zh_pdf,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 2. Historical OpenAI HTML receipt ($18.00 Sep) + current Oct ($20.00, triggers +11% price drift!) + renamed duplicate copy
        let openai_sep_html = r#"<!doctype html>
<html>
<head><style>body{font-family:sans-serif;color:#111}.box{border:1px solid #e2e8f0;padding:16px}</style></head>
<body>
  <div class="box">
    <h2>OpenAI Receipt (Sample - Prior Month)</h2>
    <p>From: OpenAI, LLC</p>
    <p>Account email: work@example.com</p>
    <p>Date paid: September 5, 2026</p>
    <table>
      <tr><th>Plan</th><td>ChatGPT Plus (Monthly)</td></tr>
      <tr><th>Billing period</th><td>Sep 5, 2026 - Oct 5, 2026</td></tr>
      <tr><th>Amount paid</th><td>$18.00</td></tr>
    </table>
  </div>
</body>
</html>"#;
        let html_sep = tmp_dir.join("sample-openai-sep.html");
        let _ = fs::write(&html_sep, openai_sep_html.as_bytes());
        let _ = engine::submit_html(engine::HtmlRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_2a".into()),
            source: html_sep,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        let openai_html = r#"<!doctype html>
<html>
<head><style>body{font-family:sans-serif;color:#111}.box{border:1px solid #e2e8f0;padding:16px}</style></head>
<body>
  <div class="box">
    <h2>OpenAI Receipt (Sample)</h2>
    <p>From: OpenAI, LLC</p>
    <p>Account email: work@example.com</p>
    <p>Date paid: October 5, 2026</p>
    <table>
      <tr><th>Plan</th><td>ChatGPT Plus (Monthly)</td></tr>
      <tr><th>Billing period</th><td>Oct 5, 2026 - Nov 5, 2026</td></tr>
      <tr><th>Amount paid</th><td>$20.00</td></tr>
    </table>
  </div>
</body>
</html>"#;
        let html_1 = tmp_dir.join("sample-openai-receipt.html");
        let html_2 = tmp_dir.join("sample-openai-receipt-renamed-copy.html");
        let _ = fs::write(&html_1, openai_html.as_bytes());
        let _ = fs::write(&html_2, openai_html.as_bytes());
        let _ = engine::submit_html(engine::HtmlRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_2".into()),
            source: html_1,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });
        let _ = engine::submit_html(engine::HtmlRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_3".into()),
            source: html_2,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 3. GitHub Copilot (Work account, $10.00 monthly)
        let github_html = r#"<!doctype html>
<html>
<head><style>body{font-family:sans-serif}</style></head>
<body>
  <h2>GitHub Payment Receipt</h2>
  <p>From: GitHub, Inc.</p>
  <p>Account email: dev-work@company.example</p>
  <p>Date paid: 2026-11-08</p>
  <table>
    <tr><th>Plan</th><td>GitHub Copilot Individual (Monthly)</td></tr>
    <tr><th>Billing period</th><td>2026-11-08 through 2026-12-08</td></tr>
    <tr><th>Amount paid</th><td>$10.00</td></tr>
  </table>
</body>
</html>"#;
        let gh_path = tmp_dir.join("sample-github-copilot.html");
        let _ = fs::write(&gh_path, github_html.as_bytes());
        let _ = engine::submit_html(engine::HtmlRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_gh".into()),
            source: gh_path,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 4. Apple iCloud+ 2TB (Personal account, ¥68.00 monthly)
        let apple_html = r#"<!doctype html>
<html>
<head><style>body{font-family:sans-serif}</style></head>
<body>
  <h2>Apple 电子收据（示例）</h2>
  <p>收款方：Apple iCloud</p>
  <p>账号：个人</p>
  <p>账单日期：2026-11-12</p>
  <table>
    <tr><th>套餐</th><td>iCloud+ 2TB 云存储（连续包月）</td></tr>
    <tr><th>服务周期</th><td>2026年11月12日 至 2026年12月12日</td></tr>
    <tr><th>实付金额</th><td>¥68.00</td></tr>
  </table>
</body>
</html>"#;
        let apple_path = tmp_dir.join("示例-Apple-iCloud月账单.html");
        let _ = fs::write(&apple_path, apple_html.as_bytes());
        let _ = engine::submit_html(engine::HtmlRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_ap".into()),
            source: apple_path,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 5. Netflix Standard ($15.49 monthly)
        let nf_html = r#"<!doctype html>
<html>
<head><style>body{font-family:sans-serif}</style></head>
<body>
  <h2>Netflix Payment Confirmation</h2>
  <p>From: Netflix</p>
  <p>Date paid: 2026-11-18</p>
  <table>
    <tr><th>Subscription</th><td>Standard Monthly Plan</td></tr>
    <tr><th>Billing period</th><td>Nov 18, 2026 - Dec 18, 2026</td></tr>
    <tr><th>Amount paid</th><td>$15.49</td></tr>
  </table>
</body>
</html>"#;
        let nf_path = tmp_dir.join("sample-netflix-nov.html");
        let _ = fs::write(&nf_path, nf_html.as_bytes());
        let _ = engine::submit_html(engine::HtmlRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_nf".into()),
            source: nf_path,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 6. Hardware purchase invoice PDF (Apple MacBook Air ¥8,999 with warranty countdown + cost-per-day)
        let today_days = today_epoch_days();
        let hw_buy_date = format_epoch_days(today_days - 320);
        let hw_warranty_end = format_epoch_days(today_days + 45);
        let hw_stream = format!(
            "BT\n{} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nET",
            utf16be_pdf_hex("增值税电子普通发票（示例）"),
            utf16be_pdf_hex("销售方名称：苹果电子产品商贸有限公司"),
            utf16be_pdf_hex("商品名称：MacBook Air M4 16G+512G 午夜色"),
            utf16be_pdf_hex(&format!("开票日期：{hw_buy_date}")),
            utf16be_pdf_hex("价税合计（小写）¥8999.00"),
        );
        let hw_pdf = tmp_dir.join("示例-MacBook-Air硬件保修发票.pdf");
        let _ = fs::write(&hw_pdf, build_minimal_demo_pdf(hw_stream.as_bytes()));
        let hw_rep = engine::submit_pdf(engine::PdfRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_hw".into()),
            source: hw_pdf,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });
        if let Some(sid) = hw_rep.body["source_id"].as_str() {
            let _ = engine::annotate_source(engine::AnnotateRequest {
                vault: active.path.clone(),
                source_id: sid.to_string(),
                vendor: Some("Apple MacBook Air".into()),
                plan_name: Some("M4 16G+512G 午夜色".into()),
                account_label: Some("个人".into()),
                billing_cycle: Some("one_time".into()),
                category: Some("数码硬件".into()),
                bill_date: Some(hw_buy_date),
                notes: Some("官方商城一年有限保修，序列号原件已归档".into()),
                asset_kind: Some("hardware".into()),
                warranty_end: Some(hw_warranty_end),
                license_key: None,
                license_scope: None,
                reimburse_status: Some("none".into()),
                auth: auth.clone(),
            });
        }

        // 7. Perpetual software license (Raycast Pro $99.00 with AEAD-encrypted license key)
        let lic_html = r#"<!doctype html>
<html>
<head><style>body{font-family:sans-serif;color:#111}.card{border:1px solid #e2e8f0;padding:16px}</style></head>
<body>
  <div class="card">
    <h2>Raycast Store Receipt (Sample)</h2>
    <p>From: Raycast Technologies Ltd.</p>
    <p>Date paid: 2026-08-15</p>
    <table>
      <tr><th>Product</th><td>Raycast Pro Perpetual License (One-Time)</td></tr>
      <tr><th>Amount paid</th><td>$99.00</td></tr>
    </table>
  </div>
</body>
</html>"#;
        let lic_path = tmp_dir.join("sample-raycast-license.html");
        let _ = fs::write(&lic_path, lic_html.as_bytes());
        let lic_rep = engine::submit_html(engine::HtmlRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_lic".into()),
            source: lic_path,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });
        if let Some(sid) = lic_rep.body["source_id"].as_str() {
            let _ = engine::annotate_source(engine::AnnotateRequest {
                vault: active.path.clone(),
                source_id: sid.to_string(),
                vendor: Some("Raycast".into()),
                plan_name: Some("Pro 终身买断版".into()),
                account_label: Some("个人".into()),
                billing_cycle: Some("one_time".into()),
                category: Some("办公效率".into()),
                bill_date: Some("2026-08-15".into()),
                notes: Some("激活密钥已单独加密存放在本地密钥库".into()),
                asset_kind: Some("license".into()),
                warranty_end: None,
                license_key: Some("RCST-DEMO-8842-K9X2".into()),
                license_scope: Some("终身买断 · 最多激活 3 台设备".into()),
                reimburse_status: Some("none".into()),
                auth: auth.clone(),
            });
        }

        // 8. Company reimbursable invoice PDF (腾讯云 ¥420.00, pending reimbursement)
        let reimb_stream = format!(
            "BT\n{} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nT* {} Tj\nET",
            utf16be_pdf_hex("增值税电子普通发票（示例）"),
            utf16be_pdf_hex("销售方名称：腾讯云计算（北京）有限责任公司"),
            utf16be_pdf_hex("商品名称：研发测试环境云服务器资源包"),
            utf16be_pdf_hex("开票日期：2026-11-15"),
            utf16be_pdf_hex("价税合计（小写）¥420.00"),
        );
        let reimb_pdf = tmp_dir.join("示例-腾讯云企业报销发票.pdf");
        let _ = fs::write(&reimb_pdf, build_minimal_demo_pdf(reimb_stream.as_bytes()));
        let reimb_rep = engine::submit_pdf(engine::PdfRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_rb".into()),
            source: reimb_pdf,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });
        if let Some(sid) = reimb_rep.body["source_id"].as_str() {
            let _ = engine::annotate_source(engine::AnnotateRequest {
                vault: active.path.clone(),
                source_id: sid.to_string(),
                vendor: Some("腾讯云".into()),
                plan_name: Some("测试环境云服务器资源包".into()),
                account_label: Some("公司工作".into()),
                billing_cycle: Some("one_time".into()),
                category: Some("云服务".into()),
                bill_date: Some("2026-11-15".into()),
                notes: Some("Q4 研发测试垫付费用，待月底统一提交财务报销".into()),
                asset_kind: Some("reimbursable".into()),
                warranty_end: None,
                license_key: None,
                license_scope: None,
                reimburse_status: Some("pending".into()),
                auth: auth.clone(),
            });
        }

        let _ = fs::remove_dir_all(&tmp_dir);
        match build_workspace_snapshot(active) {
            Ok(snap) => HttpResponse::json(200, snap),
            Err(resp) => resp,
        }
    })
}

fn api_manual_entry(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let vendor = body["vendor"].as_str().unwrap_or("").trim();
    let plan_name = body["plan_name"].as_str().unwrap_or("").trim();
    let account_label = body["account_label"].as_str().unwrap_or("").trim();
    let billing_cycle = body["billing_cycle"].as_str().unwrap_or("monthly").trim();
    let category = body["category"].as_str().unwrap_or("").trim();
    let currency_raw = body["currency"].as_str().unwrap_or("CNY").trim();
    let amount_raw = body["amount_text"].as_str().unwrap_or("").trim();
    let bill_date = body["bill_date"].as_str().unwrap_or("").trim();
    let period_start = body["period_start"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let period_end = body["period_end"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let notes = body["notes"].as_str().unwrap_or("").trim();
    let asset_kind_raw = body["asset_kind"].as_str().unwrap_or("").trim();
    let asset_kind = if asset_kind_raw.is_empty() {
        if matches!(billing_cycle, "monthly" | "yearly" | "quarterly" | "weekly") {
            "subscription".to_string()
        } else {
            "general".to_string()
        }
    } else {
        asset_kind_raw.to_string()
    };
    let warranty_end = body
        .get("warranty_end")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let license_key = body
        .get("license_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let license_scope = body
        .get("license_scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let reimburse_status = body
        .get("reimburse_status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            if asset_kind == "reimbursable" {
                Some("pending".to_string())
            } else {
                Some("none".to_string())
            }
        });

    if vendor.is_empty() {
        return HttpResponse::error(400, "vendor_required", "请填写商家或服务名称");
    }
    let Some(norm_amt) = crate::real_extract::normalize_amount_text(amount_raw) else {
        return HttpResponse::error(400, "bad_amount", "请填写有效金额，例如 29.99 或 68");
    };
    let Some(norm_cur) = crate::real_extract::normalize_currency(currency_raw) else {
        return HttpResponse::error(400, "bad_currency", "不支持该币种");
    };

    with_active(state, |active| {
        let auth = active.auth();
        let entry_id = ids::random_id("man_").unwrap_or_else(|_| "man_1".into());
        let note_text = format!(
            "SubVault Self-Declared Entry (No Original Vendor File)\nEntry ID: {entry_id}\nVendor: {vendor}\nPlan: {plan_name}\nAccount: {account_label}\nBilling cycle: {billing_cycle}\nCategory: {category}\nCurrency: {norm_cur}\nAmount: {norm_amt}\nBill date: {bill_date}\nNotes: {notes}\nHonesty notice: Recorded directly by user without an attached vendor invoice.\n"
        );
        let display_name = sanitize_filename(&format!(
            "自录-{}-{}.txt",
            vendor,
            if bill_date.is_empty() {
                "记录"
            } else {
                bill_date
            }
        ));
        let tmp_dir = std::env::temp_dir().join(format!(
            "subvault-man-{}",
            ids::random_id("mn_").unwrap_or_else(|_| "tmp".into())
        ));
        if let Err(err) = fs::create_dir_all(&tmp_dir) {
            return HttpResponse::error(500, "temp_failed", &err.to_string());
        }
        let temp_file = tmp_dir.join(&display_name);
        if let Err(err) = fs::write(&temp_file, note_text.as_bytes()) {
            let _ = fs::remove_dir_all(&tmp_dir);
            return HttpResponse::error(500, "temp_write_failed", &err.to_string());
        }
        let imp = engine::import_file(engine::ImportRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("me_").unwrap_or_else(|_| "me_1".into()),
            batch_id: None,
            media_type: "text/plain;charset=utf-8".into(),
            source: temp_file,
            display_name: Some(display_name),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });
        let _ = fs::remove_dir_all(&tmp_dir);
        if imp.code != EXIT_OK {
            return map_engine_error(&imp);
        }
        let source_id = imp.body["source_id"].as_str().unwrap_or("").to_string();

        let rec = engine::record_manual_fact(engine::ManualFactRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("mf_").unwrap_or_else(|_| "mf_1".into()),
            source_id: source_id.clone(),
            currency: Some(norm_cur.to_string()),
            amount_text: Some(norm_amt),
            period_start,
            period_end,
            actor_id: WORKBENCH_ACTOR_ID.into(),
            auth: auth.clone(),
        });
        if rec.code != EXIT_OK {
            return map_engine_error(&rec);
        }

        let ann = engine::annotate_source(engine::AnnotateRequest {
            vault: active.path.clone(),
            source_id: source_id.clone(),
            vendor: Some(vendor.to_string()),
            plan_name: if plan_name.is_empty() {
                None
            } else {
                Some(plan_name.to_string())
            },
            account_label: if account_label.is_empty() {
                None
            } else {
                Some(account_label.to_string())
            },
            billing_cycle: Some(billing_cycle.to_string()),
            category: if category.is_empty() {
                None
            } else {
                Some(category.to_string())
            },
            bill_date: if bill_date.is_empty() {
                None
            } else {
                Some(bill_date.to_string())
            },
            notes: if notes.is_empty() {
                Some("用户手动直录（暂无商家原件）".into())
            } else {
                Some(notes.to_string())
            },
            asset_kind: Some(asset_kind),
            warranty_end,
            license_key,
            license_scope,
            reimburse_status,
            auth,
        });
        if ann.code != EXIT_OK {
            return map_engine_error(&ann);
        }

        match build_workspace_snapshot(active) {
            Ok(mut snap) => {
                if let Some(obj) = snap.as_object_mut() {
                    obj.insert("created_source_id".into(), json!(source_id));
                }
                HttpResponse::json(200, snap)
            }
            Err(resp) => resp,
        }
    })
}

fn utf16be_pdf_hex(text: &str) -> String {
    let mut hex = String::from("<FEFF");
    for unit in text.encode_utf16() {
        hex.push_str(&format!("{unit:04X}"));
    }
    hex.push('>');
    hex
}

fn build_minimal_demo_pdf(stream: &[u8]) -> Vec<u8> {
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

fn api_bootstrap(state: &ServerState) -> HttpResponse {
    let mut recent: Vec<Value> = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(ref init_p) = state.initial_vault {
        let s = init_p.display().to_string();
        if seen.insert(s) {
            recent.push(probe_vault_info(init_p));
        }
    }
    for raw in load_recent_vaults() {
        if seen.insert(raw.clone()) {
            let info = probe_vault_info(Path::new(&raw));
            if info["exists"] == json!(true) {
                recent.push(info);
            }
        }
    }
    let default_path = state
        .initial_vault
        .clone()
        .unwrap_or_else(default_vault_path);

    let active_info = state.active.lock().ok().and_then(|guard| {
        guard.as_ref().map(|a| {
            json!({
                "path": a.path.display().to_string(),
                "vault_id": a.vault_id,
                "vault_format": a.vault_format,
                "protection": a.protection.map(ProtectionMode::as_meta),
            })
        })
    });

    HttpResponse::json(
        200,
        json!({
            "ok": true,
            "version": env!("CARGO_PKG_VERSION"),
            "default_vault_path": default_path.display().to_string(),
            "recent_vaults": recent,
            "active_vault": active_info,
            "currencies": crate::real_extract::SUPPORTED_CURRENCIES,
        }),
    )
}

fn api_vault_init(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let path_str = body["path"].as_str().unwrap_or("").trim();
    if path_str.is_empty() {
        return HttpResponse::error(400, "path_required", "请填写账本保存路径");
    }
    let prot_str = body["protection"].as_str().unwrap_or("passphrase");
    let Some(protection) = ProtectionMode::from_flag(prot_str) else {
        return HttpResponse::error(400, "bad_protection", "保护模式只能选密码加密或系统盘模式");
    };
    let passphrase = body["passphrase"].as_str().unwrap_or("").to_string();
    if protection == ProtectionMode::Passphrase && passphrase.chars().count() < 6 {
        return HttpResponse::error(
            400,
            "weak_passphrase",
            "为保护您的真实账单，密码至少需要 6 位字符",
        );
    }

    let vault_path = PathBuf::from(path_str);
    if let Some(parent) = vault_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if let Err(err) = fs::create_dir_all(parent) {
                return HttpResponse::error(
                    400,
                    "parent_create_failed",
                    &format!("无法创建上级文件夹：{err}"),
                );
            }
        }
    }

    let auth = if protection == ProtectionMode::Passphrase {
        AuthInput::from_direct_passphrase(passphrase.clone())
    } else {
        AuthInput::default()
    };

    let init_report = engine::init_vault(engine::InitRequest {
        vault: vault_path.clone(),
        format: 6,
        protection: Some(protection),
        auth: auth.clone(),
        recovery_key_out: None,
        hooks: Hooks::default(),
    });
    if init_report.code != EXIT_OK {
        return map_engine_error(&init_report);
    }

    let unlocked = match engine::unlock_vault_session(&vault_path, &auth) {
        Ok(u) => u,
        Err(fail) => return map_engine_error(&fail.into_report()),
    };

    if let Ok(mut guard) = state.active.lock() {
        *guard = Some(ActiveVault {
            path: vault_path.clone(),
            vault_id: unlocked.vault_id.clone(),
            vault_format: unlocked.vault_format,
            protection: unlocked.protection,
            cipher: unlocked.cipher,
        });
    }
    remember_recent_vault(&vault_path);

    HttpResponse::json(
        200,
        json!({
            "ok": true,
            "vault_id": unlocked.vault_id,
            "path": vault_path.display().to_string(),
            "protection": protection.as_meta(),
            "recovery_key": init_report.body.get("recovery_key").cloned().unwrap_or(Value::Null),
        }),
    )
}

fn api_vault_unlock(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let path_str = body["path"].as_str().unwrap_or("").trim();
    if path_str.is_empty() {
        return HttpResponse::error(400, "path_required", "请填写要打开的账本路径");
    }
    let vault_path = PathBuf::from(path_str);
    let pass = body["passphrase"]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    let recovery = body["recovery_key"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let auth = if let Some(r) = recovery {
        AuthInput::from_direct_recovery_key(r)
    } else if let Some(p) = pass {
        AuthInput::from_direct_passphrase(p)
    } else {
        AuthInput::default()
    };

    let unlocked = match engine::unlock_vault_session(&vault_path, &auth) {
        Ok(u) => u,
        Err(fail) => return map_engine_error(&fail.into_report()),
    };

    if let Ok(mut guard) = state.active.lock() {
        *guard = Some(ActiveVault {
            path: vault_path.clone(),
            vault_id: unlocked.vault_id.clone(),
            vault_format: unlocked.vault_format,
            protection: unlocked.protection,
            cipher: unlocked.cipher,
        });
    }
    remember_recent_vault(&vault_path);

    api_workspace(state)
}

fn api_vault_lock(state: &ServerState) -> HttpResponse {
    if let Ok(mut guard) = state.active.lock() {
        *guard = None;
    }
    HttpResponse::json(200, json!({ "ok": true, "locked": true }))
}

fn api_rotate_passphrase(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let new_pass = body["new_passphrase"].as_str().unwrap_or("").to_string();
    if new_pass.chars().count() < 6 {
        return HttpResponse::error(400, "weak_passphrase", "新密码至少需要 6 位字符");
    }
    let rotate_recovery = body["rotate_recovery_key"].as_bool().unwrap_or(false);
    let current_pass = body["current_passphrase"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let recovery_key = body["recovery_key"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    with_active(state, |active| {
        let auth = if let Some(r) = recovery_key {
            AuthInput::from_direct_recovery_key(r)
        } else if let Some(p) = current_pass {
            AuthInput::from_direct_passphrase(p)
        } else {
            active.auth()
        };
        let rep = engine::rotate_passphrase(engine::RotatePassphraseRequest {
            vault: active.path.clone(),
            auth,
            new_passphrase: NewPassphraseInput::from_direct(new_pass),
            rotate_recovery_key: rotate_recovery,
            recovery_key_out: None,
        });
        if rep.code != EXIT_OK {
            return map_engine_error(&rep);
        }
        HttpResponse::json(
            200,
            json!({
                "ok": true,
                "recovery_key_rotated": rotate_recovery,
                "recovery_key": rep.body.get("recovery_key").cloned().unwrap_or(Value::Null),
            }),
        )
    })
}

fn api_workspace(state: &ServerState) -> HttpResponse {
    with_active(state, |active| match build_workspace_snapshot(active) {
        Ok(snapshot) => HttpResponse::json(200, snapshot),
        Err(resp) => resp,
    })
}

fn build_workspace_snapshot(active: &ActiveVault) -> Result<Value, HttpResponse> {
    let auth = active.auth();
    let open_rep = engine::open_vault(&active.path, auth.clone());
    if open_rep.code != EXIT_OK && open_rep.code != 3 {
        return Err(map_engine_error(&open_rep));
    }
    let facts_rep = engine::query_facts(&active.path, None, None, auth.clone());
    let cands_rep = engine::query_candidates(&active.path, None, auth.clone());
    let annotations = engine::list_annotations(&active.path, &auth)
        .map_err(|f| map_engine_error(&f.into_report()))?;

    let ann_map: BTreeMap<String, crate::store::SourceAnnotation> = annotations
        .into_iter()
        .map(|a| (a.source_id.clone(), a))
        .collect();

    let sources = open_rep.body["sources"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let occurrences = open_rep.body["occurrences"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let facts = facts_rep.body["facts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let candidates = cands_rep.body["candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut occ_by_source: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for occ in occurrences {
        if let Some(sid) = occ["source_id"].as_str() {
            occ_by_source.entry(sid.to_string()).or_default().push(occ);
        }
    }

    let mut facts_by_source: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for fact in facts {
        if let Some(sid) = fact["source_id"].as_str() {
            facts_by_source
                .entry(sid.to_string())
                .or_default()
                .push(fact);
        }
    }

    let mut cands_by_source: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for cand in candidates {
        if let Some(sid) = cand["source_id"].as_str() {
            cands_by_source
                .entry(sid.to_string())
                .or_default()
                .push(cand);
        }
    }

    // Per-currency aggregation in integer cents (zero float rounding drift!)
    #[derive(Default)]
    struct CurrencyAcc {
        total_cents: i64,
        monthly_equiv_cents: i64,
        yearly_equiv_cents: i64,
        recurring_count: usize,
        one_time_count: usize,
        bill_count: usize,
    }
    let mut by_currency: BTreeMap<String, CurrencyAcc> = BTreeMap::new();
    let mut cat_by_currency: BTreeMap<String, BTreeMap<String, (i64, usize)>> = BTreeMap::new();
    let mut acc_by_currency: BTreeMap<String, BTreeMap<String, (i64, usize)>> = BTreeMap::new();
    let mut month_by_currency: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    let mut account_set: BTreeSet<String> = BTreeSet::new();
    let mut category_set: BTreeSet<String> = BTreeSet::new();

    #[derive(Default)]
    struct ReimburseAcc {
        pending_cents: i64,
        pending_count: usize,
        submitted_cents: i64,
        submitted_count: usize,
        reimbursed_cents: i64,
        reimbursed_count: usize,
    }
    let mut reimburse_by_currency: BTreeMap<String, ReimburseAcc> = BTreeMap::new();

    struct VendorBillPoint {
        source_id: String,
        date: String,
        recorded_at: i64,
        cents: i64,
        amount_text: String,
        billing_cycle: String,
        account_label: Option<String>,
        asset_kind: String,
        vendor_display: String,
    }
    let mut vendor_groups: BTreeMap<(String, String), Vec<VendorBillPoint>> = BTreeMap::new();

    let mut asset_subscription = 0usize;
    let mut asset_license = 0usize;
    let mut asset_hardware = 0usize;
    let mut asset_reimbursable = 0usize;
    let mut asset_general = 0usize;
    let today_days = today_epoch_days();
    let mut radar_warranties: Vec<Value> = Vec::new();

    let mut items = Vec::new();
    let mut duplicate_count = 0usize;
    let mut conflict_count = 0usize;
    let mut needs_entry_count = 0usize;
    let mut csv_statement_count = 0usize;
    let mut demo_count = 0usize;

    for src in &sources {
        let source_id = src["source_id"].as_str().unwrap_or("").to_string();
        let src_occs = occ_by_source.get(&source_id).cloned().unwrap_or_default();
        let display_name = src_occs
            .first()
            .and_then(|o| o["display_name"].as_str())
            .unwrap_or("未命名账单")
            .to_string();
        let latest_recorded_at = src_occs
            .iter()
            .filter_map(|o| o["recorded_at"].as_i64())
            .max()
            .unwrap_or(0);
        let duplicate_hits = src_occs
            .iter()
            .filter(|o| o["storage_reused"] == json!(true))
            .count();
        if duplicate_hits > 0 {
            duplicate_count += 1;
        }

        let src_facts = facts_by_source.get(&source_id).cloned().unwrap_or_default();
        let amount_fact = src_facts
            .iter()
            .find(|f| f["fact_type"] == "stated_amount_paid")
            .cloned();
        let period_fact = src_facts
            .iter()
            .find(|f| f["fact_type"] == "stated_service_period")
            .cloned();

        let src_cands = cands_by_source.get(&source_id).cloned().unwrap_or_default();
        let csv_rows: Vec<Value> = src_cands
            .iter()
            .filter(|c| c["kind"] == "csv_transaction")
            .cloned()
            .collect();
        let is_csv_statement = !csv_rows.is_empty();
        if is_csv_statement {
            csv_statement_count += 1;
        }

        let has_conflict = src_cands.iter().any(|c| {
            matches!(
                c["value"]["reason"].as_str(),
                Some("conflicting_amounts" | "conflicting_periods" | "conflicting_candidates")
            )
        });
        let has_credit_or_order_warning = src_cands.iter().any(|c| {
            matches!(
                c["kind"].as_str(),
                Some("credit_note_total" | "unjudged_order_total" | "unresolved_total")
            )
        });

        let active_amount = amount_fact.as_ref().and_then(|f| {
            let amt = f["value"]["amount_text"].as_str()?;
            let cur = f["value"]["currency"].as_str().unwrap_or("USD");
            Some((cur.to_string(), amt.to_string()))
        });
        let needs_manual_entry =
            active_amount.is_none() && !is_csv_statement && !has_credit_or_order_warning;
        if has_conflict {
            conflict_count += 1;
        }
        if needs_manual_entry {
            needs_entry_count += 1;
        }

        let ann = ann_map.get(&source_id);
        let cycle = ann
            .map(|a| a.billing_cycle.as_str())
            .unwrap_or("unknown")
            .to_string();
        let raw_asset_kind = ann.map(|a| a.asset_kind.as_str()).unwrap_or("general");
        let is_recurring_cycle = matches!(
            cycle.as_str(),
            "monthly" | "yearly" | "quarterly" | "weekly"
        );
        let effective_asset_kind = if raw_asset_kind == "general" && is_recurring_cycle {
            "subscription"
        } else {
            raw_asset_kind
        };
        let reimburse_status = ann.map(|a| a.reimburse_status.as_str()).unwrap_or("none");

        match effective_asset_kind {
            "subscription" => asset_subscription += 1,
            "license" => asset_license += 1,
            "hardware" => asset_hardware += 1,
            "reimbursable" => asset_reimbursable += 1,
            _ => asset_general += 1,
        }
        if effective_asset_kind != "reimbursable" && reimburse_status != "none" {
            asset_reimbursable += 1;
        }

        if let Some(we) = ann.and_then(|a| a.warranty_end.as_deref()) {
            if let Some(we_days) = parse_iso_days(we) {
                let days_left = we_days - today_days;
                if (0..=60).contains(&days_left) {
                    let v_name = ann
                        .and_then(|a| a.vendor.as_deref())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&display_name);
                    radar_warranties.push(json!({
                        "kind": "warranty_expiring",
                        "severity": if days_left <= 14 { "danger" } else { "amber" },
                        "vendor": v_name,
                        "warranty_end": we,
                        "days_left": days_left,
                        "source_id": source_id,
                        "label": format!("{v_name} 保修还剩 {days_left} 天"),
                        "detail": format!("保修截止 {we}"),
                    }));
                }
            }
        }

        if let Some(acc) = ann.and_then(|a| a.account_label.as_ref()) {
            if !acc.is_empty() {
                account_set.insert(acc.clone());
            }
        }
        if let Some(cat) = ann.and_then(|a| a.category.as_ref()) {
            if !cat.is_empty() {
                category_set.insert(cat.clone());
            }
        }

        if let Some((ref cur, ref amt_text)) = active_amount {
            if let Some(cents) = amount_to_cents(amt_text) {
                let acc = by_currency.entry(cur.clone()).or_default();
                acc.total_cents += cents;
                acc.bill_count += 1;
                match cycle.as_str() {
                    "monthly" => {
                        acc.monthly_equiv_cents += cents;
                        acc.yearly_equiv_cents += cents * 12;
                        acc.recurring_count += 1;
                    }
                    "yearly" => {
                        acc.monthly_equiv_cents += (cents + 6) / 12;
                        acc.yearly_equiv_cents += cents;
                        acc.recurring_count += 1;
                    }
                    "quarterly" => {
                        acc.monthly_equiv_cents += (cents + 1) / 3;
                        acc.yearly_equiv_cents += cents * 4;
                        acc.recurring_count += 1;
                    }
                    "weekly" => {
                        acc.monthly_equiv_cents += (cents * 52 + 6) / 12;
                        acc.yearly_equiv_cents += cents * 52;
                        acc.recurring_count += 1;
                    }
                    "one_time" => {
                        acc.one_time_count += 1;
                    }
                    _ => {}
                }

                // Category breakdown (exact integer cents)
                let cat_label = ann
                    .and_then(|a| a.category.as_deref())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("未分类")
                    .to_string();
                let cat_entry = cat_by_currency
                    .entry(cur.clone())
                    .or_default()
                    .entry(cat_label)
                    .or_insert((0, 0));
                cat_entry.0 += cents;
                cat_entry.1 += 1;

                // Account breakdown (exact integer cents)
                let acc_label = ann
                    .and_then(|a| a.account_label.as_deref())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("未分组")
                    .to_string();
                let acc_entry = acc_by_currency
                    .entry(cur.clone())
                    .or_default()
                    .entry(acc_label)
                    .or_insert((0, 0));
                acc_entry.0 += cents;
                acc_entry.1 += 1;

                // Monthly spend rhythm (YYYY-MM from bill_date or period_start)
                let ym = ann
                    .and_then(|a| a.bill_date.as_deref())
                    .or_else(|| {
                        period_fact
                            .as_ref()
                            .and_then(|p| p["value"]["period_start"].as_str())
                    })
                    .and_then(|d| d.get(0..7))
                    .map(str::to_string);
                if let Some(month_key) = ym {
                    *month_by_currency
                        .entry(cur.clone())
                        .or_default()
                        .entry(month_key)
                        .or_insert(0) += cents;
                }

                // Reimbursement tracking per currency
                if effective_asset_kind == "reimbursable" || reimburse_status != "none" {
                    let racc = reimburse_by_currency.entry(cur.clone()).or_default();
                    match reimburse_status {
                        "submitted" => {
                            racc.submitted_cents += cents;
                            racc.submitted_count += 1;
                        }
                        "reimbursed" => {
                            racc.reimbursed_cents += cents;
                            racc.reimbursed_count += 1;
                        }
                        _ => {
                            racc.pending_cents += cents;
                            racc.pending_count += 1;
                        }
                    }
                }

                // Vendor asset portfolio tracking
                if let Some(v_name) = ann
                    .and_then(|a| a.vendor.as_deref())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let date_key = ann
                        .and_then(|a| a.bill_date.as_deref())
                        .or_else(|| {
                            period_fact
                                .as_ref()
                                .and_then(|p| p["value"]["period_start"].as_str())
                        })
                        .unwrap_or("")
                        .to_string();
                    vendor_groups
                        .entry((v_name.to_lowercase(), cur.clone()))
                        .or_default()
                        .push(VendorBillPoint {
                            source_id: source_id.clone(),
                            date: date_key,
                            recorded_at: latest_recorded_at,
                            cents,
                            amount_text: amt_text.clone(),
                            billing_cycle: cycle.clone(),
                            account_label: ann.and_then(|a| a.account_label.clone()),
                            asset_kind: effective_asset_kind.to_string(),
                            vendor_display: v_name.to_string(),
                        });
                }
            }
        }

        let annotation_json = match ann {
            Some(a) => json!({
                "vendor": a.vendor,
                "plan_name": a.plan_name,
                "account_label": a.account_label,
                "billing_cycle": a.billing_cycle,
                "category": a.category,
                "bill_date": a.bill_date,
                "notes": a.notes,
                "asset_kind": effective_asset_kind,
                "warranty_end": a.warranty_end,
                "license_key": a.license_key,
                "license_scope": a.license_scope,
                "reimburse_status": a.reimburse_status,
                "user_edited": a.user_edited,
                "updated_at": a.updated_at,
            }),
            None => json!({
                "vendor": Value::Null,
                "plan_name": Value::Null,
                "account_label": Value::Null,
                "billing_cycle": "unknown",
                "category": Value::Null,
                "bill_date": Value::Null,
                "notes": Value::Null,
                "asset_kind": effective_asset_kind,
                "warranty_end": Value::Null,
                "license_key": Value::Null,
                "license_scope": Value::Null,
                "reimburse_status": "none",
                "user_edited": false,
                "updated_at": 0,
            }),
        };

        let is_demo = is_demo_source(&display_name, &src_occs);
        if is_demo {
            demo_count += 1;
        }

        items.push(json!({
            "source_id": source_id,
            "object_id": src["object_id"],
            "content_sha256": src["content_sha256"],
            "byte_length": src["byte_length"],
            "media_type": src["media_type"],
            "object_intact": src["object_intact"],
            "display_name": display_name,
            "is_demo": is_demo,
            "latest_recorded_at": latest_recorded_at,
            "occurrence_count": src_occs.len(),
            "duplicate_hits": duplicate_hits,
            "occurrences": src_occs,
            "annotation": annotation_json,
            "amount_fact": amount_fact,
            "period_fact": period_fact,
            "candidates": src_cands,
            "csv_row_count": csv_rows.len(),
            "status_flags": {
                "has_duplicate": duplicate_hits > 0,
                "has_conflict": has_conflict,
                "needs_manual_entry": needs_manual_entry,
                "is_csv_statement": is_csv_statement,
                "is_credit_or_order_warning": has_credit_or_order_warning,
            }
        }));
    }

    // Aggregate vendor portfolios + detect recurring price drift across chronological bills
    let mut drift_by_source: BTreeMap<String, Value> = BTreeMap::new();
    let mut radar_hikes: Vec<Value> = Vec::new();
    let mut vendor_portfolio_raw: Vec<(String, i64, Value)> = Vec::new();

    for ((_, cur), mut pts) in vendor_groups {
        pts.sort_by(|a, b| a.date.cmp(&b.date).then(a.recorded_at.cmp(&b.recorded_at)));
        let total_cents: i64 = pts.iter().map(|p| p.cents).sum();
        let bill_count = pts.len();
        let vendor_name = pts
            .last()
            .map(|p| p.vendor_display.clone())
            .unwrap_or_default();
        let latest_asset_kind = pts
            .last()
            .map(|p| p.asset_kind.clone())
            .unwrap_or_else(|| "general".into());
        let latest_amount = pts
            .last()
            .map(|p| p.amount_text.clone())
            .unwrap_or_default();
        let latest_date = pts.last().map(|p| p.date.clone()).unwrap_or_default();
        let accounts: Vec<String> = pts
            .iter()
            .filter_map(|p| p.account_label.as_deref())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        // Detect price drift across recurring bills with the same billing_cycle
        let recurring_pts: Vec<&VendorBillPoint> = pts
            .iter()
            .filter(|p| {
                matches!(
                    p.billing_cycle.as_str(),
                    "monthly" | "yearly" | "quarterly" | "weekly"
                )
            })
            .collect();
        let mut price_drift = Value::Null;
        if recurring_pts.len() >= 2 {
            let curr = recurring_pts[recurring_pts.len() - 1];
            if let Some(prev) = recurring_pts[..recurring_pts.len() - 1]
                .iter()
                .rev()
                .find(|p| p.billing_cycle == curr.billing_cycle && p.cents > 0)
            {
                if curr.cents != prev.cents {
                    let diff = curr.cents - prev.cents;
                    let direction = if diff > 0 { "up" } else { "down" };
                    let pct_abs = ((diff.abs() * 100) + (prev.cents / 2)) / prev.cents;
                    let pct_signed = if diff > 0 { pct_abs } else { -pct_abs };
                    let drift_obj = json!({
                        "direction": direction,
                        "prev_text": prev.amount_text,
                        "curr_text": curr.amount_text,
                        "diff_text": cents_to_text(diff),
                        "pct_change": pct_signed,
                        "currency": cur,
                        "prev_date": prev.date,
                        "curr_date": curr.date,
                    });
                    drift_by_source.insert(curr.source_id.clone(), drift_obj.clone());
                    if direction == "up" {
                        radar_hikes.push(json!({
                            "kind": "price_hike",
                            "severity": "amber",
                            "vendor": vendor_name,
                            "currency": cur,
                            "pct_change": pct_signed,
                            "source_id": curr.source_id,
                            "label": format!("{vendor_name} 涨价 +{pct_abs}%"),
                            "detail": format!("{} {} → {} {}", cur, prev.amount_text, cur, curr.amount_text),
                        }));
                    }
                    price_drift = drift_obj;
                }
            }
        }

        let history: Vec<Value> = pts
            .iter()
            .map(|p| {
                json!({
                    "source_id": p.source_id,
                    "date": p.date,
                    "amount_text": p.amount_text,
                    "cents": p.cents,
                })
            })
            .collect();

        vendor_portfolio_raw.push((
            cur.clone(),
            total_cents,
            json!({
                "vendor": vendor_name,
                "currency": cur,
                "asset_kind": latest_asset_kind,
                "total_cents": total_cents,
                "total_text": cents_to_text(total_cents),
                "bill_count": bill_count,
                "latest_amount_text": latest_amount,
                "latest_date": latest_date,
                "accounts": accounts,
                "price_drift": price_drift,
                "history": history,
            }),
        ));
    }
    vendor_portfolio_raw.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let vendor_portfolio: Vec<Value> = vendor_portfolio_raw
        .into_iter()
        .map(|(_, _, v)| v)
        .collect();

    // Attach detected price_drift directly onto the latest bill item for that vendor
    for item in &mut items {
        if let Some(sid) = item["source_id"].as_str() {
            if let Some(drift) = drift_by_source.get(sid) {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("price_drift".into(), drift.clone());
                }
            }
        }
    }

    let mut radar_reimburse: Vec<Value> = Vec::new();
    let reimburse_summary: Vec<Value> = reimburse_by_currency
        .into_iter()
        .map(|(cur, racc)| {
            let pending_text = cents_to_text(racc.pending_cents);
            let submitted_text = cents_to_text(racc.submitted_cents);
            let reimbursed_text = cents_to_text(racc.reimbursed_cents);
            if racc.pending_count > 0 {
                radar_reimburse.push(json!({
                    "kind": "reimburse_pending",
                    "severity": "emerald",
                    "currency": cur,
                    "pending_text": pending_text,
                    "pending_count": racc.pending_count,
                    "label": format!("待报销 {cur} {pending_text}（{} 笔）", racc.pending_count),
                    "detail": "点击查看待报销凭证并一键流转状态",
                }));
            } else if racc.submitted_count > 0 {
                radar_reimburse.push(json!({
                    "kind": "reimburse_submitted",
                    "severity": "slate",
                    "currency": cur,
                    "submitted_text": submitted_text,
                    "submitted_count": racc.submitted_count,
                    "label": format!("报销审批中 {cur} {submitted_text}（{} 笔）", racc.submitted_count),
                    "detail": "点击跟踪已提交的报销单",
                }));
            }
            json!({
                "currency": cur,
                "pending_cents": racc.pending_cents,
                "pending_text": pending_text,
                "pending_count": racc.pending_count,
                "submitted_cents": racc.submitted_cents,
                "submitted_text": submitted_text,
                "submitted_count": racc.submitted_count,
                "reimbursed_cents": racc.reimbursed_cents,
                "reimbursed_text": reimbursed_text,
                "reimbursed_count": racc.reimbursed_count,
            })
        })
        .collect();

    let mut asset_radar: Vec<Value> = Vec::new();
    asset_radar.extend(radar_hikes);
    asset_radar.extend(radar_warranties);
    asset_radar.extend(radar_reimburse);

    // Sort newest first so freshly dropped bills appear immediately at the top
    items.sort_by(|a, b| {
        b["latest_recorded_at"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["latest_recorded_at"].as_i64().unwrap_or(0))
    });

    let summary_by_currency: Vec<Value> = by_currency
        .into_iter()
        .map(|(cur, acc)| {
            json!({
                "currency": cur,
                "total_recorded": cents_to_text(acc.total_cents),
                "monthly_equiv_recurring": cents_to_text(acc.monthly_equiv_cents),
                "yearly_equiv_recurring": cents_to_text(acc.yearly_equiv_cents),
                "recurring_count": acc.recurring_count,
                "one_time_count": acc.one_time_count,
                "bill_count": acc.bill_count,
            })
        })
        .collect();

    let categories_json: serde_json::Map<String, Value> = cat_by_currency
        .into_iter()
        .map(|(cur, map)| {
            let mut rows: Vec<(String, i64, usize)> = map
                .into_iter()
                .map(|(k, (cents, cnt))| (k, cents, cnt))
                .collect();
            rows.sort_by_key(|a| std::cmp::Reverse(a.1));
            (
                cur,
                Value::Array(
                    rows.into_iter()
                        .map(|(name, cents, count)| {
                            json!({
                                "name": name,
                                "cents": cents,
                                "amount_text": cents_to_text(cents),
                                "count": count
                            })
                        })
                        .collect(),
                ),
            )
        })
        .collect();

    let accounts_json: serde_json::Map<String, Value> = acc_by_currency
        .into_iter()
        .map(|(cur, map)| {
            let mut rows: Vec<(String, i64, usize)> = map
                .into_iter()
                .map(|(k, (cents, cnt))| (k, cents, cnt))
                .collect();
            rows.sort_by_key(|a| std::cmp::Reverse(a.1));
            (
                cur,
                Value::Array(
                    rows.into_iter()
                        .map(|(name, cents, count)| {
                            json!({
                                "name": name,
                                "cents": cents,
                                "amount_text": cents_to_text(cents),
                                "count": count
                            })
                        })
                        .collect(),
                ),
            )
        })
        .collect();

    let monthly_json: serde_json::Map<String, Value> = month_by_currency
        .into_iter()
        .map(|(cur, map)| {
            (
                cur,
                Value::Array(
                    map.into_iter()
                        .map(|(month, cents)| {
                            json!({
                                "month": month,
                                "cents": cents,
                                "amount_text": cents_to_text(cents)
                            })
                        })
                        .collect(),
                ),
            )
        })
        .collect();

    Ok(json!({
        "ok": true,
        "vault": {
            "path": active.path.display().to_string(),
            "name": active.path.file_name().and_then(|n| n.to_str()).unwrap_or("我的账本"),
            "vault_id": active.vault_id,
            "vault_format": active.vault_format,
            "protection": active.protection.map(ProtectionMode::as_meta),
            "encrypted": active.protection == Some(ProtectionMode::Passphrase),
            "source_count": sources.len(),
            "duplicate_count": duplicate_count,
            "conflict_count": conflict_count,
            "needs_entry_count": needs_entry_count,
            "csv_statement_count": csv_statement_count,
            "demo_count": demo_count,
            "alerts": open_rep.body["alerts"],
        },
        "summary_by_currency": summary_by_currency,
        "asset_counts": {
            "subscription": asset_subscription,
            "license": asset_license,
            "hardware": asset_hardware,
            "reimbursable": asset_reimbursable,
            "general": asset_general,
        },
        "reimburse_summary": reimburse_summary,
        "vendor_portfolio": vendor_portfolio,
        "asset_radar": asset_radar,
        "analytics": {
            "categories": Value::Object(categories_json),
            "accounts": Value::Object(accounts_json),
            "monthly_trend": Value::Object(monthly_json),
        },
        "accounts": account_set.into_iter().collect::<Vec<_>>(),
        "categories": category_set.into_iter().collect::<Vec<_>>(),
        "items": items,
    }))
}

fn api_import_file(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let raw_name = body["filename"].as_str().unwrap_or("bill.bin").trim();
    let filename = sanitize_filename(raw_name);
    let content_b64 = body["content_base64"].as_str().unwrap_or("");
    let bytes = match B64.decode(content_b64) {
        Ok(b) => b,
        Err(_) => return HttpResponse::error(400, "bad_base64", "文件内容解码失败"),
    };
    if bytes.is_empty() {
        return HttpResponse::error(400, "empty_file", "空文件无法导入");
    }
    if bytes.len() > 8 * 1024 * 1024 {
        return HttpResponse::error(413, "file_too_large", "单个文件不能超过 8 MiB");
    }
    let batch_id = body["batch_id"]
        .as_str()
        .filter(|s| ids::is_command_id(s))
        .map(str::to_string);

    with_active(state, |active| {
        let tmp_dir = std::env::temp_dir().join(format!(
            "subvault-wb-{}",
            ids::random_id("up_").unwrap_or_else(|_| "tmp".into())
        ));
        if let Err(err) = fs::create_dir_all(&tmp_dir) {
            return HttpResponse::error(500, "temp_failed", &format!("临时目录创建失败：{err}"));
        }
        let temp_file = tmp_dir.join(&filename);
        if let Err(err) = fs::write(&temp_file, &bytes) {
            let _ = fs::remove_dir_all(&tmp_dir);
            return HttpResponse::error(500, "temp_write_failed", &format!("写入失败：{err}"));
        }

        let cmd_id = ids::random_id("wb_").unwrap_or_else(|_| "wb_import".into());
        let auth = active.auth();
        let lower = filename.to_ascii_lowercase();

        let report = if lower.ends_with(".pdf") || bytes.starts_with(b"%PDF-") {
            engine::submit_pdf(engine::PdfRequest {
                vault: active.path.clone(),
                command_id: cmd_id,
                source: temp_file.clone(),
                batch_id,
                auth,
                hooks: Hooks::default(),
            })
        } else if lower.ends_with(".html") || lower.ends_with(".htm") {
            engine::submit_html(engine::HtmlRequest {
                vault: active.path.clone(),
                command_id: cmd_id,
                source: temp_file.clone(),
                batch_id,
                auth,
                hooks: Hooks::default(),
            })
        } else if lower.ends_with(".csv") {
            engine::submit_csv(engine::CsvRequest {
                vault: active.path.clone(),
                command_id: cmd_id,
                source: temp_file.clone(),
                batch_id,
                auth,
                hooks: Hooks::default(),
            })
        } else if lower.ends_with(".txt") && std::str::from_utf8(&bytes).is_ok() {
            engine::submit_fragment(engine::FragmentRequest {
                vault: active.path.clone(),
                command_id: cmd_id,
                fragment: temp_file.clone(),
                batch_id,
                auth,
                hooks: Hooks::default(),
            })
        } else {
            let media_type = guess_media_type(&lower, &bytes);
            engine::import_file(engine::ImportRequest {
                vault: active.path.clone(),
                command_id: cmd_id,
                batch_id,
                media_type: media_type.to_string(),
                source: temp_file.clone(),
                display_name: Some(filename.clone()),
                auth,
                hooks: Hooks::default(),
            })
        };

        let _ = fs::remove_dir_all(&tmp_dir);

        if report.code != EXIT_OK {
            return map_engine_error(&report);
        }

        let reused = report.body["storage_reused"].as_bool().unwrap_or(false)
            || report.body["source_reused"].as_bool().unwrap_or(false)
            || report.body["outcome"].as_str() == Some("AlreadyPreserved");

        HttpResponse::json(
            200,
            json!({
                "ok": true,
                "filename": filename,
                "reused": reused,
                "result": report.body,
            }),
        )
    })
}

fn api_csv_promote(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let parent_source_id = body["parent_source_id"].as_str().unwrap_or("").trim();
    let row_number = body["row_number"].as_u64().unwrap_or(0);
    let merchant = body["merchant"].as_str().unwrap_or("").trim();
    let description = body["description"].as_str().unwrap_or("").trim();
    let date = body["date"].as_str().unwrap_or("").trim();
    let currency = body["currency"].as_str().unwrap_or("CNY").trim();
    let amount_text = body["amount_text"].as_str().unwrap_or("").trim();
    let billing_cycle = body["billing_cycle"].as_str().unwrap_or("unknown").trim();
    let account_label = body["account_label"].as_str().map(|s| s.trim().to_string());
    let category = body["category"].as_str().map(|s| s.trim().to_string());
    let raw_line = body["raw_line"].as_str().unwrap_or("").trim();

    if parent_source_id.is_empty() || amount_text.is_empty() {
        return HttpResponse::error(400, "missing_fields", "缺少必填流水信息");
    }

    with_active(state, |active| {
        let auth = active.auth();
        // Verify parent source exists and cite its SHA-256 verbatim
        let open_rep = engine::open_vault(&active.path, auth.clone());
        let parent_sha = open_rep.body["sources"]
            .as_array()
            .and_then(|arr| arr.iter().find(|s| s["source_id"] == parent_source_id))
            .and_then(|s| s["content_sha256"].as_str())
            .unwrap_or("")
            .to_string();
        if parent_sha.is_empty() {
            return HttpResponse::error(404, "parent_not_found", "未找到对应的 CSV 源文件");
        }

        let receipt_text = format!(
            "SubVault CSV Row Evidence\nParent CSV Source: {parent_source_id}\nParent CSV SHA-256: {parent_sha}\nCSV Row: {row_number}\nDate: {date}\nMerchant: {merchant}\nDescription: {description}\nCurrency: {currency}\nAmount: {amount_text}\nRaw CSV Line: {raw_line}\n"
        );
        let safe_merchant = if merchant.is_empty() {
            "csv-item"
        } else {
            merchant
        };
        let display_name =
            sanitize_filename(&format!("{safe_merchant}-{date}-row{row_number}.txt"));

        let tmp_dir = std::env::temp_dir().join(format!(
            "subvault-csvp-{}",
            ids::random_id("pr_").unwrap_or_else(|_| "tmp".into())
        ));
        if let Err(err) = fs::create_dir_all(&tmp_dir) {
            return HttpResponse::error(500, "temp_failed", &err.to_string());
        }
        let temp_file = tmp_dir.join(&display_name);
        if let Err(err) = fs::write(&temp_file, receipt_text.as_bytes()) {
            let _ = fs::remove_dir_all(&tmp_dir);
            return HttpResponse::error(500, "temp_write_failed", &err.to_string());
        }

        let imp_cmd = ids::random_id("csvp_").unwrap_or_else(|_| "csvp_1".into());
        let imp = engine::import_file(engine::ImportRequest {
            vault: active.path.clone(),
            command_id: imp_cmd,
            batch_id: None,
            media_type: "text/plain;charset=utf-8".into(),
            source: temp_file,
            display_name: Some(display_name),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });
        let _ = fs::remove_dir_all(&tmp_dir);
        if imp.code != EXIT_OK {
            return map_engine_error(&imp);
        }
        let new_source_id = imp.body["source_id"].as_str().unwrap_or("").to_string();
        let reused = imp.body["storage_reused"].as_bool().unwrap_or(false);

        if !reused {
            let fact_cmd = ids::random_id("csvf_").unwrap_or_else(|_| "csvf_1".into());
            let _ = engine::record_manual_fact(engine::ManualFactRequest {
                vault: active.path.clone(),
                command_id: fact_cmd,
                source_id: new_source_id.clone(),
                currency: Some(currency.to_string()),
                amount_text: Some(amount_text.to_string()),
                period_start: None,
                period_end: None,
                actor_id: WORKBENCH_ACTOR_ID.into(),
                auth: auth.clone(),
            });
            let _ = engine::annotate_source(engine::AnnotateRequest {
                vault: active.path.clone(),
                source_id: new_source_id.clone(),
                vendor: if merchant.is_empty() {
                    None
                } else {
                    Some(merchant.to_string())
                },
                plan_name: if description.is_empty() {
                    None
                } else {
                    Some(description.to_string())
                },
                account_label,
                billing_cycle: Some(billing_cycle.to_string()),
                category,
                bill_date: if date.is_empty() {
                    None
                } else {
                    Some(date.to_string())
                },
                notes: Some(format!(
                    "源自 CSV 流水第 {row_number} 行（{parent_source_id}）"
                )),
                asset_kind: Some(
                    if matches!(billing_cycle, "monthly" | "yearly" | "quarterly" | "weekly") {
                        "subscription".into()
                    } else {
                        "general".into()
                    },
                ),
                warranty_end: None,
                license_key: None,
                license_scope: None,
                reimburse_status: None,
                auth,
            });
        }

        HttpResponse::json(
            200,
            json!({
                "ok": true,
                "source_id": new_source_id,
                "storage_reused": reused,
            }),
        )
    })
}

fn api_source_save(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let source_id = body["source_id"].as_str().unwrap_or("").trim().to_string();
    if source_id.is_empty() {
        return HttpResponse::error(400, "source_id_required", "缺少账单编号");
    }

    with_active(state, |active| {
        let auth = active.auth();

        // 1. Save encrypted user annotation metadata
        let ann_rep = engine::annotate_source(engine::AnnotateRequest {
            vault: active.path.clone(),
            source_id: source_id.clone(),
            vendor: body
                .get("vendor")
                .and_then(Value::as_str)
                .map(str::to_string),
            plan_name: body
                .get("plan_name")
                .and_then(Value::as_str)
                .map(str::to_string),
            account_label: body
                .get("account_label")
                .and_then(Value::as_str)
                .map(str::to_string),
            billing_cycle: body
                .get("billing_cycle")
                .and_then(Value::as_str)
                .map(str::to_string),
            category: body
                .get("category")
                .and_then(Value::as_str)
                .map(str::to_string),
            bill_date: body
                .get("bill_date")
                .and_then(Value::as_str)
                .map(str::to_string),
            notes: body
                .get("notes")
                .and_then(Value::as_str)
                .map(str::to_string),
            asset_kind: body
                .get("asset_kind")
                .and_then(Value::as_str)
                .map(str::to_string),
            warranty_end: body
                .get("warranty_end")
                .and_then(Value::as_str)
                .map(str::to_string),
            license_key: body
                .get("license_key")
                .and_then(Value::as_str)
                .map(str::to_string),
            license_scope: body
                .get("license_scope")
                .and_then(Value::as_str)
                .map(str::to_string),
            reimburse_status: body
                .get("reimburse_status")
                .and_then(Value::as_str)
                .map(str::to_string),
            auth: auth.clone(),
        });
        if ann_rep.code != EXIT_OK {
            return map_engine_error(&ann_rep);
        }

        // 2. Handle amount fact (manual entry OR auditable correction)
        let incoming_amt = body
            .get("amount_text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let incoming_cur = body
            .get("currency")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let incoming_pstart = body
            .get("period_start")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let incoming_pend = body
            .get("period_end")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let facts_rep =
            engine::query_facts(&active.path, Some(source_id.clone()), None, auth.clone());
        let existing_facts = facts_rep.body["facts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let existing_amt_fact = existing_facts
            .iter()
            .find(|f| f["fact_type"] == "stated_amount_paid");
        let existing_per_fact = existing_facts
            .iter()
            .find(|f| f["fact_type"] == "stated_service_period");

        if let Some(raw_amt) = incoming_amt {
            let Some(norm_amt) = crate::real_extract::normalize_amount_text(raw_amt) else {
                return HttpResponse::error(400, "bad_amount", "金额格式不正确，例如 29.99 或 98");
            };
            let norm_cur = incoming_cur
                .and_then(crate::real_extract::normalize_currency)
                .unwrap_or("USD");
            if let Some(fact) = existing_amt_fact {
                let cur_amt = fact["value"]["amount_text"].as_str().unwrap_or("");
                let cur_code = fact["value"]["currency"].as_str().unwrap_or("USD");
                if cur_amt != norm_amt || cur_code != norm_cur {
                    let fact_id = fact["fact_id"].as_str().unwrap_or("").to_string();
                    let rev = fact["revision"].as_i64().unwrap_or(1).to_string();
                    let cmd_id = ids::random_id("corr_").unwrap_or_else(|_| "corr_1".into());
                    let corr = engine::correct_fact(engine::CorrectRequest {
                        vault: active.path.clone(),
                        command_id: cmd_id,
                        fact_id,
                        expected_revision: rev,
                        amount_text: norm_amt,
                        currency: Some(norm_cur.to_string()),
                        actor_id: WORKBENCH_ACTOR_ID.into(),
                        auth: auth.clone(),
                        hooks: Hooks::default(),
                    });
                    if corr.code != EXIT_OK {
                        return map_engine_error(&corr);
                    }
                }
            } else {
                let cmd_id = ids::random_id("manf_").unwrap_or_else(|_| "manf_1".into());
                let rec = engine::record_manual_fact(engine::ManualFactRequest {
                    vault: active.path.clone(),
                    command_id: cmd_id,
                    source_id: source_id.clone(),
                    currency: Some(norm_cur.to_string()),
                    amount_text: Some(norm_amt),
                    period_start: None,
                    period_end: None,
                    actor_id: WORKBENCH_ACTOR_ID.into(),
                    auth: auth.clone(),
                });
                if rec.code != EXIT_OK {
                    return map_engine_error(&rec);
                }
            }
        }

        // 3. Handle service period fact
        if let (Some(ps), Some(pe)) = (incoming_pstart, incoming_pend) {
            let changed = existing_per_fact.is_none_or(|f| {
                f["value"]["period_start"].as_str() != Some(ps.as_str())
                    || f["value"]["period_end"].as_str() != Some(pe.as_str())
            });
            if changed {
                let cmd_id = ids::random_id("manp_").unwrap_or_else(|_| "manp_1".into());
                let rec = engine::record_manual_fact(engine::ManualFactRequest {
                    vault: active.path.clone(),
                    command_id: cmd_id,
                    source_id: source_id.clone(),
                    currency: None,
                    amount_text: None,
                    period_start: Some(ps),
                    period_end: Some(pe),
                    actor_id: WORKBENCH_ACTOR_ID.into(),
                    auth,
                });
                if rec.code != EXIT_OK {
                    return map_engine_error(&rec);
                }
            }
        }

        match build_workspace_snapshot(active) {
            Ok(snap) => HttpResponse::json(200, snap),
            Err(resp) => resp,
        }
    })
}

fn api_source_reimburse(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let source_id = body["source_id"].as_str().unwrap_or("").trim().to_string();
    let status = body["reimburse_status"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if source_id.is_empty() {
        return HttpResponse::error(400, "source_id_required", "缺少账单编号");
    }
    if !matches!(
        status.as_str(),
        "none" | "pending" | "submitted" | "reimbursed"
    ) {
        return HttpResponse::error(400, "bad_reimburse_status", "无效的报销状态");
    }

    with_active(state, |active| {
        let rep = engine::annotate_source(engine::AnnotateRequest {
            vault: active.path.clone(),
            source_id,
            vendor: None,
            plan_name: None,
            account_label: None,
            billing_cycle: None,
            category: None,
            bill_date: None,
            notes: None,
            asset_kind: None,
            warranty_end: None,
            license_key: None,
            license_scope: None,
            reimburse_status: Some(status),
            auth: active.auth(),
        });
        if rep.code != EXIT_OK {
            return map_engine_error(&rep);
        }
        match build_workspace_snapshot(active) {
            Ok(snap) => HttpResponse::json(200, snap),
            Err(resp) => resp,
        }
    })
}

fn api_clear_demo_pack(state: &ServerState) -> HttpResponse {
    with_active(state, |active| {
        let snap = match build_workspace_snapshot(active) {
            Ok(s) => s,
            Err(resp) => return resp,
        };
        let demo_ids: Vec<String> = snap["items"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|it| it["is_demo"] == json!(true))
                    .filter_map(|it| it["source_id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let removed = if demo_ids.is_empty() {
            0
        } else {
            match engine::delete_sources(&active.path, &demo_ids, &active.auth()) {
                Ok(n) => n,
                Err(fail) => return map_engine_error(&fail.into_report()),
            }
        };

        match build_workspace_snapshot(active) {
            Ok(mut updated) => {
                if let Some(obj) = updated.as_object_mut() {
                    obj.insert("removed_count".into(), json!(removed));
                }
                HttpResponse::json(200, updated)
            }
            Err(resp) => resp,
        }
    })
}

fn api_source_delete(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let source_id = body["source_id"].as_str().unwrap_or("").trim().to_string();
    if source_id.is_empty() {
        return HttpResponse::error(400, "source_id_required", "缺少要移出的记录编号");
    }
    with_active(state, |active| {
        let removed = match engine::delete_sources(&active.path, &[source_id], &active.auth()) {
            Ok(n) => n,
            Err(fail) => return map_engine_error(&fail.into_report()),
        };
        match build_workspace_snapshot(active) {
            Ok(mut updated) => {
                if let Some(obj) = updated.as_object_mut() {
                    obj.insert("removed_count".into(), json!(removed));
                }
                HttpResponse::json(200, updated)
            }
            Err(resp) => resp,
        }
    })
}

fn api_fact_undo(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let decision_id = body["decision_id"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let expected_revision = body["expected_revision"]
        .as_i64()
        .map(|n| n.to_string())
        .or_else(|| body["expected_revision"].as_str().map(str::to_string))
        .unwrap_or_default();
    if decision_id.is_empty() || expected_revision.is_empty() {
        return HttpResponse::error(400, "missing_fields", "缺少要撤销的修改记录");
    }

    with_active(state, |active| {
        let cmd_id = ids::random_id("undo_").unwrap_or_else(|_| "undo_1".into());
        let rep = engine::undo_decision(engine::UndoRequest {
            vault: active.path.clone(),
            command_id: cmd_id,
            decision_id,
            expected_revision,
            actor_id: WORKBENCH_ACTOR_ID.into(),
            auth: active.auth(),
            hooks: Hooks::default(),
        });
        if rep.code != EXIT_OK {
            return map_engine_error(&rep);
        }
        match build_workspace_snapshot(active) {
            Ok(snap) => HttpResponse::json(200, snap),
            Err(resp) => resp,
        }
    })
}

fn api_source_raw(req: &HttpRequest, state: &ServerState, source_id: &str) -> HttpResponse {
    let download = req
        .query
        .get("download")
        .is_some_and(|v| v == "1" || v == "true");
    with_active(state, |active| {
        match engine::read_source_bytes(&active.path, source_id, &active.auth()) {
            Ok((bytes, media_type, display_name)) => {
                let mut content_type = media_type.clone();
                let mut resp = if media_type.starts_with("text/html") && !download {
                    // Render HTML safely in strict sandbox with ZERO JavaScript and ZERO remote network!
                    HttpResponse::ok_bytes(&content_type, bytes).with_header(
                        "Content-Security-Policy",
                        "sandbox; default-src 'none'; style-src 'unsafe-inline'; img-src data:",
                    )
                } else {
                    if content_type.starts_with("text/csv") {
                        content_type = "text/plain; charset=utf-8".into();
                    }
                    HttpResponse::ok_bytes(&content_type, bytes)
                };
                let disp = if download {
                    format!(
                        "attachment; filename=\"{}\"",
                        display_name.replace('"', "_")
                    )
                } else {
                    format!("inline; filename=\"{}\"", display_name.replace('"', "_"))
                };
                resp = resp.with_header("Content-Disposition", disp);
                resp
            }
            Err(fail) => map_engine_error(&fail.into_report()),
        }
    })
}

fn api_source_text(state: &ServerState, source_id: &str) -> HttpResponse {
    with_active(state, |active| {
        match engine::read_source_bytes(&active.path, source_id, &active.auth()) {
            Ok((bytes, media_type, display_name)) => {
                let (kind, text) = if media_type == "application/pdf" || bytes.starts_with(b"%PDF-")
                {
                    match crate::pdf_parse::extract_pages_for_redaction(&bytes) {
                        Ok(pages) => {
                            let mut buf = String::new();
                            for (page_no, lines) in pages {
                                buf.push_str(&format!("--- 第 {page_no} 页 ---\n"));
                                for line in lines {
                                    buf.push_str(&line);
                                    buf.push('\n');
                                }
                            }
                            if buf.trim().is_empty() {
                                (
                                    "pdf_empty",
                                    "该 PDF 未包含可直接读取的文字层（可能是图片扫描件）。原件已完整存证，您可直接在左侧查看原版 PDF 并手动补录金额与备注。".to_string(),
                                )
                            } else {
                                ("pdf_text", buf)
                            }
                        }
                        Err(reason) => (
                            "pdf_unsupported",
                            format!("无法静态提取文字（原因：{reason}），原件已完整存证。"),
                        ),
                    }
                } else if media_type.starts_with("text/csv")
                    || display_name.to_ascii_lowercase().ends_with(".csv")
                {
                    let (decoded, enc) = crate::real_extract::decode_csv_bytes(&bytes);
                    let preview: String = decoded.chars().take(16_000).collect();
                    ("csv_text", format!("[自动解码编码：{enc}]\n{preview}"))
                } else if media_type.starts_with("text/html") {
                    let s = String::from_utf8_lossy(&bytes);
                    let lines = crate::real_extract::html_to_lines(&s);
                    let joined: String = lines
                        .into_iter()
                        .map(|l| l.text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    ("html_text", joined)
                } else if media_type.starts_with("image/") {
                    ("image", String::new())
                } else if let Ok(s) = std::str::from_utf8(&bytes) {
                    ("plain_text", s.chars().take(16_000).collect())
                } else {
                    (
                        "binary",
                        format!(
                            "二进制原件（{} 字节），可点击上方按钮下载原件。",
                            bytes.len()
                        ),
                    )
                };
                HttpResponse::json(
                    200,
                    json!({
                        "ok": true,
                        "kind": kind,
                        "media_type": media_type,
                        "display_name": display_name,
                        "text": text,
                    }),
                )
            }
            Err(fail) => map_engine_error(&fail.into_report()),
        }
    })
}

fn api_source_redact(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let source_id = body["source_id"].as_str().unwrap_or("").trim().to_string();
    let terms: Vec<String> = body["redact_terms"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    with_active(state, |active| {
        let tmp_dir = std::env::temp_dir().join(format!(
            "subvault-red-{}",
            ids::random_id("rd_").unwrap_or_else(|_| "tmp".into())
        ));
        if let Err(err) = fs::create_dir_all(&tmp_dir) {
            return HttpResponse::error(500, "temp_failed", &err.to_string());
        }
        let out_file = tmp_dir.join("redacted.txt");
        let rep = engine::redact_template(engine::RedactRequest {
            source: None,
            vault: Some(active.path.clone()),
            source_id: Some(source_id),
            out: out_file.clone(),
            redact_terms: terms,
            auth: active.auth(),
        });
        if rep.code != EXIT_OK {
            let _ = fs::remove_dir_all(&tmp_dir);
            return map_engine_error(&rep);
        }
        let text = fs::read_to_string(&out_file).unwrap_or_default();
        let _ = fs::remove_dir_all(&tmp_dir);
        HttpResponse::json(
            200,
            json!({
                "ok": true,
                "redacted_text": text,
                "counts": rep.body["counts"],
                "total_redactions": rep.body["total_redactions"],
                "redacted_lines": rep.body["redacted_lines"],
            }),
        )
    })
}

fn api_export_csv(state: &ServerState) -> HttpResponse {
    with_active(state, |active| match build_excel_csv_bytes(active) {
        Ok(bytes) => HttpResponse::ok_bytes("text/csv; charset=utf-8", bytes).with_header(
            "Content-Disposition",
            "attachment; filename=\"subvault-ledger.csv\"",
        ),
        Err(resp) => resp,
    })
}

fn build_excel_csv_bytes(active: &ActiveVault) -> Result<Vec<u8>, HttpResponse> {
    let snap = build_workspace_snapshot(active)?;
    // Note: Raw license_key is deliberately omitted from plain Excel CSV exports for security.
    let mut out = String::from("\u{FEFF}资产类型,商家/服务,项目/套餐,归属账号,计费周期,分类,币种,金额,账单日期,服务开始,服务结束,保修截止,报销状态,授权范围,原件文件名,重复导入次数,原件SHA256,备注\n");
    if let Some(items) = snap["items"].as_array() {
        for item in items {
            let ann = &item["annotation"];
            let amt = &item["amount_fact"]["value"];
            let per = &item["period_fact"]["value"];
            let kind_zh = match ann["asset_kind"].as_str().unwrap_or("general") {
                "subscription" => "周期订阅",
                "license" => "买断授权",
                "hardware" => "硬件保修",
                "reimbursable" => "报销凭证",
                _ => "普通账单",
            };
            let cycle_zh = match ann["billing_cycle"].as_str().unwrap_or("unknown") {
                "monthly" => "月付",
                "yearly" => "年付",
                "quarterly" => "季付",
                "weekly" => "周付",
                "one_time" => "一次性",
                _ => "未标注",
            };
            let reimb_zh = match ann["reimburse_status"].as_str().unwrap_or("none") {
                "pending" => "待报销",
                "submitted" => "已提交",
                "reimbursed" => "已到账",
                _ => "",
            };
            let cols = [
                kind_zh,
                ann["vendor"].as_str().unwrap_or(""),
                ann["plan_name"].as_str().unwrap_or(""),
                ann["account_label"].as_str().unwrap_or(""),
                cycle_zh,
                ann["category"].as_str().unwrap_or(""),
                amt["currency"].as_str().unwrap_or(""),
                amt["amount_text"].as_str().unwrap_or(""),
                ann["bill_date"].as_str().unwrap_or(""),
                per["period_start"].as_str().unwrap_or(""),
                per["period_end"].as_str().unwrap_or(""),
                ann["warranty_end"].as_str().unwrap_or(""),
                reimb_zh,
                ann["license_scope"].as_str().unwrap_or(""),
                item["display_name"].as_str().unwrap_or(""),
                &item["duplicate_hits"].as_u64().unwrap_or(0).to_string(),
                item["content_sha256"].as_str().unwrap_or(""),
                ann["notes"].as_str().unwrap_or(""),
            ];
            let line = cols
                .iter()
                .map(|c| csv_escape(c))
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out.into_bytes())
}

fn api_export_package(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let out_str = body["out_dir"].as_str().unwrap_or("").trim();
    if out_str.is_empty() {
        return HttpResponse::error(400, "out_dir_required", "请填写备份包导出路径");
    }
    let ack = body["acknowledge_plaintext"].as_bool().unwrap_or(false);
    let out_dir = PathBuf::from(out_str);
    if let Some(parent) = out_dir.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }

    with_active(state, |active| {
        let rep = engine::export_copy(engine::ExportRequest {
            vault: active.path.clone(),
            out: out_dir.clone(),
            allow_partial: false,
            acknowledge_plaintext: ack,
            auth: active.auth(),
            hooks: Hooks::default(),
        });
        if rep.code != EXIT_OK {
            return map_engine_error(&rep);
        }
        let verify = crate::verify_package::verify_package(&out_dir);
        HttpResponse::json(
            200,
            json!({
                "ok": true,
                "export": rep.body,
                "verification": verify.body,
                "verified": verify.code == EXIT_OK,
            }),
        )
    })
}

fn api_export_zip(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let ack = req
        .query
        .get("acknowledge_plaintext")
        .is_some_and(|v| v == "yes" || v == "true" || v == "1");
    with_active(state, |active| {
        if active.protection == Some(ProtectionMode::Passphrase) && !ack {
            return HttpResponse::error(
                400,
                "plaintext_export_ack_required",
                "加密账本导出明文备份包前需要明确确认风险",
            );
        }
        let tmp_root = std::env::temp_dir().join(format!(
            "subvault-zip-{}",
            ids::random_id("zp_").unwrap_or_else(|_| "tmp".into())
        ));
        if let Err(err) = fs::create_dir_all(&tmp_root) {
            return HttpResponse::error(500, "temp_failed", &err.to_string());
        }
        let pack_dir = tmp_root.join("exit-pack");
        let review_file = tmp_root.join("review.txt");
        let rep = engine::export_copy(engine::ExportRequest {
            vault: active.path.clone(),
            out: pack_dir.clone(),
            allow_partial: false,
            acknowledge_plaintext: ack,
            auth: active.auth(),
            hooks: Hooks::default(),
        });
        if rep.code != EXIT_OK {
            let _ = fs::remove_dir_all(&tmp_root);
            return map_engine_error(&rep);
        }
        let _ = engine::review_sheet(&active.path, &review_file, Some(&pack_dir), active.auth());

        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        collect_zip_entries(&pack_dir, "", &mut entries);
        if let Ok(rev_bytes) = fs::read(&review_file) {
            entries.push(("review-sheet.txt".into(), rev_bytes));
        }
        if let Ok(csv_bytes) = build_excel_csv_bytes(active) {
            entries.push(("ledger-excel.csv".into(), csv_bytes));
        }
        let _ = fs::remove_dir_all(&tmp_root);

        let zip_bytes = build_store_zip(&entries);
        HttpResponse::ok_bytes("application/zip", zip_bytes).with_header(
            "Content-Disposition",
            "attachment; filename=\"subvault-exit-pack.zip\"",
        )
    })
}

fn api_verify_package(req: &HttpRequest) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let path_str = body["package_dir"].as_str().unwrap_or("").trim();
    if path_str.is_empty() {
        return HttpResponse::error(400, "package_dir_required", "请填写要校验的备份包路径");
    }
    let rep = crate::verify_package::verify_package(Path::new(path_str));
    HttpResponse::json(
        200,
        json!({
            "ok": rep.code == EXIT_OK,
            "verified": rep.code == EXIT_OK,
            "report": rep.body,
        }),
    )
}

// --- Pure-Rust Minimal Store ZIP Builder (CRC32, zero extra crates) ---

fn collect_zip_entries(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if p.is_dir() {
            collect_zip_entries(&p, &rel, out);
        } else if p.is_file() {
            if let Ok(bytes) = fs::read(&p) {
                out.push((rel, bytes));
            }
        }
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn build_store_zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut zip = Vec::new();
    let mut central = Vec::new();

    for (name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32(data);
        let size = data.len() as u32;
        let offset = zip.len() as u32;

        // Local file header (UTF-8 flag 0x0800)
        zip.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&0x0800u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // store
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&crc.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(name_bytes);
        zip.extend_from_slice(data);

        // Central directory header
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let central_offset = zip.len() as u32;
    let central_size = central.len() as u32;
    let count = entries.len() as u16;
    zip.extend_from_slice(&central);

    // End of central directory
    zip.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&count.to_le_bytes());
    zip.extend_from_slice(&count.to_le_bytes());
    zip.extend_from_slice(&central_size.to_le_bytes());
    zip.extend_from_slice(&central_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip
}

// --- Helpers ---

fn map_engine_error(report: &Report) -> HttpResponse {
    let reason = report.body["reason"].as_str().unwrap_or("operation_failed");
    let (status, friendly) = match reason {
        "passphrase_required" => (401, "该账本已加密，请输入密码或离线恢复密钥"),
        "invalid_passphrase" => (401, "密码不正确，请重新输入"),
        "invalid_recovery_key" | "bad_recovery_key_format" => {
            (401, "恢复密钥格式有误或校验不通过，请核对后重试")
        }
        "writer_busy" => (409, "另一个 SubVault 窗口正在写入此账本，请稍后重试"),
        "path_exists" | "output_exists" => (409, "目标文件夹或文件已存在，请换一个新名称"),
        "output_inside_vault" => (400, "导出位置不能放在账本文件夹内部，请选其他目录"),
        "plaintext_export_ack_required" => (400, "导出加密账本的明文备份前请先勾选确认"),
        "not_a_vault" | "vault_missing" => (404, "该路径不是有效的 SubVault 账本文件夹"),
        "requires_format_6" => (400, "该文件夹是旧版测试账本，请新建正式本地账本使用"),
        "invalid_pdf_header" => (400, "该文件不是有效的 PDF 文件"),
        "invalid_utf8" => (400, "该 HTML/文本文件不是有效的 UTF-8 编码"),
        "over_input_budget" => (413, "文件体积超出单文件安全上限"),
        "revision_conflict" | "successor_conflict" => (409, "该记录刚刚已被修改，请刷新后重试"),
        "ciphertext_auth_failed" => (500, "检测到账本文件被篡改或损坏，已拒绝读取以保护安全"),
        _ => (400, "操作未能完成"),
    };
    HttpResponse::json(
        status,
        json!({
            "ok": false,
            "error": reason,
            "message": friendly,
            "detail": report.body,
        }),
    )
}

fn sanitize_filename(raw: &str) -> String {
    let base = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("bill.bin");
    let cleaned: String = base
        .chars()
        .filter(|&c| {
            !matches!(
                c,
                '\0' | '/' | '\\' | '\n' | '\r' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        })
        .take(160)
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "bill.bin".to_string()
    } else {
        cleaned
    }
}

fn guess_media_type(lower_name: &str, bytes: &[u8]) -> &'static str {
    if lower_name.ends_with(".png") || bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if lower_name.ends_with(".jpg")
        || lower_name.ends_with(".jpeg")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
    {
        "image/jpeg"
    } else if lower_name.ends_with(".webp") {
        "image/webp"
    } else if lower_name.ends_with(".txt") {
        "text/plain;charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

fn amount_to_cents(text: &str) -> Option<i64> {
    let (whole, frac) = text.split_once('.')?;
    let w: i64 = whole.parse().ok()?;
    let f: i64 = frac.parse().ok()?;
    Some(w * 100 + f)
}

fn cents_to_text(cents: i64) -> String {
    let abs = cents.abs();
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

fn today_epoch_days() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    secs / 86_400
}

fn parse_iso_days(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn format_epoch_days(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}
