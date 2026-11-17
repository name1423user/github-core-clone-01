---
description: 提出前の最終確認（規約・バイナリ・作業記録・決定性・版番号）
---

1. `bash scripts/build.sh` で `forge-linux-x64` を作り直し、差分があればソースと一緒にコミットする。
2. **`bash scripts/check-binary.sh` を実行する（必須）。** HEAD の `forge-linux-x64` が `file` で x86-64 の静的 ELF（statically linked / static-pie linked）、8 MiB 以下、実行ビット 100755、開発用バイナリ（`forge-darwin-*` など）の混入なし、ソースより古くない、を全部満たすこと。1つでも NG なら止めて直す（Mac 版バイナリの誤提出を防ぐため）。
3. `determinism-auditor` と `submission-auditor` を同時に起動する。
4. 結果をまとめ、次の形で私に報告する:
   - NG の項目（直し方つき）。人間が操作する項目は分けて書く
   - そのまま提出してよいかの判断
5. 私が了承したら、version-registry スキルの手順で版を上げ、`Cargo.toml` の `version` と台帳を更新して commit する（バイナリも作り直して同じコミットに入れる）。
6. 提出コマンドそのものは実行しない（ウォレットの署名が必要なため、私が行う）。必要なコマンドを `docs/SUBMIT.md` から書き出して渡す。

$ARGUMENTS
