// GraphQL v4 (graphql-engineer). Stub.
import { sendError, type Ctx } from '../http.ts';

export function handleGraphql(ctx: Ctx): void | Promise<void> {
  sendError(ctx.res, 404, 'Not Found');
}
