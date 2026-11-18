//! Pull requests: create, list, get, patch, files, squash merge.

use super::{body, json_array, list_response, paged, repo_api, user_json, validation, WEB};
use crate::http::{Request, Response};
use crate::store::{lock, node_id, now, Kind, Pull, Repo, RepoRef, StoreError};
use crate::util::json::{obj, Value};
use crate::util::sha1::hex;

pub fn handle(req: &Request, repo: &RepoRef, rest: &[&str]) -> Response {
    let num = |s: &str| s.parse::<u64>().ok();
    match (req.method.as_str(), rest) {
        ("GET", ["pulls"]) => list(req, repo),
        ("POST", ["pulls"]) => create(req, repo),
        ("GET", ["pulls", n]) => {
            let r = lock(repo);
            match num(n).and_then(|n| r.pull(n)) {
                Some(p) => Response::json(200, pull_json(&r, p).to_json()),
                None => Response::not_found(),
            }
        }
        ("PATCH", ["pulls", n]) => match num(n) {
            Some(n) => patch(req, repo, n),
            None => Response::not_found(),
        },
        ("GET", ["pulls", n, "files"]) => files(req, repo, num(n)),
        ("GET", ["pulls", n, "merge"]) => {
            let r = lock(repo);
            match num(n).and_then(|n| r.pull(n)) {
                Some(p) if p.merged => Response::json(204, Vec::new()),
                _ => Response::not_found(),
            }
        }
        ("PUT", ["pulls", n, "merge"]) => match num(n) {
            Some(n) => merge(req, repo, n),
            None => Response::not_found(),
        },
        _ => Response::not_found(),
    }
}

fn side(r: &Repo, branch: &str, sha: &[u8; 20]) -> Value {
    obj(vec![
        ("label", format!("{}:{branch}", r.owner).into()),
        ("ref", branch.into()),
        ("sha", hex(sha).into()),
        ("user", user_json(&r.owner)),
        ("repo", obj(vec![("id", r.id.into()), ("name", r.name.as_str().into()), ("full_name", format!("{}/{}", r.owner, r.name).into())])),
    ])
}

pub fn pull_json(r: &Repo, p: &Pull) -> Value {
    let t = now().iso.as_str();
    let api = repo_api(r);
    let (head, base) = r.pull_shas(p);
    let when = |b: bool| if b { Value::from(t) } else { Value::Null };
    obj(vec![
        ("id", p.id.into()),
        ("node_id", node_id(Kind::Pull, p.id).into()),
        ("url", format!("{api}/pulls/{}", p.number).into()),
        ("html_url", format!("{WEB}/{}/{}/pull/{}", r.owner, r.name, p.number).into()),
        ("issue_url", format!("{api}/issues/{}", p.number).into()),
        ("number", p.number.into()),
        ("state", if p.open { "open" } else { "closed" }.into()),
        ("locked", false.into()),
        ("title", p.title.as_str().into()),
        ("user", user_json(&r.owner)),
        ("body", p.body.as_str().into()),
        ("labels", Value::Arr(vec![])),
        ("created_at", t.into()),
        ("updated_at", t.into()),
        ("closed_at", when(!p.open)),
        ("merged_at", when(p.merged)),
        ("merge_commit_sha", p.merge_sha.as_ref().map(hex).into()),
        ("draft", false.into()),
        ("head", side(r, &p.head, &head)),
        ("base", side(r, &p.base, &base)),
        ("merged", p.merged.into()),
        ("mergeable", if p.open { Value::from(r.merged_files(p).is_ok()) } else { Value::Null }),
        ("author_association", "OWNER".into()),
    ])
}

fn list(req: &Request, repo: &RepoRef) -> Response {
    let r = lock(repo);
    let state = req.query_param("state");
    let want = |p: &&Pull| match state.as_deref() {
        Some("all") => true,
        Some("closed") => !p.open,
        _ => p.open,
    };
    let mut items: Vec<&Pull> = r.pulls.iter().filter(want).collect();
    if req.query_param("direction").as_deref() != Some("asc") {
        items.reverse();
    }
    let (range, link) = paged(req, items.len());
    list_response(json_array(items[range].iter().map(|p| pull_json(&r, p))), link)
}

