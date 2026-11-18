//! GraphQL v4（POST /api/graphql）: 自前のパーサ・実行器・スキーマ。持ち場: graphql-engineer。
//! Scope: the documents in checks-graphql.ts:107-317 (gh / github-mcp-server shapes). Resolvers call the same
//! store functions as REST, and ids are `store::node_id` (= REST `node_id`).

mod parse;

use crate::http::{Request, Response};
use crate::store::{self, lock, node_id, parse_node_id, store, Issue, Kind, Pull, Repo, RepoRef, VIEWER};
use crate::util::json::{obj, Value};
use parse::{Field, Sel};

pub fn handle(req: &Request) -> Response {
    let body = match req.json() {
        Ok(v) => v,
        Err(r) => return r,
    };
    // A JSON array is GitHub's operation batching: one result per operation, in order.
    let out = match &body {
        Value::Arr(ops) => Value::Arr(ops.iter().map(run).collect()),
        b => run(b),
    };
    Response::json(200, out.to_json())
}

fn error_only(msg: String) -> Value {
    obj(vec![("errors", Value::Arr(vec![obj(vec![("message", msg.into())])]))])
}

fn run(body: &Value) -> Value {
    let Some(src) = body.str_field("query") else { return error_only("A query attribute must be specified".into()) };
    let ops = match parse::parse(src) {
        Ok(o) => o,
        Err(e) => return error_only(e),
    };
    let op = match body.str_field("operationName") {
        Some(n) => match ops.iter().find(|o| o.name.as_deref() == Some(n)) {
            Some(o) => o,
            None => return error_only(format!("Unknown operation named '{n}'")),
        },
        None => &ops[0],
    };
    let given = body.get("variables");
    let vars = Value::Obj(
        op.vars
            .iter()
            .map(|(n, def)| {
                let v = match given.and_then(|g| g.get(n)) {
                    Some(v) => v.clone(),
                    None => def.as_ref().map_or(Value::Null, |d| d.eval(&Value::Null)),
                };
                (n.clone(), v)
            })
            .collect(),
    );
    let mut cx = Ctx { vars, errors: Vec::new(), path: Vec::new() };
    let data = if op.mutation {
        object(&mut cx, &op.sel, "Mutation", &mut mutation_root)
    } else {
        object(&mut cx, &op.sel, "Query", &mut query_root)
    };
    let mut out = vec![("data", data)];
    if !cx.errors.is_empty() {
        out.push(("errors", Value::Arr(cx.errors)));
    }
    obj(out)
}

// ---------------------------------------------------------------- executor

struct Ctx {
    vars: Value,
    errors: Vec<Value>,
    path: Vec<String>,
}

impl Ctx {
    fn arg(&self, f: &Field, name: &str) -> Value {
        f.args.iter().find(|(k, _)| k == name).map_or(Value::Null, |(_, a)| a.eval(&self.vars))
    }

    /// A field-level error; the field reads as null.
    fn error(&mut self, f: &Field, msg: String) -> Option<Value> {
        let mut path: Vec<Value> = self.path.iter().map(|p| p.as_str().into()).collect();
        path.push(f.key().into());
        self.errors.push(obj(vec![
            ("message", msg.into()),
            ("locations", Value::Arr(vec![obj(vec![("line", (f.line as i64).into()), ("column", (f.col as i64).into())])])),
            ("path", Value::Arr(path)),
        ]));
        Some(Value::Null)
    }
}

/// A resolver answers one field of an object, or `None` when the type has no such field.
type Resolve<'a> = dyn FnMut(&mut Ctx, &Field) -> Option<Value> + 'a;

fn object(cx: &mut Ctx, sel: &[Sel], ty: &str, f: &mut Resolve) -> Value {
    let mut out = Vec::new();
    collect(cx, sel, ty, f, &mut out);
    Value::Obj(out)
}

