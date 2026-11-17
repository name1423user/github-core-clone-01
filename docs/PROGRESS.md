# 進捗ログ（新しいものが上）

| 日時 | 版 | 合格/総数 | スコア | push | clone | issue読 | issue書 | 決定性 | メモ |
|---|---|---|---|---|---|---|---|---|---|
| 2026-11-18 08:20 | 0.1.0 | – | 15 (working 1) | – | – | – | – | replay 一致 | Rust 土台（ライフサイクル）のみ |

## 作業キュー（1項目＝1コミット。どこで止まっても次のセッションはここの先頭から再開する）

- [x] キット取得・台帳照合（0.1.0 一致）・Rust 構成を main にマージ（PR #1）
- [x] `src/util/json.rs`
- [ ] spec-analyst の結果（INTERFACES/CHECKS/SCORING）をコミットし、るりあに4点を報告
- [ ] `src/store/` 型と ID カウンタ・日時（MAKEFUN_TRIAL_NOW）・node_id
- [ ] `src/store/` repos / issues / labels / comments の操作関数
- [ ] `src/store/` PR・git オブジェクト・ref の置き場（git-engineer と型を合わせる）
- [ ] router: 壊れた JSON → 400、共通エラーのヘルパ
- [ ] `cargo test` → `build.sh` → `grade.sh` → バイナリごとコミット
- [ ] `/parallel`（git・rest・graphql・mcp を1担当ずつ、細かいチェック単位で）
