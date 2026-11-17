# 起動・登録・認証・export の仕様

`spec-analyst` がキットから抽出・補完する。推測は書かない。根拠（ファイル名:行）を付ける。
下の「確認済み」はリードが再構築時（2026-11-17）にキットで確かめた事実。

## 確認済み
- 起動: 採点は `node /bot/main.ts`（Node 22）。`/bot` は読み取り専用、書き込みは `/tmp` のみ（RULES.txt「Runtime and sandbox」、practice/trial-0/input/prompt.txt「Limits」）。本リポジトリでは main.ts が `forge-linux-x64` を起動する。
- 環境変数（RULES.txt「A trial」）: `MAKEFUN_ENV_URL`（env の origin、例 http://store:8080）、`MAKEFUN_SHOP_TOKEN`（Bearer トークン）、`MAKEFUN_TRIAL_NOW`（インスタンスの開始時刻。日時はここから）、`MAKEFUN_TRIAL_SEED`。
- ライフサイクル（starter/main.ts、RULES.txt「The suite and the wire」）: ポート0で bind（8080 禁止）→ `POST $MAKEFUN_ENV_URL/register {"port":N}`（200。409 は先に誰かが登録済みで待ち続ける。400/405 は契約違反）→ `GET $MAKEFUN_ENV_URL/done` を `{"done":true}` まで即答ポーリング → exit 0。env はリクエストの送信元アドレス＋ポートにつなぐ。全員 0.0.0.0 に bind。
- 予算（program/env/src/wire.ts:300-316）: boot 20 s（登録まで）、/healthz を2回 10 s 以内、conformance 240 s、benchmark 240 s、check 1件 15 s、ベンチ1種 60 s。壁時計 900 s。
- 認証: `/api/*` と `/mcp` は `Authorization: Bearer $MAKEFUN_SHOP_TOKEN`、無ければ 401。`/healthz` と git は認証なし（git-client.ts:1344, 2080 はリポジトリ作成の REST だけトークンを送る）。
- state export: env が `<stateDir>/export/state.json`（合否ビットと件数だけ、キーソートの正準 JSON）と `perf.json`（計測値）を書く（program/env/src/export.ts 冒頭）。フォージ側に export のエンドポイントはない。
- model_call: 不要（RULES.txt「This competition does not require model calls (harness.model_calls: optional)」）。
- ローカル採点: `bash scripts/grade.sh`（内部で `node cli/makefun.mjs grade-local --kit . --bot <一時リポジトリ> --driver process`）。

## spec-analyst が補完する
- 各インターフェースのリクエストの細部（ヘッダ、ボディ形式、タイムアウト）
- viewer・stranger のログインなど suite の固定値と、その根拠
- 不明点（該当ファイル:行つき）
