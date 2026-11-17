# 設計判断の記録（リードが書く）

1件ずつ: 日付・決めたこと・理由・捨てた案。

- 2026-11-17: **本体を Rust の静的バイナリ（x86_64-unknown-linux-musl）にし、main.ts はランチャーにする。** 理由: RULES.txt「a statically linked Linux amd64 binary you commit runs from your repository as vendored source」、competition.yaml「A vendored git library or a static ELF is legal」。上位（953〜966点）はほぼ Go/C の静的バイナリで、feature・agent を満点にして perf で競っている。Node では perf が届かない。捨てた案: Node/TypeScript の標準ライブラリだけで書く（旧方針）、Python（ローカル採点が Docker 経由になる）。
- 2026-11-17: ポートの bind・登録・/done のポーリングはバイナリ側（`src/main.rs`）が行う。starter の main.ts のライフサイクルをそのまま移植した。ランチャーはシグナルの転送と終了コードの中継だけ。リスク: プロセスは Node＋バイナリの2つになる（ルールの「one process」は1つのフォージという意味に読む。上位の静的バイナリ勢も同じ構成のはず）。
- 2026-11-17: **外部クレートを使わない（std のみ）。** 理由: (1) ルールは「標準ライブラリ＋自分でコミットしたソース」で、クレートは vendor してコミットする必要がある、(2) vendor したソースは作業記録審査の counted lines に入り、AI セッション由来 30% 以上（thinking_reproduces）を割る危険がある。HTTP・JSON・SHA-1・zlib・GraphQL は自作。捨てた案: tokio/hyper/serde/flate2 を vendor。
- 2026-11-17: 提出バイナリの名前は `forge-linux-x64`（`forge-${process.platform}-${process.arch}` の規則）。Mac 用は `forge-darwin-arm64` で .gitignore。`FORGE_BIN` で上書き可。実行ビットが落ちていたら、ランチャーが /tmp に写して chmod して起動する（/bot は読み取り専用）。
- 2026-11-17: ローカル採点は `scripts/grade.sh` が一時リポジトリを作って回す。理由: `grade-local --worktree` は「追跡中＋未追跡（.gitignore 対象外）」だけを拾う（cli/makefun.mjs の `ls-files --others --exclude-standard`）ので、.gitignore された Mac 用バイナリが入らない。
- 2026-11-17: Mac からのクロスコンパイルは cargo-zigbuild に統一する（musl-cross は使わない）。Linux x64 では rustc 同梱の musl で足りる（このコンテナで確認済み: 512 KB、static-pie）。
- 2026-11-17: 版番号ファイルは `Cargo.toml` の `version`。`package.json` から version を外した（二重管理しない）。台帳の 0.1.0 を引き継ぐ。
- 2026-11-17: model_call は入れない。RULES.txt「This competition does not require model calls (harness.model_calls: optional)」。
- 2026-11-17: state export は suite（env）が書く（`program/env/src/export.ts`）。フォージは export を持たない。決定性の要件は「同じ入力に同じ応答を返す」こと（ID はカウンタ、時刻は MAKEFUN_TRIAL_NOW、反復する集合は BTreeMap/Vec）。
- 2026-11-17: トークンの持ち主のログインは封印された env の定数（`wire.ts` の `SCENARIO_SEED` → users[0]）で、インスタンスに依存しない。実装時に `wire.ts` から確かめて使う（practice の具体値ではない）。
- 2026-11-17: 認証は `/api/*` と `/mcp` だけに要求する。git-client はトークンを送らない（REST でのリポジトリ作成の補助のときだけ送る）。
- 2026-11-17: キットはリポジトリの外（`../github-core-clone-01-kit`）に置く。サイズ上限と受付審査のため。
- 2026-11-17: 持ち場を分けて並列実装する（CLAUDE.md §3）。ストア・HTTP・JSON はリードだけが変更する。ビルド担当（build-engineer）を新設。
