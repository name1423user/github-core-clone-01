# CLAUDE.md — make.fun bounty `github-core-clone-01`（Rust 版）

目的: **ハウス参照実装（house entry）を統計的に有意に上回り、1位を取る。**
締切: 2026-11-21 06:30 UTC（日本時間 11/21 15:30）。最終提出は締切の数時間前までに済ませる。
ハウスは締切の4時間前（02:30 UTC）に入る。上位（953〜966点）は feature 600・agent 200 を満点にして perf で競っている。**満点は前提、勝負は perf。**

## ディレクトリ配置（前提）

```
<親フォルダ>/
├── github-core-clone-01-kit/   ← キット（scripts/fetch-kit.sh で取得。コミットしない）
└── <このリポジトリ>/
    ├── main.ts                 ← ランチャー（バイナリを起動して終了を待つだけ）
    ├── forge-linux-x64         ← 提出用の静的バイナリ（コミットする唯一のバイナリ）
    ├── forge-darwin-arm64      ← Mac のローカル採点用（.gitignore。コミット禁止）
    ├── Cargo.toml / src/       ← フォージ本体（Rust、std のみ）
    └── scripts/                ← build.sh・check-binary.sh・grade.sh・compliance.mjs・fetch-kit.sh
```

キットのパスは `../github-core-clone-01-kit`（環境変数 `KIT` で上書き可）。

---

## 0. 絶対ルール（違反＝失格または0点）

- **エントリは repo 直下の `main.ts`（Node 22）。中身はランチャーだけ**: `forge-<platform>-<arch>` を起動し、シグナルを転送し、子の終了コードで exit する。`node:` の組み込み以外を import しない。`package.json` に dependencies を書かない。
- **本体は Rust 製の静的バイナリ（x86_64-unknown-linux-musl）を同梱する。** 根拠は RULES.txt「a statically linked Linux amd64 binary you commit runs from your repository as vendored source」と competition.yaml「A vendored git library or a static ELF is legal」。
- **サイズ上限**: 1ファイル 8 MiB 以下（バイナリ単体を含む）、リポジトリ全体 16 MiB 以下（competition.yaml `max_file_bytes`/`max_tree_bytes`）。
- **ポートの bind と suite への登録はバイナリ側が行う。** `0.0.0.0:0` で bind → `POST $MAKEFUN_ENV_URL/register {"port":N}`（200 か 409 で完了、400/405 は致命）→ `GET $MAKEFUN_ENV_URL/done` を `{"done":true}` までポーリング → exit 0。**8080 は絶対に bind しない**（env が使う）。登録が boot budget（20 秒）を過ぎると全損。
- **外部クレートは使わない（Rust std のみ）。** `Cargo.toml` の `[dependencies]` は空。理由: (1) ルールは「標準ライブラリ＋自分でコミットしたソース」で、クレートは vendor してコミットしないと使えない、(2) vendor したソースは作業記録審査の「counted lines」に入り、AI セッション由来 30% 以上の条件（thinking_reproduces）を割る危険がある。HTTP・JSON・SHA-1・zlib は自作する。
- **決定性**: 同じインスタンスで2回走らせた state export がバイト一致しないと失格扱い（unranked）。state export は suite（env）が書く（`export.ts`）が、その中身＝チェックの合否を決めるのはフォージの応答なので、応答を決定的にする。
  - 現在時刻（`SystemTime::now`）、乱数、プロセスID、ポインタのアドレスを応答・ID・ハッシュ入力に入れない。日時は `MAKEFUN_TRIAL_NOW` から作る。
  - ID は単調カウンタ。到着順が毎回変わる並行処理（50並列 clone など）で ID や結果が変わらない設計にする。
  - **std の `HashMap`/`HashSet` は反復順がプロセスごとに変わる**（RandomState）。反復して出力するものは `BTreeMap`・`Vec`（挿入順）にする。引くだけなら HashMap でよい。
  - JSON を出すときキーの順序を固定する（構造体なら宣言順、マップならソート）。
- **model_call は入れない。** RULES.txt「This competition does not require model calls (harness.model_calls: optional)」。
- 秘密情報（APIキー、ウォレットの鍵）を絶対にコミットしない。`scripts/compliance.mjs` が検査する。
- Claude は**提出コマンドを実行しない**（§6）。

## 1. ビルドとローカル実行の決まり（事故りやすい）