fn create(req: &Request, repo: &RepoRef) -> Response {
    let b = body!(req);
    let Some(title) = b.str_field("title").filter(|t| !t.is_empty()) else {
        return validation("PullRequest", "title", "missing_field", "title is required");
    };
    let (Some(head), Some(base)) = (b.str_field("head"), b.str_field("base")) else {
        return validation("PullRequest", "head", "missing_field", "head and base are required");
    };
    // `owner:branch` form for head.
    let head = head.split_once(':').map_or(head, |(_, h)| h);
    let mut r = lock(repo);
    if r.branch(head).is_none() {
        return validation("PullRequest", "head", "invalid", "head does not exist");
    }
    if r.branch(base).is_none() {
        return validation("PullRequest", "base", "invalid", "base does not exist");
    }
    match r.create_pull(title, b.str_field("body").unwrap_or(""), head, base) {
        Ok(n) => Response::json(201, pull_json(&r, r.pull(n).unwrap()).to_json()),
        Err(_) => validation("PullRequest", "head", "invalid", "head or base does not exist"),
    }
}

fn patch(req: &Request, repo: &RepoRef, n: u64) -> Response {
    let b = body!(req);
    let mut r = lock(repo);
    let Some(p) = r.pull(n) else { return Response::not_found() };
    let shas = r.pull_shas(p);
    let p = r.pull_mut(n).unwrap();
    if let Some(t) = b.str_field("title") {
        p.title = t.to_string();
    }
    if let Some(t) = b.get("body") {
        p.body = t.as_str().unwrap_or("").to_string();
    }
    match b.str_field("state") {
        Some("closed") if p.open => {
            // Freeze the tips at close time (pull_shas reads the recorded ones once closed).
            (p.head_sha, p.base_sha) = shas;
            p.open = false;
        }
        Some("open") if !p.merged => p.open = true,
        _ => {}
    }
    let p = r.pull(n).unwrap();
    Response::json(200, pull_json(&r, p).to_json())
}

fn files(req: &Request, repo: &RepoRef, n: Option<u64>) -> Response {
    let r = lock(repo);
    let Some(p) = n.and_then(|n| r.pull(n)) else { return Response::not_found() };
    let changes = r.pull_files(p);
    let (range, link) = paged(req, changes.len());
    let items = changes[range].iter().map(|f| {
        obj(vec![
            ("filename", f.path.as_str().into()),
            ("status", f.status.into()),
            ("additions", f.additions.into()),
            ("deletions", f.deletions.into()),
            ("changes", (f.additions + f.deletions).into()),
            ("contents_url", format!("{}/contents/{}", repo_api(&r), f.path).into()),
        ])
    });
    list_response(json_array(items), link)
}

fn merge(req: &Request, repo: &RepoRef, n: u64) -> Response {
    let b = body!(req);
    let mut r = lock(repo);
    let Some(p) = r.pull(n) else { return Response::not_found() };
    if p.merged {
        return Response::error(405, "Pull Request is not mergeable");
    }
    let title = b.str_field("commit_title").map(str::to_string).unwrap_or_else(|| format!("{} (#{n})", p.title));
    let msg = match b.str_field("commit_message").filter(|m| !m.is_empty()) {
        Some(m) => format!("{title}\n\n{m}"),
        None => title,
    };
    match r.merge_pull(n, Some(&msg)) {
        Ok(sha) => Response::json(
            200,
            obj(vec![("sha", hex(&sha).into()), ("merged", true.into()), ("message", "Pull Request successfully merged".into())]).to_json(),
        ),
        Err(StoreError::NotFound) => Response::not_found(),
        Err(_) => Response::error(405, "Pull Request is not mergeable"),
    }
}
