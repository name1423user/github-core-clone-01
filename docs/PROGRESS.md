# 進捗ログ（新しいものが上）

| 日時 | 版 | 合格/総数 | スコア | push | clone | issue読 | issue書 | 決定性 | メモ |
|---|---|---|---|---|---|---|---|---|---|
| 2026-11-18 16:31 | 0.3.0 | 120/120 | **992** (f600 a200 p192) | 50 (219ms) | **46 (84.6ms)** | 50 (654ms) | **46 (108.7ms)** | replay 一致 | **docker ドライバ（2CPU/1024MB/256pids、本番相当）**。process では 1000 だが本番条件で perf 満点割れ |
| 2026-11-18 16:17 | 0.3.0 | 120/120 | 1000 (f600 a200 p200) | 50 (p50 188/205/175ms) | 50 (40/41/42ms) | 50 (340/331/410ms) | 50 (87/63/81ms) | replay 一致×3 | 自作アロケータ（perf）、practice 3 インスタンス、p50 は trial0/1/2 |
| 2026-11-18 13:20 | 0.1.0 | 120/120 | 1000 (f600 a200 p200) | 50 | 50 | 50 | 50 | replay 一致×3 | graphql 完成、practice 3 インスタンス |
| 2026-11-18 13:10 | 0.1.0 | 101/120 | 905 (f505 a200 p200) | 50 | 50 | 50 | 50 | replay 一致 | git/REST/MCP 全合格、graphql 1/20 |
| 2026-11-18 08:20 | 0.1.0 | – | 15 (working 1) | – | – | – | – | replay 一致 | Rust 土台（ライフサイクル）のみ |

## 作業キュー（1項目＝1コミット。どこで止まっても次のセッションはここの先頭から再開する）

- [x] キット取得・台帳照合（0.1.0 一致）・Rust 構成を main にマージ（PR #1）
- [x] `src/util/json.rs`
- [x] spec-analyst の結果（INTERFACES/CHECKS/SCORING）をコミットし、るりあに4点を報告
- [x] `src/store/` 型と ID カウンタ・日時（MAKEFUN_TRIAL_NOW）・node_id
- [x] `src/store/` repos / issues / labels / comments の操作関数
- [x] `src/store/` PR・git オブジェクト・ref の置き場（git-engineer と型を合わせる）
- [x] router: 壊れた JSON → 400、共通エラーのヘルパ
- [x] `cargo test` → `build.sh` → `grade.sh` → バイナリごとコミット
- [x] `/parallel` 第1波: rest・git・mcp を並列（2026-11-18 12:50 JST 開始。担当の成果は未コミットの可能性 → `git status` で確認し、`cargo test` が通ればコミット）
- [x] `/parallel` 第2波: graphql（REST・git の後）
- [x] 120/120（0.3.0 に版上げ）
- [x] perf 第1弾: 自作アロケータ（9ae97dd）
- [ ] perf 第2弾: docker ドライバ基準で clone・write を満点に戻す（docs/HANDOFF_MAC.md §3 の手順で計測）
- [ ] Mac: 作業記録（トランスクリプト）の確認と提出準備（docs/HANDOFF_MAC.md §1、るりあ）
