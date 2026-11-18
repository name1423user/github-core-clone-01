//! Repositories, refs, branches, commits.

use super::{body, json_array, list_response, paged, repo_api, validation, API, WEB};
use crate::http::{Request, Response};
use crate::store::{self, lock, node_id, now, Kind, Oid, Repo, RepoRef, VIEWER};
use crate::util::json::{obj, Value};
use crate::util::sha1::{from_hex, hex};

pub fn user_json(login: &str) -> Value {
    obj(vec![
        ("login", login.into()),
        ("id", 1u64.into()),
        ("node_id", "U_kgDOAAAAAQ".into()),
        ("url", format!("{API}/users/{login}").into()),
        ("html_url", format!("{WEB}/{login}").into()),
        ("type", "User".into()),
        ("site_admin", false.into()),
    ])
}

pub fn repo_json(r: &Repo) -> Value {
    let t = now().iso.as_str();
    obj(vec![
        ("id", r.id.into()),
        ("node_id", node_id(Kind::Repo, r.id).into()),
        ("name", r.name.as_str().into()),
        ("full_name", format!("{}/{}", r.owner, r.name).into()),
        ("private", r.private.into()),
        ("owner", user_json(&r.owner)),
        ("html_url", format!("{WEB}/{}/{}", r.owner, r.name).into()),
        ("description", if r.description.is_empty() { Value::Null } else { r.description.as_str().into() }),
        ("fork", false.into()),
        ("url", repo_api(r).into()),
        ("clone_url", format!("{WEB}/{}/{}.git", r.owner, r.name).into()),
        ("created_at", t.into()),
        ("updated_at", t.into()),
        ("pushed_at", t.into()),
        ("visibility", if r.private { "private" } else { "public" }.into()),
        ("default_branch", r.default_branch.as_str().into()),
        ("permissions", obj(vec![("admin", true.into()), ("push", true.into()), ("pull", true.into())])),
    ])
}

pub fn list(req: &Request) -> Response {
    let repos = store::store().list_repos(VIEWER);
    let (range, link) = paged(req, repos.len());
    list_response(json_array(repos[range].iter().map(|r| repo_json(&lock(r)))), link)
}

pub fn create(req: &Request) -> Response {
    let b = body!(req);
    let Some(name) = b.str_field("name").filter(|n| !n.is_empty()) else {
        return validation("Repository", "name", "missing_field", "name is required");
    };
    let desc = b.str_field("description").unwrap_or("");
    let private = b.get("private").and_then(Value::as_bool).unwrap_or(false);
    match store::store().create_repo(VIEWER, name, desc, private) {
        Ok(r) => Response::json(201, repo_json(&lock(&r)).to_json()),
        Err(_) => validation("Repository", "name", "custom", "name already exists on this account"),
    }
}

pub fn patch(req: &Request, repo: &RepoRef) -> Response {
    let b = body!(req);
    let mut r = lock(repo);
    if let Some(d) = b.get("description") {
        r.description = d.as_str().unwrap_or("").to_string();
    }
    if let Some(p) = b.get("private").and_then(Value::as_bool) {
        r.private = p;
    }
    if let Some(v) = b.get("visibility").and_then(Value::as_str) {
        r.private = v != "public";
    }
    if let Some(d) = b.str_field("default_branch") {
        if r.branch(d).is_none() && !r.refs.is_empty() {
            return validation("Repository", "default_branch", "invalid", "The branch does not exist");
        }
        r.default_branch = d.to_string();
    }
    // shortcut: rename (`name`) is not supported; it needs a store re-key.
    Response::json(200, repo_json(&r).to_json())
}

fn ref_json(r: &Repo, name: &str, sha: &Oid) -> Value {
    obj(vec![
        ("ref", name.into()),
        ("node_id", format!("REF_{}", hex(sha)).into()),
        ("url", format!("{}/git/{name}", repo_api(r)).into()),
        ("object", obj(vec![("sha", hex(sha).into()), ("type", "commit".into()), ("url", format!("{}/git/commits/{}", repo_api(r), hex(sha)).into())])),
    ])
}

pub fn create_ref(req: &Request, repo: &RepoRef) -> Response {
    let b = body!(req);
    let (Some(name), Some(sha)) = (b.str_field("ref"), b.str_field("sha")) else {
        return validation("Reference", "ref", "missing_field", "ref and sha are required");
    };
    if !name.starts_with("refs/") || name.matches('/').count() < 2 {
        return validation("Reference", "ref", "invalid", "Reference name must start with 'refs/' and have at least two slashes");
    }
    let mut r = lock(repo);
    let Some(oid) = from_hex(sha).filter(|o| r.object(o).is_some()) else {
        return validation("Reference", "sha", "invalid", "Object does not exist");
    };
    if r.refs.contains_key(name) {
        return validation("Reference", "ref", "already_exists", "Reference already exists");
    }
    r.refs.insert(name.to_string(), oid);
    Response::json(201, ref_json(&r, name, &oid).to_json())
}