fn collect(cx: &mut Ctx, sel: &[Sel], ty: &str, f: &mut Resolve, out: &mut Vec<(String, Value)>) {
    for s in sel {
        match s {
            Sel::Inline(cond, inner) => {
                if cond.as_deref().is_none_or(|c| c == ty || c == "Node") {
                    collect(cx, inner, ty, f, out);
                }
            }
            Sel::Field(fl) => {
                let v = if fl.name == "__typename" {
                    Some(ty.into())
                } else {
                    cx.path.push(fl.key().to_string());
                    let v = f(cx, fl);
                    cx.path.pop();
                    v
                };
                match v {
                    Some(v) => out.push((fl.key().to_string(), v)),
                    None => {
                        cx.error(fl, format!("Field '{}' doesn't exist on type '{ty}'", fl.name));
                    }
                }
            }
        }
    }
}

fn cursor(i: usize) -> Value {
    format!("Y3Vyc29yOnYyOpHO{i}").into()
}

/// A Relay connection over `items`: nodes, edges{cursor node}, pageInfo, totalCount; `first`/`after`.
fn connection<T>(cx: &mut Ctx, f: &Field, ty: &str, items: &[T], node: &mut dyn FnMut(&mut Ctx, &Field, &T) -> Value) -> Value {
    let total = items.len();
    let after = cx.arg(f, "after");
    let start = after.as_str().and_then(|c| c.strip_prefix("Y3Vyc29yOnYyOpHO")?.parse::<usize>().ok()).map_or(0, |i| i + 1).min(total);
    let first = cx.arg(f, "first").as_i64().map_or(100, |n| n.max(0) as usize);
    let end = (start + first).min(total);
    let page = &items[start..end];
    object(cx, &f.sel, &format!("{ty}Connection"), &mut |cx, g| {
        Some(match g.name.as_str() {
            "totalCount" => (total as i64).into(),
            "nodes" => {
                let mut v = Vec::new();
                for t in page {
                    v.push(node(cx, g, t));
                }
                Value::Arr(v)
            }
            "edges" => {
                let mut v = Vec::new();
                for (k, t) in page.iter().enumerate() {
                    v.push(object(cx, &g.sel, &format!("{ty}Edge"), &mut |cx, h| {
                        Some(match h.name.as_str() {
                            "cursor" => cursor(start + k),
                            "node" => node(cx, h, t),
                            _ => return None,
                        })
                    }));
                }
                Value::Arr(v)
            }
            "pageInfo" => object(cx, &g.sel, "PageInfo", &mut |_, h| {
                Some(match h.name.as_str() {
                    "hasNextPage" => (end < total).into(),
                    "hasPreviousPage" => (start > 0).into(),
                    "startCursor" => if page.is_empty() { Value::Null } else { cursor(start) },
                    "endCursor" => if page.is_empty() { Value::Null } else { cursor(end - 1) },
                    _ => return None,
                })
            }),
            _ => return None,
        })
    })
}

// ---------------------------------------------------------------- resolvers

fn user(cx: &mut Ctx, sel: &[Sel], login: &str) -> Value {
    object(cx, sel, "User", &mut |_, f| Some(match f.name.as_str() {
        "login" => login.into(),
        _ => return None,
    }))
}

fn query_root(cx: &mut Ctx, f: &Field) -> Option<Value> {
    Some(match f.name.as_str() {
        "viewer" => user(cx, &f.sel, VIEWER),
        "repository" => {
            let (owner, name) = (cx.arg(f, "owner"), cx.arg(f, "name"));
            match store().repo(owner.as_str().unwrap_or(""), name.as_str().unwrap_or("")) {
                Some(r) => repo_obj(cx, &f.sel, &lock(&r)),
                None => Value::Null,
            }
        }
        "node" => {
            let id = cx.arg(f, "id");
            match parse_node_id(id.as_str().unwrap_or("")) {
                Some((Kind::Repo, id)) => match store().repo_by_id(id) {
                    Some(r) => repo_obj(cx, &f.sel, &lock(&r)),
                    None => Value::Null,
                },
                Some((Kind::Issue, id)) => match find(|r| r.issue_by_id(id).map(|i| i.number)) {
                    Some((r, n)) => {
                        let r = lock(&r);
                        issue_obj(cx, &f.sel, r.issue(n).unwrap())
                    }
                    None => Value::Null,
                },
                Some((Kind::Pull, id)) => match find(|r| r.pull_by_id(id).map(|p| p.number)) {
                    Some((r, n)) => {
                        let r = lock(&r);
                        pull_obj(cx, &f.sel, &r, r.pull(n).unwrap())
                    }
                    None => Value::Null,
                },
                _ => Value::Null,
            }
        }
        "__type" => {
            let name = cx.arg(f, "name");
            type_obj(cx, &f.sel, name.as_str().unwrap_or(""))
        }
        _ => return None,
    })
}

