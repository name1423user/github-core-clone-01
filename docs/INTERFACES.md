# 起動・登録・認証・export・各クライアントのワイヤ仕様

`spec-analyst` がキット（`../github-core-clone-01-kit`）から抽出した。推測は書かない。根拠はキット内のパス:行。
`program/env/src/*` と `sdk/conformance/*` はバイト一致（2026-11-17 に cmp で確認）。以下 `env/` は `program/env/src/` の略。

---

## 0. 確認済み（リード記載分の検証結果）

| 項目 | 判定 | 根拠 |
|---|---|---|
| 起動は `node /bot/main.ts`（Node 22）。`/bot` 読み取り専用、書き込みは `/tmp` のみ | 正しい | RULES.txt:80, RULES.txt:167, practice/trial-0/input/prompt.txt「Limits」 |
| 環境変数 `MAKEFUN_ENV_URL`・`MAKEFUN_SHOP_TOKEN`・`MAKEFUN_TRIAL_NOW`・`MAKEFUN_TRIAL_SEED` | 正しい（他に `MAKEFUN_PROMPT_FILE` 等もある） | RULES.txt:167-181 |
| ライフサイクル（port 0 bind → register → /done ポーリング → exit 0） | 正しい | starter/main.ts:1-104, env/wire.ts:7-19, env/main.ts:295-342 |
| 予算（boot 20 s、/healthz 2回 10 s、conformance 240 s、benchmark 240 s、check 既定 15 s） | 正しい。補足: check の timeout は 5〜15 s に clamp、ベンチ1種 60 s、内部 deadline は 900 s − 90 s = 810 s | env/wire.ts:300-320, env/manifest.ts:199-205, env/manifest.ts:359-386 |
| 「git は認証なし（git-client.ts:1344, 2080 はリポジトリ作成の REST だけトークンを送る）」 | **誤り**。git-client.ts:1344 は `this.token` があれば**すべての git リクエスト**に `Authorization: Bearer` を付ける。checks・fixture・bench はすべて token 付きで GitClient を作る（checks-git.ts:259-264, env/main.ts:525-530, bench.ts:445-452）。**token なしの git リクエストを送るチェックは存在しない**ので、git で認証を要求するかどうかは採点に影響しない（どちらでも可） | env/git-client.ts:1337-1344 |
| state export は env が書く。フォージ側に export エンドポイントはない | 正しい | env/export.ts:1-25, env/main.ts:769-802 |
| model_call 不要 | 正しい | RULES.txt:149, competition.yaml:342-345 |

---

## 1. 起動とランタイム

- 実行コマンド: `node /bot/main.ts`（Node 22）または `python3 /bot/main.py`（RULES.txt:80, RULES.txt:149）。kit の CLI は Node が型ストリップ非対応なら `--experimental-strip-types` を付ける（cli/makefun.mjs:309374-309376）。starter/README.md:29-31 は「Node 22.18 以降は自前で型ストリップ」と書く。
- 静的リンクの Linux amd64 バイナリをコミットして起動してよい（RULES.txt:80, competition.yaml:338-341）。1ファイル 8 MiB、ツリー 16 MiB まで。
- 制限: 2 CPU、1024 MB、256 プロセス、壁時計 900 s。suite も同じポッドで動きリソースを共有する（RULES.txt:80, RULES.txt:167）。
- stdin は閉じている。ネットワークは env（`store`）とゲートウェイ（`llm`）のみ。全員 0.0.0.0 に bind（RULES.txt:167）。
- 環境変数（RULES.txt:169-179）: `MAKEFUN_ENV_URL`（例 `http://store:8080`）、`MAKEFUN_SHOP_TOKEN`、`MAKEFUN_TRIAL_SEED`、`MAKEFUN_TRIAL_NOW`、`MAKEFUN_PROMPT_FILE`、ほか store 用・ゲートウェイ用。
- トークンの値: ローカル採点では `shpat_` + 32 hex（試行から導出）（cli/makefun.mjs:223365）。本番での形式は不明（RULES.txt:181 は「instance と trial だけから導出」とのみ記載）。**トークンの値に依存しない実装にすること。**

## 2. ポート登録（env 側の実装に合わせた正確な手順）

1. `MAKEFUN_ENV_URL` が空なら starter は exit 1（starter/main.ts:21-25）。
2. ポート 0 で listen。8080 は禁止（RULES.txt:10, starter/main.ts:7）。
3. `POST $MAKEFUN_ENV_URL/register`、ヘッダ `content-type: application/json`、ボディ `{"port":N}`（starter/main.ts:48-53）。
   - env の検証: ボディ 65,536 バイト以下、JSON オブジェクト、`port` は 1..65535 の整数（余分なキーは無視）（env/wire.ts:129-171, env/main.ts:299-329）。
   - 応答: 200 `{"ok":true}`（最初の登録）、409 `{"ok":false,"error":"already_registered"}`（2回目以降。starter はこれも成功扱い）、400（ボディ不正）、405（POST 以外）（env/wire.ts:227-244）。
   - env は **register リクエストの送信元アドレス（`req.socket.remoteAddress`、`::ffff:` 除去）＋名乗ったポート**を base URL にする（env/wire.ts:179-202, env/main.ts:318）。
   - starter の再試行: 200/409 で終了、400/405 は致命、それ以外とネットワークエラーは指数バックオフ 250 ms→最大 2 s、1回 30 s タイムアウト（starter/main.ts:27-61）。
