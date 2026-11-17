//! Git smart HTTP（upload-pack v2 / receive-pack v0）、pkt-line、pack の読み書き、マージ。持ち場: git-engineer。
use crate::http::{Request, Response};

/// /{owner}/{repo}.git/... に来たリクエスト。`parts` はデコード済みのパス要素。
pub fn handle(_req: &Request, _parts: &[&str]) -> Response {
    Response::not_found()
}
