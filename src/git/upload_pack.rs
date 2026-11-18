//! upload-pack protocol v2: ls-refs and fetch (wants/haves/shallow/deepen). Packs are cached per request.

use super::{ls_refs, pack, parse_oid, pkt, pkt_str, read_pkt, sideband, trim_lf, Pkt};
use crate::store::{lock, parse_commit, parse_tree, ObjType, Oid, Repo, RepoRef};
use crate::util::sha1::hex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

pub fn handle(repo: &RepoRef, body: &[u8]) -> Result<Vec<u8>, String> {
    let mut pos = 0;
    let mut command: Option<&[u8]> = None;
    // capability lines up to the delimiter, then argument lines up to flush
    loop {
        match read_pkt(body, &mut pos)? {
            Some(Pkt::Data(d)) => {
                if let Some(c) = trim_lf(d).strip_prefix(b"command=") {
                    command = Some(c);
                }
            }
            Some(Pkt::Delim) => break,
            Some(Pkt::Flush) | None => break,
            Some(Pkt::End) => return Err("unexpected response-end".into()),
        }
    }
    let args_start = pos;
    let mut args: Vec<&[u8]> = Vec::new();
    while let Some(p) = read_pkt(body, &mut pos)? {
        match p {
            Pkt::Data(d) => args.push(trim_lf(d)),
            _ => break,
        }
    }
    match command {
        Some(b"ls-refs") => Ok(ls_refs(repo, &args)),
        Some(b"fetch") => fetch(repo, &args, &body[args_start..pos]),
        _ => Err("unknown or missing command".into()),
    }
}

struct FetchArgs {
    wants: Vec<Oid>,
    haves: Vec<Oid>,
    shallow: Vec<Oid>,
    depth: Option<usize>,
    done: bool,
}

type Cache = Mutex<HashMap<(u64, Vec<u8>), Arc<OnceLock<Arc<Vec<u8>>>>>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fetch(repo: &RepoRef, args: &[&[u8]], raw_args: &[u8]) -> Result<Vec<u8>, String> {
    let mut a = FetchArgs { wants: Vec::new(), haves: Vec::new(), shallow: Vec::new(), depth: None, done: false };
    for l in args {
        let (k, v) = match l.iter().position(|&b| b == b' ') {
            Some(i) => (&l[..i], &l[i + 1..]),
            None => (*l, &b""[..]),
        };
        match k {
            b"want" => a.wants.push(parse_oid(v).ok_or("bad want")?),
            b"have" => a.haves.push(parse_oid(v).ok_or("bad have")?),
            b"shallow" => a.shallow.push(parse_oid(v).ok_or("bad shallow")?),
            b"deepen" => a.depth = Some(std::str::from_utf8(v).ok().and_then(|s| s.parse().ok()).ok_or("bad deepen")?),
            b"done" => a.done = true,
            _ => {} // thin-pack, no-progress, ofs-delta, include-tag, ...: accepted, no effect
        }
    }
    // Objects are immutable and content-addressed, so a request whose wants and haves all exist
    // always yields the same answer: cache by (repo id, argument bytes). Refs don't enter into it.
    let (id, cacheable) = {
        let g = lock(repo);
        (g.id, a.wants.iter().chain(&a.haves).all(|o| g.objects.contains_key(o)))
    };
    if !cacheable {
        return Ok(build(repo, &a));
    }
    let cell = {
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        if c.len() > 256 {
            c.clear();
        }
        c.entry((id, raw_args.to_vec())).or_default().clone()
    };
    // Concurrent identical requests wait here for the first one's result.
    Ok(cell.get_or_init(|| Arc::new(build(repo, &a))).as_ref().clone())
}

fn build(repo: &RepoRef, a: &FetchArgs) -> Vec<u8> {
    let (objs, shallow, unshallow, common) = {
        let g = lock(repo);
        let r = collect(&g, a);
        let common: Vec<Oid> = a.haves.iter().filter(|h| g.objects.contains_key(*h)).copied().collect();
        (r.0, r.1, r.2, common)
    };
    let packed = pack::write_pack(&objs);
    let mut out = Vec::with_capacity(packed.len() + packed.len() / 65515 * 5 + 256);
    if !a.done {
        pkt_str(&mut out, "acknowledgments\n");
        if common.is_empty() {
            pkt_str(&mut out, "NAK\n");
        }
        for c in &common {
            pkt_str(&mut out, &format!("ACK {}\n", hex(c)));
        }
        pkt_str(&mut out, "ready\n");
        out.extend_from_slice(b"0001");
    }
    if !shallow.is_empty() || !unshallow.is_empty() {
        pkt_str(&mut out, "shallow-info\n");
        for s in &shallow {
            pkt_str(&mut out, &format!("shallow {}\n", hex(s)));
        }
        for s in &unshallow {
            pkt_str(&mut out, &format!("unshallow {}\n", hex(s)));
        }
        out.extend_from_slice(b"0001");
    }
    pkt(&mut out, b"packfile\n");
    sideband(&mut out, 1, &packed);
    out.extend_from_slice(b"0000");
    out
}

