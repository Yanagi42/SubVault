//! Embedded local-only graphical workbench server (`subvault-app` / `subvault app`).
//!
//! Security model:
//! - Strictly binds loopback `127.0.0.1:<port>` (never LAN/WAN).
//! - Requires a 256-bit cryptographic session token on every `/api/*` call
//!   via `X-SubVault-Token` (never in URL query strings).
//! - Verifies `Host`, `Origin`, and `Sec-Fetch-Site` headers to prevent
//!   cross-site and DNS-rebinding CSRF.
//! - Holds unlocked Format 6 `VaultCipher` strictly in process memory (`Zeroizing` DEK)
//!   and drops it immediately on lock or shutdown; passphrases never touch disk.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};
use zeroize::{Zeroize, Zeroizing};

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
const MAX_REQUEST_LINE: usize = 8 * 1024;
const MAX_HEADERS_BYTES: usize = 32 * 1024;
const MAX_CONCURRENT_CONNS: usize = 32;
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);
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

/// Fixed loopback port used by default so browser password managers and
/// local preferences (theme, etc.) persist across restarts on a stable origin.
pub const DEFAULT_WORKBENCH_PORT: u16 = 18421;

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
    /// Serializes engine operations on this vault inside the workbench process so
    /// concurrent browser requests (e.g. `/raw` + `/text` when opening the drawer)
    /// queue cleanly instead of colliding on the OS single-writer `VaultLock`.
    op_lock: Mutex<()>,
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
    active: Mutex<Option<Arc<ActiveVault>>>,
    shutdown: AtomicBool,
    in_flight: AtomicUsize,
}

struct PrivateTempDir {
    path: PathBuf,
}

