#!/usr/bin/env bash
# ローカル採点。使い方: bash scripts/grade.sh [grade-local に渡す追加オプション（例: --instances 3）]
#   TWICE=1 bash scripts/grade.sh   … 2回走らせてログを比較（決定性の目視確認用）
#
# 手順: (1) このマシン用のバイナリがソースより古ければ作り直す（build.sh dev）
#       (2) 追跡中＋未追跡（.gitignore 対象外）のファイルと、このマシン用のバイナリを一時ディレクトリに写して
#           1コミットの git リポジトリにする
#       (3) そこを --bot にして grade-local（process ドライバ）を回す
# (2) が要る理由: grade-local の --worktree は .gitignore されたファイルを拾わない。Mac 用の
# forge-darwin-arm64 は .gitignore 対象（コミット禁止）なので、そのままでは採点に入らない。
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

echo "== バイナリ =="
BIN="$REPO/forge-$(node -p 'process.platform + "-" + process.arch')"
newest_src=$(find "$REPO/src" "$REPO/Cargo.toml" -type f -newer "$BIN" 2>/dev/null | head -1)
if [ ! -x "$BIN" ] || [ -n "$newest_src" ]; then
  bash "$REPO/scripts/build.sh" dev || exit 1
else
  echo "$(basename "$BIN") は最新"
fi

STAGE="$(mktemp -d)/bot"
mkdir -p "$STAGE"
( cd "$REPO" && git ls-files -co --exclude-standard -z | tar --null -T - -cf - ) | tar -xf - -C "$STAGE"
cp -p "$BIN" "$STAGE/"
( cd "$STAGE" && git init -q && git add -A && git -c user.name=grade -c user.email=grade@localhost commit -qm stage ) || exit 1

runs=1; [ "${TWICE:-0}" = "1" ] && runs=2
status=0
for i in $(seq 1 $runs); do
  echo "== grade-local (run $i) =="
  ( cd "$KIT" && node cli/makefun.mjs grade-local --kit . --bot "$STAGE" --driver process "$@" ) 2>&1 | tee "$OUT/run$i.log"
  s=${PIPESTATUS[0]}; [ "$s" -ne 0 ] && status=$s
done

if [ "$runs" = "2" ]; then
  echo "== run1 / run2 の差分（時間・ポート番号の行は除外） =="
  filt() { grep -viE '(ms|sec|elapsed|duration|took|port|[0-9]+\.[0-9]+s|trials kept|kept|/tmp/)' "$1"; }
  if diff <(filt "$OUT/run1.log") <(filt "$OUT/run2.log") > "$OUT/diff.txt"; then
    echo "差分なし"
  else
    echo "差分あり → $OUT/diff.txt"; head -40 "$OUT/diff.txt"
  fi
fi

rm -rf "$(dirname "$STAGE")"
echo "ログ: $OUT"
exit $status
