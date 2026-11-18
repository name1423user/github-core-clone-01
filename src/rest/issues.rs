//! Labels, issues, issue comments.

use super::{body, json_array, list_response, paged, repo_api, user_json, validation, WEB};
use crate::http::{Request, Response};
use crate::store::{lock, node_id, now, Comment, Issue, Kind, Label, Repo, RepoRef, StoreError};
use crate::util::json::{obj, Value};
use std::sync::Arc;

pub fn handle(req: &Request, repo: &RepoRef, rest: &[&str]) -> Response {
    let num = |s: &str| s.parse::<u64>().ok();
    match (req.method.as_str(), rest) {
        ("GET", ["labels"]) => {
            let r = lock(repo);
            let (range, link) = paged(req, r.labels.len());
            list_response(json_array(r.labels[range].iter().map(|l| label_json(&r, l))), link)
        }
        ("POST", ["labels"]) => create_label(req, repo),
        ("GET", ["issues"]) => list(req, repo),
        ("POST", ["issues"]) => create(req, repo),
        ("GET", ["issues", n]) => match num(n) {
            Some(n) => get(repo, n),
            None => Response::not_found(),
        },
        ("PATCH", ["issues", n]) => match num(n) {
            Some(n) => patch(req, repo, n),
            None => Response::not_found(),
        },
        ("GET", ["issues", n, "comments"]) => {
            let r = lock(repo);
            let Some(i) = num(n).and_then(|n| r.issue(n)) else { return Response::not_found() };
            let (range, link) = paged(req, i.comments.len());
            list_response(json_array(i.comments[range].iter().map(|c| comment_json(&r, i.number, c))), link)
        }
        ("POST", ["issues", n, "comments"]) => {
            let b = body!(req);
            let Some(text) = b.str_field("body") else { return validation("IssueComment", "body", "missing_field", "body is required") };
            let mut r = lock(repo);
            let Some(n) = num(n) else { return Response::not_found() };
            match r.add_comment(n, text) {
                Ok(c) => {
                    let c = Comment { id: c.id, body: c.body.clone() };
                    Response::json(201, comment_json(&r, n, &c).to_json())
                }
                Err(_) => Response::not_found(),
            }
        }
        _ => Response::not_found(),
    }
}

pub fn label_json(r: &Repo, l: &Label) -> Value {
    obj(vec![
        ("id", l.id.into()),
        ("node_id", node_id(Kind::Label, l.id).into()),
        ("url", format!("{}/labels/{}", repo_api(r), l.name).into()),
        ("name", l.name.as_str().into()),
        ("color", l.color.as_str().into()),
        ("default", false.into()),
        ("description", if l.description.is_empty() { Value::Null } else { l.description.as_str().into() }),
    ])
}

pub fn comment_json(r: &Repo, number: u64, c: &Comment) -> Value {
    let t = now().iso.as_str();
    let api = repo_api(r);
    obj(vec![
        ("id", c.id.into()),
        ("node_id", node_id(Kind::Comment, c.id).into()),
        ("url", format!("{api}/issues/comments/{}", c.id).into()),
        ("html_url", format!("{WEB}/{}/{}/issues/{number}#issuecomment-{}", r.owner, r.name, c.id).into()),
        ("issue_url", format!("{api}/issues/{number}").into()),
        ("body", c.body.as_str().into()),
        ("user", user_json(&r.owner)),
        ("created_at", t.into()),
        ("updated_at", t.into()),
        ("author_association", "OWNER".into()),
    ])
}

pub fn issue_json(r: &Repo, i: &Issue) -> Value {
    let t = now().iso.as_str();
    let api = repo_api(r);
    obj(vec![
        ("id", i.id.into()),
        ("node_id", node_id(Kind::Issue, i.id).into()),
        ("url", format!("{api}/issues/{}", i.number).into()),
        ("repository_url", api.as_str().into()),
        ("html_url", format!("{WEB}/{}/{}/issues/{}", r.owner, r.name, i.number).into()),
        ("number", i.number.into()),
        ("state", if i.open { "open" } else { "closed" }.into()),
        ("state_reason", if i.open { Value::Null } else { "completed".into() }),
        ("title", i.title.as_str().into()),
        ("body", i.body.as_str().into()),
        ("user", user_json(&r.owner)),
        ("labels", json_array(i.labels.iter().filter_map(|&x| r.labels.get(x)).map(|l| label_json(r, l)))),
        ("assignee", Value::Null),
        ("assignees", Value::Arr(vec![])),
        ("locked", false.into()),
        ("comments", (i.comments.len() as u64).into()),
        ("created_at", t.into()),
        ("updated_at", t.into()),
        ("closed_at", if i.open { Value::Null } else { t.into() }),
        ("author_association", "OWNER".into()),
    ])
}

