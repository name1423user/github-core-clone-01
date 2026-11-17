#!/usr/bin/env bash
# ローカル採点。使い方: bash scripts/grade.sh [grade-local に渡す追加オプション]
#   TWICE=1 bash scripts/grade.sh   … 2回走らせてログを比較（決定性の目視確認用）
# grade-local のオプションはキットの版で変わり得る。初回は
#   node "$KIT/cli/makefun.mjs" grade-local --help
# で確認し、必要ならこのスクリプトを直すこと。
set -uo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
KIT="${KIT:-$(dirname "$REPO")/github-core-clone-01-kit}"
OUT="$REPO/.grade/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"

if [ ! -f "$KIT/cli/makefun.mjs" ]; then
  echo "キットが見つかりません: $KIT （bash scripts/fetch-kit.sh を実行）" >&2
  exit 2
fi

echo "== 規約チェック =="
node "$REPO/scripts/compliance.mjs" | tee "$OUT/compliance.log"

runs=1; [ "${TWICE:-0}" = "1" ] && runs=2
status=0
for i in $(seq 1 $runs); do
  echo "== grade-local (run $i) =="
  ( cd "$KIT" && node cli/makefun.mjs grade-local --kit . --bot "$REPO" --driver process "$@" ) 2>&1 | tee "$OUT/run$i.log"
  s=${PIPESTATUS[0]}; [ "$s" -ne 0 ] && status=$s
done

if [ "$runs" = "2" ]; then
  echo "== run1 / run2 の差分（時間・ポート番号の行は除外） =="
  filt() { grep -viE '(ms|sec|elapsed|duration|took|port|[0-9]+\.[0-9]+s)' "$1"; }
  if diff <(filt "$OUT/run1.log") <(filt "$OUT/run2.log") > "$OUT/diff.txt"; then
    echo "差分なし"
  else
    echo "差分あり → $OUT/diff.txt"; head -40 "$OUT/diff.txt"
  fi
fi

echo "ログ: $OUT"
exit $status
