#!/usr/bin/env node
// 規約チェック（標準ライブラリのみ）。使い方: node scripts/compliance.mjs
// ERROR が1つでもあれば終了コード1。WARN は要確認。
import { readdirSync, statSync, readFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, relative, extname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const SKIP_DIRS = new Set([".git", "node_modules", ".grade", ".claude", "target"]);
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
// 提出に入るもの＝追跡中＋未追跡（.gitignore 対象外）。target/ や開発用バイナリは数えない
let files;
try {
  files = execFileSync("git", ["ls-files", "-co", "--exclude-standard", "-z"], { cwd: ROOT, encoding: "utf8" })
    .split("\0").filter(p => p && existsSync(join(ROOT, p)))
    .map(p => ({ path: p, full: join(ROOT, p), size: statSync(join(ROOT, p)).size }));
} catch { files = walk(ROOT); }

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

// 3. 依存なし: package.json に dependencies なし、Cargo.toml の [dependencies] も空（std のみの方針）
const pkgPath = join(ROOT, "package.json");
if (existsSync(pkgPath)) {
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  for (const k of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]) {
    if (pkg[k] && Object.keys(pkg[k]).length) errors.push(`package.json に ${k} があります（ランチャーは Node 組み込みのみ）`);
  }
}
const cargoPath = join(ROOT, "Cargo.toml");
if (!existsSync(cargoPath)) errors.push("Cargo.toml がありません");
else {
  const cargo = readFileSync(cargoPath, "utf8");
  if (!/^version\s*=\s*"\d+\.\d+\.\d+"/m.test(cargo)) warns.push("Cargo.toml に version がありません（版番号ファイル）");
  let section = "";
  for (const line of cargo.split("\n")) {
    const t = line.replace(/#.*/, "").trim();
    const m = t.match(/^\[(.+)\]$/);
    if (m) { section = m[1]; continue; }
    if (t && /(^|\.)(dependencies|build-dependencies|dev-dependencies)$/.test(section)) errors.push(`Cargo.toml [${section}] に依存があります: ${t}（std のみの方針。docs/DECISIONS.md）`);
  }
}

// 4. 提出用バイナリ: forge-linux-x64 があること、開発用バイナリをコミットしていないこと（詳細は scripts/check-binary.sh）
if (!existsSync(join(ROOT, "forge-linux-x64"))) errors.push("forge-linux-x64 がありません（bash scripts/build.sh）");
try {
  const tracked = execFileSync("git", ["ls-files"], { cwd: ROOT, encoding: "utf8" }).split("\n");
  for (const t of tracked) if (/^forge-/.test(t) && t !== "forge-linux-x64") errors.push(`開発用バイナリ ${t} がコミットされています（git rm --cached ${t}）`);
} catch { warns.push("git ls-files を実行できませんでした"); }

// 5. ソースの検査（main.ts と src/**/*.rs）
const srcFiles = files.filter(f => f.path === "main.ts" || (f.path.startsWith("src/") && extname(f.path) === ".rs"));
const importRe = /(?:import\s[^'"]*?from\s*|import\s*\(\s*|require\s*\(\s*|^\s*import\s*)['"]([^'"]+)['"]/gm;
const nondetRs = [
  [/\bSystemTime::now\s*\(/, "SystemTime::now()"],
  [/\bInstant::now\s*\(/, "Instant::now()（計測専用なら可）"],
  [/\bprocess::id\s*\(/, "process::id()"],
  [/\bRandomState\b|\brand::/, "乱数"],
  [/\bHash(Map|Set)\b/, "HashMap/HashSet（反復順がプロセスごとに変わる。反復するなら BTreeMap/Vec）"],
];
const nondetTs = [[/\bDate\.now\s*\(|\bnew Date\s*\(|\bMath\.random\s*\(|\bprocess\.pid\b/, "時刻・乱数・pid"]];
for (const f of srcFiles) {
  const text = readFileSync(f.full, "utf8");
  const isTs = f.path === "main.ts";
  if (isTs) for (const m of text.matchAll(importRe)) {
    const spec = m[1];
    if (!spec.startsWith("node:")) errors.push(`${f.path}: "${spec}" の import（ランチャーは node: の組み込みのみ）`);
  }
  text.split("\n").forEach((line, i) => {
    if (/\b8080\b/.test(line)) errors.push(`${f.path}:${i + 1}: 8080 が含まれています`);
    if (isTs && /\b(enum|namespace)\s+\w+/.test(line) && !/^\s*\/\//.test(line)) warns.push(`${f.path}:${i + 1}: enum/namespace は型ストリップで動きません`);
    if (/^\s*\/\//.test(line)) return; // コメント行は対象外
    for (const [re, name] of isTs ? nondetTs : nondetRs) if (re.test(line) && !/allow-nondeterminism/.test(line)) {
      warns.push(`${f.path}:${i + 1}: ${name} — state・ID・応答に流れ込まないか確認（意図的なら行末に // allow-nondeterminism）`);
    }
  });
}

// 6. 秘密情報の疑い（全ファイル）
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
