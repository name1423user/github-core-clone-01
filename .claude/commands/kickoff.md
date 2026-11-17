---
description: 最初の1回。キットの確認→仕様抽出→台帳照合→土台づくりまでを進める
---

make.fun バウンティ `github-core-clone-01` の作業を始めます。CLAUDE.md を読んでから、次の順で進めてください。

1. キットの存在確認: `ls ${KIT:-../github-core-clone-01-kit}`。無ければ止めて「`bash scripts/fetch-kit.sh` を実行して」とだけ伝える。
2. version-registry スキルで台帳と `package.json` の `version` を照合する。未登録なら、GitHub のリポジトリURLが分かった時点で新規登録するよう、私に確認する。
3. `spec-analyst` サブエージェントに、キットの読解と `docs/INTERFACES.md`・`docs/CHECKS.md`・`docs/SCORING.md` の作成を任せる。
4. その結果を読み、私に次を短く報告する:
   - 採点式の要点と、点が多く乗る場所
   - `model_call` が必須かどうか
   - ポート登録・export の仕様
   - 実装の優先順位
5. 私の了承を待たずに、土台（`main.ts` のポート登録、`src/http.ts` のルーター、`src/store/` のストアと決定的 export、`src/types.ts`）を作る。キットの `starter/` があれば、それを出発点にする。
6. `bash scripts/grade.sh` で起動と登録が通ることを確認し、`/parallel` に進める状態にする。

$ARGUMENTS
