// REST v3 (rest-engineer). Stub: API root only; everything else 404.
import { sendError, sendJson, type Ctx } from '../http.ts';

export function handleRest(ctx: Ctx): void | Promise<void> {
  if (ctx.method === 'GET' && ctx.parts.length === 2) {
    return sendJson(ctx.res, 200, { current_user_url: '/api/v3/user', rate_limit_url: '/api/v3/rate_limit' }, { 'x-oauth-scopes': 'repo, read:org' });
  }
  sendError(ctx.res, 404, 'Not Found');
}
