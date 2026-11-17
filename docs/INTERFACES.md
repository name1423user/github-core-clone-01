# 起動・登録・export の仕様

`spec-analyst` がキットから抽出した。根拠はキット内のパス:行（`$KIT` = `../github-core-clone-01-kit`）。
`env/src/X` は `program/env/src/X`（`sdk/conformance/X` とバイト一致。`diff -r` で確認済み）。

## 0. 先に知っておくべき3点（CLAUDE.md の想定と違うところ）

1. **state export を書くのは entry ではなく env（suite）**。entry に export 用のエンドポイントは無い。
   env が `<stateDir>/export/state.json`（チェックごとの 0/1 と件数だけ）と `perf.json`（計測時間）を書く
   （env/src/export.ts:4-7, env/src/main.ts:769-781）。replay で比較されるのは state.json だけで、中身は合否ビットだけ
   （RULES.txt:51, env/src/export.ts:9-16）。
   → entry の決定性で守るべきことは「**同じ入力に毎回同じ合否になること**」。タイムアウトすれすれの遅さや競合で
   合否が揺れると、replay の不一致で **nondeterministic（順位なし）** になる（RULES.txt:67, 184）。
2. **ref 更新は原子的にしない**。1回の push に fast-forward の ref と非 fast-forward の ref が混ざっていたら、前者は適用して
   `ok`、後者は `ng <ref> non-fast-forward` と1行ずつ返す（RULES.txt:38, env/src/checks-git.ts:890-925）。
3. **トークンからログイン名は分からない**。トークンは `shpat_` + 32 hex（harness/harness-src.tar.gz の grader/agent.ts:790）。
   suite が「トークンの持ち主」とみなすのは scenario の viewer で、`wire.ts` の固定シード
   `SCENARIO_SEED = 0x6a09e667f3bcc909n`（env/src/wire.ts:750-752、「Never change it」）から決まる **`adaiversen`**
   （env/src/wire.ts:1263-1266 の `users[0]`。`buildScenario()` を実行して確認）。
   practice の3インスタンスも scenario_seed は同じ `6a09e667f3bcc909`（practice/trial-*/reference/scenario.json）。
   GraphQL `viewer.login` はこれと完全一致が必要（env/src/checks-graphql.ts:843-849）。
   他人のログイン（stranger = `olgabauer`）配下の同名リポジトリは 404（env/src/checks-rest.ts:734-744）。
   → ログイン名の決め方（固定値で持つか、wire.ts と同じ生成器を移植するか）はリードが決める（docs/DECISIONS.md）。

## 1. 起動方法

- 実行コマンド: `node /bot/main.ts`（Node 22）。リポジトリ直下の `main.ts`（RULES.txt:80, competition.yaml:322）。
  starter は `node --experimental-strip-types main.ts` を推奨（Node 22.18 以上なら不要）（starter/README.md「Run it」, starter/package.json:12）。
- インストール工程なし。標準ライブラリとコミットしたソースのみ（RULES.txt:80）。
- `/bot` は読み取り専用。書き込みは `/tmp` のみ（practice/trial-0/input/prompt.txt「Limits」, starter/README.md「Rules and limits」）。
- stdin は閉じている（RULES.txt:167）。
- entry が受け取る環境変数（RULES.txt:169-181）:

| 変数 | 値・用途 |
|---|---|
| `MAKEFUN_ENV_URL` | env の origin（コンテナでは `http://store:8080`）。登録先と `/done` のポーリング先 |
| `MAKEFUN_SHOP_TOKEN` | suite が `Authorization: Bearer <token>` で出すトークン（RULES.txt:17） |
| `MAKEFUN_TRIAL_SEED` / `MAKEFUN_TRIAL_NOW` | トライアルのシードと開始時刻。使ってよい（決定的） |
| `MAKEFUN_PROMPT_FILE` | `/input/prompt.txt` |
| `ANTHROPIC_*`, `OPENAI_*` | ゲートウェイ。本競技では使わない（§6） |
| `MAKEFUN_SHOP_GRAPHQL_URL`, `MAKEFUN_SUPPLIER_URL` | store 用。本競技では意味なし（RULES.txt:181） |

- 上限: 2 CPU、1024 MB、256 プロセス、900 秒（rules/instance.yaml:limits, RULES.txt:167）。suite も同じ枠を共有する（RULES.txt:80）。

## 2. ポート登録の正確な手順

starter/main.ts をそのまま使えばよい（現在の `main.ts` も同じ構造）。

