//! Git objects, trees as flat path maps, file commits (REST contents), first-parent logs.
//! Call with the repo locked. 持ち場: リード。

use super::{now, ObjType, Object, Oid, Repo, StoreError, VIEWER};
use crate::util::sha1::{hex, Sha1};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const MODE_FILE: &str = "100644";
pub const MODE_TREE: &str = "40000";

/// path ("a/b.txt") → (mode, blob oid). Only files (and submodules/symlinks as-is); directories are implied.
pub type Flat = BTreeMap<String, (String, Oid)>;

pub fn type_name(t: ObjType) -> &'static str {
    match t {
        ObjType::Commit => "commit",
        ObjType::Tree => "tree",
        ObjType::Blob => "blob",
        ObjType::Tag => "tag",
    }
}

pub fn hash_object(kind: ObjType, data: &[u8]) -> Oid {
    let mut h = Sha1::new();
    h.update(format!("{} {}\0", type_name(kind), data.len()).as_bytes());
    h.update(data);
    h.finish()
}

/// Parsed commit header fields we need.
pub struct CommitInfo<'a> {
    pub tree: Oid,
    pub parents: Vec<Oid>,
    pub message: &'a str,
}

pub fn parse_commit(data: &[u8]) -> Option<CommitInfo<'_>> {
    let text = std::str::from_utf8(data).ok()?;
    let (head, message) = text.split_once("\n\n").unwrap_or((text, ""));
    let mut tree = None;
    let mut parents = Vec::new();
    for line in head.lines() {
        if let Some(h) = line.strip_prefix("tree ") {
            tree = crate::util::sha1::from_hex(h);
        } else if let Some(h) = line.strip_prefix("parent ") {
            parents.push(crate::util::sha1::from_hex(h)?);
        }
    }
    Some(CommitInfo { tree: tree?, parents, message: message.strip_suffix('\n').unwrap_or(message) })
}

/// Tree entries: (mode, name, oid), in stored order.
pub fn parse_tree(data: &[u8]) -> Vec<(&str, &str, Oid)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(sp) = data[i..].iter().position(|&b| b == b' ') {
        let Some(nul) = data[i + sp..].iter().position(|&b| b == 0) else { break };
        let (m, n) = (i + sp, i + sp + nul);
        if n + 21 > data.len() {
            break;
        }
        let mode = std::str::from_utf8(&data[i..m]).unwrap_or("");
        let name = std::str::from_utf8(&data[m + 1..n]).unwrap_or("");
        out.push((mode, name, data[n + 1..n + 21].try_into().unwrap()));
        i = n + 21;
    }
    out
}

impl Repo {
    pub fn put_object(&mut self, kind: ObjType, data: Vec<u8>) -> Oid {
        let oid = hash_object(kind, &data);
        self.objects.entry(oid).or_insert_with(|| Object { kind, data: Arc::from(data) });
        oid
    }

    pub fn object(&self, oid: &Oid) -> Option<&Object> {
        self.objects.get(oid)
    }

