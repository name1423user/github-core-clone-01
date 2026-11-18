//! Entry: the lifecycle of kit/starter/main.ts, ported. main.ts only launches this binary.
//!   1. Bind an ephemeral port on 0.0.0.0 (never the env's own port).
//!   2. POST {"port": N} to $MAKEFUN_ENV_URL/register until 200 (or 409: someone registered first).
//!   3. Poll GET $MAKEFUN_ENV_URL/done until {"done": true}, then exit 0.
//! 持ち場: リード。

// scaffold: the http API is used fully once the interfaces land; drop this then.
#![allow(dead_code)]

mod git;
mod graphql;
mod http;
mod mcp;
mod rest;
mod router;
mod store;
mod util;

#[global_allocator]
static ALLOC: util::alloc::Alloc = util::alloc::Alloc;

use std::net::TcpListener;
use std::process::exit;
use std::thread;
use std::time::Duration;

/// Backoff between failed register/done requests: 250 ms doubling to 2 s (as the starter).
fn retry_delay(attempt: u32) -> Duration {
    Duration::from_millis((250u64 << attempt.min(3)).min(2000))
}

/// "http://host:port/..." → (host, port). The env URL is plain HTTP.
fn parse_env_url(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?;
    match authority.rsplit_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse().ok()?)),
        None => Some((authority.to_string(), 80)),
    }
}

fn register(host: &str, env_port: u16, port: u16) {
    let body = format!("{{\"port\":{port}}}");
    for attempt in 0.. {
        match http::call(host, env_port, "POST", "/register", body.as_bytes()) {
            Ok((200, _)) | Ok((409, _)) => return,
            Ok((s @ (400 | 405), b)) => {
                eprintln!("[entry] register answered HTTP {s}: {}", String::from_utf8_lossy(&b));
                exit(1);
            }
            _ => thread::sleep(retry_delay(attempt)),
        }
    }
}

fn wait_for_done(host: &str, env_port: u16) {
    for attempt in 0.. {
        if let Ok((200, b)) = http::call(host, env_port, "GET", "/done", b"") {
            let compact: Vec<u8> = b.into_iter().filter(|c| !c.is_ascii_whitespace()).collect();
            if compact.windows(11).any(|w| w == b"\"done\":true") {
                return;
            }
        }
        thread::sleep(retry_delay(attempt));
    }
}

fn main() {
    let env_url = std::env::var("MAKEFUN_ENV_URL").unwrap_or_default();
    let Some((host, env_port)) = parse_env_url(env_url.trim_end_matches('/')) else {
        eprintln!("[entry] MAKEFUN_ENV_URL is not set: there is no env to register with");
        exit(1);
    };
    let listener = TcpListener::bind("0.0.0.0:0").unwrap_or_else(|e| {
        eprintln!("[entry] bind failed: {e}");
        exit(1);
    });
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    println!("[entry] listening on port {port}");
    thread::spawn(move || http::serve(listener, router::route));

    register(&host, env_port, port);
    println!("[entry] registered; waiting for the env to say done");
    wait_for_done(&host, env_port);
    println!("[entry] done");
    exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_url() {
        assert_eq!(parse_env_url("http://store:9000"), Some(("store".into(), 9000)));
        assert_eq!(parse_env_url("http://127.0.0.1:18080/x"), Some(("127.0.0.1".into(), 18080)));
        assert_eq!(parse_env_url("http://store"), Some(("store".into(), 80)));
        assert_eq!(parse_env_url(""), None);
        assert_eq!(retry_delay(0).as_millis(), 250);
        assert_eq!(retry_delay(9).as_millis(), 2000);
    }
}