1. `MAKEFUN_ENV_URL` が空なら exit 1（starter/main.ts:21-25）。
2. `server.listen(0)`（host 指定なし）で空きポートを取る。**8080 は bind しない**（RULES.txt:10, starter/main.ts:82-87）。
   env は「register リクエストの送信元アドレス + 申告ポート」をベース URL にする（env/src/main.ts:318, env/src/wire.ts:198-203）。
   split レイアウトでは送信元が entry ポッドの IP になるので、loopback 限定の bind にしない（env/src/wire.ts:21-23）。
3. `POST $MAKEFUN_ENV_URL/register`、ボディ `{"port": N}`（JSON、1..65535 の整数、64 KiB 以下）
   （env/src/wire.ts:134, 159-170）。応答: 200 `{"ok":true}`、2回目以降は 409、ボディ不正は 400、メソッド違いは 405
   （env/src/main.ts:299-329, env/src/wire.ts:228-245）。starter は 200/409 で成功、400/405 で終了、それ以外は再試行
   （starter/main.ts:45-61）。
4. `GET $MAKEFUN_ENV_URL/done` を短いバックオフ（250 ms から最大 2 s）でポーリング。env は即答で `{"done":false}`、
   export 書き込み後 `{"done":true}`（env/src/main.ts:331-338, env/src/wire.ts:279-285, starter/main.ts:64-77）。
5. `{"done":true}` で server を閉じて exit 0（starter/main.ts:93-100）。途中で落ちる・抜けると、その時点以降のチェックは全滅。

### ヘルスチェック（boot）
- register から最大 20 s（BOOT_BUDGET_MS）待つ。来なければ全チェック 0、working 0（env/src/wire.ts:300, env/src/main.ts:627-643）。
- register 後、env が `GET /healthz` を2回（HEALTHZ_ATTEMPTS）、合計 10 s 以内に送る。**200 が2回**で bootOk
  （env/src/wire.ts:312-314, env/src/main.ts:644-665）。`/healthz` は認証なし（starter/README.md「The surface」）。
  ボディは starter では `{"ok":true}`（starter/app.ts:35-38）。env はステータスしか見ない（env/src/main.ts:658）。
- bootOk が false でもチェックは走るが、`working` 列が 0 になる（env/src/main.ts:819-828, env/src/wire.ts:706-708）。

## 3. 実行の順序と時間

env/src/main.ts:9-38, 805-838。
1. ready → boot（上記）
2. GraphQL 用 git フィクスチャを設置（env/src/main.ts:669-682）
3. 適合チェック 120 個を**登録順に1つずつ**（git 30 → repos 15 → issues 15 → pulls 20 → protocol 5 → graphql 20 → mcp 15）
   （env/src/main.ts:622-624, 697-735）。フェーズ上限 240 s（env/src/wire.ts:302）。
4. ベンチ 4 本（push → clone → read → write）（env/src/manifest.ts:351-388, env/src/bench.ts のループ）。フェーズ上限 240 s、
   各ワークロード 60 s（env/src/wire.ts:304, env/src/manifest.ts:359）。
5. export → `/done` が true。

内部デッドラインは 900 s − 90 s = 810 s（env/src/wire.ts:308-310）。

## 4. タイムアウト

| 対象 | 値 | 根拠 |
|---|---|---|
| チェック1個 | 既定 15 s、5〜15 s にクランプ（各チェックが個別指定あり） | env/src/wire.ts:316-320, env/src/manifest.ts:200-205 |
| REST 1リクエスト（checks-rest） | min(チェック時間, 5 s) | env/src/checks-rest.ts:288 |
| git 1リクエスト | min(チェック時間, 8 s) | env/src/git-client.ts:40, env/src/checks-git.ts:263 |
| MCP 1リクエスト | **3 s** | env/src/mcp-client.ts:61, RULES.txt:44 |
| GraphQL 1リクエスト | チェックの残り時間 | env/src/checks-graphql.ts:637-643 |
| ベンチ1ワークロード | 60 s（予算超過は 0 点） | env/src/manifest.ts:359 ほか |

タイムアウトしたチェックは 0（"deadline"）で、次のチェックは最大 1 s の drain の後に始まる（env/src/main.ts:548-583）。

## 5. 認証と共通の応答規約

- すべての API（REST・GraphQL・MCP・git）に `Authorization: Bearer $MAKEFUN_SHOP_TOKEN` が付く（RULES.txt:17, env/src/rest-client.ts:「endpoint contract」,
  env/src/git-client.ts:1344, env/src/mcp-client.ts:671）。