4. 登録後、env は `GET /healthz` を2回（10 s 以内）呼ぶ。両方 200 でないと `bootOk=false`（env/main.ts:644-665）。`bootOk=false` でも登録済みなら conformance は走るが、`working` は 0（env/wire.ts:706-708, env/export.ts:127）。ヘッダは `accept: application/json`、認証なし。応答ボディは見ていない（`{"ok":true}` は starter/app.ts:35-37 の形）。
5. 登録は boot 予算 20 s 以内（env/wire.ts:300, env/main.ts:627-638）。それを過ぎると全チェック 0。
6. `GET $MAKEFUN_ENV_URL/done` をポーリング。env は即答（1 s 以内）で `{"done":false}`、export 書き込み後 `{"done":true}`（env/main.ts:331-339, env/wire.ts:278-295）。starter は 250 ms→2 s のバックオフ（starter/main.ts:64-77）。
7. `{"done":true}` を受けたら server を閉じて exit 0（starter/main.ts:93-100）。

## 3. state export（フォージは何も書かない）

- env が `<stateDir>/export/state.json` と `perf.json` を書く。`stateDir` はコンテナ内 `/state`、外では `MAKEFUN_STATE_DIR`（env/wire.ts:89-96, env/wire.ts:26-33）。
- state.json: `{version:1, working, checks:{<id>:0|1}, counts:{total:120, passed, byGroup:{<group>:{total,passed}}}, perfTiers:{push:0,clone:0,read:0,write:0}}`、キーを全階層でソート、末尾改行（env/export.ts:4-16, env/export.ts:122-130, env/wire.ts:683-703）。**時刻・ポート・計測値は入らない**（perfTiers は常に 0 固定、env/export.ts:59-68）。
- perf.json: 各ワークロードの rounds・p50_ms・p99_ms・points、`points_total`、`perf_p50_us`（env/wire.ts:573-607, env/bench.ts:43-62）。
- 決定性の要件（フォージ側）: 2回の実行で**同じ合否ビット**になること。state.json はビットだけから決まる（env/export.ts:9-16）。
- 書き込みは「悪くならない時だけ上書き」（env/export.ts:164-177）。

## 4. 認証とユーザー（suite の固定値）

- suite のトークンは `MAKEFUN_SHOP_TOKEN`。`Authorization: Bearer <token>`（RULES.txt:17, env/wire.ts:81-82）。
- **「token 無し → 401」を確認するチェック**: repos.unauthorized、protocol.content-type-json（anon の JSON 401）、protocol.request-id（anon にも X-GitHub-Request-Id）、protocol.error-message、graphql.unauthenticated（**正確に 401**）、mcp.auth-required（HTTP 401）。**誤ったトークンを送るチェックは無い**（checks-*.ts を `wrong|withToken` で検索して該当なし）。
- **トークンの持ち主のログイン = `adaiversen`**（scenario.viewer.login）。これは practice 固有の値ではない: `SCENARIO_SEED = 0x6a09e667f3bcc909n` は wire.ts の定数（env/wire.ts:751-754「Never change it」）で、viewer は `generateUsers(scenarioRng('users'))[0]`（env/wire.ts:1263-1266）。practice 3 件の scenario.json はすべて `scenario_seed: 6a09e667f3bcc909`（practice/trial-*/reference/scenario.json）、generator も同じ定数（program/generator/draw.mjs:21, :441）。本番インスタンスでも同じ値になるとソースは述べている（competition.yaml:29-31「All scenario data derives from one constant seed in env/src/wire.ts」）。
  - suite はこのログインをトークンから問い合わせず、scenario から直接使う（checks-git.ts:265, checks-rest.ts:137, checks-graphql.ts:625, bench.ts:465-468）。MCP だけは `GET /api/v3/user` の `login` を先に試し、失敗時に scenario の値へ落ちる（checks-mcp.ts:395-400）。
  - 他の固定値: stranger = `olgabauer`（foreign-token の 404 確認、checks-rest.ts:138, :734-744）、teammate = `quinnkeller`、org = `faint-meadow-org`（未使用）。scenario の値は `node --experimental-strip-types` で wire.ts の `buildScenario()` を呼んで確認した。
- `POST /api/v3/user/repos` で作るリポジトリの owner はトークンのログイン（`adaiversen`）。同名を他 owner で引くと 404（403 ではない）（RULES.txt:17）。

## 5. REST v3 クライアント（env/rest-client.ts と各チェックの直叩き）

### 5.1 送るもの
- RestClient: `accept: application/vnd.github+json`、token があれば `authorization: Bearer`、JSON ボディには `content-type: application/json`。ボディは `JSON.stringify`（env/rest-client.ts:378-389）。リダイレクトは follow。タイムアウトは 1 回 1〜5 s（checks-rest.ts:288）。
- クエリは `URL.searchParams.set` で付ける（env/rest-client.ts:357-367）。パスは区切りごとに `encodeURIComponent`、ファイルパス内の `/` は残す（env/rest-client.ts:169-185）。
- malformed ボディは raw の `{"name":` を `content-type: application/json` で送る（checks-rest.ts:1218-1222）。
- DELETE /contents は JSON ボディ付き（checks-rest.ts:791）。
- GraphQL・MCP のチェックは自前の fetch で REST を叩く: `accept: application/vnd.github+json` または `application/json`、`user-agent` 付き、`authorization: Bearer`（checks-graphql.ts:462-476, checks-mcp.ts:337-350）。

