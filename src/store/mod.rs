//! インメモリのストア（リポジトリ・git オブジェクト・ref・issue・PR・コメント・ラベル）。持ち場: リード。
//! REST / GraphQL / MCP / git はすべてここの公開関数だけを呼ぶ。
//! 決定性: ID は単調カウンタ、時刻は MAKEFUN_TRIAL_NOW から、反復する集合は BTreeMap か Vec
//! （std の HashMap は反復順がプロセスごとに変わるので、反復するものには使わない）。
//! ロック: リポジトリの一覧は RwLock、各リポジトリは自分の Mutex（別リポジトリの操作は並行できる）。

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

mod issues;

pub type Oid = [u8; 20];

/// The token owner's login: a sealed suite constant (wire.ts SCENARIO_SEED → users[0]), not instance data.
/// 根拠は docs/DECISIONS.md。
pub const VIEWER: &str = "adaiversen";

// ---------------------------------------------------------------- time

/// The trial's fixed now (MAKEFUN_TRIAL_NOW): every timestamp the forge writes. Never the wall clock.
pub struct Now {
    pub secs: i64,
    /// RFC 3339, second precision, `Z` (GitHub's REST format).
    pub iso: String,
}

pub fn now() -> &'static Now {
    static N: OnceLock<Now> = OnceLock::new();
    N.get_or_init(|| {
        let secs = std::env::var("MAKEFUN_TRIAL_NOW").ok().and_then(|t| parse_time(t.trim())).unwrap_or(1_735_689_600);
        Now { secs, iso: iso(secs) }
    })
}

/// RFC 3339 instant (`2026-11-02T09:00:00Z`, `…+09:00`, fractional seconds) or epoch milliseconds → epoch seconds.
fn parse_time(t: &str) -> Option<i64> {
    if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) {
        return Some(t.parse::<i64>().ok()? / 1000);
    }
    let b = t.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
    }
    let offset = match b.get(i)? {
        b'Z' | b'z' => 0,
        sign @ (b'+' | b'-') => {
            let off = n(i + 1..i + 3)? * 3600 + n(i + 4..i + 6)? * 60;
            if *sign == b'+' { off } else { -off }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s - offset)
}

/// Howard Hinnant's days_from_civil.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn iso(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

// ---------------------------------------------------------------- ids

/// One monotonic counter per kind. Ids only advance on creates, which the suite issues one at a time,
/// so the same run gives the same ids.
pub struct Counters {
    pub repo: AtomicU64,
    pub issue: AtomicU64,
    pub pull: AtomicU64,
    pub comment: AtomicU64,
    pub label: AtomicU64,
}

fn bump(c: &AtomicU64) -> u64 {
    c.fetch_add(1, Ordering::Relaxed) + 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Repo,
    Issue,
    Pull,
    Comment,
    Label,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Repo => "R_kgDO",
            Kind::Issue => "I_kwDO",
            Kind::Pull => "PR_kwDO",
            Kind::Comment => "IC_kwDO",
            Kind::Label => "LA_kwDO",
        }
    }
}

/// Global node id, shared by REST `node_id` and GraphQL `id`. Never equal to the bare database id or number.
pub fn node_id(kind: Kind, id: u64) -> String {
    format!("{}{id}", kind.prefix())
}

pub fn parse_node_id(s: &str) -> Option<(Kind, u64)> {
    // Longest prefixes first: "PR_kwDO" must not be read by a shorter one.
    [Kind::Pull, Kind::Comment, Kind::Label, Kind::Repo, Kind::Issue]
        .into_iter()
        .find_map(|k| Some((k, s.strip_prefix(k.prefix())?.parse().ok()?)))
}

// ---------------------------------------------------------------- data

pub struct Label {
    pub id: u64,
    pub name: String,
    pub color: String,
    pub description: String,
}

pub struct Comment {
    pub id: u64,
    pub body: String,
}

pub struct Issue {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub open: bool,
    /// Indexes into `Repo::labels`.
    pub labels: Vec<usize>,
    pub comments: Vec<Comment>,
    /// Pre-serialized REST JSON (read bench); `None` after any change.
    pub json: Option<Arc<[u8]>>,
}