/// `path` is after `git/refs/` (e.g. `heads/main`).
pub fn get_ref(repo: &RepoRef, path: &str) -> Response {
    let r = lock(repo);
    let name = format!("refs/{path}");
    match r.refs.get(&name) {
        Some(oid) => Response::json(200, ref_json(&r, &name, oid).to_json()),
        None => Response::not_found(),
    }
}

fn branch_json(r: &Repo, name: &str, sha: &Oid) -> Value {
    obj(vec![
        ("name", name.into()),
        ("commit", obj(vec![("sha", hex(sha).into()), ("url", format!("{}/commits/{}", repo_api(r), hex(sha)).into())])),
        ("protected", false.into()),
    ])
}

pub fn branches(req: &Request, repo: &RepoRef) -> Response {
    let r = lock(repo);
    let list: Vec<(&str, &Oid)> = r.refs.iter().filter_map(|(k, v)| Some((k.strip_prefix("refs/heads/")?, v))).collect();
    let (range, link) = paged(req, list.len());
    list_response(json_array(list[range].iter().map(|(n, s)| branch_json(&r, n, s))), link)
}

pub fn branch(repo: &RepoRef, name: &str) -> Response {
    let r = lock(repo);
    match r.branch(name) {
        Some(sha) => Response::json(200, branch_json(&r, name, &sha).to_json()),
        None => Response::error(404, "Branch not found"),
    }
}

/// "Name <email> 1735689600 +0000" → (name, email, iso date).
fn person(line: &str) -> (String, String, String) {
    let (name, rest) = line.split_once(" <").unwrap_or((line, ""));
    let (email, rest) = rest.split_once('>').unwrap_or((rest, ""));
    let secs = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(now().secs);
    (name.to_string(), email.to_string(), store::iso(secs))
}

pub fn commit_json(r: &Repo, oid: &Oid) -> Option<Value> {
    let o = r.object(oid)?;
    let info = store::parse_commit(&o.data)?;
    let text = String::from_utf8_lossy(&o.data);
    let header = text.split("\n\n").next().unwrap_or("");
    let who = |key: &str| {
        let (n, e, d) = header.lines().find_map(|l| l.strip_prefix(key)).map(person).unwrap_or_default();
        obj(vec![("name", n.into()), ("email", e.into()), ("date", d.into())])
    };
    let sha = hex(oid);
    let api = repo_api(r);
    Some(obj(vec![
        ("sha", sha.as_str().into()),
        ("node_id", format!("C_{sha}").into()),
        (
            "commit",
            obj(vec![
                ("author", who("author ")),
                ("committer", who("committer ")),
                ("message", info.message.into()),
                ("tree", obj(vec![("sha", hex(&info.tree).into()), ("url", format!("{api}/git/trees/{}", hex(&info.tree)).into())])),
                ("url", format!("{api}/git/commits/{sha}").into()),
            ]),
        ),
        ("url", format!("{api}/commits/{sha}").into()),
        ("html_url", format!("{WEB}/{}/{}/commit/{sha}", r.owner, r.name).into()),
        ("parents", json_array(info.parents.iter().map(|p| obj(vec![("sha", hex(p).into()), ("url", format!("{api}/commits/{}", hex(p)).into())])))),
    ]))
}

pub fn commits(req: &Request, repo: &RepoRef) -> Response {
    let r = lock(repo);
    let rev = req.query_param("sha").unwrap_or_else(|| r.default_branch.clone());
    let Some(tip) = r.resolve(&rev) else {
        if r.refs.is_empty() {
            return Response::error(409, "Git Repository is empty.");
        }
        return validation("Commit", "sha", "invalid", &format!("No commit found for SHA: {rev}"));
    };
    let log = r.log(tip, usize::MAX);
    let (range, link) = paged(req, log.len());
    list_response(json_array(log[range].iter().filter_map(|c| commit_json(&r, c))), link)
}

pub fn commit(repo: &RepoRef, rev: &str) -> Response {
    let r = lock(repo);
    match r.resolve(rev).and_then(|c| commit_json(&r, &c)) {
        Some(v) => Response::json(200, v.to_json()),
        None => Response::error(404, "No commit found"),
    }
}