/// (objects to send in pack order, new shallow boundary, unshallowed client commits).
fn collect(repo: &Repo, a: &FetchArgs) -> (Vec<(ObjType, Arc<[u8]>)>, Vec<Oid>, Vec<Oid>) {
    let client_shallow: HashSet<Oid> = a.shallow.iter().copied().collect();
    // What the client already has: commits from its haves, not past its shallow boundary.
    let mut have_commits: HashSet<Oid> = HashSet::new();
    let mut stack: Vec<Oid> = a.haves.iter().filter(|h| repo.objects.contains_key(*h)).copied().collect();
    while let Some(c) = stack.pop() {
        if !have_commits.insert(c) {
            continue;
        }
        if client_shallow.contains(&c) {
            continue;
        }
        if let Some(info) = repo.commit(&c) {
            stack.extend(info.parents);
        }
    }
    let mut have_objs: HashSet<Oid> = HashSet::new();
    for c in &have_commits {
        if let Some(info) = repo.commit(c) {
            mark_tree(repo, info.tree, &mut have_objs);
        }
    }

    let mut out: Vec<(ObjType, Arc<[u8]>)> = Vec::new();
    let mut sent: HashSet<Oid> = HashSet::new();
    let mut commits: Vec<Oid> = Vec::new();
    let mut roots: Vec<Oid> = Vec::new(); // trees/blobs wanted directly
    for w in &a.wants {
        let mut oid = *w;
        for _ in 0..16 {
            let Some(o) = repo.object(&oid) else { break };
            match o.kind {
                ObjType::Tag => {
                    if sent.insert(oid) {
                        out.push((o.kind, o.data.clone()));
                    }
                    match o.data.strip_prefix(b"object ").and_then(|r| r.get(..40)).and_then(parse_oid) {
                        Some(t) => oid = t,
                        None => break,
                    }
                }
                ObjType::Commit => {
                    commits.push(oid);
                    break;
                }
                _ => {
                    roots.push(oid);
                    break;
                }
            }
        }
    }

    // Commit walk, breadth first from the wants.
    let mut shallow = Vec::new();
    let mut unshallow = Vec::new();
    let mut send_commits: Vec<Oid> = Vec::new();
    let mut seen: HashSet<Oid> = HashSet::new();
    let mut q: VecDeque<(Oid, usize)> = commits.iter().map(|c| (*c, 1)).collect();
    while let Some((c, level)) = q.pop_front() {
        if !seen.insert(c) {
            continue;
        }
        let Some(o) = repo.object(&c) else { continue };
        let Some(info) = parse_commit(&o.data) else { continue };
        let have = have_commits.contains(&c);
        if !have {
            send_commits.push(c);
        }
        match a.depth {
            Some(d) => {
                if level >= d {
                    if !info.parents.is_empty() && !client_shallow.contains(&c) {
                        shallow.push(c);
                    }
                    continue;
                }
                if client_shallow.contains(&c) && !info.parents.is_empty() {
                    unshallow.push(c);
                }
            }
            None => {
                if have {
                    continue;
                }
            }
        }
        for p in info.parents {
            q.push_back((p, level + 1));
        }
    }
    for c in &send_commits {
        let o = repo.object(c).unwrap();
        sent.insert(*c);
        out.push((o.kind, o.data.clone()));
    }
    for c in &send_commits {
        if let Some(info) = repo.commit(c) {
            emit_tree(repo, info.tree, &have_objs, &mut sent, &mut out);
        }
    }
    for r in roots {
        emit_tree(repo, r, &have_objs, &mut sent, &mut out);
    }
    (out, shallow, unshallow)
}

fn mark_tree(repo: &Repo, oid: Oid, set: &mut HashSet<Oid>) {
    if !set.insert(oid) {
        return;
    }
    let Some(o) = repo.object(&oid) else { return };
    if o.kind != ObjType::Tree {
        return;
    }
    for (mode, _, e) in parse_tree(&o.data) {
        if mode == "160000" {
            continue;
        }
        if mode.starts_with('4') {
            mark_tree(repo, e, set);
        } else {
            set.insert(e);
        }
    }
}