### 5.2 応答の判定（全チェック共通）
- ステータスは**完全一致**（範囲ではない）（env/rest-client.ts:209-214）。
- `expectJsonResponse`: ステータス一致 → HTML でない（`text/html` でなく、本文が `<` で始まらない）→ content-type が `application/json`（`+json`・charset 可）→ JSON としてパース可 → shape 一致（env/rest-client.ts:225-254, :127-130）。
- shape は「必須キーが存在し型が合う」だけ。余分なキーは許される（env/rest-client.ts:298-314, `allowExtra` は既定 undefined）。
- エラー本文は `{"message": 非空文字列}`（checks-rest.ts:315）。
- `Link` は `<url>; rel="next"` を解析（env/rest-client.ts:152-165）。**next の URL は env が使った base URL と同じ origin（scheme・host・port）でないと失敗**（checks-rest.ts:669, env/rest-client.ts:139-145）。→ リクエストの Host ヘッダから組み立てる必要がある。

### 5.3 ルートと期待（全チェックが使うもの）
| メソッド・パス | 期待 | 根拠 |
|---|---|---|
| GET `/api/v3/` | 200 JSON オブジェクト、`X-OAuth-Scopes` ヘッダあり（値は問わない） | checks-rest.ts:593-598 |
| GET `/api/v3/meta` | 200 `{installed_version: 非空文字列}` | checks-rest.ts:600-603 |
| GET `/api/v3/user` | 200 `{login}`（MCP の owner 解決。失敗しても scenario 値で続行） | checks-mcp.ts:395-400 |
| POST `/api/v3/user/repos` `{name, private, description}`（git-client は `auto_init:false` も） | 201 `{name, private(bool, 送った値), default_branch?}`。**同名が既にあれば 422 で本文に `already exists`（大文字小文字無視）を含める**（git-client.createRepo が 422+`already exists` だけ成功扱い。GraphQL fixture・bench が同名で再作成する） | checks-rest.ts:396-401, :649-662, env/git-client.ts:2068-2109, checks-graphql.ts:684-691, env/main.ts:456 |
| GET `/api/v3/repos/{o}/{r}` | 200 `{id, name, private, default_branch(非空・読むたび同じ), description}`、無ければ 404 `{message}` | checks-rest.ts:605-631, checks-mcp.ts:917-923 |
| PATCH `/api/v3/repos/{o}/{r}` `{description}` | 200、`description` 更新済み、後の GET でも | checks-rest.ts:633-639 |
| DELETE `/api/v3/repos/{o}/{r}` | 204、後の GET は 404 JSON。存在しない時の DELETE は応答を見ない（bench） | checks-rest.ts:641-647, bench.ts:476-482 |
| GET `/api/v3/user/repos?per_page=10` | 200 配列 `[{name}]`、1ページ ≤10 件、`Link rel="next"` で全件たどれる、重複なし、3ページ以上、20ページ以内 | checks-rest.ts:664-684 |
| POST `/repos/{o}/{r}/git/refs` `{ref:"refs/heads/X", sha}` | 201 `{ref, object:{sha}}` | checks-rest.ts:461-472, checks-mcp.ts:461 |
| GET `/repos/{o}/{r}/git/refs/heads/{b}` または `/branches/{b}` | 200 で `object.sha` / `sha` / `commit.sha` のいずれか（MCP のフォールバック経路でのみ使用） | checks-mcp.ts:421-439 |
| GET `/repos/{o}/{r}/branches` | 200 `[{name, commit:{sha}}]`、作成ブランチと既定ブランチを含む | checks-rest.ts:687-701 |
| GET `/repos/{o}/{r}/commits[?sha=ref]` | 200 `[{sha, commit:{message}}]`。`commit.message` は送ったメッセージと**完全一致**（末尾改行なし）。`sha` 省略時は既定ブランチの履歴 | checks-rest.ts:703-726, :1100-1107 |
| PUT `/repos/{o}/{r}/contents/{path}` `{message, content(base64), branch?, sha?}` | 新規 201、更新 200（201 も可）、stale sha 409 `{message}`。応答 `{content:{sha(blob), encoding:"base64"}, commit:{sha}}`（**content.encoding が必須**）。**空リポジトリ（push 前）でも受け付け、既定ブランチ（main）に最初のコミットを作る** | checks-rest.ts:389-429, :777-785, :475-479 |
| GET `/repos/{o}/{r}/contents/{path}[?ref=]` | ファイル: 200 `{type:"file", encoding:"base64", content, sha, size(バイト数)}`。ディレクトリ: 200 `[{type:"file"|"dir", name, path, sha}]`。削除後 404 | checks-rest.ts:438-458, :758-775, :787-795 |
| DELETE `/repos/{o}/{r}/contents/{path}` `{message, sha}` | 200 `{commit:{sha}}` | checks-rest.ts:787-795 |
| POST `/repos/{o}/{r}/labels` `{name, color}` | 201 `{name}` | checks-rest.ts:552-558 |
| POST `/repos/{o}/{r}/issues` `{title, body, labels?}` | 201 `{id, number(≥1), title, body(文字列), state:"open", labels:[{name}]}`。未作成ラベル → 422 `{message}`。連続作成で number は +1 | checks-rest.ts:538-565, :820-825, :957-961 |
| GET `/repos/{o}/{r}/issues?state=open|all&sort=created&direction=asc|desc&per_page=N` | 200 issue 配列。state=open で closed を除外、作成順の昇順/降順 | checks-rest.ts:567-573, :827-871 |
| GET `/repos/{o}/{r}/issues/{n}` | 200 issue（`id` は読むたび同じ） | checks-rest.ts:884-895, checks-mcp.ts:938-947 |
| PATCH `/repos/{o}/{r}/issues/{n}` `{state}` | 200 `{number, state}` | checks-rest.ts:575-580 |
| POST/GET `/repos/{o}/{r}/issues/{n}/comments` | POST 201 `{id(≥1), body}`、GET 200 `[{id, body}]` | checks-rest.ts:917-955 |
| POST `/repos/{o}/{r}/pulls` `{head, base, title, body?}` | 201 `{number, state, title, body(文字列), head:{ref, sha}, base:{ref, sha}}`（MCP の merge 準備は body なしで送る） | checks-rest.ts:481-496, checks-mcp.ts:739 |
| GET `/repos/{o}/{r}/pulls[?state=all][&per_page=N]` | 200 pull 配列。state 省略時は open のみ | checks-rest.ts:1148-1182 |
| GET `/repos/{o}/{r}/pulls/{n}` | 200 pull、マージ後は `state:"closed", merged:true, merged_at(非空), merge_commit_sha`。未知は 404 | checks-rest.ts:1084-1123, :1184-1188 |
| GET `/repos/{o}/{r}/pulls/{n}/files` | 200 `[{filename, status, additions, deletions, changes}]`（status は added/modified/changed、additions ≥1） | checks-rest.ts:1015-1061 |
| PUT `/repos/{o}/{r}/pulls/{n}/merge` `{merge_method:"squash"}` | 200 `{sha, merged:true, message}`、既定ブランチにスカッシュコミット。未知 404、衝突 405 `{message}` | checks-rest.ts:498-500, :1063-1146 |
| 未知のルート（例 `/api/v3/no-such-route/amber-thistle`） | 404 JSON `{message}` | checks-rest.ts:1211-1216 |
| 全 REST 応答 | `content-type: application/json`、非空の `X-GitHub-Request-Id`（401・404 にも） | checks-rest.ts:1193-1237, RULES.txt:17 |

