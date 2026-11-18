//! MCP（POST /mcp、JSON-RPC 2.0 over Streamable HTTP）。持ち場: mcp-engineer。
//! Both eras: 2026-07-28 (`server/discover`) and the handshake (`initialize` → `notifications/initialized`).
//! No MCP-only state: tools call the same store functions as REST, so ids match.
use crate::http::{Request, Response};
use crate::store::{self, lock, node_id, store, Kind, RepoRef, StoreError, VIEWER};
use crate::util::json::{obj, parse, Value};
use crate::util::sha1::hex;
use std::sync::atomic::{AtomicU64, Ordering};

const REVISIONS: &str = r#"["2026-07-28","2025-11-25","2025-06-18","2025-03-26","2024-11-05"]"#;
const LEGACY: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Fixed text: two tools/list answers must be byte-identical. Argument names follow github-mcp-server.
const TOOLS: &str = r#"{"tools":[
{"name":"create_repo","description":"Create a new repository owned by the authenticated user.","inputSchema":{"type":"object","properties":{"name":{"type":"string","description":"Repository name"},"description":{"type":"string","description":"Repository description"},"private":{"type":"boolean","description":"Whether the repository is private"}},"required":["name"]}},
{"name":"get_repo","description":"Get a repository by owner and name.","inputSchema":{"type":"object","properties":{"owner":{"type":"string","description":"Repository owner"},"repo":{"type":"string","description":"Repository name"}},"required":["owner","repo"]}},
{"name":"create_issue","description":"Create a new issue in a repository.","inputSchema":{"type":"object","properties":{"owner":{"type":"string","description":"Repository owner"},"repo":{"type":"string","description":"Repository name"},"title":{"type":"string","description":"Issue title"},"body":{"type":"string","description":"Issue body"},"labels":{"type":"array","items":{"type":"string"},"description":"Label names (must already exist)"}},"required":["owner","repo","title"]}},
{"name":"list_issues","description":"List issues in a repository.","inputSchema":{"type":"object","properties":{"owner":{"type":"string","description":"Repository owner"},"repo":{"type":"string","description":"Repository name"},"state":{"type":"string","enum":["open","closed","all"],"description":"Filter by state (default open)"}},"required":["owner","repo"]}},
{"name":"create_pull_request","description":"Open a pull request from head into base.","inputSchema":{"type":"object","properties":{"owner":{"type":"string","description":"Repository owner"},"repo":{"type":"string","description":"Repository name"},"title":{"type":"string","description":"Pull request title"},"body":{"type":"string","description":"Pull request description"},"head":{"type":"string","description":"Branch with the changes"},"base":{"type":"string","description":"Branch to merge into"}},"required":["owner","repo","title","head","base"]}},
{"name":"merge_pull_request","description":"Squash-merge a pull request into its base branch.","inputSchema":{"type":"object","properties":{"owner":{"type":"string","description":"Repository owner"},"repo":{"type":"string","description":"Repository name"},"pull_number":{"type":"number","description":"Pull request number"},"commit_message":{"type":"string","description":"Merge commit message"}},"required":["owner","repo","pull_number"]}}
]}"#;

static SESSIONS: AtomicU64 = AtomicU64::new(0);