/// Tree (or blob) and everything under it the client lacks, depth first.
fn emit_tree(repo: &Repo, oid: Oid, have: &HashSet<Oid>, sent: &mut HashSet<Oid>, out: &mut Vec<(ObjType, Arc<[u8]>)>) {
    if have.contains(&oid) || !sent.insert(oid) {
        return;
    }
    let Some(o) = repo.object(&oid) else { return };
    out.push((o.kind, o.data.clone()));
    if o.kind != ObjType::Tree {
        return;
    }
    for (mode, _, e) in parse_tree(&o.data) {
        if mode != "160000" {
            emit_tree(repo, e, have, sent, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::pkts;
    use crate::store::store;

    fn fetch_req(lines: &[String]) -> Vec<u8> {
        let mut b = Vec::new();
        pkt_str(&mut b, "command=fetch\n");
        b.extend_from_slice(b"0001");
        for l in lines {
            pkt_str(&mut b, &format!("{l}\n"));
        }
        b.extend_from_slice(b"0000");
        b
    }

    /// Parse a fetch response: (shallow-info lines, objects).
    fn parse(resp: &[u8]) -> (Vec<String>, Vec<pack::PackObject>) {
        let mut pos = 0;
        let mut section = String::new();
        let mut info = Vec::new();
        let mut packbytes = Vec::new();
        while let Some(p) = read_pkt(resp, &mut pos).unwrap() {
            match p {
                Pkt::Data(d) if section.is_empty() => section = String::from_utf8_lossy(d).into_owned(),
                Pkt::Data(d) if section == "packfile\n" => {
                    assert!(d.len() + 4 <= 65520 && d[0] == 1);
                    packbytes.extend_from_slice(&d[1..]);
                }
                Pkt::Data(d) => info.push(String::from_utf8_lossy(d).into_owned()),
                _ => section.clear(),
            }
        }
        (info, pack::read_pack(&packbytes, |_| None).unwrap())
    }

    #[test]
    fn full_incremental_and_shallow() {
        let r = store().create_repo("t-git-up", "r", "", false).unwrap();
        let (c1, c2, c3) = {
            let mut g = lock(&r);
            let (c1, _) = g.commit_file("main", "a.txt", Some(b"one".to_vec()), "1", None).unwrap();
            let (c2, _) = g.commit_file("main", "d/b.txt", Some(vec![9u8; 100_000]), "2", None).unwrap();
            let (c3, _) = g.commit_file("main", "c.txt", Some(b"three".to_vec()), "3", None).unwrap();
            (c1, c2, c3)
        };
        let full = parse(&handle(&r, &fetch_req(&[format!("want {}", hex(&c3)), "done".into()])).unwrap());
        // 3 commits, 3 root trees, 1 subtree, 3 blobs
        assert_eq!(full.1.len(), 10);
        let ids: HashSet<Oid> = full.1.iter().map(|o| o.oid).collect();
        assert_eq!(ids.len(), 10);
        assert!(ids.contains(&c1) && ids.contains(&c2));

        let inc = parse(&handle(&r, &fetch_req(&[format!("want {}", hex(&c3)), format!("have {}", hex(&c2)), "done".into()])).unwrap());
        assert_eq!(inc.1.len(), 3); // c3, its root tree, c.txt
        let none = parse(&handle(&r, &fetch_req(&[format!("want {}", hex(&c3)), format!("have {}", hex(&c3)), "done".into()])).unwrap());
        assert_eq!(none.1.len(), 0);

        let d1 = parse(&handle(&r, &fetch_req(&[format!("want {}", hex(&c3)), "deepen 1".into(), "done".into()])).unwrap());
        assert_eq!(d1.0, vec![format!("shallow {}\n", hex(&c3))]);
        let ids: HashSet<Oid> = d1.1.iter().map(|o| o.oid).collect();
        assert!(ids.contains(&c3) && !ids.contains(&c2) && !ids.contains(&c1));

        let deep = parse(
            &handle(
                &r,
                &fetch_req(&[format!("want {}", hex(&c3)), format!("have {}", hex(&c3)), format!("shallow {}", hex(&c3)), "deepen 2".into(), "done".into()]),
            )
            .unwrap(),
        );
        let ids: HashSet<Oid> = deep.1.iter().map(|o| o.oid).collect();
        assert!(ids.contains(&c2) && !ids.contains(&c1) && !ids.contains(&c3));
        assert!(deep.0.contains(&format!("shallow {}\n", hex(&c2))));
        let tree2 = lock(&r).commit(&c2).unwrap().tree;
        assert!(ids.contains(&tree2));
        let _ = pkts;
    }
}
