//! receive-pack protocol v0: commands, pack, fast-forward-only ref updates, report-status (optionally in side-band-64k).

use super::{pack, parse_oid, pkt_str, read_pkt, sideband, trim_lf, Pkt, ZERO};
use crate::store::{lock, Object, ObjType, Oid, Repo, RepoRef};

struct Command {
    old: Oid,
    new: Oid,
    name: String,
}

pub fn handle(repo: &RepoRef, body: &[u8]) -> Result<Vec<u8>, String> {
    let mut pos = 0;
    let mut cmds: Vec<Command> = Vec::new();
    let mut use_sideband = false;
    while let Some(p) = read_pkt(body, &mut pos)? {
        let Pkt::Data(d) = p else { break };
        let d = trim_lf(d);
        let (line, caps) = match d.iter().position(|&b| b == 0) {
            Some(i) => (&d[..i], &d[i + 1..]),
            None => (d, &b""[..]),
        };
        if !caps.is_empty() {
            use_sideband = caps.split(|&b| b == b' ').any(|c| c == b"side-band-64k" || c == b"side-band");
        }
        if line.len() < 83 || line[40] != b' ' || line[81] != b' ' {
            return Err("bad command line".into());
        }
        let old = parse_oid(&line[..40]).ok_or("bad old oid")?;
        let new = parse_oid(&line[41..81]).ok_or("bad new oid")?;
        let name = String::from_utf8(line[82..].to_vec()).map_err(|_| "bad ref name")?;
        cmds.push(Command { old, new, name });
    }
    let packbytes = &body[pos..];

    let unpacked = if packbytes.is_empty() {
        Ok(Vec::new())
    } else {
        pack::read_pack(packbytes, |oid| lock(repo).object(oid).map(|o| (o.kind, o.data.clone())))
    };

    let mut report = Vec::with_capacity(64 + 64 * cmds.len());
    match unpacked {
        Err(e) => {
            pkt_str(&mut report, &format!("unpack {}\n", e.replace('\n', " ")));
            for c in &cmds {
                pkt_str(&mut report, &format!("ng {} unpacker error\n", c.name));
            }
        }
        Ok(objs) => {
            pkt_str(&mut report, "unpack ok\n");
            let mut g = lock(repo);
            for o in objs {
                g.objects.entry(o.oid).or_insert(Object { kind: o.kind, data: o.data });
            }
            for c in &cmds {
                match update(&mut g, c) {
                    Ok(()) => pkt_str(&mut report, &format!("ok {}\n", c.name)),
                    Err(why) => pkt_str(&mut report, &format!("ng {} {why}\n", c.name)),
                }
            }
        }
    }
    report.extend_from_slice(b"0000");
    if !use_sideband {
        return Ok(report);
    }
    let mut out = Vec::with_capacity(report.len() + 16);
    sideband(&mut out, 1, &report);
    out.extend_from_slice(b"0000");
    Ok(out)
}