pub fn handle(req: &Request) -> Response {
    match req.method.as_str() {
        "POST" => {}
        "DELETE" => return Response::new(202, "application/json", Vec::new()),
        _ => return rpc_reply(req, 405, error_body("null", -32600, "Method not allowed"), None),
    }
    let Ok(msg) = parse(&req.body) else {
        return rpc_reply(req, 400, error_body("null", -32700, "Parse error"), None);
    };
    if !matches!(msg, Value::Obj(_)) || msg.str_field("method").is_none() {
        let id = msg.get("id").map_or("null".to_string(), Value::to_json);
        return rpc_reply(req, 400, error_body(&id, -32600, "Invalid Request"), None);
    }
    let Some(id) = msg.get("id").map(Value::to_json) else {
        return Response::new(202, "application/json", Vec::new()); // notification
    };
    let method = msg.str_field("method").unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let mut session = None;
    let result: Result<String, (i64, String)> = match method {
        "server/discover" => Ok(format!(
            r#"{{"supportedVersions":{REVISIONS},"capabilities":{{"tools":{{"listChanged":false}}}},"serverInfo":{}}}"#,
            server_info()
        )),
        "initialize" => {
            let asked = params.str_field("protocolVersion").unwrap_or("");
            let v = LEGACY.iter().find(|r| **r == asked).unwrap_or(&LEGACY[0]);
            session = Some(format!("mcp-session-{}", SESSIONS.fetch_add(1, Ordering::Relaxed) + 1));
            Ok(format!(r#"{{"protocolVersion":"{v}","capabilities":{{"tools":{{"listChanged":false}}}},"serverInfo":{}}}"#, server_info()))
        }
        "ping" => Ok("{}".to_string()),
        "tools/list" => Ok(TOOLS.to_string()),
        "tools/call" => call(&params),
        _ => Err((-32601, format!("Method not found: {method}"))),
    };
    let body = match result {
        Ok(r) => format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{r}}}"#),
        Err((code, m)) => error_body(&id, code, &m),
    };
    rpc_reply(req, 200, body, session)
}

fn server_info() -> String {
    format!(r#"{{"name":"forge","version":"{}"}}"#, env!("CARGO_PKG_VERSION"))
}

fn error_body(id: &str, code: i64, message: &str) -> String {
    let mut m = String::new();
    crate::util::json::write_str(&mut m, message);
    format!(r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":{code},"message":{m}}}}}"#)
}

/// JSON unless the client only accepts SSE.
fn rpc_reply(req: &Request, status: u16, body: String, session: Option<String>) -> Response {
    let accept = req.header("accept").unwrap_or("");
    let sse = accept.contains("text/event-stream") && !accept.contains("application/json") && !accept.contains("*/*");
    let r = if sse {
        Response::new(status, "text/event-stream", format!("event: message\ndata: {body}\n\n").into_bytes())
    } else {
        Response::new(status, "application/json", body.into_bytes())
    };
    match session {
        Some(s) => r.with_header("mcp-session-id", s),
        None => r,
    }
}

// ---------------------------------------------------------------- tools

type ToolResult = Result<Value, String>;

fn call(params: &Value) -> Result<String, (i64, String)> {
    let name = params.str_field("name").unwrap_or("");
    let empty = obj(vec![]);
    let args = params.get("arguments").filter(|a| matches!(a, Value::Obj(_))).unwrap_or(&empty);
    let out = match name {
        "create_repo" => create_repo(args),
        "get_repo" => with_repo(args, |r| Ok(repo_json(r))),
        "create_issue" => create_issue(args),
        "list_issues" => list_issues(args),
        "create_pull_request" => create_pull(args),
        "merge_pull_request" => merge_pull(args),
        _ => return Err((-32602, format!("Unknown tool: {name}"))),
    };
    let (text, is_error) = match out {
        Ok(v) => (v.to_json(), false),
        Err(e) => (e, true),
    };
    Ok(obj(vec![
        ("content", Value::Arr(vec![obj(vec![("type", "text".into()), ("text", text.into())])])),
        ("isError", is_error.into()),
    ])
    .to_json())
}

fn need<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.str_field(key).filter(|s| !s.is_empty()).ok_or_else(|| format!("missing required parameter: {key}"))
}

fn number_arg(args: &Value, key: &str) -> Result<u64, String> {
    let v = args.get(key).ok_or_else(|| format!("missing required parameter: {key}"))?;
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|n| *n > 0)
        .map(|n| n as u64)
        .ok_or_else(|| format!("parameter {key} must be a positive number"))
}

fn find_repo(args: &Value) -> Result<RepoRef, String> {
    let (owner, repo) = (need(args, "owner")?, need(args, "repo")?);
    store().repo(owner, repo).ok_or_else(|| format!("Not Found: repository {owner}/{repo}"))
}

