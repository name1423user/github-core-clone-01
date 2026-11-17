---
description: 最初の1回。キットの確認→仕様抽出→台帳照合→Rust の土台づくりまでを進める
---

make.fun バウンティ `github-core-clone-01` の作業を始めます。CLAUDE.md を読んでから、次の順で進めてください。

1. キットの存在確認: `ls ${KIT:-../github-core-clone-01-kit}`。無ければ `bash scripts/fetch-kit.sh` を実行する（失敗したら止めて理由を伝える）。
2. ツールの確認: `node -v`（22）、`cargo --version`、`rustup target list --installed` に `x86_64-unknown-linux-musl`、Mac なら `cargo zigbuild --version`。足りなければ START_HERE.md §1〜3 の手順を示して止める。
3. version-registry スキルで台帳と `Cargo.toml` の `version` を照合する。未登録なら、GitHub のリポジトリURLが分かった時点で新規登録するよう、私に確認する。
4. `spec-analyst` サブエージェントに、キットの読解と `docs/INTERFACES.md`・`docs/CHECKS.md`・`docs/SCORING.md` の完成を任せる。
5. その結果を読み、私に次を短く報告する:
   - 採点式の要点と、点が多く乗る場所
   - `model_call` が必須かどうか
   - ポート登録・export の仕様
   - 実装の優先順位
6. 私の了承を待たずに、リードの持ち場の土台を Rust（std のみ）で作る: `src/util/json.rs`（パース・シリアライズ）、`src/store/`（リポジトリ・オブジェクト・ref・issue・PR・ラベル・コメント、ID カウンタ、MAKEFUN_TRIAL_NOW 由来の日時、node_id の生成、issue JSON のキャッシュ）、`src/http.rs`・`src/router.rs` の拡張（認証・共通エラー）。雛形の `src/main.rs`（ライフサイクル）は維持する。
7. `cargo build --release && cargo test` を通し、`bash scripts/grade.sh` で起動・登録（working 1）が通ることを確認する。`bash scripts/build.sh` で `forge-linux-x64` を作り直してソースと一緒にコミットし、`/parallel` に進める状態にする。

$ARGUMENTS
