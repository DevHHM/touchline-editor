//! Loopback-only automation transport for the same semantic engine as WPF.
//! One request at a time; all edits still pass through preview tokens/journals.
use crate::{
    bridge::api::{ApiSession, capabilities},
    transaction,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
};

const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq)]
struct Options {
    port: u16,
    discovery: PathBuf,
}
impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut out = Self {
            port: 0,
            discovery: transaction::data_dir().join("api.json"),
        };
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--api" => {}
                "--port" => {
                    i += 1;
                    out.port = args.get(i).context("--port requires 0–65535")?.parse()?;
                }
                "--discovery" => {
                    i += 1;
                    out.discovery =
                        PathBuf::from(args.get(i).context("--discovery requires a path")?);
                }
                other => bail!("Unknown API option: {other}"),
            }
            i += 1;
        }
        if !out.discovery.is_absolute() {
            out.discovery = std::env::current_dir()?.join(out.discovery);
        }
        Ok(out)
    }
}

struct Instance {
    mutex: HANDLE,
    discovery: PathBuf,
    token: String,
}
impl Instance {
    fn acquire(discovery: &Path) -> Result<Self> {
        use std::os::windows::ffi::OsStrExt;
        let parent = discovery
            .parent()
            .context("Discovery path must have a parent")?;
        std::fs::create_dir_all(parent)?;
        let canonical = parent
            .canonicalize()?
            .join(discovery.file_name().context("Discovery file missing")?);
        let mut hash = 0xcbf29ce484222325u64;
        for b in canonical.to_string_lossy().to_lowercase().bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let name: Vec<u16> = std::ffi::OsStr::new(&format!("Local\\Touchline.Api.{hash:016x}"))
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        ensure!(
            !mutex.is_null(),
            "Cannot acquire API instance lock: {}",
            std::io::Error::last_os_error()
        );
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(mutex);
            }
            bail!("Touchline API already running for {}", discovery.display());
        }
        let token = match random_token() {
            Ok(t) => t,
            Err(e) => {
                unsafe {
                    CloseHandle(mutex);
                }
                return Err(e);
            }
        };
        Ok(Self {
            mutex,
            discovery: canonical,
            token,
        })
    }
    fn publish(&self, port: u16) -> Result<()> {
        transaction::persist_json(
            &self.discovery,
            &json!({
                "protocol_version": 1, "service": "touchline-editor", "pid": std::process::id(),
                "base_url": format!("http://127.0.0.1:{port}"), "token": self.token,
                "executable": std::env::current_exe()?.to_string_lossy(),
            }),
        )
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        if let Ok(bytes) = std::fs::read(&self.discovery) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if v["pid"] == std::process::id() && v["token"] == self.token {
                    let _ = std::fs::remove_file(&self.discovery);
                }
            }
        }
        unsafe {
            CloseHandle(self.mutex);
        }
    }
}
fn random_token() -> Result<String> {
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(h: *mut std::ffi::c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
    }
    let mut bytes = [0u8; 32];
    let result = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            2,
        )
    };
    ensure!(result == 0, "Could not generate API authentication token");
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Debug)]
struct HttpError(u16, &'static str);
type HttpResult<T> = std::result::Result<T, HttpError>;
struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    length: usize,
}
fn parse_header(bytes: &[u8]) -> HttpResult<Request> {
    let source = std::str::from_utf8(bytes).map_err(|_| HttpError(400, "Invalid HTTP header"))?;
    let mut lines = source.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or_default().split(' ').collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return Err(HttpError(400, "HTTP/1.1 required"));
    }
    if !matches!(parts[0], "GET" | "POST") {
        return Err(HttpError(405, "GET or POST required"));
    }
    if !parts[1].starts_with('/') || parts[1].contains(['?', '#']) {
        return Err(HttpError(400, "Exact API route required"));
    }
    let mut headers = HashMap::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let Some((key, value)) = line.split_once(':') else {
            return Err(HttpError(400, "Invalid header"));
        };
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(HttpError(400, "Invalid header name"));
        }
        if value.bytes().any(|b| b < 32 && b != b'\t') {
            return Err(HttpError(400, "Invalid header value"));
        }
        if headers
            .insert(key.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err(HttpError(400, "Duplicate HTTP headers rejected"));
        }
    }
    if headers.contains_key("transfer-encoding") || headers.contains_key("expect") {
        return Err(HttpError(400, "Chunked and Expect requests unsupported"));
    }
    let length = match headers.get("content-length") {
        Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse().map_err(|_| HttpError(413, "Body too large"))?
        }
        Some(_) => return Err(HttpError(400, "Invalid Content-Length")),
        None if parts[0] == "POST" => return Err(HttpError(411, "Content-Length required")),
        None => 0,
    };
    if length > MAX_BODY {
        return Err(HttpError(413, "Maximum JSON request is 1 MiB"));
    }
    if parts[0] == "GET" && length != 0 {
        return Err(HttpError(400, "GET body unsupported"));
    }
    Ok(Request {
        method: parts[0].into(),
        path: parts[1].into(),
        headers,
        length,
    })
}
fn authenticate(req: &Request, token: &str, port: u16) -> HttpResult<()> {
    if req.headers.get("host").map(String::as_str) != Some(format!("127.0.0.1:{port}").as_str()) {
        return Err(HttpError(403, "Loopback Host required"));
    }
    // A page in any browser must not be able to drive the local editor.
    if req.headers.contains_key("origin") || req.headers.contains_key("sec-fetch-site") {
        return Err(HttpError(403, "Browser-origin requests unsupported"));
    }
    if req.method == "GET" && req.path == "/health" {
        return Ok(());
    }
    let expected = format!("Bearer {token}");
    let actual = req
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let mismatch = actual
        .as_bytes()
        .iter()
        .zip(expected.as_bytes())
        .fold(0u8, |a, (b, c)| a | (b ^ c));
    if actual.len() != expected.len() || mismatch != 0 {
        return Err(HttpError(401, "Bearer token required"));
    }
    if req.method == "POST"
        && req.headers.get("content-type").map(|s| {
            s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        }) != Some("application/json".into())
    {
        return Err(HttpError(415, "Content-Type application/json required"));
    }
    Ok(())
}
fn receive(stream: &mut TcpStream, token: &str, port: u16) -> HttpResult<(Request, Vec<u8>)> {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut data = Vec::with_capacity(4096);
    let header_end = loop {
        if let Some(i) = data.windows(4).position(|b| b == b"\r\n\r\n") {
            if i + 4 > MAX_HEADER {
                return Err(HttpError(431, "Header too large"));
            }
            break i + 4;
        }
        if data.len() >= MAX_HEADER {
            return Err(HttpError(431, "Header too large"));
        }
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or(HttpError(408, "Request timeout"))?;
        stream
            .set_read_timeout(Some(left))
            .map_err(|_| HttpError(400, "Socket timeout unavailable"))?;
        let mut chunk = [0u8; 4096];
        let n = stream
            .read(&mut chunk)
            .map_err(|_| HttpError(408, "Request timeout"))?;
        if n == 0 {
            return Err(HttpError(400, "Incomplete request"));
        }
        data.extend_from_slice(&chunk[..n]);
    };
    let req = parse_header(&data[..header_end])?;
    let mut body = data.split_off(header_end);
    if body.len() > req.length {
        return Err(HttpError(400, "Trailing request data rejected"));
    }
    while body.len() < req.length {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or(HttpError(408, "Request timeout"))?;
        stream
            .set_read_timeout(Some(left))
            .map_err(|_| HttpError(400, "Socket timeout unavailable"))?;
        let mut chunk = [0u8; 8192];
        let take = chunk.len().min(req.length - body.len());
        let n = stream
            .read(&mut chunk[..take])
            .map_err(|_| HttpError(408, "Request timeout"))?;
        if n == 0 {
            return Err(HttpError(400, "Incomplete JSON body"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    // Consume the bounded declared body before rejecting authentication. On
    // Windows, closing a socket with unread request bytes may reset it and hide
    // the 401/403 response from an otherwise well-behaved local client.
    authenticate(&req, token, port)?;
    Ok((req, body))
}
fn send(stream: &mut TcpStream, code: u16, data: &Value) -> Result<()> {
    let body = serde_json::to_vec(data)?;
    let label = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        _ => "Internal Server Error",
    };
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    write!(
        stream,
        "HTTP/1.1 {code} {label}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}
fn route(req: &Request, body: &[u8], session: &mut ApiSession) -> HttpResult<(Value, bool)> {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => Ok((
            json!({"service":"touchline-editor","protocol_version":1,"engine_version":env!("CARGO_PKG_VERSION"),"pid":std::process::id(),"ready":true}),
            false,
        )),
        ("GET", "/v1/capabilities") => Ok((capabilities(), false)),
        ("POST", "/v1/command") => {
            let v =
                serde_json::from_slice(body).map_err(|_| HttpError(400, "Invalid JSON body"))?;
            Ok((session.handle(v), false))
        }
        ("POST", "/v1/shutdown") => {
            let v: Value =
                serde_json::from_slice(body).map_err(|_| HttpError(400, "JSON object required"))?;
            if v != json!({}) {
                return Err(HttpError(400, "Shutdown body must be {}"));
            }
            Ok((json!({"ok":true,"stopping":true}), true))
        }
        _ => Err(HttpError(404, "Unknown API endpoint")),
    }
}

pub fn run(args: &[String]) -> Result<()> {
    let options = Options::parse(args)?;
    let instance = Instance::acquire(&options.discovery)?;
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, options.port))
        .context("Cannot bind local Touchline API")?;
    let port = listener.local_addr()?.port();
    instance.publish(port)?;
    eprintln!(
        "Touchline API listening on 127.0.0.1:{port}; discovery: {}",
        options.discovery.display()
    );
    let mut session = ApiSession::default();
    for connection in listener.incoming() {
        let mut stream = connection?;
        if !stream.peer_addr()?.ip().is_loopback() {
            continue;
        }
        let result = receive(&mut stream, &instance.token, port)
            .and_then(|(r, b)| route(&r, &b, &mut session));
        match result {
            Ok((value, stop)) => {
                // Losing the response must NEVER replay the operation. The
                // original receipt remains available through history commands.
                if let Err(e) = send(&mut stream, 200, &value) {
                    eprintln!("API response not delivered: {e}");
                }
                if stop {
                    break;
                }
            }
            Err(HttpError(code, message)) => {
                let _ = send(&mut stream, code, &json!({"ok":false,"error":message}));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn req(extra: &str) -> Request {
        parse_header(format!("POST /v1/command HTTP/1.1\r\nHost: 127.0.0.1:1234\r\nContent-Length: 2\r\nContent-Type: application/json\r\n{extra}\r\n").as_bytes()).unwrap()
    }
    #[test]
    fn rejects_ambiguous_body_and_oversize() {
        for raw in [
            "POST /v1/command HTTP/1.1\r\nContent-Length: 2\r\ncontent-length: 3\r\n\r\n",
            "POST /v1/command HTTP/1.1\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n",
            "POST /v1/command HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n",
            "POST /v1/command HTTP/1.1\r\n\r\n",
        ] {
            assert!(parse_header(raw.as_bytes()).is_err());
        }
    }
    #[test]
    fn rejects_browser_and_missing_token_but_allows_exact_authorization() {
        assert!(authenticate(&req(""), "secret", 1234).is_err());
        assert!(
            authenticate(
                &req("Authorization: Bearer secret\r\nOrigin: https://evil.test\r\n"),
                "secret",
                1234
            )
            .is_err()
        );
        assert!(authenticate(&req("Authorization: Bearer secret\r\n"), "secret", 1234).is_ok());
        assert!(authenticate(&req("Authorization: Bearer secret\r\n"), "secret", 4321).is_err());
    }
    #[test]
    fn loopback_health_has_no_secrets_and_malformed_json_never_dispatches() {
        let mut session = ApiSession::default();
        let r = parse_header(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n").unwrap();
        assert!(authenticate(&r, "secret", 1234).is_ok());
        let (health, stop) = route(&r, b"", &mut session).unwrap();
        assert!(!stop);
        assert_eq!(health["protocol_version"], 1);
        assert!(health.get("token").is_none());
        assert!(route(&req(""), b"{", &mut session).is_err());
    }
    #[test]
    fn rejects_unknown_options_and_generates_distinct_secure_tokens() {
        assert!(Options::parse(&["--api".into(), "--host".into(), "0.0.0.0".into()]).is_err());
        assert!(Options::parse(&["--port".into(), "65536".into()]).is_err());
        let a = random_token().unwrap();
        let b = random_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
    #[test]
    fn rejected_request_with_split_body_delivers_http_error_without_reset() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            match receive(&mut stream, "secret", port) {
                Err(HttpError(401, message)) => {
                    send(&mut stream, 401, &json!({"ok":false,"error":message})).unwrap()
                }
                _ => panic!("Expected unauthenticated rejection"),
            }
        });
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(client,"POST /v1/command HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n").unwrap();
        std::thread::sleep(Duration::from_millis(25));
        client.write_all(b"{}").unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
        worker.join().unwrap();
    }
}
