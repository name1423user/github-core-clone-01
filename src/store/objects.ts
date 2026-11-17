// Git object model: hashing, tree/commit encode/decode, and server-side commits (contents API, merges).
// Pure functions over a repo's object map; the git transport lives in src/git/.

import { createHash } from 'node:crypto';
import type { GitObject, ObjType, Repo } from '../types.ts';

export function hashObject(type: ObjType, raw: Buffer): string {
  return createHash('sha1').update(`${type} ${raw.length}\0`).update(raw).digest('hex');
}

/** Store an object and return its sha (no-op when already present). */
export function putObject(repo: Repo, type: ObjType, raw: Buffer): string {
  const sha = hashObject(type, raw);
  if (!repo.objects.has(sha)) repo.objects.set(sha, { type, raw });
  return sha;
}

export function getObject(repo: Repo, sha: string): GitObject | undefined {
  return repo.objects.get(sha);
}

export interface TreeEntry {
  mode: string; // "100644" | "100755" | "40000" | "120000" | "160000"
  name: string;
  sha: string;
}

export function parseTree(raw: Buffer): TreeEntry[] {
  const out: TreeEntry[] = [];
  let i = 0;
  while (i < raw.length) {
    const sp = raw.indexOf(0x20, i);
    const nul = raw.indexOf(0, sp);
    out.push({ mode: raw.toString('latin1', i, sp), name: raw.toString('utf8', sp + 1, nul), sha: raw.toString('hex', nul + 1, nul + 21) });
    i = nul + 21;
  }
  return out;
}

/** Git sorts tree entries by name, with directories compared as if suffixed by "/". */
function treeKey(e: TreeEntry): string {
  return e.mode === '40000' ? `${e.name}/` : e.name;
}

export function encodeTree(entries: TreeEntry[]): Buffer {
  const sorted = [...entries].sort((a, b) => {
    const x = treeKey(a), y = treeKey(b);
    return x < y ? -1 : x > y ? 1 : 0;
  });
  const parts: Buffer[] = [];
  for (const e of sorted) parts.push(Buffer.from(`${e.mode} ${e.name}\0`, 'utf8'), Buffer.from(e.sha, 'hex'));
  return Buffer.concat(parts);
}

export interface Commit {
  tree: string;
  parents: string[];
  author: string; // "Name <email> 1700000000 +0000"
  committer: string;
  message: string;
}

export function parseCommit(raw: Buffer): Commit {
  const text = raw.toString('utf8');
  const end = text.indexOf('\n\n');
  const head = end < 0 ? text : text.slice(0, end);
  const c: Commit = { tree: '', parents: [], author: '', committer: '', message: end < 0 ? '' : text.slice(end + 2) };
  for (const line of head.split('\n')) {
    const sp = line.indexOf(' ');
    const k = line.slice(0, sp), v = line.slice(sp + 1);
    if (k === 'tree') c.tree = v;
    else if (k === 'parent') c.parents.push(v);
    else if (k === 'author') c.author = v;
    else if (k === 'committer') c.committer = v;
  }
  return c;
}

export function encodeCommit(c: Commit): Buffer {
  let s = `tree ${c.tree}\n`;
  for (const p of c.parents) s += `parent ${p}\n`;
  s += `author ${c.author}\ncommitter ${c.committer}\n\n${c.message}`;
  return Buffer.from(s, 'utf8');
}

/** Resolve a slash path to its tree entry under `treeSha`, or undefined. */
export function lookupPath(repo: Repo, treeSha: string, path: string): TreeEntry | undefined {
  const parts = path.split('/').filter(Boolean);
  let cur = treeSha;
  let found: TreeEntry | undefined;
  for (const name of parts) {
    const obj = repo.objects.get(cur);
    if (!obj || obj.type !== 'tree') return undefined;
    found = parseTree(obj.raw).find((e) => e.name === name);
    if (!found) return undefined;
    cur = found.sha;
  }
  return found;
}

/** Return the sha of a new tree with `path` set to a blob (or removed when blobSha is null). */
export function updateTreePath(repo: Repo, treeSha: string | null, path: string, blobSha: string | null, mode = '100644'): string {
  const parts = path.split('/').filter(Boolean);
  const rec = (sha: string | null, i: number): string | null => {
    const entries = sha ? parseTree(repo.objects.get(sha)!.raw) : [];
    const name = parts[i]!;
    const idx = entries.findIndex((e) => e.name === name);
    if (i === parts.length - 1) {
      if (idx >= 0) entries.splice(idx, 1);
      if (blobSha) entries.push({ mode, name, sha: blobSha });
    } else {
      const child = idx >= 0 && entries[idx]!.mode === '40000' ? entries[idx]!.sha : null;
      const sub = rec(child, i + 1);
      if (idx >= 0) entries.splice(idx, 1);
      if (sub) entries.push({ mode: '40000', name, sha: sub });
    }
    // Git keeps an empty tree only at the root.
    if (entries.length === 0 && i > 0) return null;
    return putObject(repo, 'tree', encodeTree(entries));
  };
  return rec(treeSha, 0)!;
}

/** Walk the first-parent and merge ancestry: true when `ancestor` is reachable from `sha`. */
export function isAncestor(repo: Repo, ancestor: string, sha: string): boolean {
  if (ancestor === sha) return true;
  const seen = new Set<string>();
  const stack = [sha];
  while (stack.length) {
    const s = stack.pop()!;
    if (s === ancestor) return true;
    if (seen.has(s)) continue;
    seen.add(s);
    const obj = repo.objects.get(s);
    if (obj && obj.type === 'commit') stack.push(...parseCommit(obj.raw).parents);
  }
  return false;
}