| 用途 | コマンド | 出力 | コミット |
|---|---|---|---|
| 本番（提出） | `bash scripts/build.sh` | `./forge-linux-x64`（x86-64 ELF・静的・strip 済み） | **する（唯一のバイナリ）** |
| Mac のローカル採点 | `bash scripts/build.sh dev` | `./forge-darwin-arm64` | **しない**（.gitignore） |
| Linux x64 のローカル採点 | `bash scripts/build.sh dev` | 本番と同じ `./forge-linux-x64` | する |

- 本番ビルドは `cargo build --release --target x86_64-unknown-linux-musl`。Linux x64 ではそのまま、**Mac では `cargo zigbuild`**（`brew install zig && cargo install cargo-zigbuild`、START_HERE.md）。build.sh がホストを見て選ぶ。
- 開発機は Apple Silicon Mac。amd64 の Linux バイナリは Mac で動かないので、`main.ts` が `process.platform`/`process.arch` で `forge-darwin-arm64` か `forge-linux-x64` を選ぶ（`FORGE_BIN` で上書き可）。
- **`bash scripts/grade.sh` を使う。`grade-local --worktree` を直接使わない。** `--worktree` は .gitignore されたファイルを拾わないので、Mac 用バイナリが採点に入らない。grade.sh は「追跡中＋未追跡のファイル＋このマシン用バイナリ」を一時リポジトリに写して採点する。バイナリがソースより古ければ自動で `build.sh dev` する。
- **ソースを変えたらバイナリも作り直してコミットする。** 提出コミットの `forge-linux-x64` がソースより古いと、古い実装が採点される。`bash scripts/check-binary.sh` が検査する（x86-64 ELF・静的・≤ 8 MiB・実行ビット 100755・開発用バイナリの混入なし・ソースより古くない・未コミットのソース変更なし）。
- `/presubmit` では必ず `bash scripts/check-binary.sh` を通す（`file` コマンドで x86-64 の静的 ELF であることを確認。Mac 版の誤提出を防ぐ）。
- 本番環境: `/bot` は読み取り専用、書き込みは `/tmp` のみ、2 CPU・1024 MB・**256 プロセス（スレッドも数える）**、900 秒。ネットワークは suite のみ。

## 2. 作業の進め方（この順番を守る）

1. **キットを全部読む。コードを書く前に。** → `spec-analyst` に任せる（`/kickoff`）。
   読む順: `RULES.txt` → `competition.yaml` → `rules/instance.yaml` → `starter/*` → `program/env/src/main.ts` → `manifest.ts` → `export.ts` → `checks-*.ts` → `*-client.ts`・`wire.ts` → `bench.ts`・`calibration.json` → `program/scorer/*`。
2. `docs/CHECKS.md`（全チェックの一覧＝TODO本体）、`docs/SCORING.md`（採点式）、`docs/INTERFACES.md`（起動・登録・認証・export の正確な仕様）を完成させる。
3. 共有の土台（`src/http.rs`・`src/router.rs`・`src/util/json.rs`・`src/store/`）をリードが作る。
4. 4つのインターフェースを担当エージェントに**並列で**実装させる（`/parallel`）。
5. 機能を足すたびに `/grade`。落ちたチェックは `grader` が担当者に振り分ける。
6. 合格率 100% に達してから `perf-engineer` で速度を詰める。最適化のたびに全チェックを再実行する。
7. `determinism-auditor` で決定性を確認し、提出前に `/presubmit`。
8. practice の3インスタンスすべてで確認する（`bash scripts/grade.sh --instances 3`）。本番は締切後に別の3インスタンスが引かれるので、**practice の具体値（リポジトリ名・件数など）をハードコードしない**。
   - 例外: suite の固定値（`wire.ts` の `SCENARIO_SEED` から決まる viewer のログインなど）はインスタンスに依存しない。使うときは根拠を `docs/DECISIONS.md` に書く。

## 3. チーム構成と持ち場（並列で衝突させないための決まり）