### 5.4 REST チェックの固定名（scenario 由来、全インスタンス共通）
- リポジトリ名: `amber-thistle-<slug>`（例 `amber-thistle-repos-create`）。pagination は scenario.repos の 30 名（`amber-thistle`, `happy-willow`, …）（checks-rest.ts:122-157, :322-331）。
- ファイル: `config/token.css`、ディレクトリ `config`、head ブランチは `develop-<slug>`、ラベル `bug`、不正ラベル `bug-not-a-label`（checks-rest.ts:122-157 を実行して確認）。
- **ブランチ名・ファイルパスに `/` を含むものがある**（GraphQL fixture の `chore/mcp-tool-schemas` 等、checks-graphql.ts:553-564）。

## 6. Git smart HTTP クライアント（env/git-client.ts）

### 6.1 共通
- パスは `/{owner}/{repo}.git` 固定（`.git` なしは送らない）。owner/repo は `[A-Za-z0-9][A-Za-z0-9._-]{0,99}`（env/git-client.ts:1316-1322）。
- ヘッダ: `User-Agent: makefun-git-client/1.0`、`Accept`、ボディがあれば `Content-Type` と `Content-Length`、upload-pack の v2 時は `Git-Protocol: version=2`、token があれば `Authorization: Bearer`（env/git-client.ts:1337-1344）。
- 1 リクエストの既定タイムアウト 8 s（チェックの残り時間で短くなる）（env/git-client.ts:40, checks-git.ts:263）。
- 応答 content-type: GET は `application/x-<service>-advertisement` を**含む**こと、POST は `application/x-<service>-result` を含むこと（null なら検査しない）。200 以外はエラー（env/git-client.ts:1367-1387, :1439-1445, :1471）。
- POST の `Content-Type: application/x-<service>-request`、`Accept: application/x-<service>-result`（env/git-client.ts:1464-1470）。gzip は送らない（Content-Encoding を付けるコードはない）。
- pkt-line: 4 桁 hex 長、0000 flush、0001 delim、0002 response-end、1 行最大 65520 バイト（env/git-client.ts:33-130, :234-272）。応答の途中切れ・余りバイトはエラー（env/git-client.ts:1001, :236-238）。

