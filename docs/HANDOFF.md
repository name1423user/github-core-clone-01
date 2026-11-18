# HANDOFF — 別のチャット・セッションで再開するための引き継ぎ

最終更新: 2026-11-18 08:41 JST（2026-11-17 23:41 UTC）
締切: 2026-11-21 06:30 UTC（JST 11/21 15:30）。ハウスは 11/21 02:30 UTC に入る。

## 新しいセッションで最初にやること（この順）

1. 時刻を取る: `TZ=Asia/Tokyo date`
2. `CLAUDE.md` を読む（規約・持ち場・アーキテクチャの正本）。このファイルと `docs/PROGRESS.md` の作業キューも読む。
3. キットが無ければ取る: `ls ../github-core-clone-01-kit || bash scripts/fetch-kit.sh`（sha256 照合 63 件 OK を確認）
4. ツール: `rustup target add x86_64-unknown-linux-musl`（クラウドのコンテナは毎回入れ直しが必要）
5. 台帳の照合（version-registry スキル）。版番号ファイルは **`Cargo.toml` の `version`**（いま 0.3.0、台帳も 0.3.0）。
   - 台帳 `ruria-versions-registry` は main で読み書き・main に直接 push（強制 push 禁止）。
   - 台帳に `vela 0.1.1` の未マージのコミットがあるが別プロジェクト。触らない。
6. 作業ブランチの最新を取る: `git fetch origin && git checkout <指定の作業ブランチ> && git merge origin/main`
7. `docs/PROGRESS.md` の作業キューの、最初の `[ ]` から再開する。

## 運用ルール（るりあの指示）

- **トークンが少ない前提**: 作業は細切れ。1項目＝1コミット＝push。どこで止まっても再開できるようにする。
- サブエージェントは小さい単位で（spec-analyst 1回で約44万トークン使った）。担当1人・チェック数件ずつ渡す。
- **main へのマージは確認なしで行ってよい**（るりあの常設の許可、2026-11-18）。PR を作ってマージ、または作業ブランチを main に入れる。
- Claude は make.fun への提出コマンドを実行しない（るりあが行う）。

## いまの状態

- ブランチ: main = PR #1 マージ済み（Rust 構成）。作業ブランチ `claude/zealous-knuth-sqg5ur` は main より先にコミットあり（json.rs・docs）。
- 構成: 本体は Rust 静的バイナリ（std のみ、x86_64-unknown-linux-musl）、`main.ts` はランチャー。理由は `docs/DECISIONS.md`。
- 完成済み:
  - `src/main.rs` ライフサイクル（bind 0 → register → /done ポーリング → exit 0）
  - `src/http.rs` HTTP/1.1（接続ごとのスレッド、keep-alive、chunked）、`Response::json` は `X-GitHub-Request-Id` 付き
  - `src/router.rs` パス分割、`/api/*`・`/mcp` の Bearer 認証、`/healthz`
  - `src/util/json.rs` パーサ・シリアライザ（オブジェクトは挿入順の Vec＝出力順が固定）。API: `json::parse(&[u8]) -> Result<Value,String>`、`Value::{get,str_field,as_str,as_i64,as_bool,as_array,to_json,write}`、`json::obj(vec![("k", v.into())])`、`json::write_str`
  - `docs/CHECKS.md`（120件、全行に根拠、状態は全部 TODO）、`docs/SCORING.md`、`docs/INTERFACES.md`
- 2026-11-18 午後: `src/store/`（repos・issues・labels・comments・objects・contents の commit_file・PR の squash マージ）、`src/util/{sha1,zlib}.rs`、`Response::error`・`Request::json` 完成。作業ブランチは `claude/sharp-davinci-sqambb`。
- REST・git・MCP は担当エージェントが実装中（PROGRESS の作業キュー参照）。GraphQL は未着手。
- 直近のローカル採点: `bash scripts/grade.sh` → `working 1, feature 15, score 15`、replay はバイト一致。

