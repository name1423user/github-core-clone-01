//! Git smart HTTP（upload-pack v2 / receive-pack v0）、pkt-line、pack の読み書き。持ち場: git-engineer。
use crate::http::{Request, Response};
use crate::store::{lock, store, ObjType, Oid, Repo, RepoRef};
use crate::util::sha1::{from_hex, hex};

mod pack;
mod receive_pack;
mod upload_pack;

pub const ZERO: Oid = [0; 20];

/// /{owner}/{repo}.git/... に来たリクエスト。`parts` はデコード済みのパス要素。
pub fn handle(req: &Request, parts: &[&str]) -> Response {
    let (owner, name, rest) = (parts[0], parts[1], &parts[2..]);
    let name = name.strip_suffix(".git").unwrap_or(name);
    let Some(repo) = store().repo(owner, name) else { return Response::not_found() };
    let gz;
    let body: &[u8] = if req.header("content-encoding").is_some_and(|e| e.eq_ignore_ascii_case("gzip")) {
        match crate::util::zlib::gunzip(&req.body) {
            Ok(b) => {
                gz = b;
                &gz
            }
            Err(e) => return Response::new(400, "text/plain", e.into_bytes()),
        }
    } else {
        &req.body
    };
    match (req.method.as_str(), rest) {
        ("GET", ["info", "refs"]) => match req.query_param("service").as_deref() {
            Some("git-upload-pack") => {
                let v2 = req.header("git-protocol").is_some_and(|v| v.split(':').any(|p| p.trim() == "version=2"));
                let body = if v2 { advertise_v2() } else { advertise_v0(&lock(&repo), "git-upload-pack") };
                Response::new(200, "application/x-git-upload-pack-advertisement", body).with_header("cache-control", "no-cache")
            }
            Some("git-receive-pack") => {
                let body = advertise_v0(&lock(&repo), "git-receive-pack");
                Response::new(200, "application/x-git-receive-pack-advertisement", body).with_header("cache-control", "no-cache")
            }
            _ => Response::new(403, "text/plain", b"dumb HTTP is not supported\n".to_vec()),
        },
        ("POST", ["git-upload-pack"]) => match upload_pack::handle(&repo, body) {
            Ok(b) => Response::new(200, "application/x-git-upload-pack-result", Vec::new()).with_shared(b),
            Err(e) => Response::new(400, "text/plain", e.into_bytes()),
        },
        ("POST", ["git-receive-pack"]) => match receive_pack::handle(&repo, body) {
            Ok(b) => Response::new(200, "application/x-git-receive-pack-result", b),
            Err(e) => Response::new(400, "text/plain", e.into_bytes()),
        },
        _ => Response::not_found(),
    }
}

// ---------------------------------------------------------------- pkt-line

#[derive(Debug, PartialEq)]
pub enum Pkt<'a> {
    Data(&'a [u8]),
    Flush,
    Delim,
    End,
}

/// Next packet at `*pos`; `Ok(None)` at the end of the buffer.
pub fn read_pkt<'a>(buf: &'a [u8], pos: &mut usize) -> Result<Option<Pkt<'a>>, String> {
    if *pos >= buf.len() {
        return Ok(None);
    }
    let h = buf.get(*pos..*pos + 4).ok_or("truncated pkt-line")?;
    let s = std::str::from_utf8(h).map_err(|_| "bad pkt-line length")?;
    let n = usize::from_str_radix(s, 16).map_err(|_| "bad pkt-line length")?;
    *pos += 4;
    Ok(Some(match n {
        0 => Pkt::Flush,
        1 => Pkt::Delim,
        2 => Pkt::End,
        3 => return Err("bad pkt-line length".into()),
        _ => {
            let d = buf.get(*pos..*pos - 4 + n).ok_or("truncated pkt-line")?;
            *pos += n - 4;
            Pkt::Data(d)
        }
    }))
}

pub fn pkt(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(format!("{:04x}", data.len() + 4).as_bytes());
    out.extend_from_slice(data);
}

pub fn pkt_str(out: &mut Vec<u8>, s: &str) {
    pkt(out, s.as_bytes())
}

/// Wrap `data` in side-band channel `band`, pkt-lines of at most 65520 bytes.
pub fn sideband(out: &mut Vec<u8>, band: u8, data: &[u8]) {
    for c in data.chunks(65515) {
        out.extend_from_slice(format!("{:04x}", c.len() + 5).as_bytes());
        out.push(band);
        out.extend_from_slice(c);
    }
}

pub fn trim_lf(d: &[u8]) -> &[u8] {
    d.strip_suffix(b"\n").unwrap_or(d)
}

pub fn parse_oid(d: &[u8]) -> Option<Oid> {
    from_hex(std::str::from_utf8(d).ok()?)
}

// ---------------------------------------------------------------- refs

fn head_target(repo: &Repo) -> String {
    format!("refs/heads/{}", repo.default_branch)
}

/// Follow tag objects to the first non-tag object.
pub fn peel(repo: &Repo, mut oid: Oid) -> Oid {
    for _ in 0..16 {
        match repo.object(&oid) {
            Some(o) if o.kind == ObjType::Tag => match o.data.strip_prefix(b"object ").and_then(|r| r.get(..40)).and_then(parse_oid) {
                Some(t) => oid = t,
                None => break,
            },
            _ => break,
        }
    }
    oid
}

fn advertise_v2() -> Vec<u8> {
    let mut out = Vec::with_capacity(96);
    pkt_str(&mut out, "version 2\n");
    pkt_str(&mut out, "ls-refs=unborn\n");
    pkt_str(&mut out, "fetch=shallow\n");
    out.extend_from_slice(b"0000");
    out
}