/// The repo holding an object, by a per-repo lookup (issues/PRs carry no repo pointer).
fn find<T>(pred: impl Fn(&Repo) -> Option<T>) -> Option<(RepoRef, T)> {
    let all: Vec<RepoRef> = store().repos_read().values().cloned().collect();
    all.into_iter().find_map(|r| {
        let v = pred(&lock(&r))?;
        Some((r, v))
    })
}

fn repo_obj(cx: &mut Ctx, sel: &[Sel], r: &Repo) -> Value {
    object(cx, sel, "Repository", &mut |cx, f| {
        Some(match f.name.as_str() {
            "id" => node_id(Kind::Repo, r.id).into(),
            "databaseId" => (r.id as i64).into(),
            "name" => r.name.as_str().into(),
            "nameWithOwner" => format!("{}/{}", r.owner, r.name).into(),
            "owner" => user(cx, &f.sel, &r.owner),
            "description" => r.description.as_str().into(),
            "isPrivate" => r.private.into(),
            "viewerPermission" => "ADMIN".into(),
            "hasIssuesEnabled" => true.into(),
            "defaultBranchRef" => match r.branch(&r.default_branch) {
                Some(_) => object(cx, &f.sel, "Ref", &mut |_, g| Some(match g.name.as_str() {
                    "name" => r.default_branch.as_str().into(),
                    _ => return None,
                })),
                None => Value::Null,
            },
            "issue" => match r.issue(cx.arg(f, "number").as_i64().unwrap_or(0) as u64) {
                Some(i) => issue_obj(cx, &f.sel, i),
                None => Value::Null,
            },
            "pullRequest" => match r.pull(cx.arg(f, "number").as_i64().unwrap_or(0) as u64) {
                Some(p) => pull_obj(cx, &f.sel, r, p),
                None => Value::Null,
            },
            "issues" => {
                let states = states(cx, f);
                let mut items: Vec<&Issue> = r.issues.iter().filter(|i| states.is_empty() || states.iter().any(|s| s == issue_state(i))).collect();
                // All issues share the trial's now, so CREATED_AT order is number order (ties by number).
                if desc(cx, f) {
                    items.reverse();
                }
                connection(cx, f, "Issue", &items, &mut |cx, g, i| issue_obj(cx, &g.sel, i))
            }
            "pullRequests" => {
                let states = states(cx, f);
                let mut items: Vec<&Pull> = r.pulls.iter().filter(|p| states.is_empty() || states.iter().any(|s| s == pull_state(p))).collect();
                if desc(cx, f) {
                    items.reverse();
                }
                connection(cx, f, "PullRequest", &items, &mut |cx, g, p| pull_obj(cx, &g.sel, r, p))
            }
            _ => return None,
        })
    })
}

fn states(cx: &Ctx, f: &Field) -> Vec<String> {
    match cx.arg(f, "states") {
        Value::Arr(a) => a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect(),
        Value::Str(s) => vec![s],
        _ => Vec::new(),
    }
}

fn desc(cx: &Ctx, f: &Field) -> bool {
    cx.arg(f, "orderBy").str_field("direction") == Some("DESC")
}

