//! REST v3（/api/v3/...）。持ち場: rest-engineer。
use crate::http::{Request, Response};

/// `parts` は /api/v3 より後のパス要素。
pub fn handle(_req: &Request, _parts: &[&str]) -> Response {
    Response::not_found()
}