fn with_repo(args: &Value, f: impl FnOnce(&mut store::Repo) -> ToolResult) -> ToolResult {
    let r = find_repo(args)?;
    let mut g = lock(&r);
    f(&mut g)
}

fn store_err(e: StoreError) -> String {
    match e {
        StoreError::AlreadyExists => "name already exists on this account".to_string(),
        StoreError::NotFound => "Not Found".to_string(),
        StoreError::UnknownLabel(l) => format!("label does not exist: {l}"),
        StoreError::Conflict => "Pull Request is not mergeable".to_string(),
    }
}

fn repo_json(r: &store::Repo) -> Value {
    let full = format!("{}/{}", r.owner, r.name);
    obj(vec![
        ("id", r.id.into()),
        ("node_id", node_id(Kind::Repo, r.id).into()),
        ("name", r.name.as_str().into()),
        ("full_name", full.into()),
        ("owner", obj(vec![("login", r.owner.as_str().into())])),
        ("private", r.private.into()),
        ("description", r.description.as_str().into()),
        ("default_branch", r.default_branch.as_str().into()),
    ])
}

fn issue_json(i: &store::Issue) -> Value {
    obj(vec![
        ("id", i.id.into()),
        ("node_id", node_id(Kind::Issue, i.id).into()),
        ("number", i.number.into()),
        ("title", i.title.as_str().into()),
        ("body", i.body.as_str().into()),
        ("state", if i.open { "open" } else { "closed" }.into()),
    ])
}

fn pull_json(r: &store::Repo, p: &store::Pull) -> Value {
    let (head, base) = r.pull_shas(p);
    let side = |name: &str, sha: &store::Oid| obj(vec![("ref", name.into()), ("sha", hex(sha).into())]);
    obj(vec![
        ("id", p.id.into()),
        ("node_id", node_id(Kind::Pull, p.id).into()),
        ("number", p.number.into()),
        ("title", p.title.as_str().into()),
        ("body", p.body.as_str().into()),
        ("state", if p.open { "open" } else { "closed" }.into()),
        ("merged", p.merged.into()),
        ("head", side(&p.head, &head)),
        ("base", side(&p.base, &base)),
    ])
}

fn create_repo(args: &Value) -> ToolResult {
    let name = need(args, "name")?;
    let desc = args.str_field("description").unwrap_or("");
    let private = args.get("private").and_then(Value::as_bool).unwrap_or(false);
    let r = store().create_repo(VIEWER, name, desc, private).map_err(store_err)?;
    let g = lock(&r);
    Ok(repo_json(&g))
}

fn create_issue(args: &Value) -> ToolResult {
    let title = need(args, "title")?;
    let body = args.str_field("body").unwrap_or("");
    let labels: Vec<&str> = args.get("labels").and_then(Value::as_array).unwrap_or(&[]).iter().filter_map(Value::as_str).collect();
    with_repo(args, |r| Ok(issue_json(r.create_issue(title, body, &labels).map_err(store_err)?)))
}

fn list_issues(args: &Value) -> ToolResult {
    let filter = match args.str_field("state").unwrap_or("open").to_ascii_lowercase().as_str() {
        "closed" => Some(false),
        "all" => None,
        _ => Some(true),
    };
    with_repo(args, |r| Ok(Value::Arr(r.list_issues(filter).map(issue_json).collect())))
}

fn create_pull(args: &Value) -> ToolResult {
    let (title, head, base) = (need(args, "title")?, need(args, "head")?, need(args, "base")?);
    let body = args.str_field("body").unwrap_or("");
    with_repo(args, |r| {
        let n = r.create_pull(title, body, head, base).map_err(|e| match e {
            StoreError::NotFound => format!("head {head} or base {base} branch not found"),
            e => store_err(e),
        })?;
        Ok(pull_json(r, r.pull(n).unwrap()))
    })
}

