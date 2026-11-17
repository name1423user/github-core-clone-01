# 設計判断の記録（リードが書く）

1件ずつ: 日付・決めたこと・理由・捨てた案。

- 2026-11-17: 言語は TypeScript（Node 22）。規定で Node 22 か Python 3.13 のみ。Python はローカル採点が Docker 経由になるため不採用。
- 2026-11-17: キットはリポジトリの外（`../github-core-clone-01-kit`）に置く。サイズ上限と受付審査のため。
- 2026-11-17: 持ち場を分けて並列実装する（CLAUDE.md の表）。ストアはリードだけが変更する。
- 2026-11-17: model_call は入れない。RULES.txt「This competition does not require model calls (harness.model_calls: optional)」。
- 2026-11-17: state export は env（suite）が書く（export.ts）。フォージ側に export 機能は不要。決定性の要件は「同じ入力に同じ応答を返す」こと（ID はカウンタ、時刻は MAKEFUN_TRIAL_NOW）。CLAUDE.md の「決定的 export」はこの意味に読み替える。
- 2026-11-17: トークンの持ち主のログインは、封印された env の定数（wire.ts の SCENARIO_SEED → users[0] = adaiversen）。インスタンスに依存しないので store/index.ts の VIEWER に置き、FORGE_LOGIN で上書きできるようにする。practice の値を埋め込んでいるわけではない。
- 2026-11-17: 認証は /api と /mcp だけに要求する。git-client はトークンを送らない（REST でのリポジトリ作成の補助のときだけ送る）。
- 2026-11-17: グレードは process ドライバに --worktree を付けて回す（付けないと HEAD のコミットが採点される）。
- 2026-11-17: main.ts はキットの starter をそのまま使い、import 先だけ src/http.ts にした。8080 を含むコメント2行は compliance に合わせて書き換えた。
