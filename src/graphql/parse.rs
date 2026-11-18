//! GraphQL lexer + parser: just the syntax the suite's 18 documents use (checks-graphql.ts:107-317).
//! shortcut: no fragment definitions/spreads, directives, or block strings; add them if a document needs them.

use crate::util::json::Value;

pub enum Arg {
    Var(String),
    Lit(Value),
    List(Vec<Arg>),
    Obj(Vec<(String, Arg)>),
}

impl Arg {
    /// Literal with variables substituted; enum values read as strings.
    pub fn eval(&self, vars: &Value) -> Value {
        match self {
            Arg::Var(n) => vars.get(n).cloned().unwrap_or(Value::Null),
            Arg::Lit(v) => v.clone(),
            Arg::List(l) => Value::Arr(l.iter().map(|a| a.eval(vars)).collect()),
            Arg::Obj(o) => Value::Obj(o.iter().map(|(k, a)| (k.clone(), a.eval(vars))).collect()),
        }
    }
}

pub struct Field {
    pub alias: Option<String>,
    pub name: String,
    pub args: Vec<(String, Arg)>,
    pub sel: Vec<Sel>,
    pub line: usize,
    pub col: usize,
}

impl Field {
    pub fn key(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

pub enum Sel {
    Field(Field),
    /// `... on Type { }` (type condition optional).
    Inline(Option<String>, Vec<Sel>),
}

pub struct Op {
    pub name: Option<String>,
    pub mutation: bool,
    pub vars: Vec<(String, Option<Arg>)>,
    pub sel: Vec<Sel>,
}

#[derive(Debug, PartialEq)]
enum Tok {
    Name(String),
    Int(i64),
    Float(f64),
    Str(String),
    Punct(u8),
    Spread,
}

struct P {
    toks: Vec<(Tok, usize, usize)>,
    i: usize,
}

fn lex(src: &str) -> Result<Vec<(Tok, usize, usize)>, String> {
    let b = src.as_bytes();
    let (mut i, mut line, mut ls) = (0, 1, 0);
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        let col = i - ls + 1;
        match c {
            b'\n' => {
                line += 1;
                i += 1;
                ls = i;
            }
            b' ' | b'\t' | b'\r' | b',' => i += 1,
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'.' if b[i..].starts_with(b"...") => {
                out.push((Tok::Spread, line, col));
                i += 3;
            }
            b'{' | b'}' | b'(' | b')' | b'[' | b']' | b':' | b'$' | b'!' | b'=' | b'@' => {
                out.push((Tok::Punct(c), line, col));
                i += 1;
            }
            b'"' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match b.get(i) {
                        None | Some(b'\n') => return Err(format!("unterminated string at {line}:{col}")),
                        Some(b'"') => break,
                        Some(b'\\') => {
                            let e = *b.get(i + 1).ok_or("bad escape")?;
                            i += 2;
                            match e {
                                b'n' => s.push('\n'),
                                b't' => s.push('\t'),
                                b'r' => s.push('\r'),
                                b'b' => s.push('\u{8}'),
                                b'f' => s.push('\u{c}'),
                                b'u' => {
                                    let h = src.get(i..i + 4).and_then(|h| u32::from_str_radix(h, 16).ok()).ok_or("bad \\u escape")?;
                                    s.push(char::from_u32(h).unwrap_or('\u{fffd}'));
                                    i += 4;
                                }
                                _ => s.push(e as char),
                            }
                        }
                        Some(_) => {
                            let start = i;
                            while i < b.len() && !matches!(b[i], b'"' | b'\\' | b'\n') {
                                i += 1;
                            }
                            s.push_str(&src[start..i]);
                        }
                    }
                }
                i += 1;
                out.push((Tok::Str(s), line, col));
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < b.len() && matches!(b[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                    i += 1;
                }
                let t = &src[start..i];
                let tok = match t.parse::<i64>() {
                    Ok(n) => Tok::Int(n),
                    Err(_) => Tok::Float(t.parse().map_err(|_| format!("bad number {t} at {line}:{col}"))?),
                };
                out.push((tok, line, col));
            }
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let start = i;
                while i < b.len() && (b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                out.push((Tok::Name(src[start..i].to_string()), line, col));
            }
            _ => return Err(format!("Syntax Error: unexpected character {:?} at {line}:{col}", c as char)),
        }
    }
    Ok(out)
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.0)
    }
    fn err<T>(&self, what: &str) -> Result<T, String> {
        match self.toks.get(self.i) {
            Some((t, l, c)) => Err(format!("Syntax Error: expected {what}, got {t:?} at {l}:{c}")),
            None => Err(format!("Syntax Error: expected {what}, got end of document")),
        }
    }
    fn eat(&mut self, p: u8) -> bool {
        if self.peek() == Some(&Tok::Punct(p)) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: u8) -> Result<(), String> {
        if self.eat(p) { Ok(()) } else { self.err(&format!("'{}'", p as char)) }
    }
    fn name(&mut self) -> Result<String, String> {
        if let Some(Tok::Name(n)) = self.peek() {
            let n = n.clone();
            self.i += 1;
            Ok(n)
        } else {
            self.err("a name")
        }
    }

    fn value(&mut self, consts: bool) -> Result<Arg, String> {
        let t = match self.toks.get(self.i) {
            Some((t, _, _)) => t,
            None => return self.err("a value"),
        };
        let v = match t {
            Tok::Punct(b'$') if !consts => {
                self.i += 1;
                return Ok(Arg::Var(self.name()?));
            }
            Tok::Punct(b'[') => {
                self.i += 1;
                let mut l = Vec::new();
                while !self.eat(b']') {
                    l.push(self.value(consts)?);
                }
                return Ok(Arg::List(l));
            }
            Tok::Punct(b'{') => {
                self.i += 1;
                let mut o = Vec::new();
                while !self.eat(b'}') {
                    let k = self.name()?;
                    self.expect(b':')?;
                    o.push((k, self.value(consts)?));
                }
                return Ok(Arg::Obj(o));
            }
            Tok::Int(n) => Value::Int(*n),
            Tok::Float(f) => Value::Float(*f),
            Tok::Str(s) => Value::Str(s.clone()),
            Tok::Name(n) => match n.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                _ => Value::Str(n.clone()),
            },
            _ => return self.err("a value"),
        };
        self.i += 1;
        Ok(Arg::Lit(v))
    }

    fn selection_set(&mut self) -> Result<Vec<Sel>, String> {
        self.expect(b'{')?;
        let mut out = Vec::new();
        while !self.eat(b'}') {
            if self.peek() == Some(&Tok::Spread) {
                self.i += 1;
                let cond = if self.peek() == Some(&Tok::Name("on".into())) {
                    self.i += 1;
                    Some(self.name()?)
                } else {
                    None
                };
                if !matches!(self.peek(), Some(Tok::Punct(b'{'))) {
                    return self.err("an inline fragment (named fragments are not supported)");
                }
                out.push(Sel::Inline(cond, self.selection_set()?));
                continue;
            }
            let (line, col) = self.toks.get(self.i).map(|t| (t.1, t.2)).unwrap_or((0, 0));
            let mut name = self.name()?;
            let mut alias = None;
            if self.eat(b':') {
                alias = Some(name);
                name = self.name()?;
            }
            let mut args = Vec::new();
            if self.eat(b'(') {
                while !self.eat(b')') {
                    let k = self.name()?;
                    self.expect(b':')?;
                    args.push((k, self.value(false)?));
                }
            }
            let sel = if matches!(self.peek(), Some(Tok::Punct(b'{'))) { self.selection_set()? } else { Vec::new() };
            out.push(Sel::Field(Field { alias, name, args, sel, line, col }));
        }
        Ok(out)
    }

    fn type_ref(&mut self) -> Result<(), String> {
        if self.eat(b'[') {
            self.type_ref()?;
            self.expect(b']')?;
        } else {
            self.name()?;
        }
        self.eat(b'!');
        Ok(())
    }

    fn operation(&mut self) -> Result<Op, String> {
        let mut op = Op { name: None, mutation: false, vars: Vec::new(), sel: Vec::new() };
        if let Some(Tok::Name(k)) = self.peek() {
            match k.as_str() {
                "query" => {}
                "mutation" => op.mutation = true,
                _ => return self.err("'query', 'mutation' or '{' (subscriptions and fragments are not supported)"),
            }
            self.i += 1;
            if let Some(Tok::Name(_)) = self.peek() {
                op.name = Some(self.name()?);
            }
            if self.eat(b'(') {
                while !self.eat(b')') {
                    self.expect(b'$')?;
                    let n = self.name()?;
                    self.expect(b':')?;
                    self.type_ref()?;
                    let def = if self.eat(b'=') { Some(self.value(true)?) } else { None };
                    op.vars.push((n, def));
                }
            }
        }
        op.sel = self.selection_set()?;
        Ok(op)
    }
}

pub fn parse(src: &str) -> Result<Vec<Op>, String> {
    let mut p = P { toks: lex(src)?, i: 0 };
    let mut ops = Vec::new();
    while p.i < p.toks.len() {
        ops.push(p.operation()?);
    }
    if ops.is_empty() {
        return Err("Syntax Error: the document holds no operation".into());
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_suite_shapes() {
        let ops = parse(
            "query Q($owner: String!, $first: Int = 3, $after: String) { repository(owner: $owner, name: \"x\") {
               issues(first: $first, states: [OPEN], orderBy: {field: CREATED_AT, direction: DESC}) { nodes { number } }
             } I: __type(name: \"Issue\") { fields(includeDeprecated: true) { name } }
             node(id: $owner) { id ... on Repository { name } } }",
        )
        .unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].name.as_deref(), Some("Q"));
        assert_eq!(ops[0].vars.len(), 3);
        assert_eq!(ops[0].sel.len(), 3);
        let Sel::Field(f) = &ops[0].sel[1] else { panic!() };
        assert_eq!((f.key(), f.name.as_str()), ("I", "__type"));
        assert!(parse("{ a(").is_err());
        assert!(parse("mutation M { x }").unwrap()[0].mutation);
    }
}
