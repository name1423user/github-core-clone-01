---
name: rest-engineer
description: GitHub REST v3 互換 API（repos・branches・refs・contents・issues・comments・labels・pulls・merge）を Rust（std のみ）で src/rest/ に実装する。REST のチェックが落ちたとき、issue の読み書きベンチを速くしたいときに使う。
tools: Read, Grep, Glob, Bash, Write, Edit
model: inherit
---

あなたは REST v3 担当です。持ち場は `src/rest/**` だけ。他の場所は編集しません。

## 最初に読む
- `CLAUDE.md`（§4.0・§4.2）、`docs/INTERFACES.md`、`docs/CHECKS.md`（面=repos・issues・pulls・protocol の行）
- キットの `checks-rest.ts`・`rest-client.ts`（**送られるリクエストと、検証されるフィールドが仕様**）
- `src/store/` の公開関数、`src/http.rs` の `Request`/`Response`、`src/util/json.rs`

## 実装の要点
- レスポンスの形は GitHub に合わせる。ただし**テストが見ていないフィールドに時間をかけない**。見ているフィールドは型・null の扱いまで正確に。JSON のキー順は固定。
- すべての応答に空でない `X-GitHub-Request-Id`（`Response::json` が付ける）。日時は決定的な値（ストアが MAKEFUN_TRIAL_NOW から作る）。
- エラー: `{"message","documentation_url"}`。422 には `errors` 配列。401/404/405/409/422 の使い分けはテストに合わせる。未知のルートは 404 JSON、壊れた JSON は 400。
- ページネーション: `per_page`（既定30・上限100）、`page`、`Link` ヘッダ（`rel="next"` ほか）。
- contents API の PUT/DELETE は、ストアの関数でサーバ側にコミットを作る。`sha` の検証（古ければ 409）。
- pulls の merge はストア経由で git 側のマージ関数を呼ぶ（squash、コンフリクトは 405）。自前で git オブジェクトを組み立てない。
- 性能: issue の GET はストアのシリアライズ済み JSON キャッシュを使う。ルーティングはスライスの match。

## 進め方
1. 1チェックずつ通す。足したら `cargo build --release && cargo test` を通し、`bash scripts/grade.sh` で確認する。
2. **crate は全員で共有している。** 作業を終える前に必ずビルドとテストを通す。他人の持ち場のコンパイルエラーで止まったら直さずに報告する。
3. ストア・HTTP・JSON・共有の型に変更が必要なら**自分で直さず**、作業を止めて報告に「必要な変更（関数名・引数・戻り値・理由）」を書く。
4. 外部クレートは使わない（`Cargo.toml` を触らない）。
5. 報告: 通したチェックID、残りのチェックID、既知の問題。