## 分かっている重要な事実（spec-analyst の調査、根拠は docs/*）

- 採点: feature `round(600×合格/120)`（1件5点）＋ agent `round(200×MCP合格/15)` ＋ perf（4ベンチ×最大50）。ハウスは 120/120 → **800点は前提、勝負は perf**。
- 1件あたりの重さ: MCP 約18.3点（feature＋agent）＞ REST・git 5点。
- perf の満点境界（約 T_ref/3.89）: push 422 ms、clone 68.6 ms、read 710 ms、write 89.7 ms。
- model_call は不要（RULES.txt:149、competition.yaml:345）。
- export は env が書く。フォージは「同じ入力に同じ応答」を守ればよい（ID はカウンタ、日時は MAKEFUN_TRIAL_NOW、反復は BTreeMap/Vec）。
- トークンの持ち主ログイン = `adaiversen`、他者 = `olgabauer`。suite の固定値（wire.ts:751 SCENARIO_SEED）でインスタンスに依存しない。使うときは DECISIONS.md に根拠を書く。
- git-client はトークン付きで git も叩く（git-client.ts:1344）が、git に認証を要求する必要はない。
- 見落としやすい必須事項:
  - 同名リポジトリの再作成 → 422、本文に `already exists`（git-client.ts:2096。GraphQL fixture と bench が依存）
  - `Link` ヘッダの URL はリクエストの Host から組む（checks-rest.ts:669）
  - PUT contents の応答に `content.encoding`
  - REST の `commit.message` は末尾改行なしで完全一致
  - ls-refs の ref-prefix はサーバ側で絞り、HEAD を除く
  - MCP の modern 方式（`server/discover`）では parse error を HTTP 400 で返す

## 実装の優先順位

1. ストア＋REST の `POST /user/repos`・PUT contents（すべての面の前提）
2. MCP（点が最大。REST の repos・contents・refs・issues・pulls・merge が前提）
3. git（v2 ls-refs・fetch・shallow、v0 receive-pack＋side-band 内の report-status）。GraphQL の PR 4件が依存
4. REST の issues・pulls（squash マージ、パス単位の衝突で 405、files の行数）
5. GraphQL（送られてくる文書は18個だけ）
6. 120/120 の後に perf

## ストアの公開関数の案（REST・GraphQL・MCP・git が共有する）

- repo: `viewer_login()`、`create_repo(owner,name,desc,private) -> Result<Repo, AlreadyExists>`、`get_repo`、`update_repo_description`、`delete_repo`、`list_repos(owner,page,per_page)`、`repo_by_node_id`
- object/ref: `put_object(type,bytes)->oid`（deflate 済みも保持）、`get_object`、`list_refs(prefix)`、`head_symref`、`update_refs(cmds)->Vec<RefResult>`、`reachable(wants,haves,shallow,depth)`
- contents: `read_path`、`list_dir`、`commit_file(branch,path,content,msg,expected_sha)`（空リポジトリなら main を作る）、`delete_file`、`log`、`create_branch`、`list_branches`
- issue: `create_label`、`create_issue -> Result<Issue, UnknownLabel>`、`get_issue`、`list_issues`、`set_issue_state`、`add_comment`、`list_comments`
- PR: `create_pr`、`get_pr`、`list_prs`、`pr_files`、`merge_pr_squash -> Result<sha, Conflict|NotFound>`
- 決定的な `node_id` 生成、issue のシリアライズ済み JSON キャッシュ（read ベンチ用）

## よく使うコマンド

```
cargo build --release && cargo test       # 作業の終わりに必ず
bash scripts/build.sh                      # forge-linux-x64 を作り直す（ソース変更時はコミットに含める）
bash scripts/grade.sh                      # ローカル採点（grade-local --worktree を直接使わない）
bash scripts/grade.sh --instances 3        # practice 3インスタンス
bash scripts/check-binary.sh               # 提出前
```
