<!-- TODO-README: キットの starter/README.md（README テンプレ）で、このファイルを丸ごと置き換える。
     バッジの URL は必ずテンプレのものを使う（下の3つは形の目安。OWNER/REPO は自分のものに）。 -->

[![make.fun bounty](https://make.fun/badge/github-core-clone-01.svg)](https://make.fun/bounty/github-core-clone-01)
[![rank](https://make.fun/badge/github-core-clone-01/name1423user/github-core-clone-01/rank.svg)](https://make.fun/bounty/github-core-clone-01)
[![reward](https://make.fun/badge/github-core-clone-01/name1423user/github-core-clone-01/reward.svg)](https://make.fun/bounty/github-core-clone-01)

# GitHub-compatible git forge

make.fun バウンティ [github-core-clone-01](https://make.fun/bounty/github-core-clone-01) への応募作。
UI を持たない単一プロセスのフォージで、git smart HTTP・REST v3・GraphQL・MCP を自前のストアから提供します。
本体は Rust（標準ライブラリのみ）の静的バイナリ `forge-linux-x64` で、`main.ts` はそれを起動するだけのランチャーです。外部サービスや git バイナリは使いません。

## Run it

```sh
bash scripts/build.sh   # forge-linux-x64 をビルド（Mac は bash scripts/build.sh dev でローカル用）
MAKEFUN_ENV_URL=http://<suite> node main.ts
```

## License

MIT
