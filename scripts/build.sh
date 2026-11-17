#!/usr/bin/env bash
# フォージのバイナリを作る。
#   bash scripts/build.sh        本番用: x86_64-unknown-linux-musl の静的バイナリ → ./forge-linux-x64（これだけをコミットする）
#   bash scripts/build.sh dev    ローカル採点用: このマシンのネイティブ → ./forge-<platform>-<arch>
#                                （Linux x64 では本番用と同じものになる。Mac では forge-darwin-arm64、コミットしない）
# リンカ: Linux x64 では rustc 同梱の musl で足りる。Mac からの本番ビルドは cargo-zigbuild を使う（START_HERE.md）。
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
MAX=8388608          # 1ファイルの上限 8 MiB（competition.yaml max_file_bytes）
WARN=$((6 * 1024 * 1024))

host_os="$(uname -s)"; host_arch="$(uname -m)"
mode="${1:-release}"

if [ "$mode" = "dev" ]; then
  case "$host_os/$host_arch" in
    Linux/x86_64) mode=release ;;                         # 本番用と同じものを作る
    Darwin/arm64) name=forge-darwin-arm64 ;;
    Darwin/x86_64) name=forge-darwin-x64 ;;
    Linux/aarch64) name=forge-linux-arm64 ;;
    *) echo "未対応のホスト: $host_os/$host_arch" >&2; exit 2 ;;
  esac
  if [ "$mode" = "dev" ]; then
    cargo build --release
    cp target/release/forge "$name"; chmod +x "$name"
    echo "dev: ./$name ($(wc -c < "$name") bytes)。コミットしないこと"
    exit 0
  fi
fi

TARGET=x86_64-unknown-linux-musl
rustup target list --installed 2>/dev/null | grep -qx "$TARGET" || rustup target add "$TARGET"
if [ "$host_os/$host_arch" = "Linux/x86_64" ]; then
  cargo build --release --target "$TARGET"
elif command -v cargo-zigbuild >/dev/null 2>&1; then
  cargo zigbuild --release --target "$TARGET"
else
  echo "Mac からの本番ビルドには cargo-zigbuild が要ります: brew install zig && cargo install cargo-zigbuild" >&2
  exit 2
fi
cp "target/$TARGET/release/forge" forge-linux-x64
chmod +x forge-linux-x64
bash "$REPO/scripts/check-binary.sh" --file forge-linux-x64
size=$(wc -c < forge-linux-x64)
[ "$size" -gt "$WARN" ] && echo "WARN: forge-linux-x64 が ${size} bytes。上限 8 MiB に近い"
echo "release: ./forge-linux-x64 (${size} bytes)"
