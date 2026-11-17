// Launcher only. The forge is a statically linked Rust binary committed beside this file (RULES: "a statically
// linked Linux amd64 binary you commit runs from your repository as vendored source"). The binary does the whole
// lifecycle itself: it binds an ephemeral port, registers with $MAKEFUN_ENV_URL, polls /done and exits 0.
// This file starts it, forwards signals, and exits with its code. No npm package, Node built-ins only.
//
// Which binary: forge-<platform>-<arch> next to this file (forge-linux-x64 is the one committed for the grader;
// forge-darwin-arm64 is a local, never-committed build for grading on an Apple Silicon Mac). FORGE_BIN overrides.

import { spawn } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const bin = process.env.FORGE_BIN ?? join(import.meta.dirname, `forge-${process.platform}-${process.arch}`);
if (!existsSync(bin)) {
  process.stderr.write(`[launcher] ${bin} not found: build it with bash scripts/build.sh\n`);
  process.exit(1);
}

function start(path: string, retried: boolean): void {
  const child = spawn(path, [], { stdio: 'inherit' });
  child.on('error', (err: NodeJS.ErrnoException) => {
    // /bot is read-only; if the exec bit was lost in a copy, run a chmod'ed copy from /tmp instead.
    if (err.code === 'EACCES' && !retried) {
      const copy = join(tmpdir(), `forge-${process.pid}`); // allow-nondeterminism: a temp file name, never state
      copyFileSync(path, copy);
      chmodSync(copy, 0o755);
      start(copy, true);
      return;
    }
    process.stderr.write(`[launcher] cannot start ${path}: ${err.message}\n`);
    process.exit(1);
  });
  for (const sig of ['SIGTERM', 'SIGINT'] as const) process.on(sig, () => child.kill(sig));
  child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
}

start(bin, false);