    pub fn commit(&self, oid: &Oid) -> Option<CommitInfo<'_>> {
        let o = self.objects.get(oid).filter(|o| o.kind == ObjType::Commit)?;
        parse_commit(&o.data)
    }

    /// `refs/heads/<branch>` tip.
    pub fn branch(&self, branch: &str) -> Option<Oid> {
        self.refs.get(&format!("refs/heads/{branch}")).copied()
    }

    /// A branch name or a 40-hex commit sha → commit oid.
    pub fn resolve(&self, rev: &str) -> Option<Oid> {
        self.branch(rev)
            .or_else(|| self.refs.get(&format!("refs/tags/{rev}")).copied())
            .or_else(|| crate::util::sha1::from_hex(rev).filter(|o| self.objects.contains_key(o)))
    }

    /// Flatten a tree into path → (mode, oid) for files.
    pub fn flatten(&self, tree: &Oid) -> Flat {
        let mut out = Flat::new();
        self.flatten_into(tree, "", &mut out);
        out
    }

    fn flatten_into(&self, tree: &Oid, prefix: &str, out: &mut Flat) {
        let Some(o) = self.objects.get(tree) else { return };
        for (mode, name, oid) in parse_tree(&o.data) {
            let path = format!("{prefix}{name}");
            if mode == MODE_TREE {
                self.flatten_into(&oid, &format!("{path}/"), out);
            } else {
                out.insert(path, (mode.to_string(), oid));
            }
        }
    }

    /// Write a flat map back as trees; returns the root tree oid.
    pub fn write_flat(&mut self, flat: &Flat) -> Oid {
        let entries: Vec<(&str, &(String, Oid))> = flat.iter().map(|(p, v)| (p.as_str(), v)).collect();
        self.write_dir(&entries)
    }

    /// `entries` are paths relative to this directory, sorted.
    fn write_dir(&mut self, entries: &[(&str, &(String, Oid))]) -> Oid {
        // (git sort key, mode, name, oid): git orders directories as if their name ended in '/'.
        let mut items: Vec<(String, String, String, Oid)> = Vec::new();
        let mut i = 0;
        while i < entries.len() {
            let (path, (mode, oid)) = entries[i];
            match path.split_once('/') {
                None => {
                    items.push((path.to_string(), mode.clone(), path.to_string(), *oid));
                    i += 1;
                }
                Some((dir, _)) => {
                    let start = i;
                    let pre = format!("{dir}/");
                    while i < entries.len() && entries[i].0.starts_with(&pre) {
                        i += 1;
                    }
                    let sub: Vec<_> = entries[start..i].iter().map(|(p, v)| (&p[pre.len()..], *v)).collect();
                    let oid = self.write_dir(&sub);
                    items.push((pre.clone(), MODE_TREE.to_string(), dir.to_string(), oid));
                }
            }
        }
        items.sort_by(|a, b| a.0.cmp(&b.0));
        let mut data = Vec::new();
        for (_, mode, name, oid) in &items {
            data.extend_from_slice(mode.as_bytes());
            data.push(b' ');
            data.extend_from_slice(name.as_bytes());
            data.push(0);
            data.extend_from_slice(oid);
        }
        self.put_object(ObjType::Tree, data)
    }

    pub fn write_commit(&mut self, tree: Oid, parents: &[Oid], message: &str) -> Oid {
        let who = format!("{VIEWER} <{VIEWER}@users.noreply.github.com> {} +0000", now().secs);
        let mut s = format!("tree {}\n", hex(&tree));
        for p in parents {
            s.push_str(&format!("parent {}\n", hex(p)));
        }
        s.push_str(&format!("author {who}\ncommitter {who}\n\n{message}\n"));
        self.put_object(ObjType::Commit, s.into_bytes())
    }

    /// Files of a commit (empty for an unknown commit).
    pub fn files_at(&self, commit: &Oid) -> Flat {
        self.commit(commit).map(|c| self.flatten(&c.tree)).unwrap_or_default()
    }

    /// Create, update (`content` Some) or delete (`None`) one file on a branch, as one commit.
    /// `expected` is the blob sha the caller believes is there: required for update/delete
    /// (GitHub 422/409 semantics: a mismatch → `Conflict`). An empty repo gets the branch created.
    /// Returns (commit oid, new blob oid or None for a delete).
    pub fn commit_file(
        &mut self,
        branch: &str,
        path: &str,
        content: Option<Vec<u8>>,
        message: &str,
        expected: Option<Oid>,
    ) -> Result<(Oid, Option<Oid>), StoreError> {
        let path = path.trim_matches('/');
        let parent = self.branch(branch);
        if parent.is_none() && !self.refs.is_empty() {
            return Err(StoreError::NotFound); // unknown branch in a non-empty repo
        }
        let mut flat = parent.map(|p| self.files_at(&p)).unwrap_or_default();
        let current = flat.get(path).map(|(_, o)| *o);
        match (&content, current, expected) {
            (None, None, _) => return Err(StoreError::NotFound),
            (_, Some(cur), Some(exp)) if cur != exp => return Err(StoreError::Conflict),
            (_, Some(_), None) => return Err(StoreError::Conflict), // update without sha (GitHub: 422)
            _ => {}
        }
        let blob = match content {
            Some(bytes) => {
                let oid = self.put_object(ObjType::Blob, bytes);
                flat.insert(path.to_string(), (MODE_FILE.to_string(), oid));
                Some(oid)
            }
            None => {
                flat.remove(path);
                None
            }
        };
        let tree = self.write_flat(&flat);
        let parents: Vec<Oid> = parent.into_iter().collect();
        let commit = self.write_commit(tree, &parents, message);
        self.refs.insert(format!("refs/heads/{branch}"), commit);
        Ok((commit, blob))
    }

    /// First-parent history from `tip`, newest first.
    pub fn log(&self, tip: Oid, limit: usize) -> Vec<Oid> {
        let mut out = Vec::new();
        let mut cur = Some(tip);
        while let Some(c) = cur {
            if out.len() >= limit {
                break;
            }
            let Some(info) = self.commit(&c) else { break };
            out.push(c);
            cur = info.parents.first().copied();
        }
        out
    }

    /// Is `ancestor` reachable from `tip` (any parent)? Checks the direct parent first (the push fast path).
    pub fn is_ancestor(&self, ancestor: Oid, tip: Oid) -> bool {
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![tip];
        while let Some(c) = stack.pop() {
            if c == ancestor {
                return true;
            }
            if !seen.insert(c) {
                continue;
            }
            if let Some(info) = self.commit(&c) {
                stack.extend(info.parents);
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    #[test]
    fn files_commits_and_trees() {
        let r = store().create_repo("t-objects", "r", "", false).unwrap();
        let mut r = lock(&r);
        let (c1, b1) = r.commit_file("main", "config/seed.txt", Some(b"seed".to_vec()), "first", None).unwrap();
        assert_eq!(r.branch("main"), Some(c1));
        // An update needs the current sha.
        assert_eq!(r.commit_file("main", "config/seed.txt", Some(b"x".to_vec()), "m", None).err(), Some(StoreError::Conflict));
        let (c2, _) = r.commit_file("main", "config/seed.txt", Some(b"x".to_vec()), "m", b1).unwrap();
        r.commit_file("main", "a.txt", Some(b"a".to_vec()), "m2", None).unwrap();
        r.commit_file("main", "config-x", Some(b"y".to_vec()), "m3", None).unwrap();
        let tip = r.branch("main").unwrap();
        let files = r.files_at(&tip);
        assert_eq!(files.keys().collect::<Vec<_>>(), vec!["a.txt", "config-x", "config/seed.txt"]);
        // git order: "config-x" < "config/" (0x2d < 0x2f), so the tree lists config-x before config.
        let root = r.commit(&tip).unwrap().tree;
        let names: Vec<String> = parse_tree(&r.object(&root).unwrap().data).iter().map(|e| e.1.to_string()).collect();
        assert_eq!(names, vec!["a.txt", "config-x", "config"]);
        assert_eq!(r.log(tip, 10).len(), 4);
        assert!(r.is_ancestor(c1, tip) && !r.is_ancestor(tip, c1));
        assert_eq!(r.commit(&c2).unwrap().message, "m");
        // Known blob hash: `printf seed | git hash-object --stdin`.
        assert_eq!(hex(&b1.unwrap()), hex(&hash_object(ObjType::Blob, b"seed")));
        assert_eq!(hex(&hash_object(ObjType::Blob, b"")), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }
}