fn update(g: &mut Repo, c: &Command) -> Result<(), &'static str> {
    if !c.name.starts_with("refs/") || c.name.contains("..") || c.name.ends_with('/') {
        return Err("funny refname");
    }
    let cur = g.refs.get(&c.name).copied().unwrap_or(ZERO);
    if cur != c.old {
        return Err("stale info");
    }
    if c.new == ZERO {
        if cur != ZERO {
            g.refs.remove(&c.name);
        }
        return Ok(());
    }
    let Some(obj) = g.object(&c.new) else { return Err("missing necessary objects") };
    if obj.kind == ObjType::Commit {
        // shortcut: checks the commit's tree and parents exist, not the whole closure.
        let Some(info) = g.commit(&c.new) else { return Err("bad commit") };
        if !g.objects.contains_key(&info.tree) || info.parents.iter().any(|p| !g.objects.contains_key(p)) {
            return Err("missing necessary objects");
        }
    }
    if cur != ZERO {
        let ff = g.commit(&c.new).is_some_and(|i| i.parents.contains(&cur)) || g.is_ancestor(cur, c.new);
        if !ff {
            return Err("non-fast-forward");
        }
    }
    g.refs.insert(c.name.clone(), c.new);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::pkts;
    use crate::store::{hash_object, store};
    use crate::util::sha1::hex;
    use std::sync::Arc;

    fn push(r: &RepoRef, cmds: &[(Oid, Oid, &str)], objs: &[(ObjType, Vec<u8>)], sb: bool) -> Vec<String> {
        let mut b = Vec::new();
        for (i, (o, n, name)) in cmds.iter().enumerate() {
            let caps = if i == 0 { if sb { "\0report-status side-band-64k" } else { "\0report-status" } } else { "" };
            pkt_str(&mut b, &format!("{} {} {name}{caps}\n", hex(o), hex(n)));
        }
        b.extend_from_slice(b"0000");
        if cmds.iter().any(|c| c.1 != ZERO) {
            let v: Vec<(ObjType, Arc<[u8]>)> = objs.iter().map(|(k, d)| (*k, Arc::from(d.clone()))).collect();
            b.extend_from_slice(&pack::write_pack(&v));
        }
        let resp = handle(r, &b).unwrap();
        if !sb {
            return pkts(&resp);
        }
        let mut inner = Vec::new();
        let mut pos = 0;
        while let Some(p) = read_pkt(&resp, &mut pos).unwrap() {
            if let Pkt::Data(d) = p {
                assert_eq!(d[0], 1);
                inner.extend_from_slice(&d[1..]);
            }
        }
        pkts(&inner)
    }

    fn commit(tree: &Oid, parent: Option<&Oid>, msg: &str) -> Vec<u8> {
        let p = parent.map(|p| format!("parent {}\n", hex(p))).unwrap_or_default();
        format!("tree {}\n{p}author A <a@b> 1 +0000\ncommitter A <a@b> 1 +0000\n\n{msg}\n", hex(tree)).into_bytes()
    }

    #[test]
    fn push_ff_nonff_mixed_delete() {
        let r = store().create_repo("t-git-rp", "r", "", false).unwrap();
        let blob = b"hi".to_vec();
        let mut tree = b"100644 a.txt\0".to_vec();
        tree.extend_from_slice(&hash_object(ObjType::Blob, &blob));
        let t = hash_object(ObjType::Tree, &tree);
        let c1d = commit(&t, None, "1");
        let c1 = hash_object(ObjType::Commit, &c1d);
        let rep = push(&r, &[(ZERO, c1, "refs/heads/main")], &[(ObjType::Blob, blob), (ObjType::Tree, tree), (ObjType::Commit, c1d)], true);
        assert_eq!(rep, vec!["unpack ok\n", "ok refs/heads/main\n", "0000"]);
        let c2d = commit(&t, Some(&c1), "2");
        let c2 = hash_object(ObjType::Commit, &c2d);
        let sibd = commit(&t, Some(&c1), "sib");
        let sib = hash_object(ObjType::Commit, &sibd);
        // side branch at c2-sibling, then mixed: main ff to c2, side non-ff
        assert_eq!(push(&r, &[(ZERO, sib, "refs/heads/side")], &[(ObjType::Commit, sibd)], false)[1], "ok refs/heads/side\n");
        let rep = push(&r, &[(c1, c2, "refs/heads/main"), (sib, c1, "refs/heads/side")], &[(ObjType::Commit, c2d)], true);
        assert_eq!(rep, vec!["unpack ok\n", "ok refs/heads/main\n", "ng refs/heads/side non-fast-forward\n", "0000"]);
        assert_eq!(lock(&r).refs["refs/heads/main"], c2);
        assert_eq!(lock(&r).refs["refs/heads/side"], sib);
        // delete without pack
        assert_eq!(push(&r, &[(sib, ZERO, "refs/heads/side")], &[], true)[1], "ok refs/heads/side\n");
        assert!(!lock(&r).refs.contains_key("refs/heads/side"));
        // missing object
        assert_eq!(push(&r, &[(ZERO, [5; 20], "refs/heads/x")], &[], true)[1], "ng refs/heads/x missing necessary objects\n");
        // corrupt
        assert!(handle(&r, b"zzzznot-a-pkt-line").is_err());
    }
}