fn issue_state(i: &Issue) -> &'static str {
    if i.open { "OPEN" } else { "CLOSED" }
}

fn pull_state(p: &Pull) -> &'static str {
    if p.merged { "MERGED" } else if p.open { "OPEN" } else { "CLOSED" }
}

fn issue_obj(cx: &mut Ctx, sel: &[Sel], i: &Issue) -> Value {
    object(cx, sel, "Issue", &mut |_, f| {
        Some(match f.name.as_str() {
            "id" => node_id(Kind::Issue, i.id).into(),
            "databaseId" => (i.id as i64).into(),
            "number" => (i.number as i64).into(),
            "title" => i.title.as_str().into(),
            "body" => i.body.as_str().into(),
            "state" => issue_state(i).into(),
            "closed" => (!i.open).into(),
            _ => return None,
        })
    })
}

fn pull_obj(cx: &mut Ctx, sel: &[Sel], r: &Repo, p: &Pull) -> Value {
    object(cx, sel, "PullRequest", &mut |cx, f| {
        Some(match f.name.as_str() {
            "id" => node_id(Kind::Pull, p.id).into(),
            "databaseId" => (p.id as i64).into(),
            "number" => (p.number as i64).into(),
            "title" => p.title.as_str().into(),
            "body" => p.body.as_str().into(),
            "state" => pull_state(p).into(),
            "closed" => (!p.open).into(),
            "merged" => p.merged.into(),
            "headRefName" => p.head.as_str().into(),
            "baseRefName" => p.base.as_str().into(),
            "mergeable" => if !p.open { "UNKNOWN" } else if r.merged_files(p).is_ok() { "MERGEABLE" } else { "CONFLICTING" }.into(),
            "files" => {
                let files = r.pull_files(p);
                connection(cx, f, "PullRequestChangedFile", &files, &mut |cx, g, c| {
                    object(cx, &g.sel, "PullRequestChangedFile", &mut |_, h| Some(match h.name.as_str() {
                        "path" => c.path.as_str().into(),
                        "additions" => (c.additions as i64).into(),
                        "deletions" => (c.deletions as i64).into(),
                        "changeType" => match c.status {
                            "added" => "ADDED",
                            "removed" => "DELETED",
                            _ => "MODIFIED",
                        }
                        .into(),
                        _ => return None,
                    }))
                })
            }
            _ => return None,
        })
    })
}

// ---------------------------------------------------------------- mutations

/// `input.<key>` as a global id of `kind`.
fn input_id(cx: &Ctx, f: &Field, key: &str, kind: Kind) -> Result<u64, String> {
    let input = cx.arg(f, "input");
    let raw = input.str_field(key).unwrap_or("");
    match parse_node_id(raw) {
        Some((k, id)) if k == kind => Ok(id),
        _ => Err(format!("Could not resolve to a node with the global id of '{raw}'")),
    }
}

fn input_str(cx: &Ctx, f: &Field, key: &str) -> Option<String> {
    cx.arg(f, "input").str_field(key).map(str::to_string)
}

/// `{ <field> { ... } }` payload around one result.
fn payload(cx: &mut Ctx, f: &Field, ty: &str, inner: &mut Resolve) -> Value {
    object(cx, &f.sel, ty, &mut |cx, g| if g.name == "clientMutationId" { Some(Value::Null) } else { inner(cx, g) })
}