impl PrivateTempDir {
    fn create(prefix: &str) -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "subvault-{prefix}-{}",
            ids::random_id("tmp_").unwrap_or_else(|_| "0".into())
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&path)?;
        }
        #[cfg(not(unix))]
        {
            fs::create_dir(&path)?;
        }
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn clear_active_vault(state: &ServerState) {
    let mut guard = state
        .active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

fn set_active_vault(state: &ServerState, vault: ActiveVault) {
    let mut guard = state
        .active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(Arc::new(vault));
}

pub struct WorkbenchServer {
    addr: SocketAddr,
    token: String,
    state: Arc<ServerState>,
    handle: Option<JoinHandle<()>>,
}

impl WorkbenchServer {
    pub fn start(opts: WorkbenchOptions) -> io::Result<Self> {
        let listener = match TcpListener::bind(("127.0.0.1", opts.port)) {
            Ok(l) => l,
            Err(err)
                if opts.port == DEFAULT_WORKBENCH_PORT
                    && matches!(
                        err.kind(),
                        io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                    ) =>
            {
                TcpListener::bind(("127.0.0.1", 0))?
            }
            Err(err) => return Err(err),
        };
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
            in_flight: AtomicUsize::new(0),
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
        clear_active_vault(&self.state);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WorkbenchServer {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        clear_active_vault(&self.state);
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
    println!("关闭工作台请在页面左上角点击账本名称 → 「退出 SubVault」，或在此窗口按 Ctrl+C。");
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

struct ConnSlot {
    state: Arc<ServerState>,
}

impl Drop for ConnSlot {
    fn drop(&mut self) {
        self.state.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

fn serve_loop(listener: TcpListener, state: Arc<ServerState>) {
    while !state.shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                if state.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let prev = state.in_flight.fetch_add(1, Ordering::SeqCst);
                if prev >= MAX_CONCURRENT_CONNS {
                    state.in_flight.fetch_sub(1, Ordering::SeqCst);
                    let _ = write_http_response(
                        &mut stream,
                        HttpResponse::error(503, "server_busy", "请求过于频繁，请稍后重试"),
                    );
                    continue;
                }
                let slot = ConnSlot {
                    state: Arc::clone(&state),
                };
                let _ = thread::Builder::new().spawn(move || {
                    let _slot = slot;
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(REQUEST_DEADLINE));
                    let _ = stream.set_write_timeout(Some(REQUEST_DEADLINE));
                    handle_connection(stream, &_slot.state);
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
            501 => "Not Implemented",
            503 => "Service Unavailable",
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

fn is_allowed_host(host: &str, port: u16) -> bool {
    let lower = host.trim().to_ascii_lowercase();
    lower == format!("127.0.0.1:{port}") || lower == format!("localhost:{port}")
}

fn is_allowed_origin(origin: &str, port: u16) -> bool {
    let lower = origin.trim().to_ascii_lowercase();
    lower == format!("http://127.0.0.1:{port}") || lower == format!("http://localhost:{port}")
}

fn read_line_limited<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Option<String>, HttpResponse> {
    let mut buf = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|_| HttpResponse::error(400, "bad_request", "请求读取失败"))?;
        if available.is_empty() {
            if buf.is_empty() {
                return Ok(None);
            }
            break;
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            if buf.len() + take > max_bytes {
                return Err(HttpResponse::error(400, "header_too_large", "请求头过长"));
            }
            buf.extend_from_slice(&available[..take]);
            reader.consume(take);
            break;
        } else {
            let len = available.len();
            if buf.len() + len > max_bytes {
                return Err(HttpResponse::error(400, "header_too_large", "请求头过长"));
            }
            buf.extend_from_slice(available);
            reader.consume(len);
        }
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| HttpResponse::error(400, "bad_header", "请求头包含非法字符"))
}

fn handle_connection(mut stream: TcpStream, state: &ServerState) {
    let mut req = match read_http_request(&mut stream) {
        Ok(Some(req)) => req,
        Ok(None) => return,
        Err(resp) => {
            let _ = write_http_response(&mut stream, resp);
            return;
        }
    };

    let host_ok = req
        .headers
        .get("host")
        .is_some_and(|h| is_allowed_host(h, state.port));
    if !host_ok {
        req.body.zeroize();
        let _ = write_http_response(
            &mut stream,
            HttpResponse::error(403, "host_forbidden", "仅允许通过本机回环地址访问"),
        );
        return;
    }

    if let Some(origin) = req.headers.get("origin") {
        if !is_allowed_origin(origin, state.port) {
            req.body.zeroize();
            let _ = write_http_response(
                &mut stream,
                HttpResponse::error(403, "origin_forbidden", "拒绝来自外部页面的访问"),
            );
            return;
        }
    }

    if req.path.starts_with("/api/") {
        if let Some(site) = req.headers.get("sec-fetch-site") {
            let lower = site.trim().to_ascii_lowercase();
            if lower != "same-origin" && lower != "none" {
                req.body.zeroize();
                let _ = write_http_response(
                    &mut stream,
                    HttpResponse::error(403, "fetch_site_forbidden", "拒绝跨站请求"),
                );
                return;
            }
        }
    }

    let resp = dispatch(&req, state);
    req.body.zeroize();
    let _ = write_http_response(&mut stream, resp);
}

fn read_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>, HttpResponse> {
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|_| HttpResponse::error(500, "stream_error", "连接读取失败"))?,
    );
    let Some(request_line) = read_line_limited(&mut reader, MAX_REQUEST_LINE)? else {
        return Ok(None);
    };
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(HttpResponse::error(400, "bad_request", "请求行无效"));
    }
    let method = parts[0].to_ascii_uppercase();
    let raw_target = parts[1];
    let (path, query) = split_path_query(raw_target);

    let mut headers = BTreeMap::new();
    let mut header_count = 0usize;
    let mut header_bytes = request_line.len();
    loop {
        if Instant::now() > deadline {
            return Err(HttpResponse::error(400, "request_timeout", "请求读取超时"));
        }
        let Some(line) = read_line_limited(&mut reader, MAX_REQUEST_LINE)? else {
            break;
        };
        if line == "\r\n" || line == "\n" {
            break;
        }
        header_bytes += line.len();
        header_count += 1;
        if header_count > 64 || header_bytes > MAX_HEADERS_BYTES {
            return Err(HttpResponse::error(
                400,
                "too_many_headers",
                "请求头过多或过大",
            ));
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if matches!(key.as_str(), "host" | "origin" | "content-length")
                && headers.contains_key(&key)
            {
                return Err(HttpResponse::error(
                    400,
                    "duplicate_header",
                    "存在重复的关键请求头",
                ));
            }
            headers.insert(key, val);
        }
    }

    if headers.contains_key("transfer-encoding") {
        return Err(HttpResponse::error(
            501,
            "transfer_encoding_unsupported",
            "不支持分块传输编码",
        ));
    }

    let content_len = match headers.get("content-length") {
        None => 0,
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| HttpResponse::error(400, "bad_content_length", "Content-Length 无效"))?,
    };
    if content_len > MAX_HTTP_BODY {
        return Err(HttpResponse::error(
            413,
            "payload_too_large",
            "上传文件超过单次 8 MiB 限制",
        ));
    }
    let mut body = vec![0u8; content_len];
    if content_len > 0 {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| HttpResponse::error(400, "request_timeout", "请求读取超时"))?;
        let _ = reader.get_ref().set_read_timeout(Some(remaining));
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

fn sanitize_header_value(raw: &str) -> String {
    raw.chars()
        .filter(|&c| c != '\r' && c != '\n' && c != '\0')
        .collect()
}

fn write_http_response(stream: &mut TcpStream, resp: HttpResponse) -> io::Result<()> {
    let safe_ct = sanitize_header_value(&resp.content_type);
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: SAMEORIGIN\r\nConnection: close\r\n",
        resp.status,
        resp.reason,
        safe_ct,
        resp.body.len()
    );
    let has_custom_csp = resp
        .extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-security-policy"));
    if !has_custom_csp {
        head.push_str(
            "Content-Security-Policy: default-src 'self'; img-src 'self' data: blob:; frame-src 'self' blob:; object-src 'self' blob:; style-src 'self' 'unsafe-inline'\r\n",
        );
    }
    for (k, v) in resp.extra_headers {
        let safe_k = sanitize_header_value(&k);
        let safe_v = sanitize_header_value(&v);
        if !safe_k.is_empty() {
            head.push_str(&format!("{safe_k}: {safe_v}\r\n"));
        }
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
            clear_active_vault(state);
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
    let active = {
        let guard = match state.active.lock() {
            Ok(g) => g,
            Err(_) => return HttpResponse::error(500, "lock_poisoned", "内部状态异常"),
        };
        guard.as_ref().map(Arc::clone)
    };
    match active {
        Some(active) => {
            let _op_guard = active
                .op_lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            f(&active)
        }
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
    if std::env::var("SUBVAULT_NO_RECENT").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        return None;
    }
    if let Ok(dir) = std::env::var("SUBVAULT_CONFIG_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join("recent_vaults.json"));
        }
    }
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
    if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
        let _ = crate::fsutil::atomic_write(&store_path, &bytes);
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
    let title: String = body["title"]
        .as_str()
        .unwrap_or("选择文件夹")
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    let title = if title.trim().is_empty() {
        "选择文件夹".to_string()
    } else {
        title.trim().to_string()
    };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const SCRIPT: &str = "\
            [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
            Add-Type -AssemblyName System.Windows.Forms; \
            $f = New-Object System.Windows.Forms.Form; $f.TopMost = $true; \
            $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
            $d.Description = $env:SUBVAULT_PICKER_TITLE; $d.ShowNewFolderButton = $true; \
            if ($d.ShowDialog($f) -eq [System.Windows.Forms.DialogResult]::OK) { Write-Output $d.SelectedPath }";
        match std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", SCRIPT])
            .env("SUBVAULT_PICKER_TITLE", &title)
            .creation_flags(CREATE_NO_WINDOW)
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
        let title_arg = format!("--title={title}");
        let zenity = std::process::Command::new("zenity")
            .args(["--file-selection", "--directory", &title_arg])
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
        let tmp_guard = match PrivateTempDir::create("demo") {
            Ok(d) => d,
            Err(err) => return HttpResponse::error(500, "temp_failed", &err.to_string()),
        };
        let tmp_dir = tmp_guard.path().to_path_buf();
        let batch_id = ids::random_id("b_").unwrap_or_else(|_| "b_demo".into());
        let auth = active.auth();

        let today_days = today_epoch_days();
        let d_shot = format_epoch_days(today_days - 1);
        let d_csv_1 = format_epoch_days(today_days - 14);
        let d_csv_2 = format_epoch_days(today_days - 9);
        let d_csv_3 = format_epoch_days(today_days - 4);
        let d_wps_start = format_epoch_days(today_days - 60);
        let d_wps_end = format_epoch_days(today_days + 305);
        let d_wps_start_zh = format_zh_epoch_days(today_days - 60);
        let d_wps_end_zh = format_zh_epoch_days(today_days + 305);
        let d_oai_prev_start = format_epoch_days(today_days - 38);
        let d_oai_prev_end = format_epoch_days(today_days - 8);
        let d_oai_curr_start = format_epoch_days(today_days - 8);
        let d_oai_curr_end = format_epoch_days(today_days + 22);
        let d_gh_start = format_epoch_days(today_days - 5);
        let d_gh_end = format_epoch_days(today_days + 25);
        let d_ap_start = format_epoch_days(today_days - 12);
        let d_ap_start_zh = format_zh_epoch_days(today_days - 12);
        let d_ap_end_zh = format_zh_epoch_days(today_days + 18);
        let d_nf_start = format_epoch_days(today_days - 2);
        let d_nf_end = format_epoch_days(today_days + 28);
        let hw_buy_date = format_epoch_days(today_days - 320);
        let hw_warranty_end = format_epoch_days(today_days + 45);
        let d_raycast = format_epoch_days(today_days - 90);
        let d_tencent = format_epoch_days(today_days - 3);

        // 0a. Scanned-receipt style image (unextracted, lets user try Quick Triage!)
        let receipt_png = build_demo_receipt_png(&d_shot);
        let shot_path = tmp_dir.join("示例-待补录收据截图.png");
        let _ = fs::write(&shot_path, &receipt_png);
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
        let alipay_str = format!(
            "支付宝交易记录明细查询（示例）\n账号:[personal@example.com]\n------------------------------------\n交易时间,交易分类,交易对方,商品说明,收/支,金额,收/付款方式,交易状态\n{d_csv_1} 09:00:00,云服务,Apple,iCloud+ 2TB 连续包月,支出,68.00,招商银行,交易成功\n{d_csv_2} 14:20:00,影音娱乐,哔哩哔哩,大会员连续包月,支出,15.00,余额宝,交易成功\n{d_csv_3} 20:00:00,开发工具,JetBrains,All Products Pack 年付,支出,1980.00,信用卡,交易成功\n"
        );
        let (gbk_bytes, _, _) = encoding_rs::GB18030.encode(&alipay_str);
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

        // 1. Chinese electronic invoice PDF (WPS ¥98.00 yearly, crisp visual vector layout + Chinese text layer)
        let wps_pdf_bytes = build_visual_demo_fapiao_pdf(
            "2631200000018421",
            "Kingsoft Office Software (Jinshan)",
            "WPS Super VIP Annual Plan",
            &format!("{d_wps_start} to {d_wps_end}"),
            "CNY 98.00",
            &[
                "增值税电子普通发票（示例）".to_string(),
                "销售方名称：金山办公软件股份有限公司".to_string(),
                "商品名称：WPS超级会员年卡".to_string(),
                format!("服务周期：{d_wps_start_zh} 至 {d_wps_end_zh}"),
                "价税合计（小写）¥98.00".to_string(),
            ],
        );
        let zh_pdf = tmp_dir.join("示例-WPS年卡电子发票.pdf");
        let _ = fs::write(&zh_pdf, &wps_pdf_bytes);
        let _ = engine::submit_pdf(engine::PdfRequest {
            vault: active.path.clone(),
            command_id: ids::random_id("dm_").unwrap_or_else(|_| "dm_1".into()),
            source: zh_pdf,
            batch_id: Some(batch_id.clone()),
            auth: auth.clone(),
            hooks: Hooks::default(),
        });

        // 2. Historical OpenAI HTML receipt ($18.00 prior month) + current ($20.00, triggers +11% price drift!) + renamed duplicate copy
        let openai_sep_html = format!(
            r#"<!doctype html>
<html>
<head><meta charset="utf-8"><style>body{{font-family:sans-serif;color:#111;margin:20px}}.box{{border:1px solid #e2e8f0;border-radius:10px;padding:18px;max-width:560px}}table{{width:100%;border-collapse:collapse;margin-top:12px}}th,td{{text-align:left;padding:8px 0;border-bottom:1px solid #f1f5f9}}</style></head>
<body>
  <div class="box">
    <h2>OpenAI Receipt (Sample - Prior Month)</h2>
    <p>From: OpenAI, LLC</p>
    <p>Account email: work@example.com</p>
    <p>Date paid: {d_oai_prev_start}</p>
    <table>
      <tr><th>Plan</th><td>ChatGPT Plus (Monthly)</td></tr>
      <tr><th>Billing period</th><td>{d_oai_prev_start} through {d_oai_prev_end}</td></tr>
      <tr><th>Amount paid</th><td>$18.00</td></tr>
    </table>
  </div>
</body>
</html>"#
        );
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

        let openai_html = format!(
            r#"<!doctype html>
<html>
<head><meta charset="utf-8"><style>body{{font-family:sans-serif;color:#111;margin:20px}}.box{{border:1px solid #e2e8f0;border-radius:10px;padding:18px;max-width:560px}}table{{width:100%;border-collapse:collapse;margin-top:12px}}th,td{{text-align:left;padding:8px 0;border-bottom:1px solid #f1f5f9}}</style></head>
<body>
  <div class="box">
    <h2>OpenAI Receipt (Sample)</h2>
    <p>From: OpenAI, LLC</p>
    <p>Account email: work@example.com</p>
    <p>Date paid: {d_oai_curr_start}</p>
    <table>
      <tr><th>Plan</th><td>ChatGPT Plus (Monthly)</td></tr>
      <tr><th>Billing period</th><td>{d_oai_curr_start} through {d_oai_curr_end}</td></tr>
      <tr><th>Amount paid</th><td>$20.00</td></tr>
    </table>
  </div>
</body>
</html>"#
        );
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
        let github_html = format!(
            r#"<!doctype html>
<html>
<head><meta charset="utf-8"><style>body{{font-family:sans-serif;color:#111;margin:20px}}table{{width:100%;max-width:560px;border-collapse:collapse;margin-top:12px}}th,td{{text-align:left;padding:8px 0;border-bottom:1px solid #f1f5f9}}</style></head>
<body>
  <h2>GitHub Payment Receipt</h2>
  <p>From: GitHub, Inc.</p>
  <p>Account email: dev-work@company.example</p>
  <p>Date paid: {d_gh_start}</p>
  <table>
    <tr><th>Plan</th><td>GitHub Copilot Individual (Monthly)</td></tr>
    <tr><th>Billing period</th><td>{d_gh_start} through {d_gh_end}</td></tr>
    <tr><th>Amount paid</th><td>$10.00</td></tr>
  </table>
</body>
</html>"#
        );
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
        let apple_html = format!(
            r#"<!doctype html>
<html>
<head><meta charset="utf-8"><style>body{{font-family:sans-serif;color:#111;margin:20px}}table{{width:100%;max-width:560px;border-collapse:collapse;margin-top:12px}}th,td{{text-align:left;padding:8px 0;border-bottom:1px solid #f1f5f9}}</style></head>
<body>
  <h2>Apple 电子收据（示例）</h2>
  <p>收款方：Apple iCloud</p>
  <p>账号：个人</p>
  <p>账单日期：{d_ap_start}</p>
  <table>
    <tr><th>套餐</th><td>iCloud+ 2TB 云存储（连续包月）</td></tr>
    <tr><th>服务周期</th><td>{d_ap_start_zh} 至 {d_ap_end_zh}</td></tr>
    <tr><th>实付金额</th><td>¥68.00</td></tr>
  </table>
</body>
</html>"#
        );
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
        let nf_html = format!(
            r#"<!doctype html>
<html>
<head><meta charset="utf-8"><style>body{{font-family:sans-serif;color:#111;margin:20px}}table{{width:100%;max-width:560px;border-collapse:collapse;margin-top:12px}}th,td{{text-align:left;padding:8px 0;border-bottom:1px solid #f1f5f9}}</style></head>
<body>
  <h2>Netflix Payment Confirmation</h2>
  <p>From: Netflix</p>
  <p>Date paid: {d_nf_start}</p>
  <table>
    <tr><th>Subscription</th><td>Standard Monthly Plan</td></tr>
    <tr><th>Billing period</th><td>{d_nf_start} through {d_nf_end}</td></tr>
    <tr><th>Amount paid</th><td>$15.49</td></tr>
  </table>
</body>
</html>"#
        );
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
        let hw_pdf_bytes = build_visual_demo_fapiao_pdf(
            "2631200000089990",
            "Apple Electronic Products Commerce Co., Ltd.",
            "MacBook Air M4 16GB+512GB Midnight",
            &format!("Issued {hw_buy_date} (1Y Warranty to {hw_warranty_end})"),
            "CNY 8999.00",
            &[
                "增值税电子普通发票（示例）".to_string(),
                "销售方名称：苹果电子产品商贸有限公司".to_string(),
                "商品名称：MacBook Air M4 16G+512G 午夜色".to_string(),
                format!("开票日期：{hw_buy_date}"),
                "价税合计（小写）¥8999.00".to_string(),
            ],
        );
        let hw_pdf = tmp_dir.join("示例-MacBook-Air硬件保修发票.pdf");
        let _ = fs::write(&hw_pdf, &hw_pdf_bytes);
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
                notes: Some("官方一年保修，电子发票已保存".into()),
                asset_kind: Some("hardware".into()),
                warranty_end: Some(hw_warranty_end),
                license_key: None,
                license_scope: None,
                reimburse_status: Some("none".into()),
                auth: auth.clone(),
            });
        }

        // 7. Perpetual software license (Raycast Pro $99.00 with AEAD-encrypted license key)
        let lic_html = format!(
            r#"<!doctype html>
<html>
<head><meta charset="utf-8"><style>body{{font-family:sans-serif;color:#111;margin:20px}}.card{{border:1px solid #e2e8f0;border-radius:10px;padding:18px;max-width:560px}}table{{width:100%;border-collapse:collapse;margin-top:12px}}th,td{{text-align:left;padding:8px 0;border-bottom:1px solid #f1f5f9}}</style></head>
<body>
  <div class="card">
    <h2>Raycast Store Receipt (Sample)</h2>
    <p>From: Raycast Technologies Ltd.</p>
    <p>Date paid: {d_raycast}</p>
    <table>
      <tr><th>Product</th><td>Raycast Pro Perpetual License (One-Time)</td></tr>
      <tr><th>Amount paid</th><td>$99.00</td></tr>
    </table>
  </div>
</body>
</html>"#
        );
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
                bill_date: Some(d_raycast),
                notes: Some("密钥已加密保存在本地".into()),
                asset_kind: Some("license".into()),
                warranty_end: None,
                license_key: Some("RCST-DEMO-8842-K9X2".into()),
                license_scope: Some("终身买断 · 最多激活 3 台设备".into()),
                reimburse_status: Some("none".into()),
                auth: auth.clone(),
            });
        }

        // 8. Company reimbursable invoice PDF (腾讯云 ¥420.00, pending reimbursement)
        let reimb_pdf_bytes = build_visual_demo_fapiao_pdf(
            "2611000000042000",
            "Tencent Cloud Computing (Beijing) Co., Ltd.",
            "R&D Test Cloud Server Resource Pack",
            &format!("Issued {d_tencent}"),
            "CNY 420.00",
            &[
                "增值税电子普通发票（示例）".to_string(),
                "销售方名称：腾讯云计算（北京）有限责任公司".to_string(),
                "商品名称：研发测试环境云服务器资源包".to_string(),
                format!("开票日期：{d_tencent}"),
                "价税合计（小写）¥420.00".to_string(),
            ],
        );
        let reimb_pdf = tmp_dir.join("示例-腾讯云企业报销发票.pdf");
        let _ = fs::write(&reimb_pdf, &reimb_pdf_bytes);
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
                bill_date: Some(d_tencent),
                notes: Some("测试环境费用，待提交报销".into()),
                asset_kind: Some("reimbursable".into()),
                warranty_end: None,
                license_key: None,
                license_scope: None,
                reimburse_status: Some("pending".into()),
                auth: auth.clone(),
            });
        }

        drop(tmp_guard);
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
        let tmp_guard = match PrivateTempDir::create("man") {
            Ok(d) => d,
            Err(err) => return HttpResponse::error(500, "temp_failed", &err.to_string()),
        };
        let temp_file = tmp_guard.path().join(&display_name);
        if let Err(err) = fs::write(&temp_file, note_text.as_bytes()) {
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
        drop(tmp_guard);
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

fn pdf_ascii_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push(' '),
        }
    }
    out
}

fn build_visual_demo_fapiao_pdf(
    invoice_no: &str,
    seller_en: &str,
    item_en: &str,
    term_en: &str,
    amount_en: &str,
    zh_lines: &[String],
) -> Vec<u8> {
    let mut stream = String::new();
    // 1. Invisible UTF-16BE Chinese extraction layer placed at the top margin (y = 396 down by 5pt)
    //    so each Chinese line stays in its own row and never collides with visible Helvetica text (y <= 338).
    stream.push_str("BT\n/F1 1 Tf\n3 Tr\n5 TL\n36 396 Td\n");
    for (idx, line) in zh_lines.iter().enumerate() {
        if idx > 0 {
            stream.push_str("T* ");
        }
        stream.push_str(&utf16be_pdf_hex(line));
        stream.push_str(" Tj\n");
    }
    stream.push_str("0 Tr\nET\n");

    // 2. Crisp vector Chinese e-fapiao layout using PDF built-in Helvetica / Helvetica-Bold
    // Warm paper background & crimson double border
    stream.push_str("q\n0.992 0.988 0.976 rg\n0 0 595 400 re f\n");
    stream.push_str("0.74 0.16 0.16 RG 1.8 w\n24 24 547 352 re S\n");
    stream.push_str("0.74 0.16 0.16 RG 0.6 w\n29 29 537 342 re S\n");
    // Header crimson double underline
    stream.push_str("0.74 0.16 0.16 RG 1.4 w\n150 326 m 445 326 l S\n");
    stream.push_str("0.74 0.16 0.16 RG 0.7 w\n150 322 m 445 322 l S\n");
    // Decorative circular stamp on top-right
    stream.push_str("0.80 0.22 0.22 RG 1.2 w\n");
    stream.push_str("488 345 m 506 345 520 331 520 313 c 520 295 506 281 488 281 c 470 281 456 295 456 313 c 456 331 470 345 488 345 c S\n");
    // Inner table grid
    stream.push_str("0.74 0.16 0.16 RG 0.8 w\n42 82 511 196 re S\n");
    stream.push_str("42 232 m 553 232 l S\n42 184 m 553 184 l S\n42 132 m 553 132 l S\n");
    stream.push_str("168 82 m 168 278 l S\n");
    stream.push_str("Q\n");

    // Title & stamp label
    let inv_esc = pdf_ascii_escape(invoice_no);
    let sel_esc = pdf_ascii_escape(seller_en);
    let itm_esc = pdf_ascii_escape(item_en);
    let trm_esc = pdf_ascii_escape(term_en);
    let amt_esc = pdf_ascii_escape(amount_en);

    stream.push_str(&format!(
        "BT\n0.74 0.16 0.16 rg\n/F2 16 Tf\n158 338 Td (ELECTRONIC VAT INVOICE  /  DEMO) Tj\nET\n\
         BT\n0.80 0.22 0.22 rg\n/F2 8 Tf\n466 315 Td (DEMO SEAL) Tj\n2 -11 Td (SUBVAULT) Tj\nET\n\
         BT\n0.35 0.35 0.38 rg\n/F1 9 Tf\n42 292 Td (Invoice No: {inv_esc}) Tj\n230 0 Td (Sample Document - Local Preview) Tj\nET\n"
    ));

    // Table labels (left column, muted crimson)
    stream.push_str(
        "BT\n0.62 0.18 0.18 rg\n/F2 10 Tf\n\
         54 251 Td (Issued By) Tj\n\
         0 -48 Td (Description) Tj\n\
         0 -50 Td (Term / Date) Tj\n\
         0 -50 Td (Total Paid) Tj\nET\n",
    );

    // Table values (right column, crisp charcoal ink)
    stream.push_str(&format!(
        "BT\n0.12 0.13 0.16 rg\n/F1 11 Tf\n\
         182 251 Td ({sel_esc}) Tj\n\
         0 -48 Td ({itm_esc}) Tj\n\
         0 -50 Td ({trm_esc}) Tj\n\
         /F2 14 Tf\n0 -50 Td ({amt_esc}) Tj\nET\n\
         BT\n0.45 0.45 0.50 rg\n/F1 8 Tf\n\
         42 48 Td (SubVault Format 6 Sample PDF - Original bytes preserved verbatim; bilingual extraction verified.) Tj\nET\n"
    ));

    let raw_stream = stream.as_bytes();
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    let o1 = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let o2 = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    let o3 = pdf.len();
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 400] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    let o4 = pdf.len();
    pdf.extend_from_slice(
        format!("4 0 obj\n<< /Length {} >>\nstream\n", raw_stream.len()).as_bytes(),
    );
    pdf.extend_from_slice(raw_stream);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let o5 = pdf.len();
    pdf.extend_from_slice(
        b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );
    let o6 = pdf.len();
    pdf.extend_from_slice(
        b"6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>\nendobj\n",
    );
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
    for off in [o1, o2, o3, o4, o5, o6] {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x0E, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '$' => [0x04, 0x0F, 0x14, 0x0E, 0x05, 0x1E, 0x04],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '#' => [0x0A, 0x1F, 0x0A, 0x0A, 0x1F, 0x0A, 0x00],
        _ => [0x00; 7],
    }
}

fn png_chunk(chunk_type: &[u8; 4], data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in chunk_type.iter().chain(data.iter()) {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}

fn build_demo_receipt_png(date_iso: &str) -> Vec<u8> {
    const W: usize = 360;
    const H: usize = 420;
    let mut rgb = vec![0u8; W * H * 3];

    // Background desk tint (#E8ECEF) and paper receipt sheet (#FCFBF8)
    for y in 0..H {
        for x in 0..W {
            let in_paper = (20..W - 20).contains(&x) && (18..H - 18).contains(&y);
            let on_border = in_paper && (x == 20 || x == W - 21 || y == 18 || y == H - 19);
            let color: [u8; 3] = if on_border {
                [210, 214, 220]
            } else if in_paper {
                [252, 251, 248]
            } else {
                [235, 239, 243]
            };
            let idx = (y * W + x) * 3;
            rgb[idx..idx + 3].copy_from_slice(&color);
        }
    }

    let mut fill_rect = |rx: usize, ry: usize, rw: usize, rh: usize, col: [u8; 3]| {
        for y in ry..(ry + rh).min(H) {
            for x in rx..(rx + rw).min(W) {
                let idx = (y * W + x) * 3;
                rgb[idx..idx + 3].copy_from_slice(&col);
            }
        }
    };

    // Top accent bar & dashed dividers
    fill_rect(20, 18, W - 40, 8, [31, 111, 95]);
    for dx in (38..W - 38).step_by(10) {
        fill_rect(dx, 108, 6, 2, [198, 203, 210]);
        fill_rect(dx, 252, 6, 2, [198, 203, 210]);
        fill_rect(dx, 326, 6, 2, [198, 203, 210]);
    }

    let mut draw_text = |text: &str, x0: usize, y0: usize, scale: usize, col: [u8; 3]| {
        let mut cx = x0;
        for ch in text.chars() {
            let rows = glyph_5x7(ch);
            for (ry, &bits) in rows.iter().enumerate() {
                for rx in 0..5 {
                    if (bits >> (4 - rx)) & 1 != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                let px = cx + rx * scale + sx;
                                let py = y0 + ry * scale + sy;
                                if px < W && py < H {
                                    let idx = (py * W + px) * 3;
                                    rgb[idx..idx + 3].copy_from_slice(&col);
                                }
                            }
                        }
                    }
                }
            }
            cx += 6 * scale;
        }
    };

    let ink = [28, 32, 38];
    let muted = [100, 110, 122];
    let accent = [24, 102, 86];

    draw_text("COFFEE ROASTERS LAB", 46, 44, 2, ink);
    draw_text("SAMPLE RECEIPT IMAGE", 62, 74, 2, muted);
    draw_text(&format!("DATE: {date_iso}"), 44, 130, 2, ink);
    draw_text("ORDER: #CR-2048", 44, 158, 2, muted);
    draw_text("ITEM: BEAN BOX", 44, 192, 2, ink);
    draw_text("CYCLE: MONTHLY", 44, 220, 2, muted);
    draw_text("TOTAL", 44, 274, 2, muted);
    draw_text("USD 19.99", 154, 270, 3, accent);
    draw_text("IMAGE ONLY - NO OCR", 66, 348, 2, muted);
    draw_text("TRY QUICK TRIAGE", 84, 372, 2, accent);

    // Build raw PNG scanlines (filter byte 0 per row)
    let mut raw = Vec::with_capacity(H * (1 + W * 3));
    for y in 0..H {
        raw.push(0);
        let row_start = y * W * 3;
        raw.extend_from_slice(&rgb[row_start..row_start + W * 3]);
    }
    let idat = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);

    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1A\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(W as u32).to_be_bytes());
    ihdr.extend_from_slice(&(H as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type RGB
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    png_chunk(b"IHDR", &ihdr, &mut png);
    png_chunk(b"IDAT", &idat, &mut png);
    png_chunk(b"IEND", &[], &mut png);
    png
}

fn format_zh_epoch_days(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}年{m}月{d}日")
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
    let passphrase = Zeroizing::new(body["passphrase"].as_str().unwrap_or("").to_string());
    if protection == ProtectionMode::Passphrase && passphrase.chars().count() < 6 {
        return HttpResponse::error(
            400,
            "weak_passphrase",
            "为保护您的真实账单，密码至少需要 6 位字符",
        );
    }

    let vault_path = PathBuf::from(path_str);
    if vault_path.exists() {
        return HttpResponse::error(
            409,
            "vault_path_exists",
            "目标文件夹或文件已存在，请换一个新名称",
        );
    }
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
        AuthInput::from_direct_passphrase((*passphrase).clone())
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

    set_active_vault(
        state,
        ActiveVault {
            path: vault_path.clone(),
            vault_id: unlocked.vault_id.clone(),
            vault_format: unlocked.vault_format,
            protection: unlocked.protection,
            cipher: unlocked.cipher,
            op_lock: Mutex::new(()),
        },
    );
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

    set_active_vault(
        state,
        ActiveVault {
            path: vault_path.clone(),
            vault_id: unlocked.vault_id.clone(),
            vault_format: unlocked.vault_format,
            protection: unlocked.protection,
            cipher: unlocked.cipher,
            op_lock: Mutex::new(()),
        },
    );
    remember_recent_vault(&vault_path);

    api_workspace(state)
}