| 担当 | 持ち場（書いてよい場所） | 役割 |
|---|---|---|
| **リード（メインセッション）** | `main.ts`、`src/main.rs`、`src/http.rs`、`src/router.rs`、`src/store/**`、`src/util/mod.rs`、`src/util/json.rs`、`docs/DECISIONS.md`、`CLAUDE.md` | 土台・HTTP・ルーティング・JSON・ストア・共有の型、統合、判断 |
| `spec-analyst` | `docs/CHECKS.md`・`docs/SCORING.md`・`docs/INTERFACES.md` | キットの読解と仕様の抽出（コードは書かない） |
| `build-engineer` | `Cargo.toml`（`[profile]` のみ）、`scripts/build.sh`、`scripts/check-binary.sh`、`forge-linux-x64` | クロスコンパイル、バイナリの差し替え、サイズ・形式の確認 |
| `git-engineer` | `src/git/**`、`src/util/sha1.rs`、`src/util/zlib.rs` | smart HTTP、pkt-line、pack、receive/upload-pack、shallow、PRマージ用のツリーマージ、SHA-1、zlib |
| `rest-engineer` | `src/rest/**` | REST v3 |
| `graphql-engineer` | `src/graphql/**` | 自前の GraphQL パーサ・実行器・スキーマ |
| `mcp-engineer` | `src/mcp/**` | MCP（JSON-RPC 2.0、Streamable HTTP） |
| `grader` | `docs/CHECKS.md` の状態欄、`docs/PROGRESS.md` | ローカル採点の実行・失敗の切り分け・振り分け |
| `perf-engineer` | 担当者の合意のうえで各 `src/**` | 4つのベンチの高速化 |
| `determinism-auditor` | なし（読むだけ） | 不定要素の検出と2回実行の差分確認 |
| `submission-auditor` | なし（読むだけ） | 提出前の規約・審査項目・バイナリの確認 |

- **他人の持ち場は編集しない。** ストアや共有の型・HTTP に変更が必要なら、作業を止めて「必要な変更（関数名・引数・戻り値・理由）」をリードへの報告に書く。
- 各インターフェースはストアの公開関数だけを呼ぶ。REST・GraphQL・MCP は同じストア関数を共有する（同じ操作の結果が食い違わないように）。
- **1つの crate を全員で共有している。** 自分の変更でビルドを壊さない: 作業を終える前に必ず `cargo build --release` と `cargo test` を通す。他人の持ち場のコンパイルエラーで止まったら、直さずに報告する。
- 担当エージェントは作業の最後に、`docs/CHECKS.md` のどのチェックを通したか・残りは何かを報告する。

## 4. アーキテクチャ（Rust、std のみ）

```
main.ts              ランチャー（バイナリ選択・起動・シグナル転送・終了コードの中継）
src/main.rs          ライフサイクル（bind・register・/done ポーリング・exit）
src/http.rs          HTTP/1.1 サーバ（接続ごとのスレッド、keep-alive、Content-Length/chunked、TCP_NODELAY）と env 用の小さなクライアント
src/router.rs        パス分割＋match のルーター（正規表現の羅列にしない）、認証
src/util/            json（リード）、sha1・zlib（git-engineer）
src/store/           リポジトリ・オブジェクト・ref・issue・PR・コメント・ラベルのインメモリストア（Arc + Mutex/RwLock）
src/git/             pktline / pack / upload_pack（v2） / receive_pack（v0） / merge
src/rest/            REST v3
src/graphql/         lexer / parser / executor / schema / resolvers
src/mcp/             transport / tools
```

### 4.0 ワイヤ契約（RULES.txt「The wire contract」）
- `/api/*` と `/mcp` は `Authorization: Bearer $MAKEFUN_SHOP_TOKEN` 必須。無ければ 401。`/healthz`（200 `{"ok":true}`、boot で2回）と git smart HTTP は認証なし（suite の git クライアントはトークンを送らない）。
- REST の応答はすべて `application/json`（HTML 禁止）で、空でない `X-GitHub-Request-Id` を付ける。エラーは文字列の `message` を持つ JSON。未知のルートは 404 JSON、壊れた JSON ボディは 400。
- リポジトリはトークンのログインのもの。同じ名前でも別オーナーなら 404（403 ではない）。

