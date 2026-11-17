#!/usr/bin/env bash
# キットをこのリポジトリの「隣」に取得する（リポジトリの中には置かない＝サイズ上限と審査のため）
set -euo pipefail
SLUG="github-core-clone-01"
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PARENT="$(dirname "$REPO_DIR")"
DEST="$PARENT/$SLUG-kit"

cd "$PARENT"
if [ -d "$DEST" ]; then
  backup="$DEST.old.$(date +%Y%m%d%H%M%S)"
  echo "既存のキットを $backup に退避します"
  mv "$DEST" "$backup"
fi

echo "キットを取得中: https://make.fun/c/$SLUG/kit"
curl -fsSL "https://make.fun/c/$SLUG/kit" | tar xz

if [ ! -d "$DEST" ]; then
  echo "展開先 $DEST が見つかりません。展開されたフォルダ名を確認してください:" >&2
  ls -1 "$PARENT" >&2
  exit 1
fi

# kit.json の sha256 と照合（形式が想定と違えばスキップ）
node --input-type=module - "$DEST" <<'EOF' || echo "[注意] sha256 の照合で欠落・不一致がありました。もう一度 bash scripts/fetch-kit.sh を実行してください"
import { readFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
const dir = process.argv[2];
const meta = JSON.parse(readFileSync(join(dir, "kit.json"), "utf8"));
const list = Array.isArray(meta.files) ? meta.files
  : meta.files && typeof meta.files === "object" ? Object.entries(meta.files).map(([path, v]) => ({ path, ...(typeof v === "string" ? { sha256: v } : v) }))
  : null;
if (!list) { console.log("[注意] kit.json の files 形式が想定外のため照合しません"); process.exit(0); }
let bad = 0, ok = 0;
for (const f of list) {
  const p = f.path ?? f.name; const want = f.sha256 ?? f.hash;
  if (!p || !want) continue;
  const full = join(dir, p);
  if (!existsSync(full)) { console.log("欠落:", p); bad++; continue; }
  const got = createHash("sha256").update(readFileSync(full)).digest("hex");
  if (got !== want) { console.log("不一致:", p); bad++; } else ok++;
}
console.log(`sha256 照合: OK ${ok} / NG ${bad}`);
process.exit(bad ? 1 : 0);
EOF

echo "完了: $DEST"
echo "次: このフォルダで claude を起動し、/kickoff を実行"