fn advertise_v0(repo: &Repo, service: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    pkt_str(&mut out, &format!("# service={service}\n"));
    out.extend_from_slice(b"0000");
    let target = head_target(repo);
    let caps = if service == "git-receive-pack" {
        "report-status delete-refs side-band-64k ofs-delta".to_string()
    } else {
        format!("multi_ack_detailed side-band-64k thin-pack ofs-delta shallow no-progress symref=HEAD:{target}")
    };
    let mut lines: Vec<(Oid, String)> = Vec::new();
    if service == "git-upload-pack" {
        if let Some(t) = repo.refs.get(&target) {
            lines.push((*t, "HEAD".into()));
        }
    }
    for (name, oid) in &repo.refs {
        lines.push((*oid, name.clone()));
        if service == "git-upload-pack" {
            let p = peel(repo, *oid);
            if p != *oid {
                lines.push((p, format!("{name}^{{}}")));
            }
        }
    }
    if lines.is_empty() {
        lines.push((ZERO, "capabilities^{}".into()));
    }
    for (i, (oid, name)) in lines.iter().enumerate() {
        let l = if i == 0 { format!("{} {name}\0{caps}\n", hex(oid)) } else { format!("{} {name}\n", hex(oid)) };
        pkt_str(&mut out, &l);
    }
    out.extend_from_slice(b"0000");
    out
}

/// v2 ls-refs. `args` are the argument lines after the delimiter.
pub fn ls_refs(repo: &RepoRef, args: &[&[u8]]) -> Vec<u8> {
    let (mut peel_on, mut symrefs, mut unborn) = (false, false, false);
    let mut prefixes: Vec<&[u8]> = Vec::new();
    for a in args {
        match *a {
            b"peel" => peel_on = true,
            b"symrefs" => symrefs = true,
            b"unborn" => unborn = true,
            _ => {
                if let Some(p) = a.strip_prefix(b"ref-prefix ") {
                    prefixes.push(p);
                }
            }
        }
    }
    let keep = |name: &str| prefixes.is_empty() || prefixes.iter().any(|p| name.as_bytes().starts_with(p));
    let repo = lock(repo);
    let mut out = Vec::with_capacity(64 * (repo.refs.len() + 2));
    let target = head_target(&repo);
    if keep("HEAD") {
        let sym = if symrefs { format!(" symref-target:{target}") } else { String::new() };
        match repo.refs.get(&target) {
            Some(t) => pkt_str(&mut out, &format!("{} HEAD{sym}\n", hex(t))),
            None if unborn => pkt_str(&mut out, &format!("unborn HEAD{sym}\n")),
            None => {}
        }
    }
    for (name, oid) in &repo.refs {
        if !keep(name) {
            continue;
        }
        let mut l = format!("{} {name}", hex(oid));
        if peel_on {
            let p = peel(&repo, *oid);
            if p != *oid {
                l.push_str(" peeled:");
                l.push_str(&hex(&p));
            }
        }
        l.push('\n');
        pkt_str(&mut out, &l);
    }
    out.extend_from_slice(b"0000");
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::hash_object;

    pub fn pkts(buf: &[u8]) -> Vec<String> {
        let mut pos = 0;
        let mut v = Vec::new();
        while let Some(p) = read_pkt(buf, &mut pos).unwrap() {
            v.push(match p {
                Pkt::Data(d) => String::from_utf8_lossy(d).into_owned(),
                Pkt::Flush => "0000".into(),
                Pkt::Delim => "0001".into(),
                Pkt::End => "0002".into(),
            });
        }
        v
    }

    #[test]
    fn pktline() {
        let mut o = Vec::new();
        pkt_str(&mut o, "a\n");
        o.extend_from_slice(b"00010000");
        assert_eq!(pkts(&o), vec!["a\n", "0001", "0000"]);
        let mut pos = 0;
        assert!(read_pkt(b"zzzznot-a-pkt-line", &mut pos).is_err());
        let mut pos = 0;
        assert!(read_pkt(b"00ffab", &mut pos).is_err());
    }

    #[test]
    fn ls_refs_prefix_and_peel() {
        let r = store().create_repo("t-git-ls", "r", "", false).unwrap();
        {
            let mut g = lock(&r);
            let (c, _) = g.commit_file("main", "a.txt", Some(b"a".to_vec()), "m", None).unwrap();
            let tag = format!("object {}\ntype commit\ntag v1\ntagger x <x> 0 +0000\n\nv1\n", hex(&c));
            let t = g.put_object(ObjType::Tag, tag.into_bytes());
            g.refs.insert("refs/tags/v1".into(), t);
        }
        let all = pkts(&ls_refs(&r, &[b"peel", b"symrefs"]));
        assert!(all[0].ends_with(" HEAD symref-target:refs/heads/main\n"));
        assert!(all[2].contains("refs/tags/v1 peeled:"));
        let heads = pkts(&ls_refs(&r, &[b"ref-prefix refs/heads/"]));
        assert_eq!(heads.len(), 2);
        assert!(heads[0].ends_with(" refs/heads/main\n"));
        assert_eq!(pkts(&ls_refs(&r, &[b"ref-prefix refs/does-not-exist/"])), vec!["0000"]);
        let e = store().create_repo("t-git-ls", "empty", "", false).unwrap();
        assert_eq!(pkts(&ls_refs(&e, &[b"symrefs", b"unborn"])), vec!["unborn HEAD symref-target:refs/heads/main\n", "0000"]);
        let adv = pkts(&advertise_v0(&lock(&e), "git-receive-pack"));
        assert_eq!(adv[0], "# service=git-receive-pack\n");
        assert!(adv[2].starts_with("0000000000000000000000000000000000000000 capabilities^{}\0report-status"));
        let _ = hash_object;
    }
}