pub struct Pull {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub head: String,
    pub base: String,
    pub open: bool,
    pub merged: bool,
    pub merge_sha: Option<Oid>,
    /// base tip when the PR was opened (for files and conflict checks).
    pub base_sha: Oid,
    pub head_sha: Oid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjType {
    Commit = 1,
    Tree = 2,
    Blob = 3,
    Tag = 4,
}

pub struct Object {
    pub kind: ObjType,
    /// Raw object bytes (without the `"<type> <len>\0"` header).
    pub data: Arc<[u8]>,
}

pub struct Repo {
    pub id: u64,
    pub owner: String,
    pub name: String,
    pub description: String,
    pub private: bool,
    pub default_branch: String,
    /// refs/... → oid. BTreeMap: advertised in sorted order.
    pub refs: BTreeMap<String, Oid>,
    pub objects: HashMap<Oid, Object>,
    pub labels: Vec<Label>,
    pub issues: Vec<Issue>,
    pub pulls: Vec<Pull>,
    /// Issues and PRs share one number sequence, as on GitHub.
    pub last_number: u64,
}

pub type RepoRef = Arc<Mutex<Repo>>;

pub struct Store {
    pub ids: Counters,
    /// (owner, name) → repo. BTreeMap: `GET /user/repos` lists by full name.
    repos: RwLock<BTreeMap<(String, String), RepoRef>>,
}

pub fn store() -> &'static Store {
    static S: OnceLock<Store> = OnceLock::new();
    S.get_or_init(|| Store {
        ids: Counters {
            repo: AtomicU64::new(0),
            issue: AtomicU64::new(0),
            pull: AtomicU64::new(0),
            comment: AtomicU64::new(0),
            label: AtomicU64::new(0),
        },
        repos: RwLock::new(BTreeMap::new()),
    })
}

/// Lock a repo. A poisoned lock (a panicking handler) still holds consistent data for our writes, so keep serving.
pub fn lock(r: &RepoRef) -> std::sync::MutexGuard<'_, Repo> {
    r.lock().unwrap_or_else(|e| e.into_inner())
}

impl Store {
    pub fn next_id(&self, kind: Kind) -> u64 {
        bump(match kind {
            Kind::Repo => &self.ids.repo,
            Kind::Issue => &self.ids.issue,
            Kind::Pull => &self.ids.pull,
            Kind::Comment => &self.ids.comment,
            Kind::Label => &self.ids.label,
        })
    }

    pub fn repo(&self, owner: &str, name: &str) -> Option<RepoRef> {
        let map = self.repos.read().unwrap_or_else(|e| e.into_inner());
        map.get(&(owner.to_string(), name.to_string())).cloned()
    }

    pub(crate) fn repos_read(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<(String, String), RepoRef>> {
        self.repos.read().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn repos_write(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<(String, String), RepoRef>> {
        self.repos.write().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    /// 422 `name already exists on this account` (git-client.ts:2096 matches `already exists`).
    AlreadyExists,
    NotFound,
    /// 422: the named label does not exist in the repo.
    UnknownLabel(String),
}

impl Store {
    pub fn create_repo(&self, owner: &str, name: &str, description: &str, private: bool) -> Result<RepoRef, StoreError> {
        let mut map = self.repos_write();
        let key = (owner.to_string(), name.to_string());
        if map.contains_key(&key) {
            return Err(StoreError::AlreadyExists);
        }
        let repo = Arc::new(Mutex::new(Repo {
            id: self.next_id(Kind::Repo),
            owner: key.0.clone(),
            name: key.1.clone(),
            description: description.to_string(),
            private,
            default_branch: "main".to_string(),
            refs: BTreeMap::new(),
            objects: HashMap::new(),
            labels: Vec::new(),
            issues: Vec::new(),
            pulls: Vec::new(),
            last_number: 0,
        }));
        map.insert(key, repo.clone());
        Ok(repo)
    }

    pub fn delete_repo(&self, owner: &str, name: &str) -> bool {
        self.repos_write().remove(&(owner.to_string(), name.to_string())).is_some()
    }

    /// The owner's repos, by name (ascending).
    pub fn list_repos(&self, owner: &str) -> Vec<RepoRef> {
        self.repos_read().iter().filter(|((o, _), _)| o == owner).map(|(_, r)| r.clone()).collect()
    }

    pub fn repo_by_id(&self, id: u64) -> Option<RepoRef> {
        // shortcut: linear scan (the suite makes ~100 repos); index by id if a profile shows it.
        self.repos_read().values().find(|r| lock(r).id == id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time() {
        assert_eq!(parse_time("2025-01-01T00:00:00Z"), Some(1_735_689_600));
        assert_eq!(parse_time("2025-01-01T09:00:00.123+09:00"), Some(1_735_689_600));
        assert_eq!(parse_time("1735689600000"), Some(1_735_689_600));
        assert_eq!(parse_time("garbage"), None);
        assert_eq!(iso(1_735_689_600), "2025-01-01T00:00:00Z");
        assert_eq!(iso(1_709_210_096), "2024-02-29T12:34:56Z");
        assert_eq!(parse_time(&iso(1_709_210_096)), Some(1_709_210_096));
    }

    #[test]
    fn node_ids() {
        for (k, id) in [(Kind::Repo, 7), (Kind::Issue, 12), (Kind::Pull, 3), (Kind::Comment, 1), (Kind::Label, 99)] {
            let n = node_id(k, id);
            assert_ne!(n, id.to_string());
            assert_eq!(parse_node_id(&n), Some((k, id)));
        }
        assert_eq!(parse_node_id("nope"), None);
    }
}
