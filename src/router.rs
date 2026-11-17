//! Path split + match (no regex list). Auth: /api/* and /mcp need "Authorization: Bearer $MAKEFUN_SHOP_TOKEN";
//! /healthz and git smart HTTP take none (the suite's git client sends no credential).
//! 持ち場: リード。

use crate::http::{Request, Response};
use crate::{git, graphql, mcp, rest};
use std::sync::OnceLock;

fn token() -> &'static str {
    static T: OnceLock<String> = OnceLock::new();
    T.get_or_init(|| std::env::var("MAKEFUN_SHOP_TOKEN").unwrap_or_default())
}

fn authorized(req: &Request) -> bool {
    let Some(h) = req.header("authorization") else { return false };
    let Some((scheme, value)) = h.split_once(' ') else { return false };
    let scheme = scheme.to_ascii_lowercase();
    (scheme == "bearer" || scheme == "token") && (token().is_empty() || value.trim() == token())
}

fn unauthorized() -> Response {
    Response::json(401, r#"{"message":"Requires authentication","documentation_url":"https://docs.github.com/rest"}"#)
}

pub fn route(req: &Request) -> Response {
    // shortcut: segments stay percent-encoded here; each interface decodes what it needs.
    let parts: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        ["healthz"] => Response::json(200, r#"{"ok":true}"#),
        ["api", sub @ ..] => {
            if !authorized(req) {
                return unauthorized();
            }
            match sub {
                ["v3", tail @ ..] => rest::handle(req, tail),
                ["graphql"] => graphql::handle(req),
                _ => Response::not_found(),
            }
        }
        ["mcp"] => {
            if !authorized(req) {
                return unauthorized();
            }
            mcp::handle(req)
        }
        [_, _, _, ..] => git::handle(req, &parts),
        _ => Response::not_found(),
    }
}

