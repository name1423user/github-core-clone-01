//! HTTP/1.1 server on std only: one thread per connection, keep-alive, Content-Length and chunked bodies.
//! 持ち場: リード。インターフェースは Request を受けて Response を返すだけ。

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

/// Largest request body accepted (a push of the clone history is well under this).
const MAX_BODY: usize = 256 << 20;
/// Per-connection thread stack. Threads count toward the pod's 256-pid limit, so keep them light.
const STACK_SIZE: usize = 256 << 10;

pub struct Request {
    pub method: String,
    /// Path without the query string, still percent-encoded.
    pub path: String,
    /// Raw query string (after '?'), possibly empty.
    pub query: String,
    /// Header names are lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The body as JSON. Malformed → 400 `{"message":"Problems parsing JSON"}` (protocol.malformed-body-400).
    /// An empty body reads as `{}`.
    pub fn json(&self) -> Result<crate::util::json::Value, Response> {
        if self.body.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(crate::util::json::obj(vec![]));
        }
        crate::util::json::parse(&self.body).map_err(|_| Response::error(400, "Problems parsing JSON"))
    }

    /// First value of a query parameter, percent-decoded.
    pub fn query_param(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k == name).then(|| percent_decode(v))
        })
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
    /// Pre-rendered body shared with a cache; written instead of `body` without copying it first.
    pub shared: Option<std::sync::Arc<[u8]>>,
}

static REQUEST_SEQ: AtomicU64 = AtomicU64::new(0);

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Response {
        Response { status, headers: vec![("content-type", content_type.to_string())], body, shared: None }
    }

    /// A JSON reply; every one carries a non-empty X-GitHub-Request-Id (RULES: wire contract).
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Response {
        let mut r = Response::new(status, "application/json", body.into());
        let id = REQUEST_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        let mut v = Vec::with_capacity(12);
        v.push(b'R');
        push_num(&mut v, id);
        r.headers.push(("x-github-request-id", String::from_utf8(v).unwrap_or_default()));
        r
    }

    pub fn not_found() -> Response {
        Response::error(404, "Not Found")
    }

    /// GitHub's error shape: `{"message", "documentation_url"}`.
    pub fn error(status: u16, message: &str) -> Response {
        let mut b = String::with_capacity(64 + message.len());
        b.push_str("{\"message\":");
        crate::util::json::write_str(&mut b, message);
        b.push_str(",\"documentation_url\":\"https://docs.github.com/rest\"}");
        Response::json(status, b)
    }

    pub fn with_shared(mut self, body: std::sync::Arc<[u8]>) -> Response {
        self.shared = Some(body);
        self
    }

    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Response {
        self.headers.push((name, value.into()));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

/// Accept forever; each connection gets its own thread running `handler` per request.
pub fn serve(listener: TcpListener, handler: fn(&Request) -> Response) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let _ = thread::Builder::new().stack_size(STACK_SIZE).spawn(move || {
            let _ = connection(stream, handler);
            crate::util::alloc::flush();
        });
    }
}

unsafe extern "C" {
    fn setsockopt(fd: i32, level: i32, name: i32, value: *const std::ffi::c_void, len: u32) -> i32;
}

/// Ask the kernel (or gVisor's netstack) to ACK at once instead of delaying ACKs on this socket.
/// Best effort: a failure changes nothing.
fn quickack(stream: &TcpStream) {
    use std::os::fd::AsRawFd;
    const IPPROTO_TCP: i32 = 6;
    const TCP_QUICKACK: i32 = 12;
    let on: i32 = 1;
    // SAFETY: valid fd for the stream's lifetime; pointer/len describe a live i32.
    unsafe { setsockopt(stream.as_raw_fd(), IPPROTO_TCP, TCP_QUICKACK, &on as *const i32 as *const _, 4) };
}

/// Larger socket buffers so a big request or response moves in fewer windows. Best effort.
fn bufsizes(stream: &TcpStream) {
    use std::os::fd::AsRawFd;
    const SOL_SOCKET: i32 = 1;
    const SO_SNDBUF: i32 = 7;
    const SO_RCVBUF: i32 = 8;
    let size: i32 = 256 << 10;
    for opt in [SO_SNDBUF, SO_RCVBUF] {
        // SAFETY: valid fd for the stream's lifetime; pointer/len describe a live i32.
        unsafe { setsockopt(stream.as_raw_fd(), SOL_SOCKET, opt, &size as *const i32 as *const _, 4) };
    }
}

