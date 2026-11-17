// Git smart HTTP (git-engineer). Stub: /{owner}/{repo}.git/... routes land here.
import { sendError, type Ctx } from '../http.ts';

export function handleGit(ctx: Ctx): void | Promise<void> {
  sendError(ctx.res, 404, 'Not Found');
}
