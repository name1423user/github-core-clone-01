//! GraphQL v4（POST /api/graphql）: 自前のパーサ・実行器・スキーマ。持ち場: graphql-engineer。
use crate::http::{Request, Response};

pub fn handle(_req: &Request) -> Response {
    Response::not_found()
}
