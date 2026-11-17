# CLAUDE.md — make.fun bounty `github-core-clone-01`

目的: **ハウス参照実装（house entry）を統計的に有意に上回り、1位を取る。**
締切: 2026-11-21 06:30 UTC（日本時間 11/21 15:30）。最終提出は締切の数時間前までに済ませる。

## ディレクトリ配置（前提）

```
<親フォルダ>/
├── github-core-clone-01-kit/   ← キット（scripts/fetch-kit.sh で取得。コミットしない）
└── <このリポジトリ>/            ← 提出物。直下に main.ts
```

キットのパスは `../github-core-clone-01-kit`（環境変数 `KIT` で上書き可）。`.claude/settings.json` で読み取り許可済み。

---

## 0. 絶対ルール（違反＝失格または0点）

- エントリは repo 直下の `main.ts`（Node 22）。**標準ライブラリ（`node:` で始まる import）＋自分で書いたコードのみ**。npm パッケージ・git バイナリ・外部サービス禁止。`package.json` に dependencies を書かない。
- 1プロセス。**エフェメラルポートを bind して suite に登録**、8080 は絶対に bind しない。登録方法はキットの `starter/main.ts` と `program/env/src/main.ts` を読んで正確に合わせる（推測しない）。
- **決定的であること**: 同じインスタンスで2回走らせた state export がバイト一致。state に `Date.now()`・`new Date()`・`Math.random()`・`crypto.randomUUID()`・プロセスID・不定順の反復を入れない。ID は単調カウンタ、時刻が必要なら固定値か入力から導出。export は常にキーをソートして出力。
- ソース合計 16 MiB 以下、1ファイル 8 MiB 以下。
- Node 22 の型ストリップで動く TS だけ書く（`enum`・`namespace`・コンストラクタ引数プロパティ禁止、import は `.ts` 拡張子付き）。起動方法はキットの harness に合わせて確認する。
- `RULES.txt` に `model_call`（毎トライアルでゲートウェイ経由のモデル呼び出し）が本競技で必須かどうか書かれているはず。**最初に確認**し、必須なら起動時に1回だけ呼ぶ。必須でなければ入れない。
- 秘密情報（APIキー、ウォレットの鍵）を絶対にコミットしない。`scripts/compliance.mjs` が検査する。

## 1. 作業の進め方（この順番を守る）

1. **キットを全部読む。コードを書く前に。** → `spec-analyst` に任せる（`/kickoff`）
   読む順: `RULES.txt` → `competition.yaml` → `rules/instance.yaml` → `starter/*` → `program/env/src/main.ts` → `manifest.ts` → `export.ts` → `checks-*.ts` → `*-client.ts`・`wire.ts` → `bench.ts`・`calibration.json` → `program/scorer/*`。
2. `docs/CHECKS.md`（全チェックの一覧＝TODO本体）、`docs/SCORING.md`（採点式）、`docs/INTERFACES.md`（起動・登録・export の正確な仕様）を作る。
3. 共有の土台（`main.ts`・`src/store/`・ルーティング）をリードが作る。
4. 4つのインターフェースを担当エージェントに**並列で**実装させる（`/parallel`）。
5. 機能を足すたびに `/grade`。落ちたチェックは `grader` が担当者に振り分ける。
6. 合格率 100% に達してから `perf-engineer` で速度を詰める。最適化のたびに全チェックを再実行する。
7. `determinism-auditor` で決定性を確認し、提出前に `/presubmit`。
8. practice の3インスタンスすべてで確認する。本番は締切後に別の3インスタンスが引かれるので、**practice の具体値（リポジトリ名・件数など）をハードコードしない**。

## 2. チーム構成と持ち場（並列で衝突させないための決まり）

| 担当 | 持ち場（書いてよい場所） | 役割 |
|---|---|---|
| **リード（メインセッション）** | `main.ts`、`src/store/**`、`src/http.ts`、`src/types.ts`、`docs/DECISIONS.md` | 土台・共有の型・ストア・ルーティング、統合、判断 |
| `spec-analyst` | `docs/CHECKS.md`・`docs/SCORING.md`・`docs/INTERFACES.md` | キットの読解と仕様の抽出（コードは書かない） |
| `git-engineer` | `src/git/**` | smart HTTP、pack、receive/upload-pack、shallow、PRマージ用のツリーマージ |
| `rest-engineer` | `src/rest/**` | REST v3 |
| `graphql-engineer` | `src/graphql/**` | 自前の GraphQL パーサ・実行器・スキーマ |
| `mcp-engineer` | `src/mcp/**` | MCP（JSON-RPC 2.0） |
| `grader` | `docs/CHECKS.md` の状態欄、`docs/PROGRESS.md` | ローカル採点の実行・失敗の切り分け・振り分け |
| `perf-engineer` | 担当者の合意のうえで各 `src/**` | 4つのベンチの高速化 |
| `determinism-auditor` | なし（読むだけ） | 不定要素の検出と2回実行の差分確認 |
| `submission-auditor` | なし（読むだけ） | 提出前の規約・審査項目の確認 |

- **他人の持ち場は編集しない。** ストアや共有の型に変更が必要なら、作業を止めて「必要な変更」をリードへの報告に書く。リードが `src/store/` を直してから再開する。
- 各インターフェースはストアの公開関数だけを呼ぶ。REST・GraphQL・MCP は同じストア関数を共有する（同じ操作の結果が食い違わないようにするため）。
- 担当エージェントは作業の最後に、`docs/CHECKS.md` のどのチェックを通したか・残りは何かを報告する。

## 3. アーキテクチャ