### 6.2 info/refs
- `GET /{o}/{r}.git/info/refs?service=git-upload-pack`（`Git-Protocol: version=2` 付き。advertise-headers の v0Client だけヘッダなし）、`...?service=git-receive-pack`（ヘッダなし）（env/git-client.ts:1394-1421, checks-git.ts:430-439）。
- 存在しないリポジトリ: 両サービスとも 404（checks-git.ts:465-475）。
- 解析: 最初のデータ行が `# service=` なら読み飛ばす。次の行が `version 2` なら v2（以降の行は `key[=value]` の capability、`object-format` があれば `sha1` 必須）。そうでなければ v0: 1 行目の NUL 以降が capability、`<oid> <ref>` 行、`^{}` は peel、`capabilities^{}` は無視、HEAD の symref は `symref=HEAD:refs/heads/main`（env/git-client.ts:1067-1147）。
- **v2 advertisement の要件**: 1 行目 `version 2`（preamble なし推奨: RULES.txt:38「the v2 advertisement opens with "version 2"」。preamble 付きでも client は読める）、`ls-refs` と `fetch` を含む（checks-git.ts:403）、`fetch` の値に `shallow` を含む（深さ指定 fetch で必須、env/git-client.ts:1577-1580）。`ls-refs` の値に `unborn` があると client は `unborn` を送る（env/git-client.ts:1493-1500）。`agent`・`object-format` を広告すると client はそれを fetch/ls-refs のコマンド行に入れる（env/git-client.ts:1495-1496, :1585-1586）。
- **v0 upload-pack（v0Client 向け）**: 1 行目が `# service=git-upload-pack` か `version 2`（checks-git.ts:456）。
- **receive-pack advertisement**: 1 行目 `# service=git-receive-pack`（checks-git.ts:457）、v0 のみ（v2 応答は push で例外、env/git-client.ts:1957-1959）、capability に `report-status`・`side-band-64k`・`delete-refs` 必須（checks-git.ts:420-424）。`refs/heads/main` を現在の tip で列挙（checks-git.ts:416-419）。
- client は v2 応答を受けて protocolPreference=0 なら例外、v0 応答を受けて preference=2 なら例外（env/git-client.ts:1447-1452）。checks・bench は既定の `auto`。

### 6.3 ls-refs（v2）リクエストのバイト列（env/git-client.ts:1494-1507）
```
command=ls-refs\n
[agent=makefun-git-client/1.0\n]      ← agent を広告した場合
[object-format=sha1\n]                 ← object-format を広告した場合
0001
peel\n
symrefs\n
[unborn\n]                             ← ls-refs=unborn を広告した場合
ref-prefix <prefix>\n  …（0 個以上）
0000
```
- 応答: 1 ref 1 行 `<oid> <refname>[ symref-target:<t>][ peeled:<oid>]\n`、または `unborn <refname>[ symref-target:<t>]`、最後に flush（env/git-client.ts:1509-1531）。
- 要件: HEAD を `symref-target:refs/heads/main` 付きで返す、annotated tag は `peeled:<commit>` 付き（checks-git.ts:496-505）。**ref-prefix はサーバ側で絞り込み、HEAD も含めて prefix に合わない行は返さない**（`rawRefs` を検査、checks-git.ts:520-525）。合わない prefix なら 0 行。prefix は完全な ref 名（`refs/heads/main`）でも来る（env/main.ts:457-459）。
- 空リポジトリでは `refs/` で始まる行を返してはならない（checks-git.ts:535-539）。

### 6.4 fetch（v2）リクエストのバイト列（env/git-client.ts:1584-1605）
```
command=fetch\n
[agent=makefun-git-client/1.0\n]
[object-format=sha1\n]
0001
thin-pack\n
no-progress\n
ofs-delta\n
want <oid>\n          …（重複除去済み、1 個以上。0 個なら client は送信しない）
have <oid>\n          …（0 個以上）
shallow <oid>\n       …（クライアントの shallow 境界、0 個以上）
deepen <n>\n          ← depth 指定時
done\n                ← 常に送る（done:false を渡すコードはない）
0000
```
- want には annotated tag オブジェクトの oid も来る（全クローンは refs/heads と refs/tags の全 oid を want、env/git-client.ts:1898-1902）。depth 指定のクローンは HEAD の tip 1 つだけ（env/git-client.ts:1891-1897）。
- 応答のセクション（env/git-client.ts:1614-1727）: `acknowledgments\n`（NAK/ACK/ready）、`shallow-info\n`（`shallow <oid>` / `unshallow <oid>`）、`wanted-refs\n`、`packfile\n`。セクション間は delim（0001）、終わりは flush。packfile セクションの各 pkt は先頭 1 バイトがバンド（1=pack、2=progress、3=エラー→失敗）。packfile セクションに pkt が 0 個なら失敗。
- **side-band は v2 packfile セクションで必須**（バンドバイトを必ず読む）。pack の SHA-1 trailer が一致しないと例外（env/git-client.ts:1698-1703）。
- pack の要件: version 2/3、`declaredCount` 個の entry、オブジェクトが重複しない（pack-integrity は `objects.size === declaredCount`、checks-git.ts:995-996）、trailer の前に余りバイトなし、各オブジェクトの inflate 長 = ヘッダの size（env/git-client.ts:707-770）。OFS/REF_DELTA は送ってよいが不要（checks-git.ts:1003 のタイトル「when the server sends them」）。
- 欲しいもの以外を送らない: 増分 fetch は have の到達集合を除く（checks-git.ts:713-729）、tip を既に持つ fetch は 0 オブジェクト（0 オブジェクトの pack か pack なし、checks-git.ts:732-744）。
- shallow:
  - depth 1 クローン: tip とその tree・blob だけ、親を含めない、`shallow-info` に `shallow <tip>`（checks-git.ts:746-765）。
  - deepen: `want T, have T, shallow T, deepen 2` → 親 P とその tree を送り、祖父は送らない、`shallow P` または `unshallow T` を返す（checks-git.ts:767-795）。**have T があっても T の親は client に無い**ことに注意（shallow 境界）。
  - 新 tip の shallow fetch: `want N, have P, shallow P, deepen 1` → N 関連だけ、P も祖先も送らない（checks-git.ts:797-818）。
