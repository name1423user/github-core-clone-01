//! REST v3（/api/v3/...）。持ち場: rest-engineer。
//! Routing is a slice match on the path segments (still percent-encoded; decoded per segment here).
//! Serializers (`repo_json`, `issue_json`, `pull_json`, ...) are pub so MCP/GraphQL can return the same shapes.

mod contents;
mod issues;
mod pulls;
mod repos;

use crate::http::{percent_decode, Request, Response};
use crate::store::{self, Repo, RepoRef, VIEWER};
use crate::util::json::{obj, Value};

#[allow(unused_imports)]
pub use issues::{comment_json, issue_json, label_json};
#[allow(unused_imports)]
pub use pulls::pull_json;
#[allow(unused_imports)]
pub use repos::{commit_json, repo_json, user_json};

/// Base for the `url` fields in bodies. Fixed (not the request Host) so bodies never depend on the port.
pub const API: &str = "http://localhost/api/v3";
pub const WEB: &str = "http://localhost";

/// `parts` は /api/v3 より後のパス要素。
pub fn handle(req: &Request, parts: &[&str]) -> Response {
    let m = req.method.as_str();
    match parts {
        [] => match m {
            "GET" | "HEAD" => Response::json(
                200,
                format!(r#"{{"current_user_url":"{API}/user","repository_url":"{API}/repos/{{owner}}/{{repo}}"}}"#),
            )
            .with_header("x-oauth-scopes", "repo, user, workflow")
            .with_header("x-accepted-oauth-scopes", ""),
            _ => Response::not_found(),
        },
        ["meta"] => Response::json(200, r#"{"installed_version":"3.14.0","verifiable_password_authentication":false}"#),
        ["user"] => Response::json(200, user_json(VIEWER).to_json()).with_header("x-oauth-scopes", "repo, user, workflow"),
        ["user", "repos"] => match m {
            "GET" => repos::list(req),
            "POST" => repos::create(req),
            _ => Response::not_found(),
        },
        ["repos", owner, name, rest @ ..] => {
            let (owner, name) = (percent_decode(owner), percent_decode(name));
            let Some(repo) = store::store().repo(&owner, &name) else { return Response::not_found() };
            repo_routes(req, &repo, &owner, &name, rest)
        }
        _ => Response::not_found(),
    }
}

fn repo_routes(req: &Request, repo: &RepoRef, owner: &str, name: &str, rest: &[&str]) -> Response {
    let m = req.method.as_str();
    match (m, rest) {
        ("GET", []) => Response::json(200, repo_json(&store::lock(repo)).to_json()),
        ("PATCH", []) => repos::patch(req, repo),
        ("DELETE", []) => {
            store::store().delete_repo(owner, name);
            Response::json(204, Vec::new())
        }
        (_, ["contents", path @ ..]) => contents::handle(req, repo, &join(path)),
        ("POST", ["git", "refs"]) => repos::create_ref(req, repo),
        ("GET", ["git", "ref" | "refs", path @ ..]) if !path.is_empty() => repos::get_ref(repo, &join(path)),
        ("GET", ["branches"]) => repos::branches(req, repo),
        ("GET", ["branches", b @ ..]) if !b.is_empty() => repos::branch(repo, &join(b)),
        ("GET", ["commits"]) => repos::commits(req, repo),
        ("GET", ["commits", sha]) => repos::commit(repo, &percent_decode(sha)),
        (_, ["labels", ..]) | (_, ["issues", ..]) => issues::handle(req, repo, rest),
        (_, ["pulls", ..]) => pulls::handle(req, repo, rest),
        _ => Response::not_found(),
    }
}

/// Decoded segments joined with '/' (file paths and branch names may hold slashes, encoded or not).
fn join(segs: &[&str]) -> String {
    segs.iter().map(|s| percent_decode(s)).collect::<Vec<_>>().join("/")
}

/// Parse the JSON body or return the 400 response from the enclosing handler.
macro_rules! body {
    ($req:expr) => {
        match $req.json() {
            Ok(v) => v,
            Err(r) => return r,
        }
    };
}
pub(crate) use body;

/// 422 `Validation Failed` with GitHub's `errors` array.
pub fn validation(resource: &str, field: &str, code: &str, message: &str) -> Response {
    let body = obj(vec![
        ("message", "Validation Failed".into()),
        (
            "errors",
            Value::Arr(vec![obj(vec![
                ("resource", resource.into()),
                ("code", code.into()),
                ("field", field.into()),
                ("message", message.into()),
            ])]),
        ),
        ("documentation_url", "https://docs.github.com/rest".into()),
    ]);
    Response::json(422, body.to_json())
}

pub fn json_array(items: impl IntoIterator<Item = Value>) -> Value {
    Value::Arr(items.into_iter().collect())
}

/// `per_page` (default 30, max 100) and `page` (from 1) → the index range to serve, plus the `Link` header.
/// Link URLs come from the request's Host so `rel="next"` stays on the client's origin.
pub fn paged(req: &Request, total: usize) -> (std::ops::Range<usize>, Option<String>) {
    let per = req.query_param("per_page").and_then(|v| v.parse::<usize>().ok()).unwrap_or(30).clamp(1, 100);
    let page = req.query_param("page").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
    let last = total.div_ceil(per).max(1);
    let start = ((page - 1) * per).min(total);
    let range = start..(start + per).min(total);
    if last == 1 {
        return (range, None);
    }
    let host = req.header("host").unwrap_or("localhost");
    let keep: Vec<&str> = req.query.split('&').filter(|kv| !kv.is_empty() && !kv.starts_with("page=")).collect();
    let url = |p: usize| {
        let mut q = keep.join("&");
        if !q.is_empty() {
            q.push('&');
        }
        format!("<http://{host}{}?{q}page={p}>", req.path)
    };
    let mut links = Vec::new();
    if page < last {
        links.push(format!("{}; rel=\"next\"", url(page + 1)));
        links.push(format!("{}; rel=\"last\"", url(last)));
    }
    if page > 1 {
        links.push(format!("{}; rel=\"first\"", url(1)));
        links.push(format!("{}; rel=\"prev\"", url((page - 1).min(last))));
    }
    (range, Some(links.join(", ")))
}

/// A JSON array response with the Link header from `paged`.
pub fn list_response(body: Value, link: Option<String>) -> Response {
    let r = Response::json(200, body.to_json());
    match link {
        Some(l) => r.with_header("link", l),
        None => r,
    }
}

pub fn repo_api(r: &Repo) -> String {
    format!("{API}/repos/{}/{}", r.owner, r.name)
}

// ---------------------------------------------------------------- base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(d: &[u8]) -> String {
    let mut out = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard or URL-safe alphabet; whitespace and padding ignored. None on any other byte.
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            assert_eq!(b64_decode(&b64_encode(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_decode("Zm9v\nYg==").unwrap(), b"foob");
        assert!(b64_decode("!!").is_none());
    }

    fn get(path: &str, query: &str) -> Request {
        Request {
            method: "GET".into(),
            path: path.into(),
            query: query.into(),
            headers: vec![("host".into(), "h:1".into())],
            body: vec![],
        }
    }

    #[test]
    fn pagination_links() {
        let (r, l) = paged(&get("/api/v3/user/repos", "per_page=10"), 25);
        assert_eq!(r, 0..10);
        assert!(l.unwrap().starts_with("<http://h:1/api/v3/user/repos?per_page=10&page=2>; rel=\"next\""));
        let (r, l) = paged(&get("/x", "per_page=10&page=3"), 25);
        assert_eq!(r, 20..25);
        assert!(!l.unwrap().contains("next"));
        assert_eq!(paged(&get("/x", ""), 5), (0..5, None));
    }
}
