//! Contents API: GET file/dir, PUT (create/update as one commit), DELETE.

use super::{b64_decode, b64_encode, body, json_array, repo_api, validation, WEB};
use crate::http::{Request, Response};
use crate::store::{self, lock, Oid, Repo, RepoRef, StoreError, MODE_TREE};
use crate::util::json::{obj, Value};
use crate::util::sha1::{from_hex, hex};

pub fn handle(req: &Request, repo: &RepoRef, path: &str) -> Response {
    let path = path.trim_matches('/');
    match req.method.as_str() {
        "GET" => get(req, repo, path),
        "PUT" => put(req, repo, path),
        "DELETE" => delete(req, repo, path),
        _ => Response::not_found(),
    }
}

/// Walk a commit's tree to `path` ("" = root) → (mode, oid).
fn walk(r: &Repo, commit: &Oid, path: &str) -> Option<(String, Oid)> {
    let mut cur = (MODE_TREE.to_string(), r.commit(commit)?.tree);
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if cur.0 != MODE_TREE {
            return None;
        }
        let data = &r.object(&cur.1)?.data;
        cur = store::parse_tree(data).into_iter().find(|e| e.1 == seg).map(|(m, _, o)| (m.to_string(), o))?;
    }
    Some(cur)
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn entry_json(r: &Repo, path: &str, mode: &str, oid: &Oid, content: Option<&[u8]>) -> Value {
    let is_dir = mode == MODE_TREE;
    let size = if is_dir { 0 } else { r.object(oid).map_or(0, |o| o.data.len()) };
    let mut v = vec![
        ("type", if is_dir { "dir" } else { "file" }.into()),
        ("size", (size as u64).into()),
        ("name", name_of(path).into()),
        ("path", path.into()),
        ("sha", hex(oid).into()),
        ("url", format!("{}/contents/{path}", repo_api(r)).into()),
        ("html_url", format!("{WEB}/{}/{}/blob/{}/{path}", r.owner, r.name, r.default_branch).into()),
    ];
    if !is_dir {
        v.push(("encoding", "base64".into()));
        if let Some(c) = content {
            v.push(("content", b64_encode(c).into()));
        }
    }
    obj(v)
}

fn get(req: &Request, repo: &RepoRef, path: &str) -> Response {
    let r = lock(repo);
    let rev = req.query_param("ref").unwrap_or_else(|| r.default_branch.clone());
    let Some(commit) = r.resolve(&rev) else {
        return Response::error(404, if r.refs.is_empty() { "This repository is empty." } else { "No commit found for the ref" });
    };
    let Some((mode, oid)) = walk(&r, &commit, path) else { return Response::not_found() };
    if mode != MODE_TREE {
        let data = r.object(&oid).map(|o| o.data.clone());
        return Response::json(200, entry_json(&r, path, &mode, &oid, data.as_deref()).to_json());
    }
    let Some(tree) = r.object(&oid) else { return Response::not_found() };
    let prefix = if path.is_empty() { String::new() } else { format!("{path}/") };
    let items = store::parse_tree(&tree.data).into_iter().map(|(m, n, o)| entry_json(&r, &format!("{prefix}{n}"), m, &o, None));
    Response::json(200, json_array(items.collect::<Vec<_>>()).to_json())
}

fn commit_body(r: &Repo, commit: &Oid) -> Value {
    super::commit_json(r, commit)
        .and_then(|v| {
            // GitHub's contents reply puts the git-commit fields at the top level of `commit`.
            let Value::Obj(top) = &v else { return None };
            let inner = v.get("commit")?;
            let mut out = vec![("sha".to_string(), v.get("sha")?.clone())];
            if let Value::Obj(fields) = inner {
                out.extend(fields.iter().cloned());
            }
            out.extend(top.iter().filter(|(k, _)| k == "parents" || k == "html_url").cloned());
            Some(Value::Obj(out))
        })
        .unwrap_or_else(|| obj(vec![("sha", hex(commit).into())]))
}

fn put(req: &Request, repo: &RepoRef, path: &str) -> Response {
    let b = body!(req);
    let Some(msg) = b.str_field("message") else { return validation("Contents", "message", "missing_field", "message is required") };
    let Some(content) = b.str_field("content") else { return validation("Contents", "content", "missing_field", "content is required") };
    let Some(bytes) = b64_decode(content) else { return validation("Contents", "content", "invalid", "content is not valid Base64") };
    if path.is_empty() {
        return validation("Contents", "path", "invalid", "path is required");
    }
    let given = b.str_field("sha");
    let mut r = lock(repo);
    let was_empty = r.refs.is_empty();
    let branch = b.str_field("branch").map(str::to_string).unwrap_or_else(|| r.default_branch.clone());
    let existed = r.branch(&branch).and_then(|c| walk(&r, &c, path)).is_some_and(|(m, _)| m != MODE_TREE);
    match r.commit_file(&branch, path, Some(bytes), msg, given.and_then(from_hex)) {
        Ok((commit, blob)) => {
            if was_empty {
                r.default_branch = branch;
            }
            let blob = blob.unwrap_or_default();
            let body = obj(vec![
                ("content", entry_json(&r, path, store::MODE_FILE, &blob, None)),
                ("commit", commit_body(&r, &commit)),
            ]);
            Response::json(if existed { 200 } else { 201 }, body.to_json())
        }
        Err(e) => store_error(e, given.is_some()),
    }
}

fn delete(req: &Request, repo: &RepoRef, path: &str) -> Response {
    let b = body!(req);
    let msg = b.str_field("message").unwrap_or("Delete file");
    let given = b.str_field("sha");
    let mut r = lock(repo);
    let branch = b.str_field("branch").map(str::to_string).unwrap_or_else(|| r.default_branch.clone());
    match r.commit_file(&branch, path, None, msg, given.and_then(from_hex)) {
        Ok((commit, _)) => {
            let body = obj(vec![("content", Value::Null), ("commit", commit_body(&r, &commit))]);
            Response::json(200, body.to_json())
        }
        Err(e) => store_error(e, given.is_some()),
    }
}

fn store_error(e: StoreError, sha_given: bool) -> Response {
    match e {
        StoreError::Conflict if !sha_given => validation("Contents", "sha", "missing_field", "\"sha\" wasn't supplied."),
        StoreError::Conflict => Response::error(409, "sha does not match"),
        StoreError::NotFound => Response::not_found(),
        _ => Response::error(422, "Unprocessable Entity"),
    }
}
