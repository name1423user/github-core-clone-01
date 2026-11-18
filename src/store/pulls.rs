//! Pull requests: create, changed files, squash merge with path-level conflicts. Call with the repo locked.
//! 持ち場: リード。

use super::{store, Flat, Kind, Oid, Pull, Repo, StoreError};
use std::collections::HashSet;

pub struct FileChange {
    pub path: String,
    /// "added" | "modified" | "removed"
    pub status: &'static str,
    pub additions: u64,
    pub deletions: u64,
}

/// path → Some(new) / None (deleted), for paths that differ between `a` and `b`.
fn diff(a: &Flat, b: &Flat) -> Vec<(String, Option<(String, Oid)>)> {
    let mut out: Vec<_> = b.iter().filter(|(p, v)| a.get(*p) != Some(v)).map(|(p, v)| (p.clone(), Some(v.clone()))).collect();
    out.extend(a.keys().filter(|p| !b.contains_key(*p)).map(|p| (p.clone(), None)));
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

fn lines(t: &[u8]) -> Vec<&[u8]> {
    let t = t.strip_suffix(b"\n").unwrap_or(t);
    if t.is_empty() { vec![] } else { t.split(|&b| b == b'\n').collect() }
}

/// Lines added/removed after trimming the common prefix and suffix (a cheap stand-in for a real diff).
fn line_delta(old: &[u8], new: &[u8]) -> (u64, u64) {
    let (o, n) = (lines(old), lines(new));
    let pre = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let suf = o[pre..].iter().rev().zip(n[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
    ((n.len() - pre - suf) as u64, (o.len() - pre - suf) as u64)
}

impl Repo {
    /// Nearest common ancestor of two commits (BFS from `b` over `a`'s ancestors).
    pub fn merge_base(&self, a: Oid, b: Oid) -> Option<Oid> {
        let mut anc = HashSet::new();
        let mut stack = vec![a];
        while let Some(c) = stack.pop() {
            if anc.insert(c) {
                if let Some(i) = self.commit(&c) {
                    stack.extend(i.parents);
                }
            }
        }
        let mut queue = std::collections::VecDeque::from([b]);
        let mut seen = HashSet::new();
        while let Some(c) = queue.pop_front() {
            if anc.contains(&c) {
                return Some(c);
            }
            if seen.insert(c) {
                if let Some(i) = self.commit(&c) {
                    queue.extend(i.parents);
                }
            }
        }
        None
    }

    /// Both branches must exist. Returns the new PR's number.
    pub fn create_pull(&mut self, title: &str, body: &str, head: &str, base: &str) -> Result<u64, StoreError> {
        let head_sha = self.branch(head).ok_or(StoreError::NotFound)?;
        let base_sha = self.branch(base).ok_or(StoreError::NotFound)?;
        let number = self.next_number();
        let id = store().next_id(Kind::Pull);
        self.pulls.push(Pull {
            id,
            number,
            title: title.to_string(),
            body: body.to_string(),
            head: head.to_string(),
            base: base.to_string(),
            open: true,
            merged: false,
            merge_sha: None,
            base_sha,
            head_sha,
        });
        Ok(number)
    }

    pub fn pull(&self, number: u64) -> Option<&Pull> {
        self.pulls.binary_search_by_key(&number, |p| p.number).ok().map(|i| &self.pulls[i])
    }

    pub fn pull_mut(&mut self, number: u64) -> Option<&mut Pull> {
        let i = self.pulls.binary_search_by_key(&number, |p| p.number).ok()?;
        Some(&mut self.pulls[i])
    }

    pub fn pull_by_id(&self, id: u64) -> Option<&Pull> {
        self.pulls.iter().find(|p| p.id == id)
    }

    /// (head sha, base sha): live branch tips while open, the recorded ones once closed.
    pub fn pull_shas(&self, p: &Pull) -> (Oid, Oid) {
        if p.open {
            (self.branch(&p.head).unwrap_or(p.head_sha), self.branch(&p.base).unwrap_or(p.base_sha))
        } else {
            (p.head_sha, p.base_sha)
        }
    }

    pub fn pull_files(&self, p: &Pull) -> Vec<FileChange> {
        let (head, base) = self.pull_shas(p);
        let mb = self.merge_base(base, head).unwrap_or(base);
        let (old, new) = (self.files_at(&mb), self.files_at(&head));
        let blob = |o: &Oid| self.object(o).map(|o| o.data.clone()).unwrap_or_else(|| std::sync::Arc::from(&b""[..]));
        diff(&old, &new)
            .into_iter()
            .map(|(path, now)| {
                let before = old.get(&path).map(|v| blob(&v.1));
                let after = now.as_ref().map(|v| blob(&v.1));
                let (additions, deletions) = line_delta(before.as_deref().unwrap_or(b""), after.as_deref().unwrap_or(b""));
                let status = match (&before, &after) {
                    (None, _) => "added",
                    (_, None) => "removed",
                    _ => "modified",
                };
                FileChange { path, status, additions, deletions }
            })
            .collect()
    }

    /// The squash result's files, or `Conflict` if a path changed differently on both sides since the merge base.
    pub fn merged_files(&self, p: &Pull) -> Result<Flat, StoreError> {
        let (head, base) = self.pull_shas(p);
        let mb = self.merge_base(base, head).unwrap_or(base);
        let ancestor = self.files_at(&mb);
        let mut result = self.files_at(&base);
        let theirs = diff(&ancestor, &result);
        for (path, change) in diff(&ancestor, &self.files_at(&head)) {
            if let Some((_, other)) = theirs.iter().find(|(q, _)| *q == path) {
                if *other != change {
                    return Err(StoreError::Conflict);
                }
            }
            match change {
                Some(v) => result.insert(path, v),
                None => result.remove(&path),
            };
        }
        Ok(result)
    }

    /// Squash-merge onto the base branch. Unknown → NotFound, already closed or conflicting → Conflict (REST 405).
    pub fn merge_pull(&mut self, number: u64, message: Option<&str>) -> Result<Oid, StoreError> {
        let p = self.pull(number).ok_or(StoreError::NotFound)?;
        if !p.open {
            return Err(StoreError::Conflict);
        }
        let (head, base) = self.pull_shas(p);
        let flat = self.merged_files(p)?;
        let msg = message.map(str::to_string).unwrap_or_else(|| format!("{} (#{number})", p.title));
        let base_branch = p.base.clone();
        let tree = self.write_flat(&flat);
        let sha = self.write_commit(tree, &[base], &msg);
        self.refs.insert(format!("refs/heads/{base_branch}"), sha);
        let p = self.pull_mut(number).unwrap();
        p.open = false;
        p.merged = true;
        p.merge_sha = Some(sha);
        p.head_sha = head;
        p.base_sha = base;
        Ok(sha)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn squash_and_conflict() {
        let r = store().create_repo("t-pulls", "r", "", false).unwrap();
        let mut r = lock(&r);
        let (c1, _) = r.commit_file("main", "config/token.css", Some(b"a\n".to_vec()), "init", None).unwrap();
        let blob = r.files_at(&c1)["config/token.css"].1;
        for b in ["x", "y"] {
            r.refs.insert(format!("refs/heads/{b}"), c1);
            r.commit_file(b, "config/token.css", Some(format!("{b}\n").into_bytes()), b, Some(blob)).unwrap();
        }
        r.commit_file("x", "new.txt", Some(b"1\n2\n".to_vec()), "n", None).unwrap();
        let px = r.create_pull("X", "", "x", "main").unwrap();
        let py = r.create_pull("Y", "", "y", "main").unwrap();
        let files = r.pull_files(r.pull(px).unwrap());
        assert_eq!(files.iter().map(|f| (f.path.as_str(), f.status, f.additions)).collect::<Vec<_>>(),
            vec![("config/token.css", "modified", 1), ("new.txt", "added", 2)]);
        let sha = r.merge_pull(px, None).unwrap();
        assert_eq!(r.branch("main"), Some(sha));
        assert_eq!(r.commit(&sha).unwrap().message, "X (#1)");
        assert!(r.files_at(&sha).contains_key("new.txt"));
        assert_eq!(r.merge_pull(py, None), Err(StoreError::Conflict));
        assert_eq!(r.merge_pull(99, None), Err(StoreError::NotFound));
        assert!(r.pull(px).unwrap().merged && r.pull(py).unwrap().open);
    }
}