fn merge_pull(args: &Value) -> ToolResult {
    let n = number_arg(args, "pull_number")?;
    let msg = args.str_field("commit_message");
    with_repo(args, |r| {
        let sha = r.merge_pull(n, msg).map_err(store_err)?;
        Ok(obj(vec![("sha", hex(&sha).into()), ("merged", true.into()), ("message", "Pull Request successfully merged".into())]))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(body: &str) -> Response {
        let req = Request {
            method: "POST".into(),
            path: "/mcp".into(),
            query: String::new(),
            headers: vec![("accept".into(), "application/json, text/event-stream".into())],
            body: body.as_bytes().to_vec(),
        };
        handle(&req)
    }

    fn result(body: &str) -> Value {
        let r = post(body);
        assert_eq!(r.status, 200);
        parse(&r.body).unwrap()
    }

    #[test]
    fn tools_list_is_valid_and_complete() {
        let v = result(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        let tools = v.get("result").unwrap().get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 6);
        for t in tools {
            assert!(!t.str_field("description").unwrap().is_empty());
            assert_eq!(t.get("inputSchema").unwrap().str_field("type"), Some("object"));
        }
    }

    #[test]
    fn errors() {
        assert_eq!(post(r#"{"jsonrpc": "2.0", "id": 424242, "method": "tools/list""#).status, 400);
        let v = result(r#"{"jsonrpc":"2.0","id":"x","method":"tools/call","params":{"name":"nope","arguments":{}}}"#);
        assert_eq!(v.get("error").unwrap().get("code").unwrap().as_i64(), Some(-32602));
        let v = result(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"create_issue","arguments":{}}}"#);
        assert_eq!(v.get("result").unwrap().get("isError"), Some(&Value::Bool(true)));
        assert_eq!(post(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).status, 202);
    }

    #[test]
    fn repo_and_issue_flow() {
        let v = result(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_repo","arguments":{"name":"mcp-t-flow","private":true}}}"#);
        assert_eq!(v.get("result").unwrap().get("isError"), Some(&Value::Bool(false)));
        let call = format!(r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"create_issue","arguments":{{"owner":"{VIEWER}","repo":"mcp-t-flow","title":"T1"}}}}}}"#);
        result(&call);
        let call = format!(r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"list_issues","arguments":{{"owner":"{VIEWER}","repo":"mcp-t-flow","state":"open"}}}}}}"#);
        let v = result(&call);
        let text = v.get("result").unwrap().get("content").unwrap().as_array().unwrap()[0].str_field("text").unwrap().to_string();
        assert!(text.contains("\"number\":1") && text.contains("T1"), "{text}");
    }

    #[test]
    fn pull_create_and_merge() {
        let r = store().create_repo(VIEWER, "mcp-t-pr", "", false).unwrap();
        {
            let mut g = lock(&r);
            let (c1, _) = g.commit_file("main", "a.txt", Some(b"a\n".to_vec()), "init", None).unwrap();
            g.refs.insert("refs/heads/feat".into(), c1);
            g.commit_file("feat", "b.txt", Some(b"b\n".to_vec()), "b", None).unwrap();
        }
        let call = |name: &str, args: &str| {
            let v = result(&format!(r#"{{"jsonrpc":"2.0","id":"c","method":"tools/call","params":{{"name":"{name}","arguments":{{"owner":"{VIEWER}","repo":"mcp-t-pr",{args}}}}}}}"#));
            let res = v.get("result").unwrap().clone();
            assert_eq!(res.get("isError"), Some(&Value::Bool(false)), "{}", v.to_json());
            parse(res.get("content").unwrap().as_array().unwrap()[0].str_field("text").unwrap().as_bytes()).unwrap()
        };
        let pr = call("create_pull_request", r#""title":"P","head":"feat","base":"main""#);
        let n = pr.get("number").unwrap().as_i64().unwrap();
        call("merge_pull_request", &format!(r#""pull_number":{n}"#));
        assert!(lock(&r).pull(n as u64).unwrap().merged);
    }
}
