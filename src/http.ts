// HTTP entry point: one server, a path split and a switch (no regex list). Interfaces receive a Ctx.
// Auth: every /api and /mcp request needs "Authorization: Bearer $MAKEFUN_SHOP_TOKEN"; git and /healthz do not
// (the suite's git client sends no credential).

import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import { gunzipSync } from 'node:zlib';
import { TOKEN } from './store/index.ts';
import { handleRest } from './rest/index.ts';
import { handleGraphql } from './graphql/index.ts';
import { handleMcp } from './mcp/index.ts';
import { handleGit } from './git/index.ts';

export interface Ctx {
  req: IncomingMessage;
  res: ServerResponse;
  method: string;
  /** Path without the query string, e.g. "/api/v3/repos/o/r". */
  path: string;
  /** Non-empty, URL-decoded path segments. */
  parts: string[];
  query: URLSearchParams;
  /** Request body, already gunzipped when Content-Encoding: gzip. */
  body: Buffer;
}

const JSON_TYPE = 'application/json; charset=utf-8';
let requestSeq = 0;

/** Send a JSON reply. `body` may be a pre-serialized string (cached JSON) or a value. */
export function sendJson(res: ServerResponse, status: number, body: unknown, headers: Record<string, string> = {}): void {
  const text = typeof body === 'string' ? body : JSON.stringify(body);
  res.writeHead(status, {
    'content-type': JSON_TYPE,
    'content-length': Buffer.byteLength(text),
    'x-github-request-id': `R${++requestSeq}`,
    ...headers,
  });
  res.end(text);
}

export function sendError(res: ServerResponse, status: number, message: string, extra: Record<string, unknown> = {}): void {
  sendJson(res, status, { message, documentation_url: 'https://docs.github.com/rest', ...extra });
}

/** Parse a JSON body: undefined for an empty body, null after answering 400 for a malformed one. */
export function parseJsonBody(ctx: Ctx): unknown {
  if (ctx.body.length === 0) return undefined;
  try {
    return JSON.parse(ctx.body.toString('utf8'));
  } catch {
    sendError(ctx.res, 400, 'Problems parsing JSON');
    return null;
  }
}

function authorized(req: IncomingMessage): boolean {
  const h = req.headers.authorization;
  if (!h) return false;
  const sp = h.indexOf(' ');
  const scheme = h.slice(0, sp).toLowerCase();
  if (scheme !== 'bearer' && scheme !== 'token') return false;
  return TOKEN === '' || h.slice(sp + 1).trim() === TOKEN;
}

function splitPath(path: string): string[] {
  const out: string[] = [];
  for (const s of path.split('/')) {
    if (s === '') continue;
    try {
      out.push(decodeURIComponent(s));
    } catch {
      out.push(s);
    }
  }
  return out;
}

async function dispatch(ctx: Ctx): Promise<void> {
  const p = ctx.parts;
  switch (p[0]) {
    case 'healthz':
      if (p.length === 1) return sendJson(ctx.res, 200, { ok: true });
      break;
    case 'api':
      if (!authorized(ctx.req)) return sendError(ctx.res, 401, 'Requires authentication');
      if (p[1] === 'v3') return handleRest(ctx);
      if (p[1] === 'graphql' && p.length === 2) return handleGraphql(ctx);
      break;
    case 'mcp':
      if (p.length === 1) {
        if (!authorized(ctx.req)) return sendJson(ctx.res, 401, { jsonrpc: '2.0', id: null, error: { code: -32001, message: 'Requires authentication' } });
        return handleMcp(ctx);
      }
      break;
    default:
      if (p.length >= 3) return handleGit(ctx);
  }
  sendError(ctx.res, 404, 'Not Found');
}

export function createApp(): Server {
  const server = createServer({ keepAlive: true, keepAliveTimeout: 60_000 }, (req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      const url = req.url ?? '/';
      const q = url.indexOf('?');
      const path = q < 0 ? url : url.slice(0, q);
      let body = chunks.length === 1 ? chunks[0]! : Buffer.concat(chunks);
      const ctx: Ctx = { req, res, method: req.method ?? 'GET', path, parts: splitPath(path), query: new URLSearchParams(q < 0 ? '' : url.slice(q + 1)), body };
      try {
        if (req.headers['content-encoding'] === 'gzip' && body.length) ctx.body = body = gunzipSync(body);
        const r = dispatch(ctx);
        if (r) r.catch((err) => fail(res, err));
      } catch (err) {
        fail(res, err);
      }
    });
  });
  server.keepAliveTimeout = 60_000;
  server.headersTimeout = 65_000;
  return server;
}

function fail(res: ServerResponse, err: unknown): void {
  process.stderr.write(`[forge] ${err instanceof Error ? err.stack : String(err)}\n`);
  if (!res.headersSent) sendError(res, 500, 'Internal Server Error');
  else res.end();
}