fn mutation_root(cx: &mut Ctx, f: &Field) -> Option<Value> {
    match f.name.as_str() {
        "createIssue" => {
            let repo = match input_id(cx, f, "repositoryId", Kind::Repo).map(|id| store().repo_by_id(id)) {
                Ok(Some(r)) => r,
                Ok(None) => return cx.error(f, "Could not resolve to a Repository".into()),
                Err(e) => return cx.error(f, e),
            };
            let title = input_str(cx, f, "title").unwrap_or_default();
            let body = input_str(cx, f, "body").unwrap_or_default();
            let mut r = lock(&repo);
            let n = match r.create_issue(&title, &body, &[]) {
                Ok(i) => i.number,
                Err(e) => return cx.error(f, format!("could not create the issue: {e:?}")),
            };
            Some(payload(cx, f, "CreateIssuePayload", &mut |cx, g| (g.name == "issue").then(|| issue_obj(cx, &g.sel, r.issue(n).unwrap()))))
        }
        "closeIssue" | "reopenIssue" => {
            let open = f.name == "reopenIssue";
            let found = input_id(cx, f, "issueId", Kind::Issue).and_then(|id| find(|r| r.issue_by_id(id).map(|i| i.number)).ok_or_else(|| "Could not resolve to an Issue".into()));
            let (repo, n) = match found {
                Ok(x) => x,
                Err(e) => return cx.error(f, e),
            };
            let mut r = lock(&repo);
            r.issue_mut(n).unwrap().open = open;
            let ty = if open { "ReopenIssuePayload" } else { "CloseIssuePayload" };
            Some(payload(cx, f, ty, &mut |cx, g| (g.name == "issue").then(|| issue_obj(cx, &g.sel, r.issue(n).unwrap()))))
        }
        "createPullRequest" => {
            let repo = match input_id(cx, f, "repositoryId", Kind::Repo).map(|id| store().repo_by_id(id)) {
                Ok(Some(r)) => r,
                Ok(None) => return cx.error(f, "Could not resolve to a Repository".into()),
                Err(e) => return cx.error(f, e),
            };
            let [title, body, head, base] = ["title", "body", "headRefName", "baseRefName"].map(|k| input_str(cx, f, k).unwrap_or_default());
            let mut r = lock(&repo);
            let n = match r.create_pull(&title, &body, &head, &base) {
                Ok(n) => n,
                Err(_) => return cx.error(f, format!("Head sha can't be blank, Base sha can't be blank, No commits between {base} and {head}")),
            };
            Some(payload(cx, f, "CreatePullRequestPayload", &mut |cx, g| (g.name == "pullRequest").then(|| pull_obj(cx, &g.sel, &r, r.pull(n).unwrap()))))
        }
        "mergePullRequest" => {
            let found = input_id(cx, f, "pullRequestId", Kind::Pull).and_then(|id| find(|r| r.pull_by_id(id).map(|p| p.number)).ok_or_else(|| "Could not resolve to a PullRequest".into()));
            let (repo, n) = match found {
                Ok(x) => x,
                Err(e) => return cx.error(f, e),
            };
            let headline = input_str(cx, f, "commitHeadline");
            let mut r = lock(&repo);
            if let Err(e) = r.merge_pull(n, headline.as_deref()) {
                let msg = if e == store::StoreError::Conflict { "Pull Request is not mergeable" } else { "Pull Request not found" };
                return cx.error(f, msg.into());
            }
            Some(payload(cx, f, "MergePullRequestPayload", &mut |cx, g| (g.name == "pullRequest").then(|| pull_obj(cx, &g.sel, &r, r.pull(n).unwrap()))))
        }
        "addComment" => {
            let found = input_id(cx, f, "subjectId", Kind::Issue).and_then(|id| find(|r| r.issue_by_id(id).map(|i| i.number)).ok_or_else(|| "Could not resolve to an Issue".into()));
            let (repo, n) = match found {
                Ok(x) => x,
                Err(e) => return cx.error(f, e),
            };
            let body = input_str(cx, f, "body").unwrap_or_default();
            let mut r = lock(&repo);
            let (cid, cbody) = match r.add_comment(n, &body) {
                Ok(c) => (c.id, c.body.clone()),
                Err(e) => return cx.error(f, format!("could not add the comment: {e:?}")),
            };
            let comment = |cx: &mut Ctx, sel: &[Sel]| {
                object(cx, sel, "IssueComment", &mut |_, h| Some(match h.name.as_str() {
                    "id" => node_id(Kind::Comment, cid).into(),
                    "databaseId" => (cid as i64).into(),
                    "body" => cbody.as_str().into(),
                    _ => return None,
                }))
            };
            Some(payload(cx, f, "AddCommentPayload", &mut |cx, g| match g.name.as_str() {
                "commentEdge" => Some(object(cx, &g.sel, "IssueCommentEdge", &mut |cx, h| match h.name.as_str() {
                    "node" => Some(comment(cx, &h.sel)),
                    "cursor" => Some(cursor(0)),
                    _ => None,
                })),
                "subject" => Some(issue_obj(cx, &g.sel, r.issue(n).unwrap())),
                _ => None,
            }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- introspection

/// (type, kind, field names / enum values). Keep in step with the resolvers above.
const TYPES: &[(&str, &str, &[&str])] = &[
    ("Query", "OBJECT", &["viewer", "repository", "node", "__type"]),
    ("Mutation", "OBJECT", &["createIssue", "closeIssue", "reopenIssue", "createPullRequest", "mergePullRequest", "addComment"]),
    ("User", "OBJECT", &["login"]),
    (
        "Repository",
        "OBJECT",
        &[
            "id", "databaseId", "name", "nameWithOwner", "owner", "description", "isPrivate", "defaultBranchRef",
            "viewerPermission", "hasIssuesEnabled", "issue", "issues", "pullRequests", "pullRequest",
        ],
    ),
    ("Issue", "OBJECT", &["id", "databaseId", "number", "title", "body", "state", "closed"]),
    (
        "PullRequest",
        "OBJECT",
        &["id", "databaseId", "number", "title", "body", "state", "closed", "merged", "headRefName", "baseRefName", "mergeable", "files"],
    ),
    ("PullRequestChangedFile", "OBJECT", &["path", "additions", "deletions", "changeType"]),
    ("IssueComment", "OBJECT", &["id", "databaseId", "body"]),
    ("Ref", "OBJECT", &["name"]),
    ("PageInfo", "OBJECT", &["hasNextPage", "hasPreviousPage", "startCursor", "endCursor"]),
    ("IssueState", "ENUM", &["OPEN", "CLOSED"]),
    ("PullRequestState", "ENUM", &["OPEN", "CLOSED", "MERGED"]),
    ("MergeableState", "ENUM", &["MERGEABLE", "CONFLICTING", "UNKNOWN"]),
    ("PullRequestMergeMethod", "ENUM", &["MERGE", "SQUASH", "REBASE"]),
    ("RepositoryPermission", "ENUM", &["ADMIN", "MAINTAIN", "WRITE", "TRIAGE", "READ"]),
    ("SearchType", "ENUM", &["ISSUE", "ISSUE_ADVANCED", "REPOSITORY", "USER", "DISCUSSION"]),
    ("IssueOrderField", "ENUM", &["CREATED_AT", "UPDATED_AT", "COMMENTS"]),
    ("OrderDirection", "ENUM", &["ASC", "DESC"]),
];

fn type_obj(cx: &mut Ctx, sel: &[Sel], name: &str) -> Value {
    let Some(&(name, kind, members)) = TYPES.iter().find(|t| t.0 == name) else { return Value::Null };
    let list = |cx: &mut Ctx, g: &Field, ty: &str| {
        let mut v = Vec::new();
        for m in members {
            v.push(object(cx, &g.sel, ty, &mut |_, h| Some(match h.name.as_str() {
                "name" => (*m).into(),
                "description" | "deprecationReason" => Value::Null,
                "isDeprecated" => false.into(),
                _ => return None,
            })));
        }
        Value::Arr(v)
    };
    object(cx, sel, "__Type", &mut |cx, g| {
        Some(match g.name.as_str() {
            "name" => name.into(),
            "kind" => kind.into(),
            "description" => Value::Null,
            "fields" => if kind == "OBJECT" { list(cx, g, "__Field") } else { Value::Null },
            "enumValues" => if kind == "ENUM" { list(cx, g, "__EnumValue") } else { Value::Null },
            _ => return None,
        })
    })
}