- v0 fetch（v2 を広告しない場合のフォールバック）は side-band-64k の広告が必須（env/git-client.ts:1754-1757）。v2 を実装すれば使われない。

### 6.5 receive-pack（v0）push のバイト列（env/git-client.ts:1941-2015）
- 毎回まず `GET info/refs?service=git-receive-pack` を取得してから POST（env/git-client.ts:1956）。
- POST ボディ:
```
<old> <new> <ref>\0<caps>\n     ← 1 行目だけ caps 付き
<old> <new> <ref>\n             ← 2 コマンド目以降
0000
PACK…                           ← 作成/更新を含むなら必ず付く（オブジェクト 0 個の空 pack もある）。削除のみなら付かない
```
  - caps の順: `report-status-v2`（広告時）または `report-status`、`side-band-64k`（広告時）、`ofs-delta`（広告時）、`agent=makefun-git-client/1.0`（agent 広告時）（env/git-client.ts:1960-1967）。`atomic`・`push-options` は送らない。
  - pack は非デルタの通常オブジェクトのみ（env/git-client.ts:882-897）。bench は事前に作った pack を送る（bench.ts:530-534）。
  - 既にサーバにあるオブジェクトを含むことがある（non-fast-forward 系は到達集合を全部送る、checks-git.ts:871-876）。空 pack（ヘッダ count 0 + trailer、32 バイト）も来る（push-create-branch、checks-git.ts:835-848, :309-316）。
- 応答（side-band-64k を広告した場合）: 各 pkt の先頭バイト 1 が report、2 が progress、3 はエラー→失敗。band 1 のデータを連結したものが **pkt-line 化された** report-status（`unpack ok\n`、`ok <ref>\n`、`ng <ref> <reason>\n`、v2 なら `option …`、内部 flush 可）。外側も flush で閉じる（env/git-client.ts:1990-2061）。band 1 を連結したバイト列は完全な pkt-line 列であること（途中切れ・余りは例外、env/git-client.ts:2022-2027, :285-296）。
- 要件:
  - non-fast-forward は ref ごとに `ng <ref> non-fast-forward`（理由文字列は完全一致）、ref は動かさない、`unpack ok` は返す（checks-git.ts:865-888, RULES.txt:38）。
  - 混在: 1 本 ok・1 本 ng、report の ref 行は**ちょうど 2 行**、ok 側だけ反映（checks-git.ts:890-925）。
  - 削除（new = 0×40）は ok を返し ref を消す（ブランチ・タグ）（checks-git.ts:850-863, :927-938）。
  - 空リポジトリへの最初の push 後、HEAD → refs/heads/main（checks-git.ts:547-559）。
  - push で受けたオブジェクトをそのままのバイトで返す（commit・tag・tree のバイト一致、checks-git.ts:649-697）。
  - 不正ボディ `zzzznot-a-pkt-line` を upload-pack に POST されてもサーバは落ちない（応答内容は問わない）（checks-git.ts:1108-1120）。

### 6.6 リポジトリ作成と fixture
- 各 git チェックは `gitconf-<slug>` を REST で作る（checks-git.ts:258-269）。GraphQL 用 fixture は env 本体が別の GitClient で push する: 毎回 `createRepo`（2回目以降は 422+`already exists` が必要）→ `lsRefs(prefixes: refs/heads/<branch>[, refs/heads/<from>])` → 必要なら `fetch(wants:[commit])` → receive-pack advertisement → push（env/main.ts:446-507）。fixture のコミット時刻は 1735689600、作者 `Conformance <conformance@make.fun>`（env/main.ts:146, :479-490）。

## 7. GraphQL クライアント（env/graphql-client.ts）

