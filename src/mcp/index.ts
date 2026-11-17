// MCP over streamable HTTP (mcp-engineer). Stub.
import { sendError, type Ctx } from '../http.ts';

export function handleMcp(ctx: Ctx): void | Promise<void> {
  sendError(ctx.res, 404, 'Not Found');
}
