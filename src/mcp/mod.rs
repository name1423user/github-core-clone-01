//! MCP（POST /mcp、JSON-RPC 2.0 over Streamable HTTP）。持ち場: mcp-engineer。
use crate::http::{Request, Response};

pub fn handle(_req: &Request) -> Response {
    Response::not_found()
}
