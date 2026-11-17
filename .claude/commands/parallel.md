---
description: 4つのインターフェース担当を並列に動かし、落ちているチェックを一気に潰す
---

`docs/CHECKS.md` で状態が `TODO` または `FAIL` のチェックを面ごとにまとめ、次のサブエージェントを**同時に**起動してください（1つのメッセージで複数の Agent 呼び出しを出す）。

- `git-engineer`: 面=git のチェック（protocol のうち git に関わるもの）
- `rest-engineer`: 面=repos・issues・pulls のチェック（protocol のうち REST に関わるもの）
- `graphql-engineer`: 面=graphql のチェック
- `mcp-engineer`: 面=mcp のチェック

各エージェントへの指示に含めること:
- 担当するチェックIDの一覧と、`docs/CHECKS.md` の該当行
- 持ち場以外は編集しないこと。ストア・HTTP・JSON・共有の型の変更が必要なら、止めて報告すること
- **crate は全員で共有している**: 終える前に `cargo build --release && cargo test` を通すこと。他人の持ち場のコンパイルエラーは直さず報告すること
- 外部クレートを足さないこと
- 終わったら、通したID・残りのID・必要なストア変更を報告すること

全員が戻ったら:
1. 報告の「必要なストア変更」をリードとして反映し、`cargo build --release && cargo test` を通す。
2. `grader` サブエージェントで採点し（`bash scripts/grade.sh`）、`docs/CHECKS.md` を更新する。
3. まだ落ちているチェックがあれば、このコマンドの手順をもう一周する。後退（前回 PASS→今回 FAIL）があれば最優先で直す。
4. 区切りごとに、`bash scripts/build.sh` で `forge-linux-x64` を作り直し、**ソースと同じコミットに**入れて git commit する（`bash scripts/check-binary.sh` が OK であること）。

$ARGUMENTS