- `POST /api/graphql`、ヘッダ `accept: application/json`、`content-type: application/json`、`user-agent: makefun-github-core-clone-env/1`、`authorization: Bearer`（無認証チェックでは省略）（env/graphql-client.ts:278-296）。
- 単発ボディ: `{"query": <文書>, "variables": {…}}`（operationName は付けない）（env/graphql-client.ts:200-204）。バッチ: `[{"query","variables"}, …]` の JSON 配列、応答も同順の配列（env/graphql-client.ts:226-274）。
- 応答: JSON。単発はオブジェクトで `data` か `errors` を含むこと（両方無いと失敗）（env/graphql-client.ts:315-329）。`errors[].message` が文字列のものだけ読む。
- 判定: `data` を使うチェックは HTTP 200 かつ `errors` 空が必要（checks-graphql.ts:411-422）。invalid-field だけ 200 + `errors` にフィールド名 `noSuchFieldOnViewer` を含むメッセージ（checks-graphql.ts:1139-1149）。無認証は**正確に 401**（checks-graphql.ts:1152-1158）。
- 送られる文書は checks-graphql.ts:107-317 にある 18 個がすべて（`GRAPHQL_DOCUMENTS`、checks-graphql.ts:320-339）。必要な構文: 名前付き operation、変数定義（`String!`・`Int!`・`ID!`・`String`・`CreateIssueInput!` など入力型）、インライン enum リスト `states: [OPEN]`、インライン入力オブジェクト `orderBy: {field: CREATED_AT, direction: DESC}`、リテラル引数 `first: 10`・`name: "Issue"`・`includeDeprecated: true`、エイリアス `Issue: __type(...)`、インラインフラグメント `... on Repository`、`after: null` の変数。フラグメント定義・ディレクティブ・`__typename` は文書に出てこない。
- 必要なスキーマは checks-graphql.ts:23-54 のリスト（Query.viewer/repository/node/__type、Repository・Issue・PullRequest・接続型、4 つの mutation＋addComment、enum 群）。
- fixture 固定名: リポジトリ `amber-thistle-graphql`（private:false）、`amber-thistle-graphql-pages`、未知 `amber-thistle-graphql-missing`、base `main`、head `chore/mcp-tool-schemas`、create 用 `chore/conditional-requests`、merge 用 `feature/repo-pagination`、ファイル `config/token.css`、base ファイル `server.toml`（checks-graphql.ts:542-571 を実行して確認）。
- head ブランチは git push で作られる（env/main.ts:446-507）。**git で push した ref を REST/GraphQL の PR 作成から見えるようにする**必要がある。git fixture が無いと PR 系 4 チェックは失敗（checks-graphql.ts:19-21）。

## 8. MCP クライアント（env/mcp-client.ts）

### 8.1 トランスポート
- `POST /mcp`、1 リクエスト 1 POST。ヘッダ（env/mcp-client.ts:665-674）:
  - `accept: application/json, text/event-stream`
  - `content-type: application/json`
  - `mcp-protocol-version: <revision>`（legacy の initialize と notifications/initialized では省略）
  - `mcp-method: <method>`、tools/call では `mcp-name: <tool名>`
  - `mcp-session-id`（サーバが応答ヘッダで渡した場合のみ以後付与）
  - `authorization: Bearer <token>`（auth-required の呼び出しだけ省略）
- ボディ: `{"jsonrpc":"2.0","id":<id>,"method":…,"params":{…,"_meta":{"io.modelcontextprotocol/protocolVersion":rev,"io.modelcontextprotocol/clientInfo":{name:"makefun-github-core-clone-env",version:"1.0.0"},"io.modelcontextprotocol/clientCapabilities":{},"protocolVersion":rev}}}`（env/mcp-client.ts:543-556, :653-662）。id は数値（1,2,…）または文字列（`discover-1`、`initialize-1`、`concurrent-left-6a09e667` など）。
- 応答: `application/json`（JSON-RPC 応答オブジェクト、または配列のうち id 付きの要素）か `text/event-stream`（`data:` 行、空行で区切り、期待 id の応答で読み終わる）（env/mcp-client.ts:699-789）。応答ヘッダ `mcp-session-id` があれば保存（env/mcp-client.ts:699-700）。
- **1 リクエスト 3 秒以内**（`MCP_DEFAULT_TIMEOUT_MS = 3000`、env/mcp-client.ts:61, checks-mcp.ts:151-155, RULES.txt:44）。
- legacy でセッションを渡した場合、close 時に `DELETE /mcp`（`mcp-session-id`・authorization 付き）を送る。応答は見ない（env/mcp-client.ts:603-623）。

### 8.2 接続手順（env/mcp-client.ts:432-517）
1. `server/discover`（id `discover-1`）。result（オブジェクト）が返れば **modern**。`supportedVersions`（YYYY-MM-DD の配列）に `2026-07-28` があればそれ、なければ先頭を以後使う。serverInfo は `result._meta["io.modelcontextprotocol/serverInfo"]` か `result.serverInfo`、capabilities は `result.capabilities`。
2. -32022 なら `data.supported` の版で再試行。401/403/5xx や -32020/-32021/-32600/-32602 は失敗扱い。
3. 404/405 か -32601（またはそれ以外の応答）なら **legacy**: `initialize`（id `initialize-1`、params `{protocolVersion:"2025-11-25", capabilities:{}, clientInfo}`、-32602 なら 2025-06-18 → 2025-03-26 → 2024-11-05 と下げる）→ `notifications/initialized`（id なし、応答は不問）。
- initialize チェック: `serverInfo.name`・`serverInfo.version` が非空文字列、`capabilities.tools` がオブジェクト（checks-mcp.ts:541-556）。
- server-discover チェック: modern なら `supportedVersions` 非空。legacy なら接続できれば pass（checks-mcp.ts:562-580）。
- **parse-error の判定は era で変わる**: modern は HTTP 400 + -32700 のみ合格、legacy は 400 か 200 + -32700（checks-mcp.ts:789-794）。送るボディは `{"jsonrpc": "2.0", "id": 424242, "method": "tools/list"`（閉じ括弧なし）。

