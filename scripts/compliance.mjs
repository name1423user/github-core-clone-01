#!/usr/bin/env node
// 規約チェック（標準ライブラリのみ）。使い方: node scripts/compliance.mjs
// ERROR が1つでもあれば終了コード1。WARN は要確認。
import { readdirSync, statSync, readFileSync, existsSync } from "node:fs";
import { join, relative, extname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const SKIP_DIRS = new Set([".git", "node_modules", ".grade", ".claude"]);
const MiB = 1024 * 1024;
const errors = [];
const warns = [];

function walk(dir, out = []) {
  for (const name of readdirSync(dir).sort()) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) { if (!SKIP_DIRS.has(name)) walk(full, out); }
    else out.push({ path: relative(ROOT, full), full, size: st.size });
  }
  return out;
}
const files = walk(ROOT);

// 1. サイズ上限
const total = files.reduce((a, f) => a + f.size, 0);
if (total > 16 * MiB) errors.push(`合計サイズ ${(total / MiB).toFixed(2)} MiB が 16 MiB を超えています`);
for (const f of files) if (f.size > 8 * MiB) errors.push(`${f.path} が 8 MiB を超えています`);

// 2. エントリポイント・ライセンス・README
if (!existsSync(join(ROOT, "main.ts"))) errors.push("repo 直下に main.ts がありません");
if (!files.some(f => /^LICEN[SC]E/i.test(f.path))) errors.push("LICENSE がありません");
const readme = files.find(f => /^README\.md$/i.test(f.path));
if (!readme) errors.push("README.md がありません");
else {
  const t = readFileSync(readme.full, "utf8");
  const badges = (t.match(/make\.fun\/badge\//g) || []).length;
  if (badges < 3) warns.push(`README の make.fun バッジが ${badges} 個です（競技・rank・reward の3つが必要）`);
  if (/OWNER\/REPO|TODO-README/.test(t)) warns.push("README に仮の値（OWNER/REPO など）が残っています。キットの starter/README.md のテンプレで置き換えてください");
}

// 3. package.json に依存がない
const pkgPath = join(ROOT, "package.json");
if (existsSync(pkgPath)) {
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  for (const k of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]) {
    if (pkg[k] && Object.keys(pkg[k]).length) errors.push(`package.json に ${k} があります（標準ライブラリのみ）`);
  }
  if (!pkg.version) warns.push("package.json に version がありません（版番号ファイル）");
}

// 4. ソースの検査（main.ts と src/ 配下の .ts/.mjs/.js）
const srcFiles = files.filter(f => (f.path === "main.ts" || f.path.startsWith("src/")) && [".ts", ".mjs", ".js", ".mts"].includes(extname(f.path)));
const importRe = /(?:import\s[^'"]*?from\s*|import\s*\(\s*|require\s*\(\s*|^\s*import\s*)['"]([^'"]+)['"]/gm;
const nondet = [
  [/\bDate\.now\s*\(/, "Date.now()"],
  [/\bnew Date\s*\(\s*\)/, "new Date()"],
  [/\bMath\.random\s*\(/, "Math.random()"],
  [/\brandomUUID\s*\(/, "randomUUID()"],
  [/\brandomBytes\s*\(/, "randomBytes()"],
  [/\bperformance\.now\s*\(/, "performance.now()"],
  [/\bprocess\.hrtime/, "process.hrtime"],
  [/\bprocess\.pid\b/, "process.pid"],
];
for (const f of srcFiles) {
  const text = readFileSync(f.full, "utf8");
  for (const m of text.matchAll(importRe)) {
    const spec = m[1];
    if (spec.startsWith("node:") || spec.startsWith("./") || spec.startsWith("../")) continue;
    errors.push(`${f.path}: 外部モジュールの import "${spec}"（node: で始まる組み込みか相対パスのみ）`);
  }
  text.split("\n").forEach((line, i) => {
    if (/\b8080\b/.test(line)) errors.push(`${f.path}:${i + 1}: 8080 が含まれています`);
    if (/\b(enum|namespace)\s+\w+/.test(line) && !/^\s*\/\//.test(line)) warns.push(`${f.path}:${i + 1}: enum/namespace は型ストリップで動きません`);
    for (const [re, name] of nondet) if (re.test(line) && !/allow-nondeterminism/.test(line)) {
      warns.push(`${f.path}:${i + 1}: ${name} — state/export/ID に流れ込まないか確認（意図的なら行末に // allow-nondeterminism）`);
    }
  });
}

// 5. 秘密情報の疑い（全ファイル）
const secretRes = [
  [/sk-ant-[A-Za-z0-9_-]{10,}/, "Anthropic API キー"],
  [/sk-(proj-)?[A-Za-z0-9]{20,}/, "OpenAI API キー"],
  [/gh[pousr]_[A-Za-z0-9]{30,}/, "GitHub トークン"],
  [/AKIA[0-9A-Z]{16}/, "AWS アクセスキー"],
  [/-----BEGIN [A-Z ]*PRIVATE KEY-----/, "秘密鍵"],
  [/^\s*\[\s*(\d{1,3}\s*,\s*){63}\d{1,3}\s*\]\s*$/m, "Solana キーペア（64バイト配列）"],
];
for (const f of files) {
  if (f.size > 2 * MiB) continue;
  let text; try { text = readFileSync(f.full, "utf8"); } catch { continue; }
  for (const [re, name] of secretRes) if (re.test(text)) errors.push(`${f.path}: ${name}の疑い`);
  if (/^\.env/.test(f.path)) errors.push(`${f.path}: .env ファイルが含まれています`);
}

// 結果
console.log(`ファイル ${files.length} 個 / 合計 ${(total / MiB).toFixed(2)} MiB`);
for (const w of warns) console.log("WARN ", w);
for (const e of errors) console.log("ERROR", e);
console.log(errors.length ? `NG（ERROR ${errors.length} 件）` : `OK（WARN ${warns.length} 件）`);
process.exit(errors.length ? 1 : 0);
