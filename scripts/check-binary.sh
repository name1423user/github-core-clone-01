#!/usr/bin/env bash
# 提出用バイナリの検査（/presubmit と build.sh が使う）。
#   bash scripts/check-binary.sh              コミット済み（HEAD）の forge-linux-x64 を検査する
#   bash scripts/check-binary.sh --file PATH  手元のファイルだけを検査する（build.sh 用）
# 検査: x86-64 の ELF・静的リンク・8 MiB 以下。HEAD 検査ではさらに、実行ビット（100755）、
#       開発用バイナリがコミットされていないこと、バイナリがソースより古くないこと。
set -uo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
MAX=8388608
ng=0
fail() { echo "NG  $*"; ng=1; }
ok() { echo "OK  $*"; }

check_elf() { # $1 = ファイル, $2 = 表示名
  local desc size
  desc="$(file -b "$1")"
  size=$(wc -c < "$1")
  echo "    file: $desc"
  [[ "$desc" == ELF\ 64-bit\ LSB* && "$desc" == *x86-64* ]] && ok "$2 は x86-64 の ELF" || fail "$2 が x86-64 の ELF ではない（Mac 版などの誤コミット？）"
  [[ "$desc" == *"statically linked"* || "$desc" == *"static-pie linked"* ]] && ok "$2 は静的リンク" || fail "$2 が静的リンクではない"
  [ "$size" -le "$MAX" ] && ok "$2 は ${size} bytes（≤ 8 MiB）" || fail "$2 が ${size} bytes（> 8 MiB）"
}

if [ "${1:-}" = "--file" ]; then
  check_elf "$2" "$2"
  exit $ng
fi

entry="$(git ls-files -s forge-linux-x64)"
if [ -z "$entry" ]; then
  fail "forge-linux-x64 がコミットされていない（bash scripts/build.sh → git add forge-linux-x64）"
  exit 1
fi
[[ "$entry" == 100755* ]] && ok "forge-linux-x64 の実行ビット（100755）" || fail "forge-linux-x64 に実行ビットがない（git update-index --chmod=+x forge-linux-x64）"

tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
git show HEAD:forge-linux-x64 > "$tmp"
check_elf "$tmp" "HEAD:forge-linux-x64"

others="$(git ls-files | grep -E '^forge-' | grep -vx 'forge-linux-x64' || true)"
[ -z "$others" ] && ok "開発用バイナリはコミットされていない" || fail "開発用バイナリがコミットされている: $others"

src_t=$(git log -1 --format=%ct -- src Cargo.toml Cargo.lock 2>/dev/null || echo 0)
bin_t=$(git log -1 --format=%ct -- forge-linux-x64)
[ "${bin_t:-0}" -ge "${src_t:-0}" ] && ok "バイナリはソースの最新コミット以降に更新されている" || fail "ソースがバイナリより新しい（bash scripts/build.sh して forge-linux-x64 もコミットする）"
if ! git diff --quiet HEAD -- src Cargo.toml Cargo.lock; then
  fail "src/ か Cargo.* に未コミットの変更がある"
fi
exit $ng
