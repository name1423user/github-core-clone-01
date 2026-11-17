// The in-memory store every interface (git, REST, GraphQL, MCP) calls. Only the lead edits it.
// Determinism: ids are counters, times come from MAKEFUN_TRIAL_NOW, maps iterate in insertion order.

import type { Comment, Issue, Label, Pull, Repo, User } from '../types.ts';
import { encodeCommit, getObject, isAncestor, lookupPath, parseCommit, putObject, updateTreePath } from './objects.ts';

export * from './objects.ts';

// The suite's viewer is a constant of the sealed env (wire.ts SCENARIO_SEED → users[0]), the same on every
// instance; the token carries no login. FORGE_LOGIN overrides it for local experiments.
export const VIEWER: User = { id: 1000, login: process.env.FORGE_LOGIN ?? 'adaiversen' };
export const TOKEN = process.env.MAKEFUN_SHOP_TOKEN ?? '';

/** Every timestamp the forge writes (ISO, second precision) and its epoch seconds for git signatures. */
const NOW_MS = (() => {
  const t = Date.parse(process.env.MAKEFUN_TRIAL_NOW ?? '');
  return Number.isFinite(t) ? t : Date.UTC(2026, 9, 28);
})();
export const NOW = new Date(NOW_MS).toISOString().replace(/\.\d{3}Z$/, 'Z');
export const NOW_EPOCH = Math.floor(NOW_MS / 1000);

let nextId = 1;
/** Globally unique, monotonically increasing id for any created object. */
export function newId(): number {
  return nextId++;
}

const repos = new Map<string, Repo>();
const key = (owner: string, name: string): string => `${owner.toLowerCase()}/${name.toLowerCase()}`;

// ---------- repositories ----------

export function getRepo(owner: string, name: string): Repo | undefined {
  return repos.get(key(owner, name));
}

export function listRepos(owner: string): Repo[] {
  const o = owner.toLowerCase();
  return [...repos.values()].filter((r) => r.owner.login.toLowerCase() === o);
}

export interface RepoInput {
  name: string;
  description?: string | null;
  private?: boolean;
  auto_init?: boolean;
  default_branch?: string;
  has_issues?: boolean;
}

/** Create a repository for the viewer; undefined when the name is taken. */
export function createRepo(input: RepoInput, owner: User = VIEWER): Repo | undefined {
  const k = key(owner.login, input.name);
  if (repos.has(k)) return undefined;
  const repo: Repo = {
    id: newId(),
    name: input.name,
    owner,
    description: input.description ?? null,
    private: input.private ?? false,
    default_branch: input.default_branch ?? 'main',
    has_issues: input.has_issues ?? true,
    created_at: NOW,
    updated_at: NOW,
    pushed_at: NOW,
    objects: new Map(),
    refs: new Map(),
    refsVersion: 0,
    issues: [],
    labels: new Map(),
  };
  repos.set(k, repo);
  if (input.auto_init) commitFile(repo, repo.default_branch, 'README.md', Buffer.from(`# ${repo.name}\n`), 'Initial commit');
  return repo;
}

export function updateRepo(repo: Repo, patch: Partial<Pick<Repo, 'description' | 'private' | 'default_branch' | 'has_issues' | 'name'>>): void {
  if (patch.name !== undefined && patch.name !== repo.name) {
    repos.delete(key(repo.owner.login, repo.name));
    repos.set(key(repo.owner.login, patch.name), repo);
  }
  Object.assign(repo, patch);
  repo.updated_at = NOW;
}

export function deleteRepo(repo: Repo): void {
  repos.delete(key(repo.owner.login, repo.name));
}

// ---------- refs ----------

export interface RefUpdate {
  ref: string;
  old: string; // 40 zeros = must not exist
  new: string; // 40 zeros = delete
}
export const ZERO = '0'.repeat(40);

/**
 * Check one ref update against the current state: null when it may apply, else the reason git reports
 * ("non-fast-forward", "stale info", "missing object"). Applying is setRef.
 */
export function checkRefUpdate(repo: Repo, u: RefUpdate): string | null {
  const cur = repo.refs.get(u.ref) ?? ZERO;
  if (cur !== u.old) return cur === ZERO || u.old === ZERO ? 'stale info' : 'non-fast-forward';
  if (u.new === ZERO) return null;
  const obj = getObject(repo, u.new);
  if (!obj) return 'missing necessary objects';
  if (cur !== ZERO && u.ref.startsWith('refs/heads/') && !isAncestor(repo, cur, u.new)) return 'non-fast-forward';
  return null;
}

export function setRef(repo: Repo, ref: string, sha: string): void {
  if (sha === ZERO) repo.refs.delete(ref);
  else repo.refs.set(ref, sha);
  repo.refsVersion++;
  repo.pushed_at = NOW;
}

export function branchSha(repo: Repo, branch: string): string | undefined {
  return repo.refs.get(`refs/heads/${branch}`);
}

// ---------- server-side commits ----------

export function signature(user: User = VIEWER): string {
  return `${user.login} <${user.login}@users.noreply.github.com> ${NOW_EPOCH} +0000`;
}