### 4.1 Git smart HTTP（最重要・最難関）
- `GET /{owner}/{repo}.git/info/refs?service=git-upload-pack|git-receive-pack`、`POST .../git-upload-pack`、`POST .../git-receive-pack`。`.git` なしのパスも受けるかは checks で確認する。存在しないリポジトリは両サービスの info/refs で 404、サーバは落ちない。
- **upload-pack は protocol v2**（suite は `Git-Protocol: version=2` を送る）。v2 の広告は `version 2` で始まる。ls-refs・fetch を実装。v0 の upload-pack は side-band-64k を広告しないと枠付けできない。
- **receive-pack は v0 のみ**。`# service=git-receive-pack` の前置きで始め、`report-status`・`side-band-64k`・`delete-refs` を広告する。
- suite のクライアントは本物の git ではなく `git-client.ts`。**それが送るバイト列を正確に読み、その範囲を完全に実装する**（capabilities、`shallow`/`deepen`、have/done、side-band の有無、report-status の形式、ref の削除）。
- receive-pack: pack を解析 → OFS_DELTA/REF_DELTA を解決 → 全オブジェクトを検証（SHA-1 の一致、参照先の存在） → ref を更新 → report-status。**更新は fast-forward のみ**: non-fast-forward は ref ごとに `ng <ref> non-fast-forward` で拒否し ref を動かさない。1回の push で fast-forward と拒否が混ざったら、前者だけ適用してそれぞれ1行ずつ返す。old-oid が合わなければ拒否。リポジトリは REST でしか作れない（push で作らない）。
- upload-pack: wants から haves を除いた到達集合を計算（shallow 境界を正しく扱う）して pack を生成。デルタ圧縮は不要。
- リクエストボディが `Content-Encoding: gzip` なら自作の inflate で展開する。

### 4.2 REST v3（`/api/v3`）
- `checks-rest.ts` が叩くものを全部実装する。RULES に明記されたもの: `GET /api/v3/`（200、`X-OAuth-Scopes`）、`GET /meta`（`installed_version`）、`GET/POST /user/repos`（一覧は `Link rel="next"` のページネーション、作成は 201）、`GET/PATCH/DELETE /repos/{o}/{r}`、`PUT/DELETE /repos/{o}/{r}/contents/{path}`（作成 201・更新 200・古い sha は 409、削除は 200 `{commit:{sha}}`）、`POST /labels`（201、issue のラベルは先に作られている必要あり）、`POST /issues`（201、未知ラベルは 422）、`POST/GET /pulls`、`PUT /pulls/{n}/merge`（200 `{sha, merged:true, message}`、squash コミットがデフォルトブランチに乗る、未知は 404、コンフリクトは 405）。
- レスポンスの形は GitHub に合わせる（`id`、`node_id`、`number`、`url` 系、`user`、`state`、日時）。**日時は決定的な値**。テストが見ていないフィールドに時間をかけない。
- エラーは `{"message","documentation_url"}`、422 には `errors` 配列。

### 4.3 GraphQL（`POST /api/graphql`）
- ライブラリなしでパーサと実行器を書く。query/mutation、変数とデフォルト値、フラグメント、インラインフラグメント、エイリアス、`__typename`、`@include`/`@skip`。1ボディに2つの操作ドキュメントなら結果を順に2つ返す（RULES）。
- スキーマは gh と github-mcp-server が送るクエリの範囲（`checks-graphql.ts`・`graphql-client.ts` に実物）。イントロスペクション（`__type(name:)` のフィールド一覧、`SearchType` の enumValues に `ISSUE`）を RULES の列挙どおりに出す。
- connection（`edges`/`nodes`/`pageInfo`/`totalCount`、`first`/`after`）、`orderBy {field: CREATED_AT, direction: DESC}`（同時刻は number DESC）、`states: [OPEN]`。`createPullRequest` は REST と同じストア関数。
- 未知フィールドは HTTP 200 で `errors` に名前を入れる。未知のリポジトリは `data.repository = null` でエラーなし。認証なしは 401。グローバル node ID は REST の `node_id` と一致させる。

### 4.4 MCP（`POST /mcp`）
- JSON-RPC 2.0 over Streamable HTTP。`application/json` か SSE で応答、**各リクエスト 3 秒以内**。リビジョン 2026-07-28（`server/discover` が `supportedVersions` を返す）と、handshake 型（2025-11-25 以前: `initialize` → `notifications/initialized`、任意の `Mcp-Session-Id`）の両方。
- `tools/list` は6個以上（`create_repo`・`get_repo`・`create_issue`・`list_issues`・`create_pull_request`・`merge_pull_request`）。説明は空でなく、`inputSchema` は type object。**2回の tools/list はバイト一致**。引数は github-mcp-server のもの。
- 未知ツールは -32602 か -32601（HTTP 500 にしない）、必須引数の欠落は `isError:true` か -32602、JSON でないボディは HTTP 400 + -32700、認証なしは 401。並行する2つの tools/call はそれぞれの id と引数で答える。
- REST と同じストア。MCP で作ったものが REST で同じ ID で読める。