/// Serialize once and keep the bytes on the issue (read bench); `issue_mut` drops them on change.
fn cached(r: &mut Repo, number: u64) -> Option<Arc<[u8]>> {
    let idx = r.issues.binary_search_by_key(&number, |i| i.number).ok()?;
    if let Some(j) = &r.issues[idx].json {
        return Some(j.clone());
    }
    let bytes: Arc<[u8]> = Arc::from(issue_json(r, &r.issues[idx]).to_json().into_bytes());
    r.issues[idx].json = Some(bytes.clone());
    Some(bytes)
}

fn get(repo: &RepoRef, n: u64) -> Response {
    match cached(&mut lock(repo), n) {
        Some(b) => Response::json(200, b.to_vec()),
        None => Response::not_found(),
    }
}

fn create_label(req: &Request, repo: &RepoRef) -> Response {
    let b = body!(req);
    let Some(name) = b.str_field("name").filter(|n| !n.is_empty()) else {
        return validation("Label", "name", "missing_field", "name is required");
    };
    let color = b.str_field("color").unwrap_or("ededed").trim_start_matches('#');
    let desc = b.str_field("description").unwrap_or("");
    let mut r = lock(repo);
    let id = match r.create_label(name, color, desc) {
        Ok(l) => l.id,
        Err(_) => return validation("Label", "name", "already_exists", "Label already exists"),
    };
    let l = r.labels.iter().find(|l| l.id == id).unwrap();
    Response::json(201, label_json(&r, l).to_json())
}

/// `labels` as strings or `{name}` objects.
fn label_names(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|l| l.as_str().or_else(|| l.str_field("name")).map(str::to_string))
        .collect()
}

fn unknown_label(name: &str) -> Response {
    validation("Label", "labels", "invalid", &format!("Label does not exist: {name}"))
}

fn create(req: &Request, repo: &RepoRef) -> Response {
    let b = body!(req);
    let Some(title) = b.str_field("title").filter(|t| !t.is_empty()) else {
        return validation("Issue", "title", "missing_field", "title is required");
    };
    let text = b.str_field("body").unwrap_or("");
    let labels = label_names(b.get("labels"));
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut r = lock(repo);
    let n = match r.create_issue(title, text, &refs) {
        Ok(i) => i.number,
        Err(StoreError::UnknownLabel(l)) => return unknown_label(&l),
        Err(_) => return Response::error(422, "Unprocessable Entity"),
    };
    let bytes = cached(&mut r, n).unwrap();
    Response::json(201, bytes.to_vec())
}

fn list(req: &Request, repo: &RepoRef) -> Response {
    let r = lock(repo);
    let open = match req.query_param("state").as_deref() {
        Some("all") => None,
        Some("closed") => Some(false),
        _ => Some(true),
    };
    let labels: Vec<usize> = req
        .query_param("labels")
        .map(|s| s.split(',').filter_map(|n| r.label_index(n.trim())).collect())
        .unwrap_or_default();
    let mut items: Vec<&Issue> = r.list_issues(open).filter(|i| labels.iter().all(|l| i.labels.contains(l))).collect();
    // All issues share the trial's now, so creation order = number order. GitHub's default is newest first.
    if req.query_param("direction").as_deref() != Some("asc") {
        items.reverse();
    }
    let (range, link) = paged(req, items.len());
    list_response(json_array(items[range].iter().map(|i| issue_json(&r, i))), link)
}

fn patch(req: &Request, repo: &RepoRef, n: u64) -> Response {
    let b = body!(req);
    let mut r = lock(repo);
    let labels = match b.get("labels") {
        Some(v) => {
            let mut idx = Vec::new();
            for name in label_names(Some(v)) {
                match r.label_index(&name) {
                    Some(i) => idx.push(i),
                    None => return unknown_label(&name),
                }
            }
            Some(idx)
        }
        None => None,
    };
    let open = match b.str_field("state") {
        Some("closed") => Some(false),
        Some("open") => Some(true),
        Some(_) => return validation("Issue", "state", "invalid", "state must be open or closed"),
        None => None,
    };
    let Some(i) = r.issue_mut(n) else { return Response::not_found() };
    if let Some(t) = b.str_field("title") {
        i.title = t.to_string();
    }
    if let Some(t) = b.get("body") {
        i.body = t.as_str().unwrap_or("").to_string();
    }
    if let Some(o) = open {
        i.open = o;
    }
    if let Some(l) = labels {
        i.labels = l;
    }
    let bytes = cached(&mut r, n).unwrap();
    Response::json(200, bytes.to_vec())
}
