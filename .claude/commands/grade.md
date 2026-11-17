---
description: ローカル採点を回して、失敗を担当ごとに振り分ける
---

`grader` サブエージェントで `bash scripts/grade.sh $ARGUMENTS` を実行し、結果を `docs/CHECKS.md` と `docs/PROGRESS.md` に反映してください。

報告は次の形で短く:
- スコアと合格数/総数（前回比）
- 後退したチェック（あれば最初に）
- 担当ごとの失敗リスト
- ベンチ4種の値と決定性の判定