/** Commit one file change (content null = delete) on top of `branch`; returns the new commit sha. */
export function commitFile(repo: Repo, branch: string, path: string, content: Buffer | null, message: string, user: User = VIEWER): string {
  const parent = branchSha(repo, branch);
  const baseTree = parent ? parseCommit(getObject(repo, parent)!.raw).tree : null;
  const blob = content ? putObject(repo, 'blob', content) : null;
  const tree = updateTreePath(repo, baseTree, path, blob);
  return writeCommit(repo, branch, tree, parent ? [parent] : [], message, user);
}

/** Write a commit with `tree` and move `branch` to it. */
export function writeCommit(repo: Repo, branch: string, tree: string, parents: string[], message: string, user: User = VIEWER): string {
  const sig = signature(user);
  const sha = putObject(repo, 'commit', encodeCommit({ tree, parents, author: sig, committer: sig, message: message.endsWith('\n') ? message : `${message}\n` }));
  setRef(repo, `refs/heads/${branch}`, sha);
  return sha;
}

/** The blob sha at `path` on `ref` (branch name or commit sha), or undefined. */
export function fileAt(repo: Repo, ref: string, path: string): { sha: string; mode: string } | undefined {
  const commit = branchSha(repo, ref) ?? (repo.objects.get(ref)?.type === 'commit' ? ref : undefined);
  if (!commit) return undefined;
  const e = lookupPath(repo, parseCommit(getObject(repo, commit)!.raw).tree, path);
  return e ? { sha: e.sha, mode: e.mode } : undefined;
}

// ---------- labels ----------

export function createLabel(repo: Repo, name: string, color = 'ededed', description: string | null = null): Label | undefined {
  const k = name.toLowerCase();
  if (repo.labels.has(k)) return undefined;
  const label: Label = { id: newId(), name, color, description };
  repo.labels.set(k, label);
  return label;
}

export function getLabel(repo: Repo, name: string): Label | undefined {
  return repo.labels.get(name.toLowerCase());
}

// ---------- issues & pulls ----------

export function getIssue(repo: Repo, number: number): Issue | undefined {
  return repo.issues[number - 1];
}

export interface IssueInput {
  title: string;
  body?: string | null;
  labels?: string[];
  assignees?: string[];
}

/** Labels must exist (the caller answers 422 otherwise): returns the missing names. */
export function missingLabels(repo: Repo, names: string[]): string[] {
  return names.filter((n) => !repo.labels.has(n.toLowerCase()));
}

export function createIssue(repo: Repo, input: IssueInput, pull?: Pull, user: User = VIEWER): Issue {
  const issue: Issue = {
    id: newId(),
    number: repo.issues.length + 1,
    title: input.title,
    body: input.body ?? null,
    state: 'open',
    state_reason: null,
    labels: (input.labels ?? []).map((n) => getLabel(repo, n)?.name ?? n),
    assignees: input.assignees ?? [],
    user,
    comments: [],
    created_at: NOW,
    updated_at: NOW,
    closed_at: null,
    pull,
  };
  repo.issues.push(issue);
  return issue;
}

export interface IssuePatch {
  title?: string;
  body?: string | null;
  state?: 'open' | 'closed';
  state_reason?: string | null;
  labels?: string[];
  assignees?: string[];
}

export function updateIssue(issue: Issue, patch: IssuePatch): void {
  if (patch.title !== undefined) issue.title = patch.title;
  if (patch.body !== undefined) issue.body = patch.body;
  if (patch.labels !== undefined) issue.labels = patch.labels;
  if (patch.assignees !== undefined) issue.assignees = patch.assignees;
  if (patch.state !== undefined && patch.state !== issue.state) {
    issue.state = patch.state;
    issue.closed_at = patch.state === 'closed' ? NOW : null;
    issue.state_reason = patch.state === 'closed' ? (patch.state_reason ?? 'completed') : patch.state_reason ?? 'reopened';
  }
  issue.updated_at = NOW;
  issue.cache = undefined;
}

export function addComment(issue: Issue, body: string, user: User = VIEWER): Comment {
  const c: Comment = { id: newId(), body, user, created_at: NOW, updated_at: NOW };
  issue.comments.push(c);
  issue.updated_at = NOW;
  issue.cache = undefined;
  return c;
}

export function createPull(repo: Repo, input: IssueInput & { head: string; base: string; draft?: boolean }): Issue | string {
  const head_sha = branchSha(repo, input.head);
  const base_sha = branchSha(repo, input.base);
  if (!head_sha) return `head branch ${input.head} not found`;
  if (!base_sha) return `base branch ${input.base} not found`;
  return createIssue(repo, input, { head: input.head, base: input.base, head_sha, base_sha, merged: false, merge_commit_sha: null, merged_at: null, draft: input.draft ?? false });
}

export function listPulls(repo: Repo): Issue[] {
  return repo.issues.filter((i) => i.pull);
}

/**
 * Mark a pull merged with the given merge commit (the tree merge itself is src/git's job), close it.
 */
export function markMerged(issue: Issue, mergeSha: string): void {
  const p = issue.pull!;
  p.merged = true;
  p.merge_commit_sha = mergeSha;
  p.merged_at = NOW;
  updateIssue(issue, { state: 'closed' });
}
