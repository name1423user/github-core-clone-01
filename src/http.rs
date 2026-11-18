//! HTTP/1.1 server on std only: one thread per connection, keep-alive, Content-Length and chunked bodies.
//! 持ち場: リード。インターフェースは Request を受けて Response を返すだけ。

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

/// Largest request body accepted (a push of the clone history is well under this).
const MAX_BODY: usize = 256 << 20;
/// Per-connection thread stack. Threads count toward the pod's 256-pid limit, so keep them light.
const STACK_SIZE: usize = 256 << 10;
/// Connection threads started at boot. The clone bench holds ~100 keep-alive connections at once;
/// starting their threads before the bench keeps clone/mmap/stack faults (costly under gVisor) out of it.
const PREFORK: usize = 100;

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
}

static REQUEST_SEQ: AtomicU64 = AtomicU64::new(0);

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Response {
        Response { status, headers: vec![("content-type", content_type.to_string())], body }
    }

    /// A JSON reply; every one carries a non-empty X-GitHub-Request-Id (RULES: wire contract).
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Response {
        let mut r = Response::new(status, "application/json; charset=utf-8", body.into());
        let id = REQUEST_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        r.headers.push(("x-github-request-id", format!("R{id}")));
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

/// Accept forever. Connection threads are pooled: a thread that finishes a connection waits for the next
/// one instead of exiting, and a new thread starts only when none is idle (keep-alive connections hold
/// their thread, so a connection never queues behind another).
pub fn serve(listener: TcpListener, handler: fn(&Request) -> Response) {
    let (tx, rx) = channel::<TcpStream>();
    let rx = Arc::new(Mutex::new(rx));
    let idle = Arc::new(AtomicUsize::new(0));
    let start = |first: Option<TcpStream>| {
        let (rx, idle) = (rx.clone(), idle.clone());
        let _ = thread::Builder::new().stack_size(STACK_SIZE).spawn(move || worker(first, &rx, &idle, handler));
    };
    for _ in 0..PREFORK {
        start(None);
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // Reserve an idle worker (each idle count is one worker about to recv exactly one stream).
        let reserved = idle.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1)).is_ok();
        if reserved {
            let _ = tx.send(stream);
        } else {
            start(Some(stream));
        }
    }
}

fn worker(mut next: Option<TcpStream>, rx: &Mutex<Receiver<TcpStream>>, idle: &AtomicUsize, handler: fn(&Request) -> Response) {
    if next.is_none() {
        prewarm();
    }
    loop {
        if let Some(stream) = next.take() {
            let _ = connection(stream, handler);
        }
        idle.fetch_add(1, Ordering::AcqRel);
        let got = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
        match got {
            Ok(s) => next = Some(s),
            Err(_) => return,
        }
    }
}

/// Touch (fill with non-zero, so calloc's untouched fresh pages don't count) the stack and the blocks a
/// connection allocates, then free them into this thread's cache: the first connection reuses mapped pages.
fn prewarm() {
    let mut stack = [1u8; 48 << 10];
    std::hint::black_box(&mut stack);
    for size in [64 << 10, 4096, 2048, 1024, 512, 256, 128, 64] {
        let v: Vec<Vec<u8>> = (0..4).map(|_| vec![1u8; size]).collect();
        std::hint::black_box(&v);
    }
}

fn connection(stream: TcpStream, handler: fn(&Request) -> Response) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::with_capacity(64 << 10, stream);
    let mut out = Vec::with_capacity(4096);
    loop {
        let Some((req, keep_alive)) = read_request(&mut reader)? else { return Ok(()) };
        let res = handler(&req);
        out.clear();
        write!(out, "HTTP/1.1 {} {}\r\n", res.status, reason(res.status))?;
        for (k, v) in &res.headers {
            write!(out, "{k}: {v}\r\n")?;
        }
        write!(out, "content-length: {}\r\n", res.body.len())?;
        if !keep_alive {
            out.extend_from_slice(b"connection: close\r\n");
        }
        out.extend_from_slice(b"\r\n");
        if req.method != "HEAD" {
            out.extend_from_slice(&res.body);
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
    let keep_alive = match get("connection").map(|v| v.to_ascii_lowercase()) {
        Some(v) if v.contains("close") => false,
        Some(v) if v.contains("keep-alive") => true,
        _ => !http10,
    };
    let body = if get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
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

    #[test]
    fn pool_serves_more_connections_than_prefork() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || serve(l, echo));
        // All held open at once (keep-alive), then each served: none may queue behind another.
        let mut conns: Vec<TcpStream> = (0..PREFORK + 20).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
        for _ in 0..2 {
            for c in conns.iter_mut() {
                c.write_all(b"GET /x HTTP/1.1\r\n\r\n").unwrap();
                let mut buf = [0u8; 256];
                let n = c.read(&mut buf).unwrap();
                assert!(buf[..n].starts_with(b"HTTP/1.1 200"));
            }
        }
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
