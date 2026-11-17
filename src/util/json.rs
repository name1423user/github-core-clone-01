//! JSON の値・パース・シリアライズ（std のみ）。持ち場: リード。
//! オブジェクトは挿入順の Vec で持つ: 出力のキー順が常に同じになる（決定性）。

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

const MAX_DEPTH: usize = 128;

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(n) => Some(n),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(f as i64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Value::Arr(a) = self { Some(a) } else { None }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    /// 文字列キーのフィールド: `obj.str_field("title")`。
    pub fn str_field(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
    pub fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(n) => { let _ = write!(out, "{n}"); }
            Value::Float(f) if f.is_finite() => { let _ = write!(out, "{f}"); }
            Value::Float(_) => out.push_str("null"),
            Value::Str(s) => write_str(out, s),
            Value::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Obj(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    write_str(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

impl From<&str> for Value { fn from(s: &str) -> Value { Value::Str(s.to_string()) } }
impl From<String> for Value { fn from(s: String) -> Value { Value::Str(s) } }
impl From<i64> for Value { fn from(n: i64) -> Value { Value::Int(n) } }
impl From<u64> for Value { fn from(n: u64) -> Value { Value::Int(n as i64) } }
impl From<bool> for Value { fn from(b: bool) -> Value { Value::Bool(b) } }
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(o: Option<T>) -> Value { o.map_or(Value::Null, Into::into) }
}

/// オブジェクトを組み立てる: `obj(vec![("id", 1.into()), ...])`。
pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// JSON 文字列リテラルとして書く（引用符つき）。手書きのシリアライズからも使う。
pub fn write_str(out: &mut String, s: &str) {
    out.push('"');
    let mut start = 0;
    for (i, b) in s.bytes().enumerate() {
        let esc: &str = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&s[start..i]);
        if esc.is_empty() { let _ = write!(out, "\\u{:04x}", b); } else { out.push_str(esc); }
        start = i + 1;
    }
    out.push_str(&s[start..]);
    out.push('"');
}

pub fn parse(src: &[u8]) -> Result<Value, String> {
    let mut p = Parser { s: src, i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() { return Err(p.err("trailing characters")); }
    Ok(v)
}

struct Parser<'a> { s: &'a [u8], i: usize }

impl Parser<'_> {
    fn err(&self, msg: &str) -> String { format!("{msg} at byte {}", self.i) }
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') { self.i += 1; }
    }
    fn peek(&self) -> Option<u8> { self.s.get(self.i).copied() }
    fn lit(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.s[self.i..].starts_with(word.as_bytes()) { self.i += word.len(); Ok(v) } else { Err(self.err("invalid literal")) }
    }
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH { return Err(self.err("nesting too deep")); }
        match self.peek() {
            Some(b'{') => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') { self.i += 1; return Ok(Value::Obj(m)); }
                loop {
                    self.ws();
                    if self.peek() != Some(b'"') { return Err(self.err("expected key")); }
                    let k = self.string()?;
                    self.ws();
                    if self.peek() != Some(b':') { return Err(self.err("expected ':'")); }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    // 重複キーは後勝ち（JSON.parse と同じ）
                    if let Some(slot) = m.iter_mut().find(|(ek, _): &&mut (String, Value)| *ek == k) { slot.1 = v; } else { m.push((k, v)); }
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => { self.i += 1; return Ok(Value::Obj(m)); }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.peek() == Some(b']') { self.i += 1; return Ok(Value::Arr(a)); }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => { self.i += 1; return Ok(Value::Arr(a)); }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'n') => self.lit("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err("unexpected character")),
        }
    }
    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        let mut float = false;
        while let Some(c) = self.peek() {
            match c {
                b'0'..=b'9' | b'-' | b'+' => {}
                b'.' | b'e' | b'E' => float = true,
                _ => break,
            }
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).unwrap();
        if !float { if let Ok(n) = t.parse::<i64>() { return Ok(Value::Int(n)); } }
        t.parse::<f64>().map(Value::Float).map_err(|_| self.err("invalid number"))
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.s.get(self.i..self.i + 4).ok_or_else(|| self.err("short \\u escape"))?;
        let n = std::str::from_utf8(h).ok().and_then(|h| u32::from_str_radix(h, 16).ok()).ok_or_else(|| self.err("bad \\u escape"))?;
        self.i += 4;
        Ok(n)
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut out = Vec::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.err("bad escape"))?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"', b'\\' => '\\', b'/' => '/', b'b' => '\u{8}', b'f' => '\u{c}',
                        b'n' => '\n', b'r' => '\r', b't' => '\t',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = if (0xDC00..0xE000).contains(&lo) { 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00) } else { 0xFFFD };
                            }
                            char::from_u32(cp).unwrap_or('\u{FFFD}')
                        }
                        _ => return Err(self.err("bad escape")),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| self.err("invalid utf-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = r#"{"a":1,"b":[true,false,null,-2.5,"x\"\n\u00e9\ud83d\ude00"],"c":{},"a":3}"#;
        let v = parse(src.as_bytes()).unwrap();
        assert_eq!(v.get("a").unwrap().as_i64(), Some(3));
        assert_eq!(v.to_json(), "{\"a\":3,\"b\":[true,false,null,-2.5,\"x\\\"\\né😀\"],\"c\":{}}");
        assert_eq!(parse(v.to_json().as_bytes()).unwrap(), v);
        for bad in ["", "{", "[1,]", "{\"a\"}", "tru", "1 2", "\"abc"] {
            assert!(parse(bad.as_bytes()).is_err(), "{bad}");
        }
        let mut s = String::new();
        write_str(&mut s, "a\u{1}b");
        assert_eq!(s, "\"a\\u0001b\"");
    }
}