### 8.3 tools
- `tools/list`（`nextCursor` を最大 10 ページたどる）→ `result.tools` 配列（env/mcp-client.ts:580-594）。2 回の結果が `JSON.stringify` で一致すること（checks-mcp.ts:871-887）。
- 6 個以上、`create_repo`・`get_repo`・`create_issue`・`list_issues`・`create_pull_request`・`merge_pull_request`（別名とサーバ接頭辞は正規化後の完全一致→末尾一致で探す）（checks-mcp.ts:69-80, :173-182）。全ツールに非空 `description` と `inputSchema.type === "object"`（checks-mcp.ts:612-631）。
- 引数の対応: suite は意味名を inputSchema.properties の名前へ同義語表で写す（checks-mcp.ts:185-232）。**inputSchema.required に suite が埋められない名前があると呼ばずに失敗**。properties が無いと意味名のまま渡す。
  - create_repo `{owner, name, description, private}`、get_repo `{owner, repo}`、create_issue `{owner, repo, title, body}`、list_issues `{owner, repo, state:"open"}`、create_pull_request `{owner, repo, title, body, head, base}`、merge_pull_request `{owner, repo, pull_number}`（checks-mcp.ts:645, :678, :708, :743, :842, :901, :925）。
- `tools/call` params `{name, arguments}`（env/mcp-client.ts:597-601）。結果は `content:[{type:"text", text}]` と/または `structuredContent`。suite は structuredContent → text の JSON → text 中の最外 `{…}`/`[…]` の順で JSON を探す（checks-mcp.ts:270-304）。
  - create_pull_request: 結果 JSON の `number`/`pull_number`/`pull_request_number` で REST を引く（無ければタイトル/head で一覧検索）（checks-mcp.ts:713-717）。
  - list_issues: 結果テキストに issue タイトルか `"number": N` を含む（checks-mcp.ts:683-686）。
  - get_repo: 結果テキストにリポジトリ名を含む（checks-mcp.ts:855-858）。
  - create_repo: 結果 JSON に `full_name` があれば owner/repo をそこから取る（checks-mcp.ts:906-915）。
- エラー: 未知ツールは JSON-RPC エラー -32602 か -32601（結果の isError は不合格）、HTTP 500 不可（checks-mcp.ts:763-780）。create_issue を `{}` で呼ぶと isError:true か -32602（checks-mcp.ts:806-826）。Authorization なしの tools/call は HTTP 401（checks-mcp.ts:960-978）。
- 並行: 同じ client から 2 本の tools/call（get_repo）を同時に送り、それぞれ自分の id（文字列）と自分のリポジトリ名で返る（checks-mcp.ts:832-865）。
- 固定名: tag = `6a09e667`（scenario.seed の先頭 8 文字）、リポジトリ `mcp-6a09e667-issue-create` など、ブランチ `mcp-head-6a09e667-pr-create`、ファイル `mcp-6a09e667-pr-create-base.txt`（checks-mcp.ts:136-149）。MCP 準備は POST /user/repos が 2xx であること（checks-mcp.ts:405-406）、PUT contents で既定ブランチにコミット→ POST git/refs → 枝に PUT contents（checks-mcp.ts:452-478）。

## 9. ベンチマーク（env/bench.ts）

| ワークロード | 準備（計測外） | 1 操作で送るもの | 計測 |
|---|---|---|---|
| push | `DELETE /api/v3/repos/adaiversen/bench-push`（応答不問）→ `POST /user/repos` | `GET info/refs?service=git-receive-pack` → `POST git-receive-pack`（1 コマンド、blob+tree+commit の 3 オブジェクト pack、1 KiB blob）。report の ok 必須 | 200 回逐次 ×3 ラウンド、暖機 5 |
| clone | `bench-clone` を再作成し、200 コミット（600 オブジェクト）を 1 回の push で投入 | `clone(depth:1, singleBranch, parse:false)` = GET info/refs（v2）→ GET info/refs + POST ls-refs → GET info/refs + POST fetch（`want tip`、`deepen 1`、`done`）。pack trailer 一致・pack 長 >0・refs ≥1 | 50 並列 ×3 ラウンド、暖機 5（逐次） |
| read | `bench-read` を再作成、issue 1 件作成 | `GET /api/v3/repos/adaiversen/bench-read/issues/{n}` → 200 JSON オブジェクト | 2,000 回逐次、同一 RestClient（undici の keep-alive） |
| write | `bench-write` を再作成 | `POST /api/v3/repos/adaiversen/bench-write/issues` `{title, body}` → 201 | 200 回逐次 |

根拠: bench.ts:146-166, :382-440, :476-482, :515-665, env/git-client.ts:1874-1934。各ワークロード予算 60 s、失敗・時間切れは 0 点（bench.ts:23-32, env/manifest.ts:351-388）。push は 1 操作で HTTP 2 往復、clone は 1 操作で 5 往復。

## 10. 不明点
- 本番での `MAKEFUN_SHOP_TOKEN` の形式（ローカルは `shpat_…`、cli/makefun.mjs:223365。RULES.txt:181 は導出元のみ）。実装はトークン値に依存させないこと。
- undici fetch が `accept-encoding` を付けるか・圧縮応答を受け入れるかはキットのコードからは分からない（どのクライアントも明示していない）。圧縮しない応答で問題はない。
- 登録時の送信元アドレス: IPv6 で bind した場合も env は remoteAddress を正規化して使う（env/wire.ts:179-189）。`pod` レイアウトでの実アドレスは不明（env/wire.ts:21-24 は「どちらのレイアウトでもこの規則で動く」とだけ書く）。
