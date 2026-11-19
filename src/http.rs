//! HTTP/1.1 server on std only: a few epoll event-loop threads (Go's netpoller model), keep-alive,
//! Content-Length and chunked bodies. Few threads matter under gVisor, where waking or switching
//! between many application threads is costly (the 50-connection clone bench).
//! 持ち場: リード。インターフェースは Request を受けて Response を返すだけ。

use std::io::{self, BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

/// Largest request body accepted (a push of the clone history is well under this).
const MAX_BODY: usize = 256 << 20;
/// Event-loop threads (the entry has 2 CPUs). Handlers run on them, so give them a full stack.
const LOOPS: usize = 2;
const STACK_SIZE: usize = 8 << 20;
/// Largest request head (request line + headers) buffered before the connection is dropped.
const MAX_HEAD: usize = 1 << 20;

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
        let mut r = Response::new(status, "application/json; charset=utf-8", body.into());
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

/// Accept forever and hand each connection to one of the event loops (round robin).
pub fn serve(listener: TcpListener, handler: fn(&Request) -> Response) {
    let loops: Vec<i32> = (0..LOOPS)
        .map(|_| {
            // SAFETY: plain syscall wrapper.
            let ep = unsafe { epoll_create1(EPOLL_CLOEXEC) };
            assert!(ep >= 0, "epoll_create1 failed");
            thread::Builder::new().stack_size(STACK_SIZE).spawn(move || event_loop(ep, handler)).expect("spawn event loop");
            ep
        })
        .collect();
    for (i, stream) in listener.incoming().enumerate() {
        let Ok(stream) = stream else { continue };
        let _ = stream.set_nodelay(true);
        quickack(&stream);
        bufsizes(&stream);
        if stream.set_nonblocking(true).is_err() {
            continue;
        }
        let fd = stream.as_raw_fd();
        let conn = Box::into_raw(Box::new(Conn { stream, inbuf: Vec::new(), out: Vec::new(), sent: 0, close_after: false, interest: IN }));
        let mut ev = EpollEvent { events: IN, data: conn as u64 };
        // SAFETY: fd is open; after a successful ADD only that loop touches `conn`.
        if unsafe { epoll_ctl(loops[i % LOOPS], EPOLL_CTL_ADD, fd, &mut ev) } != 0 {
            // SAFETY: not registered, still ours.
            drop(unsafe { Box::from_raw(conn) });
        }
    }
}

struct Conn {
    stream: TcpStream,
    inbuf: Vec<u8>,
    out: Vec<u8>,
    sent: usize,
    close_after: bool,
    interest: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct EpollEvent {
    events: u32,
    data: u64,
}

const EPOLLIN: u32 = 0x1;
const EPOLLOUT: u32 = 0x4;
const EPOLLRDHUP: u32 = 0x2000;
const IN: u32 = EPOLLIN | EPOLLRDHUP;
const OUT: u32 = EPOLLOUT | EPOLLRDHUP;
const EPOLL_CTL_ADD: i32 = 1;
const EPOLL_CTL_DEL: i32 = 2;
const EPOLL_CTL_MOD: i32 = 3;
const EPOLL_CLOEXEC: i32 = 0x80000;

fn event_loop(ep: i32, handler: fn(&Request) -> Response) {
    let mut evs = [EpollEvent { events: 0, data: 0 }; 64];
    let mut scratch = vec![0u8; 64 << 10];
    loop {
        // SAFETY: evs is a live buffer of 64 events.
        let n = unsafe { epoll_wait(ep, evs.as_mut_ptr(), evs.len() as i32, -1) };
        for e in evs.iter().take(n.max(0) as usize) {
            let ptr = e.data as *mut Conn;
            // SAFETY: registered by serve(); only this loop uses it until it is freed below.
            let c = unsafe { &mut *ptr };
            match drive(c, &mut scratch, handler) {
                Some(want) if want == c.interest => {}
                Some(want) => {
                    c.interest = want;
                    let mut ev = EpollEvent { events: want, data: ptr as u64 };
                    // SAFETY: fd still registered.
                    unsafe { epoll_ctl(ep, EPOLL_CTL_MOD, c.stream.as_raw_fd(), &mut ev) };
                }
                None => {
                    let mut ev = EpollEvent { events: 0, data: 0 };
                    // SAFETY: deregister, then free the connection exactly once.
                    unsafe {
                        epoll_ctl(ep, EPOLL_CTL_DEL, c.stream.as_raw_fd(), &mut ev);
                        drop(Box::from_raw(ptr));
                    }
                }
            }
        }
    }
}

/// Advance one connection: flush pending output, read what arrived, answer every complete request.
/// Returns the epoll interest to keep, or None to close.
fn drive(c: &mut Conn, scratch: &mut [u8], handler: fn(&Request) -> Response) -> Option<u32> {
    if !flush(c)? {
        return Some(OUT);
    }
    if c.close_after {
        return None;
    }
    let mut eof = false;
    loop {
        match (&c.stream).read(scratch) {
            Ok(0) => {
                eof = true;
                break;
            }
            Ok(n) => {
                c.inbuf.extend_from_slice(&scratch[..n]);
                if n < scratch.len() {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    while let Some((req, keep_alive, used)) = try_parse(&c.inbuf).ok()? {
        c.inbuf.drain(..used);
        let res = handler(&req);
        encode(&mut c.out, &res, keep_alive, req.method == "HEAD");
        if !keep_alive {
            c.close_after = true;
            break;
        }
    }
    if eof {
        c.close_after = true;
    }
    if !flush(c)? {
        return Some(OUT);
    }
    if c.close_after { None } else { Some(IN) }
}

/// Write pending output. Some(true) when all of it is out, Some(false) when the socket is full.
fn flush(c: &mut Conn) -> Option<bool> {
    while c.sent < c.out.len() {
        match (&c.stream).write(&c.out[c.sent..]) {
            Ok(0) => return None,
            Ok(n) => c.sent += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Some(false),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    c.out.clear();
    c.sent = 0;
    Some(true)
}

/// One complete request from the front of `buf` (with the bytes it used), Ok(None) if more must arrive.
fn try_parse(buf: &[u8]) -> io::Result<Option<(Request, bool, usize)>> {
    let Some(head_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if buf.len() > MAX_HEAD { Err(io::ErrorKind::InvalidData.into()) } else { Ok(None) };
    };
    // Don't re-parse (and re-copy) a large body on every arriving segment.
    if let Some(n) = content_length(&buf[..head_end]) {
        if n <= MAX_BODY && buf.len() < head_end + 4 + n {
            return Ok(None);
        }
    }
    let mut r: &[u8] = buf;
    match read_request(&mut r) {
        Ok(Some((req, keep_alive))) => Ok(Some((req, keep_alive, buf.len() - r.len()))),
        Ok(None) => Ok(None),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

fn content_length(head: &[u8]) -> Option<usize> {
    head.split(|&b| b == b'\n').find_map(|l| {
        let l = l.strip_suffix(b"\r").unwrap_or(l);
        let colon = l.iter().position(|&b| b == b':')?;
        l[..colon].trim_ascii().eq_ignore_ascii_case(b"content-length").then(|| std::str::from_utf8(l[colon + 1..].trim_ascii()).ok()?.parse().ok())?
    })
}

fn encode(out: &mut Vec<u8>, res: &Response, keep_alive: bool, head: bool) {
    out.extend_from_slice(b"HTTP/1.1 ");
    push_num(out, res.status as u64);
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
    push_num(out, body.len() as u64);
    out.extend_from_slice(b"\r\n");
    if !keep_alive {
        out.extend_from_slice(b"connection: close\r\n");
    }
    out.extend_from_slice(b"\r\n");
    if !head {
        out.extend_from_slice(body);
    }
}

unsafe extern "C" {
    fn setsockopt(fd: i32, level: i32, name: i32, value: *const std::ffi::c_void, len: u32) -> i32;
    fn epoll_create1(flags: i32) -> i32;
    fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32;
    fn epoll_wait(epfd: i32, events: *mut EpollEvent, maxevents: i32, timeout: i32) -> i32;
}

/// Ask the kernel (or gVisor's netstack) to ACK at once instead of delaying ACKs on this socket.
/// Best effort: a failure changes nothing.
fn quickack(stream: &TcpStream) {
    const IPPROTO_TCP: i32 = 6;
    const TCP_QUICKACK: i32 = 12;
    let on: i32 = 1;
    // SAFETY: valid fd for the stream's lifetime; pointer/len describe a live i32.
    unsafe { setsockopt(stream.as_raw_fd(), IPPROTO_TCP, TCP_QUICKACK, &on as *const i32 as *const _, 4) };
}

/// Larger socket buffers so a big request or response moves in fewer windows. Best effort.
fn bufsizes(stream: &TcpStream) {
    const SOL_SOCKET: i32 = 1;
    const SO_SNDBUF: i32 = 7;
    const SO_RCVBUF: i32 = 8;
    let size: i32 = 256 << 10;
    for opt in [SO_SNDBUF, SO_RCVBUF] {
        // SAFETY: valid fd for the stream's lifetime; pointer/len describe a live i32.
        unsafe { setsockopt(stream.as_raw_fd(), SOL_SOCKET, opt, &size as *const i32 as *const _, 4) };
    }
}

/// Read one request; Ok(None) on a clean close before a request line.
fn read_request<R: BufRead>(r: &mut R) -> io::Result<Option<(Request, bool)>> {
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

fn read_chunked<R: BufRead>(r: &mut R) -> io::Result<Vec<u8>> {
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

    fn big(req: &Request) -> Response {
        let n: usize = req.query.parse().unwrap_or(0);
        Response::new(200, "text/plain", vec![b'x'; n])
    }

    fn start(h: fn(&Request) -> Response) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || serve(l, h));
        port
    }

    fn read_response(s: &mut TcpStream) -> (String, usize) {
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        let h = String::from_utf8(head).unwrap();
        let n: usize = h.lines().find_map(|l| l.strip_prefix("content-length: ")).unwrap().trim().parse().unwrap();
        let mut body = vec![0u8; n];
        s.read_exact(&mut body).unwrap();
        (h, n)
    }

    #[test]
    fn split_pipelined_and_large() {
        let port = start(big);
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        // A request split across writes, then two pipelined in one write, the second 8 MiB.
        s.write_all(b"GET /a?5 HT").unwrap();
        thread::sleep(std::time::Duration::from_millis(20));
        s.write_all(b"TP/1.1\r\n\r\n").unwrap();
        assert_eq!(read_response(&mut s).1, 5);
        s.write_all(b"GET /b?3 HTTP/1.1\r\n\r\nGET /c?8388608 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(read_response(&mut s).1, 3);
        assert_eq!(read_response(&mut s).1, 8 << 20);
        // A 3 MiB body arriving in pieces.
        let body = vec![b'y'; 3 << 20];
        s.write_all(format!("POST /d HTTP/1.1\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).unwrap();
        for c in body.chunks(100_000) {
            s.write_all(c).unwrap();
        }
        assert_eq!(read_response(&mut s).1, 0);
    }

    #[test]
    fn many_concurrent_keep_alive() {
        let port = start(echo);
        let mut conns: Vec<TcpStream> = (0..120).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
        for _ in 0..3 {
            for c in conns.iter_mut() {
                c.write_all(b"GET /x HTTP/1.1\r\n\r\n").unwrap();
            }
            for c in conns.iter_mut() {
                assert!(read_response(c).0.starts_with("HTTP/1.1 200"));
            }
        }
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
