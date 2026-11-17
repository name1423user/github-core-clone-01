// Shared types. Only the lead edits this file (CLAUDE.md §2).

export type ObjType = 'commit' | 'tree' | 'blob' | 'tag';

/** One git object, hashed once on arrival. `raw` is the content without the "<type> <len>\0" header. */
export interface GitObject {
  readonly type: ObjType;
  readonly raw: Buffer;
  /** zlib-deflated content (no header), filled lazily by whoever builds packs. */
  deflated?: Buffer;
}

export interface User {
  readonly id: number;
  readonly login: string;
}

export interface Label {
  id: number;
  name: string;
  color: string;
  description: string | null;
}

export interface Comment {
  id: number;
  body: string;
  user: User;
  created_at: string;
  updated_at: string;
}

export interface Issue {
  id: number;
  number: number;
  title: string;
  body: string | null;
  state: 'open' | 'closed';
  state_reason: string | null;
  labels: string[];
  assignees: string[];
  user: User;
  comments: Comment[];
  created_at: string;
  updated_at: string;
  closed_at: string | null;
  /** Set when this number belongs to a pull request (GitHub shares the number space). */
  pull?: Pull;
  /** Serialized REST JSON, cleared by every store write to this issue (read benchmark). */
  cache?: string;
}

export interface Pull {
  head: string;
  base: string;
  head_sha: string;
  base_sha: string;
  merged: boolean;
  merge_commit_sha: string | null;
  merged_at: string | null;
  draft: boolean;
}

export interface Repo {
  id: number;
  name: string;
  owner: User;
  description: string | null;
  private: boolean;
  default_branch: string;
  has_issues: boolean;
  created_at: string;
  updated_at: string;
  pushed_at: string;
  objects: Map<string, GitObject>;
  /** Full ref name ("refs/heads/main") → 40-hex sha. */
  refs: Map<string, string>;
  /** Bumped on every ref change; key for pack caches. */
  refsVersion: number;
  /** issues[number - 1]; pulls share the numbering. */
  issues: Issue[];
  labels: Map<string, Label>;
}