## 5. 性能（4つのベンチ。perf 200点の勝負所）

採点: 各ベンチ `clamp(25 − 12.5·log2(t/T_ref), 0, 50)`。T_ref（ハウスの中央値, ms）は push 1640.82・clone 266.941・read 2759.715・write 348.965（`calibration.json`）。**T_ref の 1/4 以下で満点 50**。タイブレークは4つの中央値の幾何平均。

- **push（200回の逐次 push、1 KiB の変更）**: バイト列を直接走査し、文字列化しない。オブジェクトは受信時に1回だけ inflate・ハッシュする。fast-forward 判定は祖先探索を短く（新コミットの親が現 tip かをまず見る）。ref 更新は O(1)。info/refs の広告はキャッシュして ref 更新で作り直す。
- **clone（depth 1 の shallow clone を50並列、200コミットのリポジトリ）**: 同じ（repo、ref の版、wants、depth）の pack をキャッシュして使い回し、ref 更新で無効化する。オブジェクトは deflate 済みのバイト列も持っておき、pack 生成を「ヘッダ＋保存済みの圧縮バイトの連結＋SHA-1」にする。並列でも1回だけ生成する（生成中の待ち合わせ）。
- **read（2,000回の GET issue、1本の keep-alive 接続）**: issue の JSON をシリアライズ済みバイト列でキャッシュし、更新時に無効化する。ルーティング・ヘッダ生成・書き込みを最小に（応答は1回の write、TCP_NODELAY）。
- **write（200回の issue 作成）**: 書き込みは O(1)。ホットパスにログ・同期 I/O・不要な再シリアライズを置かない。
- 共通: keep-alive。ロックの保持時間を短く。2 CPU・1024 MB を suite と共有するのでメモリとスレッドを溜めすぎない。**スレッドは pids 256 に数えられる**（ランチャーの Node も十数本使う）ので、接続スレッドに上限を設けるか epoll を検討する（std のみなら `extern "C"` で libc の epoll を呼べる）。

## 6. バージョン管理

- **版番号ファイルは `Cargo.toml` の `version`**（0.1.0 から）。Git タグは打たない。`package.json` には version を置かない（二重管理しない）。
- **最初の編集の前に version-registry スキルで台帳 `ruria-versions-registry` と照合する**（手元の版・台帳の main・未マージのブランチの3つ）。
- 節目で版を上げる（目安: 全インターフェースの骨組み → 0.2.0、合格率100% → 0.3.0、性能チューニング後 → 0.4.0、最終提出 → 1.0.0）。上げたら版番号ファイルと台帳（`project.toml`・CHANGELOG・README）をスキルの手順でまとめて更新する。台帳は main に直接 push（強制 push 禁止）。

## 7. 提出（るりあ本人が行う）

- `docs/SUBMIT.md` のチェックリストに従う。**ウォレット連携・署名・提出はるりあ本人が行い、Claude は提出コマンドを実行しない。** Claude は必要なコマンドを書き出して渡すだけ。
- 作業記録: このフォルダで行った Claude Code のセッション（**サブエージェント含む**）を `node $KIT/cli/makefun.mjs bundle` で束ねる。`--dry-run` で全部拾えているか先に確認する。作業は必ずこのフォルダで行う。
- 提出は1日5回・合計10回まで。**最後の提出が最終エントリ**なので、壊れた版を最後にしない。提出前に `bash scripts/check-binary.sh` が全部 OK であること。

## 8. やってはいけないこと

- テストを読まずに「GitHub っぽく」実装すること
- practice インスタンスの具体値をハードコードすること
- 速度のために決定性を壊すこと（export の不一致は全損）
- ローカル採点を回さずに大量のコードを書くこと
- 他の担当者の持ち場を勝手に書き換えること
- 外部クレートを足すこと（`cargo add` 禁止）
- Mac 版バイナリや `target/` をコミットすること。ソースを変えたのに `forge-linux-x64` を作り直さずに提出すること
- `grade-local --worktree` を直接使って「Mac で動かない」と誤判定すること（`scripts/grade.sh` を使う）
