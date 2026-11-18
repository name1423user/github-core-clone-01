//! 共有の下回り（std のみで自作する）。
//! 予定: json.rs（パース・シリアライズ、キーソート出力）＝リード、
//!       sha1.rs・zlib.rs（inflate/deflate）＝git-engineer。
pub mod alloc;
pub mod json;
pub mod sha1;
pub mod zlib;