fn api_vault_lock(state: &ServerState) -> HttpResponse {
    clear_active_vault(state);
    HttpResponse::json(200, json!({ "ok": true, "locked": true }))
}

fn api_rotate_passphrase(req: &HttpRequest, state: &ServerState) -> HttpResponse {
    let body = match parse_json_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let new_pass = Zeroizing::new(body["new_passphrase"].as_str().unwrap_or("").to_string());
    if new_pass.chars().count() < 6 {
        return HttpResponse::error(400, "weak_passphrase", "新密码至少需要 6 位字符");
    }
    let rotate_recovery = body["rotate_recovery_key"].as_bool().unwrap_or(false);
    let current_pass = body["current_passphrase"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| Zeroizing::new(s.to_string()));
    let recovery_key = body["recovery_key"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Zeroizing::new(s.to_string()));

    with_active(state, |active| {
        let auth = if let Some(r) = recovery_key {
            AuthInput::from_direct_recovery_key((*r).clone())
        } else if let Some(p) = current_pass {
            AuthInput::from_direct_passphrase((*p).clone())
        } else {
            return HttpResponse::error(
                400,
                "current_credential_required",
                "修改主密码前请输入当前主密码或离线恢复密钥进行身份验证",
            );
        };
        let rep = engine::rotate_passphrase(engine::RotatePassphraseRequest {
            vault: active.path.clone(),
            auth,
            new_passphrase: NewPassphraseInput::from_direct((*new_pass).clone()),
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
        plan_name: Option<String>,
        asset_kind: String,
        vendor_display: String,
    }
    let mut vendor_groups: BTreeMap<(String, String), Vec<VendorBillPoint>> = BTreeMap::new();
    // Deduplicate recurring monthly/yearly equivalents by unique subscription identity
    // (currency, vendor, plan, account, cycle) keeping the latest chronological bill rate.
    #[allow(clippy::type_complexity)]
    let mut latest_recurring: BTreeMap<
        (String, String, String, String, String),
        (String, i64, i64),
    > = BTreeMap::new();

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

        let has_amount_conflict = amount_fact.is_none()
            && src_cands.iter().any(|c| {
                matches!(
                    c["value"]["reason"].as_str(),
                    Some("conflicting_amounts" | "conflicting_candidates")
                )
            });
        let has_period_conflict = period_fact.is_none()
            && src_cands
                .iter()
                .any(|c| c["value"]["reason"].as_str() == Some("conflicting_periods"));
        let has_conflict = has_amount_conflict || has_period_conflict;
        let has_credit_or_order_warning = src_cands.iter().any(|c| {
            matches!(
                c["kind"].as_str(),
                Some("credit_note_total" | "unjudged_order_total")
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
                let date_key_for_rec = ann
                    .and_then(|a| a.bill_date.as_deref())
                    .or_else(|| {
                        period_fact
                            .as_ref()
                            .and_then(|p| p["value"]["period_start"].as_str())
                    })
                    .unwrap_or("")
                    .to_string();
                match cycle.as_str() {
                    "monthly" | "yearly" | "quarterly" | "weekly" => {
                        let v_key = ann
                            .and_then(|a| a.vendor.as_deref())
                            .map(|s| s.trim().to_lowercase())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| source_id.clone());
                        let p_key = ann
                            .and_then(|a| a.plan_name.as_deref())
                            .map(|s| s.trim().to_lowercase())
                            .unwrap_or_default();
                        let a_key = ann
                            .and_then(|a| a.account_label.as_deref())
                            .map(|s| s.trim().to_lowercase())
                            .unwrap_or_default();
                        let rec_key = (cur.clone(), v_key, p_key, a_key, cycle.clone());
                        let should_replace = match latest_recurring.get(&rec_key) {
                            None => true,
                            Some((prev_date, prev_rec_at, _)) => {
                                (&date_key_for_rec, latest_recorded_at) >= (prev_date, *prev_rec_at)
                            }
                        };
                        if should_replace {
                            latest_recurring
                                .insert(rec_key, (date_key_for_rec, latest_recorded_at, cents));
                        }
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
                            plan_name: ann.and_then(|a| a.plan_name.clone()),
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

        // Detect price drift per unique (billing_cycle, account_label, plan_name) sub-track
        // so separate accounts or tiers under the same vendor never trigger false hikes.
        let mut subtracks: BTreeMap<(String, String, String), Vec<&VendorBillPoint>> =
            BTreeMap::new();
        for p in &pts {
            if matches!(
                p.billing_cycle.as_str(),
                "monthly" | "yearly" | "quarterly" | "weekly"
            ) {
                let acc_k = p
                    .account_label
                    .as_deref()
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                let plan_k = p
                    .plan_name
                    .as_deref()
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                subtracks
                    .entry((p.billing_cycle.clone(), acc_k, plan_k))
                    .or_default()
                    .push(p);
            }
        }
        let mut price_drift = Value::Null;
        let mut latest_drift_key: Option<(String, i64)> = None;
        for track in subtracks.values() {
            if track.len() < 2 {
                continue;
            }
            let curr = track[track.len() - 1];
            let Some(prev) = track[..track.len() - 1].iter().rev().find(|p| p.cents > 0) else {
                continue;
            };
            if curr.cents == prev.cents {
                continue;
            }
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
                let acc_suffix = curr
                    .account_label
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|a| format!("（{a}）"))
                    .unwrap_or_default();
                radar_hikes.push(json!({
                    "kind": "price_hike",
                    "severity": "amber",
                    "vendor": vendor_name,
                    "currency": cur,
                    "pct_change": pct_signed,
                    "source_id": curr.source_id,
                    "label": format!("{vendor_name}{acc_suffix} 涨价 +{pct_abs}%"),
                    "detail": format!("{} {} → {} {}", cur, prev.amount_text, cur, curr.amount_text),
                }));
            }
            let key = (curr.date.clone(), curr.recorded_at);
            if latest_drift_key.as_ref().is_none_or(|k| key >= *k) {
                latest_drift_key = Some(key);
                price_drift = drift_obj;
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
                    "detail": "点击查看并更新报销状态",
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

    // Fold deduplicated recurring subscriptions into per-currency monthly/yearly equivalents
    for ((cur, _, _, _, cycle), (_, _, cents)) in latest_recurring {
        let acc = by_currency.entry(cur).or_default();
        acc.recurring_count += 1;
        match cycle.as_str() {
            "monthly" => {
                acc.monthly_equiv_cents += cents;
                acc.yearly_equiv_cents += cents * 12;
            }
            "yearly" => {
                acc.monthly_equiv_cents += (cents + 6) / 12;
                acc.yearly_equiv_cents += cents;
            }
            "quarterly" => {
                acc.monthly_equiv_cents += (cents + 1) / 3;
                acc.yearly_equiv_cents += cents * 4;
            }
            "weekly" => {
                acc.monthly_equiv_cents += (cents * 52 + 6) / 12;
                acc.yearly_equiv_cents += cents * 52;
            }
            _ => {}
        }
    }

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
        let tmp_guard = match PrivateTempDir::create("wb") {
            Ok(d) => d,
            Err(err) => {
                return HttpResponse::error(
                    500,
                    "temp_failed",
                    &format!("临时目录创建失败：{err}"),
                );
            }
        };
        let temp_file = tmp_guard.path().join(&filename);
        if let Err(err) = fs::write(&temp_file, &bytes) {
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

        drop(tmp_guard);

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
    let direction = body["direction"].as_str().unwrap_or("").trim();

    if parent_source_id.is_empty() || amount_text.is_empty() {
        return HttpResponse::error(400, "missing_fields", "缺少必填流水信息");
    }
    if matches!(direction, "income" | "refund" | "neutral") {
        return HttpResponse::error(
            400,
            "csv_row_not_expense",
            "仅支出类流水可转为正式账单；退款、收入或中性交易不应计入实付支出",
        );
    }
    let Some(norm_amt) = crate::real_extract::normalize_amount_text(amount_text) else {
        return HttpResponse::error(400, "bad_amount", "该流水行金额格式无效，无法记为支出账单");
    };
    let Some(norm_cur) = crate::real_extract::normalize_currency(currency) else {
        return HttpResponse::error(400, "bad_currency", "该流水行币种不受支持");
    };

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
            "SubVault CSV Row Evidence\nParent CSV Source: {parent_source_id}\nParent CSV SHA-256: {parent_sha}\nCSV Row: {row_number}\nDate: {date}\nMerchant: {merchant}\nDescription: {description}\nCurrency: {norm_cur}\nAmount: {norm_amt}\nRaw CSV Line: {raw_line}\n"
        );
        let safe_merchant = if merchant.is_empty() {
            "csv-item"
        } else {
            merchant
        };
        let display_name =
            sanitize_filename(&format!("{safe_merchant}-{date}-row{row_number}.txt"));

        let tmp_guard = match PrivateTempDir::create("csvp") {
            Ok(d) => d,
            Err(err) => return HttpResponse::error(500, "temp_failed", &err.to_string()),
        };
        let temp_file = tmp_guard.path().join(&display_name);
        if let Err(err) = fs::write(&temp_file, receipt_text.as_bytes()) {
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
        drop(tmp_guard);
        if imp.code != EXIT_OK {
            return map_engine_error(&imp);
        }
        let new_source_id = imp.body["source_id"].as_str().unwrap_or("").to_string();
        let reused = imp.body["storage_reused"].as_bool().unwrap_or(false);

        if !reused {
            let fact_cmd = ids::random_id("csvf_").unwrap_or_else(|_| "csvf_1".into());
            let rec = engine::record_manual_fact(engine::ManualFactRequest {
                vault: active.path.clone(),
                command_id: fact_cmd,
                source_id: new_source_id.clone(),
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
            let ann = engine::annotate_source(engine::AnnotateRequest {
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
            if ann.code != EXIT_OK {
                return map_engine_error(&ann);
            }
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

    // Validate amount, currency, and service period upfront before mutating annotations
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

    let validated_amount = if let Some(raw_amt) = incoming_amt {
        let Some(norm_amt) = crate::real_extract::normalize_amount_text(raw_amt) else {
            return HttpResponse::error(400, "bad_amount", "金额格式不正确，例如 29.99 或 98");
        };
        let norm_cur = match incoming_cur {
            Some(raw_cur) => match crate::real_extract::normalize_currency(raw_cur) {
                Some(c) => c,
                None => return HttpResponse::error(400, "bad_currency", "不支持的币种代码"),
            },
            None => "USD",
        };
        Some((norm_cur, norm_amt))
    } else {
        None
    };

    let validated_period = match (incoming_pstart, incoming_pend) {
        (Some(ps), Some(pe)) => {
            if parse_iso_days(&ps).is_none() || parse_iso_days(&pe).is_none() || ps > pe {
                return HttpResponse::error(
                    400,
                    "bad_period",
                    "服务周期日期无效或开始日期晚于结束日期",
                );
            }
            Some((ps, pe))
        }
        (None, None) => None,
        _ => {
            return HttpResponse::error(
                400,
                "period_requires_both_bounds",
                "服务周期需同时填写开始日期与结束日期",
            );
        }
    };

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

        if let Some((norm_cur, norm_amt)) = validated_amount {
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
        if let Some((ps, pe)) = validated_period {
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

fn percent_encode_rfc5987(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len() * 2);
    for &b in input.as_bytes() {
        let is_attr_char = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            );
        if is_attr_char {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

fn content_disposition(disposition_type: &str, display_name: &str) -> String {
    let clean: String = display_name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '/' | ';'))
        .take(180)
        .collect();
    let clean = if clean.trim().is_empty() {
        "bill.bin".to_string()
    } else {
        clean.trim().to_string()
    };
    let ascii_fallback: String = clean
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && !matches!(c, '"' | '\\' | ';') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let ascii_fallback = if ascii_fallback.trim_matches('_').is_empty() {
        "bill.bin".to_string()
    } else {
        ascii_fallback
    };
    let utf8_encoded = percent_encode_rfc5987(&clean);
    format!("{disposition_type}; filename=\"{ascii_fallback}\"; filename*=UTF-8''{utf8_encoded}")
}

fn api_source_raw(req: &HttpRequest, state: &ServerState, source_id: &str) -> HttpResponse {
    let download = req
        .query
        .get("download")
        .is_some_and(|v| v == "1" || v == "true");
    with_active(state, |active| {
        match engine::read_source_bytes(&active.path, source_id, &active.auth()) {
            Ok((bytes, media_type, display_name)) => {
                let base_mime = media_type
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                let (content_type, force_download, csp) = if download {
                    (
                        "application/octet-stream".to_string(),
                        true,
                        Some("sandbox; default-src 'none'"),
                    )
                } else {
                    match base_mime.as_str() {
                        "text/html" => (
                            "text/html; charset=utf-8".to_string(),
                            false,
                            Some("sandbox; default-src 'none'; style-src 'unsafe-inline'; img-src data:"),
                        ),
                        "text/plain" | "text/csv" => (
                            "text/plain; charset=utf-8".to_string(),
                            false,
                            Some("sandbox; default-src 'none'"),
                        ),
                        "application/pdf" => ("application/pdf".to_string(), false, None),
                        "image/png" | "image/jpeg" | "image/webp" => (
                            base_mime,
                            false,
                            Some("sandbox; default-src 'none'"),
                        ),
                        // Disallow active or ambiguous types (e.g. image/svg+xml, application/xhtml+xml, js/xml) inline
                        _ => (
                            "application/octet-stream".to_string(),
                            true,
                            Some("sandbox; default-src 'none'"),
                        ),
                    }
                };
                let mut resp = HttpResponse::ok_bytes(&content_type, bytes);
                if let Some(policy) = csp {
                    resp = resp.with_header("Content-Security-Policy", policy);
                }
                let disp = content_disposition(
                    if force_download {
                        "attachment"
                    } else {
                        "inline"
                    },
                    &display_name,
                );
                resp.with_header("Content-Disposition", disp)
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
                    let (decoded, _) = crate::real_extract::decode_csv_bytes(&bytes);
                    let lines = crate::real_extract::html_to_lines(&decoded);
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
        let rep =
            engine::redact_vault_source_in_memory(&active.path, &source_id, terms, &active.auth());
        if rep.code != EXIT_OK {
            return map_engine_error(&rep);
        }
        let total = rep.body["counts"]["total_redactions"]
            .as_u64()
            .or_else(|| rep.body["total_redactions"].as_u64())
            .unwrap_or(0);
        HttpResponse::json(
            200,
            json!({
                "ok": true,
                "redacted_text": rep.body["redacted_text"].as_str().unwrap_or(""),
                "counts": rep.body["counts"],
                "total_redactions": total,
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
            let dup_str = item["duplicate_hits"].as_u64().unwrap_or(0).to_string();
            let cols: [(&str, bool); 18] = [
                (kind_zh, false),
                (ann["vendor"].as_str().unwrap_or(""), false),
                (ann["plan_name"].as_str().unwrap_or(""), false),
                (ann["account_label"].as_str().unwrap_or(""), false),
                (cycle_zh, false),
                (ann["category"].as_str().unwrap_or(""), false),
                (amt["currency"].as_str().unwrap_or(""), false),
                (amt["amount_text"].as_str().unwrap_or(""), true),
                (ann["bill_date"].as_str().unwrap_or(""), false),
                (per["period_start"].as_str().unwrap_or(""), false),
                (per["period_end"].as_str().unwrap_or(""), false),
                (ann["warranty_end"].as_str().unwrap_or(""), false),
                (reimb_zh, false),
                (ann["license_scope"].as_str().unwrap_or(""), false),
                (item["display_name"].as_str().unwrap_or(""), false),
                (&dup_str, true),
                (item["content_sha256"].as_str().unwrap_or(""), false),
                (ann["notes"].as_str().unwrap_or(""), false),
            ];
            let line = cols
                .iter()
                .map(|&(c, numeric)| csv_escape_excel(c, numeric))
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out.into_bytes())
}

fn path_is_inside_vault(vault: &Path, target: &Path) -> bool {
    let vault_abs = fs::canonicalize(vault).unwrap_or_else(|_| vault.to_path_buf());
    let mut probe = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(target)
    };
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    while !probe.exists() {
        let Some(name) = probe.file_name() else {
            break;
        };
        suffix.push(name.to_os_string());
        let Some(parent) = probe.parent() else {
            break;
        };
        probe = parent.to_path_buf();
    }
    let mut resolved = fs::canonicalize(&probe).unwrap_or(probe);
    for seg in suffix.into_iter().rev() {
        resolved.push(seg);
    }
    resolved == vault_abs || resolved.starts_with(&vault_abs)
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

    with_active(state, |active| {
        if path_is_inside_vault(&active.path, &out_dir) {
            return HttpResponse::error(
                400,
                "output_inside_vault",
                "导出位置不能放在账本文件夹内部，请选其他目录",
            );
        }
        if let Some(parent) = out_dir.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                if let Err(err) = fs::create_dir_all(parent) {
                    return HttpResponse::error(
                        400,
                        "parent_create_failed",
                        &format!("无法创建导出上级目录：{err}"),
                    );
                }
            }
        }
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
        let pkg_files =
            match engine::export_package_in_memory(&active.path, false, ack, &active.auth()) {
                Ok(files) => files,
                Err(fail) => return map_engine_error(&fail.into_report()),
            };
        let review_text = match engine::review_sheet_in_memory(&active.path, active.auth()) {
            Ok(text) => text,
            Err(rep) => return map_engine_error(&rep),
        };
        let csv_bytes = match build_excel_csv_bytes(active) {
            Ok(bytes) => bytes,
            Err(resp) => return resp,
        };

        let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(pkg_files.len() + 3);
        for file in pkg_files {
            entries.push((format!("package/{}", file.path), file.bytes));
        }
        entries.push(("ledger-excel.csv".into(), csv_bytes));
        entries.push(("review-sheet.txt".into(), review_text.into_bytes()));
        entries.push((
            "README.txt".into(),
            concat!(
                "SubVault 离线退出备份包说明\n",
                "============================\n\n",
                "1. package/        标准可独立校验的 offline-exit-1 退出包（含 manifest.json、records.json、facts.csv 与全部 originals/ 原件）。\n",
                "                   解压后可脱离 SubVault 数据库运行以下校验命令：\n",
                "                     subvault verify-package --package ./package\n",
                "                   或在 SubVault「备份与核对」中选择解压后的 package 目录校验。\n",
                "2. ledger-excel.csv 含 UTF-8 BOM 的中文账单汇总表，可直接用 Excel 打开（不含明文软件密钥）。\n",
                "3. review-sheet.txt 纯文本审阅单，记录所有来源、事实与修订历史。\n\n",
                "安全提醒：本备份包内文件为明文格式，请妥善保管，勿上传至不受信任的云盘。\n"
            )
            .as_bytes()
            .to_vec(),
        ));

        let zip_bytes = match build_store_zip(&entries) {
            Ok(bytes) => bytes,
            Err(resp) => return resp,
        };
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
            "ok": true,
            "verified": rep.code == EXIT_OK,
            "report": rep.body,
        }),
    )
}

// --- Pure-Rust Minimal Store ZIP Builder (CRC32, zero extra crates) ---

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

fn build_store_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, HttpResponse> {
    let count = u16::try_from(entries.len()).map_err(|_| {
        HttpResponse::error(
            413,
            "zip_too_many_entries",
            "备份包文件数量超出标准 ZIP 上限",
        )
    })?;
    let mut zip = Vec::new();
    let mut central = Vec::new();

    for (name, data) in entries {
        let name_bytes = name.as_bytes();
        let name_len = u16::try_from(name_bytes.len())
            .map_err(|_| HttpResponse::error(400, "zip_name_too_long", "备份包内文件名过长"))?;
        let crc = crc32(data);
        let size = u32::try_from(data.len()).map_err(|_| {
            HttpResponse::error(
                413,
                "zip_entry_too_large",
                "单个备份文件超出标准 ZIP 4 GiB 上限",
            )
        })?;
        let offset = u32::try_from(zip.len()).map_err(|_| {
            HttpResponse::error(413, "zip_too_large", "备份包总大小超出标准 ZIP 4 GiB 上限")
        })?;

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
        zip.extend_from_slice(&name_len.to_le_bytes());
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
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let central_offset = u32::try_from(zip.len()).map_err(|_| {
        HttpResponse::error(413, "zip_too_large", "备份包总大小超出标准 ZIP 4 GiB 上限")
    })?;
    let central_size = u32::try_from(central.len()).map_err(|_| {
        HttpResponse::error(413, "zip_too_large", "备份包目录区超出标准 ZIP 4 GiB 上限")
    })?;
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
    Ok(zip)
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
        "vault_path_exists" | "path_exists" | "output_exists" => {
            (409, "目标文件夹或文件已存在，请换一个新名称")
        }
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
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .take(160)
        .collect();
    let trimmed = cleaned.trim_end_matches(['.', ' ']);
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || ids::is_windows_reserved_name(trimmed)
    {
        "bill.bin".to_string()
    } else {
        trimmed.to_string()
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
    let cleaned = text.trim().replace([',', ' '], "");
    if cleaned.is_empty() {
        return None;
    }
    let (neg, rest) = if let Some(r) = cleaned.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = cleaned.strip_prefix('+') {
        (false, r)
    } else {
        (false, cleaned.as_str())
    };
    if rest.is_empty() {
        return None;
    }
    let cents = if let Some((whole, frac)) = rest.split_once('.') {
        if whole.is_empty() || frac.is_empty() || frac.len() > 2 {
            return None;
        }
        if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let w: i64 = whole.parse().ok()?;
        let f_raw: i64 = frac.parse().ok()?;
        let f = if frac.len() == 1 { f_raw * 10 } else { f_raw };
        w.checked_mul(100)?.checked_add(f)?
    } else {
        if !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let w: i64 = rest.parse().ok()?;
        w.checked_mul(100)?
    };
    if neg {
        cents.checked_neg()
    } else {
        Some(cents)
    }
}

fn cents_to_text(cents: i64) -> String {
    let abs = cents.abs();
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

fn csv_escape_excel(field: &str, numeric: bool) -> String {
    let owned: String;
    let safe = if !numeric {
        let trimmed = field.trim_start_matches(' ');
        if trimmed
            .chars()
            .next()
            .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'))
        {
            owned = format!("'{field}");
            owned.as_str()
        } else {
            field
        }
    } else {
        field
    };
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe.to_string()
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
    if s.len() != 10 {
        return None;
    }
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || y < 1900 || !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    let is_leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max_d = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    if d > max_d {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn format_epoch_days(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}