- トークンなし → REST は 401 JSON `{message}`（env/src/checks-rest.ts:728-732）、GraphQL は **ちょうど 401**（env/src/checks-graphql.ts:1152-1158）、
  MCP の tools/call は HTTP 401（env/src/checks-mcp.ts:957-979）。git をトークンなしで叩くチェックは無い。
- **誤ったトークン**を送るチェックは見当たらない（`withToken`/`anon` はトークンなしのみ）。拒否するかは不明（チェック対象外）。
- REST の応答はすべて `application/json`（HTML 禁止）、空でない `X-GitHub-Request-Id` ヘッダ（401・404 含む）、
  エラーは `{"message": string}`（RULES.txt:17, env/src/checks-rest.ts:1193-1250）。`X-RateLimit-*` は不要。
- 未知ルートは 404 JSON（REST 以外も `{"message":"Not Found"}`）（RULES.txt:34, practice/trial-0/input/prompt.txt「The surface」）。
- リクエストボディの `Content-Encoding: gzip` を送るクライアントは無い（env/src 全体に gzip の記述なし）。

## 6. `model_call` は必須か → **必須ではない（入れない）**

RULES.txt:149 より引用:

> This competition does not require model calls (its harness says model_calls: optional), which waives the LLM agent floor:
> an entry may make no model call (a scripted program is a legal entry), and no trial of it scores 0, nor is a run of it
> refused, for that

competition.yaml:345 も `model_calls: optional`。starter も「no model call」（starter/main.ts:17）。

## 7. state export（env が書く。参考）

- パス: `<stateDir>/export/state.json`、`<stateDir>/export/perf.json`。stateDir はコンテナで `/state`、process driver では
  `MAKEFUN_STATE_DIR`（env/src/wire.ts:76, 90-99, 119-123）。
- state.json: `{version:1, working, checks:{<id>:0|1}, counts:{total:120, passed, byGroup}, perfTiers:{push:0,clone:0,read:0,write:0}, notes?}`。
  キーは全階層ソート、末尾改行（env/src/export.ts:4-7, 63-68, env/src/wire.ts:684-701）。perfTiers は常に 0 固定（時間を入れないため）。
- perf.json: ワークロードごとの rounds・p50・points、`points_total`、`perf_p50_us`（env/src/wire.ts:597-608, env/src/bench.ts 冒頭コメント）。
- 書き込みは「既存ファイルより悪いなら上書きしない」（env/src/export.ts:18-22）。
- replay で比較されるのは state.json のバイト列（RULES.txt:184）。

## 8. ローカル採点

- `node $KIT/cli/makefun.mjs grade-local --kit $KIT --bot <repo> --driver process` で動く（2026-11-17 に starter と本リポジトリで実行確認。
  starter は 3/120 合格・約 4 秒で終了）。`--driver docker` が既定（RULES.txt:153）。
- **注意1: grade-local は既定で HEAD のコミットを採点する**（`--worktree` で作業ツリー）（`grade-local --help`）。
  `scripts/grade.sh` は `--worktree` を付けていないので、**未コミットの変更は採点されない**。`bash scripts/grade.sh --worktree` で回避できる
  （追加引数はそのまま grade-local に渡る）。
- **注意2: grade-local はチェックごとの合否・理由を表示しない**（保存先 `trial-0/stderr.txt` は空、`state/state.json` に 0/1 だけ残る）。
  理由を見るには SDK ランナーを使う:
  ```sh
  node -e "import('./src/http.ts').then(m=>m.createApp().listen(18765,'127.0.0.1'))" &
  node $KIT/sdk/run.ts --url http://127.0.0.1:18765 --token shpat_test        # 表
  node $KIT/sdk/run.ts --url http://127.0.0.1:18765 --token shpat_test --json # JSON
  node $KIT/sdk/run.ts --url http://127.0.0.1:18765 --token shpat_test --perf # ベンチも
  ```
  （sdk/run.ts:1-17, 46-52）。サーバはチェックごとに再起動しないので、2回目の実行では「既に存在する」系の差が出る点に注意。
- ローカル機は本番（gVisor、共有ノード）より速い。calibration.json:83-91（"previous"."finding"）によると静かな検証機では同じハウス実装が本番の 2.5〜4.7 倍速かった。
  **ローカルの perf 点は過大評価になる**。
