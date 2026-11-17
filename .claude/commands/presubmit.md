---
description: 提出前の最終確認（規約・作業記録・決定性・版番号）
---

1. `determinism-auditor` と `submission-auditor` を同時に起動する。
2. 両方の結果をまとめ、次の形で私に報告する:
   - NG の項目（直し方つき）。人間が操作する項目は分けて書く
   - そのまま提出してよいかの判断
3. 私が了承したら、version-registry スキルの手順で版を上げ、`package.json` と台帳を更新して commit する。
4. 提出コマンドそのものは実行しない（ウォレットの署名が必要なため、私が行う）。必要なコマンドを `docs/SUBMIT.md` から書き出して渡す。

$ARGUMENTS