fn connection(stream: TcpStream, handler: fn(&Request) -> Response) -> io::Result<()> {
    stream.set_nodelay(true)?;
    quickack(&stream);
    bufsizes(&stream);
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::with_capacity(64 << 10, stream);
    let mut out = Vec::with_capacity(4096);
    loop {
        let Some((req, keep_alive)) = read_request(&mut reader)? else { return Ok(()) };
        let res = handler(&req);
        out.clear();
        out.extend_from_slice(b"HTTP/1.1 ");
        push_num(&mut out, res.status as u64);
        out.push(b' ');
        out.extend_from_slice(reason(res.status).as_bytes());
        out.extend_from_slice(b"\r\n");
        for (k, v) in &res.headers {
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        let body: &[u8] = res.shared.as_deref().unwrap_or(&res.body);
        out.extend_from_slice(b"content-length: ");
        push_num(&mut out, body.len() as u64);
        out.extend_from_slice(b"\r\n");
        if !keep_alive {
            out.extend_from_slice(b"connection: close\r\n");
        }
        out.extend_from_slice(b"\r\n");
        if req.method != "HEAD" {
            out.extend_from_slice(body);
        }
        writer.write_all(&out)?;
        if !keep_alive {
            return Ok(());
        }
    }
}

/// Read one request; Ok(None) on a clean close before a request line.
fn read_request(r: &mut BufReader<TcpStream>) -> io::Result<Option<(Request, bool)>> {
    let mut line = String::new();
    // Tolerate stray CRLFs between keep-alive requests.
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if !line.trim().is_empty() {
            break;
        }
    }
    let mut it = line.trim_end().splitn(3, ' ');
    let method = it.next().unwrap_or("").to_string();
    let target = it.next().unwrap_or("/");
    let http10 = it.next() == Some("HTTP/1.0");
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.to_string(), String::new()),
    };
    let mut headers = Vec::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
    let keep_alive = match get("connection") {
        Some(v) if has_token(v, b"close") => false,
        Some(v) if has_token(v, b"keep-alive") => true,
        _ => !http10,
    };
    let body = if get("transfer-encoding").is_some_and(|v| has_token(v, b"chunked")) {
        read_chunked(r)?
    } else {
        let n: usize = get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        if n > MAX_BODY {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut b = vec![0; n];
        r.read_exact(&mut b)?;
        b
    };
    Ok(Some((Request { method, path, query, headers, body }, keep_alive)))
}

/// Case-insensitive substring test without lowercasing a copy.
fn has_token(v: &str, t: &[u8]) -> bool {
    v.as_bytes().windows(t.len()).any(|w| w.eq_ignore_ascii_case(t))
}

/// Append `n` in decimal.
fn push_num(out: &mut Vec<u8>, mut n: u64) {
    let mut d = [0u8; 20];
    let mut i = d.len();
    loop {
        i -= 1;
        d[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&d[i..]);
}

fn read_chunked(r: &mut BufReader<TcpStream>) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        r.read_line(&mut line)?;
        let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        if size == 0 {
            // Trailers until the empty line.
            loop {
                line.clear();
                if r.read_line(&mut line)? == 0 || line.trim().is_empty() {
                    return Ok(body);
                }
            }
        }
        if body.len() + size > MAX_BODY {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let start = body.len();
        body.resize(start + size, 0);
        r.read_exact(&mut body[start..])?;
        line.clear();
        r.read_line(&mut line)?; // CRLF after the chunk
    }
}

/// Minimal blocking HTTP client for the env's /register and /done (Connection: close, read to EOF).
pub fn call(host: &str, port: u16, method: &str, path: &str, body: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    let timeout = Some(std::time::Duration::from_secs(30));
    let mut s = TcpStream::connect((host, port))?;
    s.set_read_timeout(timeout)?;
    s.set_write_timeout(timeout)?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nhost: {host}:{port}\r\nconnection: close\r\n").into_bytes();
    if !body.is_empty() {
        req.extend_from_slice(format!("content-type: application/json\r\ncontent-length: {}\r\n", body.len()).as_bytes());
    }
    req.extend_from_slice(b"\r\n");
    req.extend_from_slice(body);
    s.write_all(&req)?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or(io::ErrorKind::InvalidData)?;
    let status_line = String::from_utf8_lossy(&raw[..head_end]);
    let status = status_line.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or(io::ErrorKind::InvalidData)?;
    Ok((status, raw[head_end + 4..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo(req: &Request) -> Response {
        Response::json(200, format!("{} {} {}", req.method, req.path, req.body.len()))
    }

    #[test]
    fn helpers() {
        let mut o = Vec::new();
        push_num(&mut o, 0);
        o.push(b' ');
        push_num(&mut o, 18446744073709551615);
        assert_eq!(o, b"0 18446744073709551615");
        assert!(has_token("Keep-Alive, Upgrade", b"keep-alive") && !has_token("upgrade", b"close"));
    }

    #[test]
    fn keep_alive_and_chunked() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || serve(l, echo));
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"POST /a?q=1 HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc").unwrap();
        s.write_all(b"POST /b HTTP/1.1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nhi\r\n3\r\nyou\r\n0\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.contains("POST /a 3"), "{out}");
        assert!(out.contains("POST /b 5"), "{out}");
        assert_eq!(out.matches("x-github-request-id: R").count(), 2);
    }
}

/// `%XX` and `+` (as space) decoding; invalid escapes pass through.
pub fn percent_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push(h << 4 | l);
                i += 2;
            }
            (b'+', ..) => out.push(b' '),
            (c, ..) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod decode_tests {
    #[test]
    fn decode() {
        assert_eq!(super::percent_decode("a%2Fb+c%zz%4"), "a/b c%zz%4");
    }
}