```
main.ts             起動・ポート登録・http.createServer（keepAlive）
src/http.ts         パス分割＋switch のルーター（正規表現の羅列にしない）
src/types.ts        共有の型
src/store/          リポジトリ・オブジェクト・ref・issue・PR・コメント・ラベルのインメモリストア、決定的 export
src/git/            object / pack / pktline / smart-http / merge
src/rest/           REST v3
src/graphql/        parser / executor / schema / resolvers
src/mcp/            transport / tools
```

### 3.1 Git smart HTTP（最重要・最難関）
- `GET /{owner}/{repo}.git/info/refs?service=git-upload-pack|git-receive-pack`、`POST .../git-upload-pack`、`POST .../git-receive-pack`。`.git` なしのパスも受けるかは checks で確認する。
- suite のクライアントは本物の git ではなく `git-client.ts`。**それが送るリクエストを正確に読み、その範囲を完全に実装する**（capabilities、`shallow`/`deepen`、have/done のネゴシエーション、side-band の有無、report-status の形式、ref の削除、non-fast-forward の拒否、atomic）。protocol v2 を使うかどうかも client から判断する。
- receive-pack: pack を解析 → OFS_DELTA/REF_DELTA を解決 → 全オブジェクトを検証（SHA-1 の一致、参照先の存在） → ref を**原子的に**更新 → report-status。old-oid が合わなければ拒否する。
- upload-pack: wants から haves を除いた到達集合を計算（shallow 境界を正しく扱う）して pack を生成。デルタ圧縮は不要。
- リクエストボディが `Content-Encoding: gzip` なら `node:zlib` で展開する。

### 3.2 REST v3
- `checks-rest.ts` が叩くものを全部実装する。目安: repos、branches、git refs、contents（GET/PUT/DELETE でサーバ側にコミットを作る）、issues（CRUD・comments・labels・assignees・state）、pulls（作成・取得・一覧・更新・merge・files・commits）。
- レスポンスの形は GitHub に合わせる（`id`、`node_id`、`number`、`url` 系、`user`、`state`、日時）。**日時は決定的な値**にする。
- エラーは `{"message","documentation_url"}`、422 には `errors` 配列。ステータスコードは GitHub 準拠。
- ページネーション（`per_page`/`page`、`Link` ヘッダ）、ソート・フィルタは checks が使うものを正確に実装する。

### 3.3 GraphQL
- ライブラリなしでパーサと実行器を書く。対応するもの: query/mutation、変数とデフォルト値、フラグメント、インラインフラグメント、エイリアス、`__typename`、`@include`/`@skip`。
- スキーマは gh と github-mcp-server が送るクエリの範囲（`checks-graphql.ts`・`graphql-client.ts` に実物がある）。`node`/`nodes`、connection（`edges`/`nodes`/`pageInfo`/`totalCount`、`first`/`after`）。
- エラーは `{"errors":[{"message","path","locations","type"}]}`。グローバル node ID は REST の `node_id` と一致させる。

### 3.4 MCP
- transport とパスは `checks-mcp.ts`・`mcp-client.ts` に合わせる（`initialize` → `notifications/initialized` → `tools/list` → `tools/call`、セッションIDの扱い）。
- ツール名・引数・戻り値は github-mcp-server 互換にし、checks にあるものを全部実装する。`inputSchema` を正しく出す。

## 4. 性能（4つのベンチ）

- **200 pushes**: Buffer を直接走査し、文字列化しない。オブジェクトは受信時に1回だけ展開・ハッシュする。ref 更新は O(1)。
- **50 concurrent shallow clones**: 同じ（repo、ref の版、wants、depth）の pack をキャッシュして使い回し、ref 更新で無効化する。オブジェクトは deflate 済みのバイト列も持っておき、pack 生成を「ヘッダ＋保存済みの圧縮バイトの連結＋SHA-1」にする。
- **2,000 issue reads**: issue の JSON をシリアライズ済み文字列でキャッシュし、更新時に無効化する。
- **200 issue writes**: 書き込みは O(1)、export は遅延生成。ホットパスに同期 I/O やログを置かない。
- 共通: keepAlive を有効にする。上限は 2 CPU・1024 MB なのでメモリを溜めすぎない。

## 5. バージョン管理

- 版番号ファイルは `package.json` の `version`（0.1.0 から）。Git タグは打たない。
- **最初の編集の前に version-registry スキルで台帳 `ruria-versions-registry` と照合する。** 未登録なら、GitHub のリポジトリを作った後に新規登録する。
- 節目で版を上げる（目安: 全インターフェースの骨組み → 0.2.0、合格率100% → 0.3.0、性能チューニング後 → 0.4.0、最終提出 → 1.0.0）。上げたら版番号ファイルと台帳（`project.toml`・CHANGELOG・README）をスキルの手順でまとめて更新する。

## 6. 提出（るりあ本人の承認・操作が必要なもの）

- `docs/SUBMIT.md` のチェックリストに従う。ウォレット連携・署名・提出はるりあ本人が行う。
- 作業記録: このフォルダで行った Claude Code のセッション（**サブエージェント含む**）を `node $KIT/cli/makefun.mjs bundle` で束ねる。`--dry-run` で全部拾えているか先に確認する。
- 提出は1日5回・合計10回まで。**最後の提出が最終エントリ**なので、壊れた版を最後にしない。

## 7. やってはいけないこと

- テストを読まずに「GitHub っぽく」実装すること
- practice インスタンスの具体値をハードコードすること
- 速度のために決定性を壊すこと（export の不一致は全損）
- ローカル採点を回さずに大量のコードを書くこと
- 他の担当者の持ち場を勝手に書き換えること
