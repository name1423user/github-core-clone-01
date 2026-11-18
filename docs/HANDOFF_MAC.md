# HANDOFF_MAC — Mac（るりあ本人）でしかできないこと

作成: 2026-11-18 16:33 JST（07:33 UTC）。締切: 2026-11-21 06:30 UTC（JST 11/21 15:30）。最終提出は数時間前までに。

## 先に結論

- **2コア制限の測定は Mac でやらなくてよい。クラウドのほうが正確。**
  - クラウドのコンテナ（x86-64）では `dockerd` が起動でき、`grade-local --driver docker` が通る。
  - docker ドライバは `rules/instance.yaml` の制限（2 CPU・1024 MB・256 プロセス）をそのままかけた、キットが言う「exact」な採点。
  - Mac（Apple Silicon）の Docker では x86-64 の `forge-linux-x64` がエミュレーションで動くので、速度の数値が歪む。
- Mac で必要なのは**提出まわり**と**作業記録（トランスクリプト）の確認**だけ。

## 1. Mac でやること（るりあ本人）

### 1-1. 作業記録の確認（いちばん大事。早めに）
- 提出には AI セッションの作業記録が必要。`competition.yaml` の thinking_* 系の条件では、counted lines の 30% 以上が AI セッション由来であること。
- ところが、クラウドのセッションのトランスクリプトはそのコンテナの中にしか残らない。
  - 2026-11-18 の時点で、このコンテナで `bundle --dry-run` が拾えるのは今のセッションの分だけだった。
  - それより前のクラウドのセッション（`/kickoff`・`/parallel` など）の分はここに無い。
- Mac でやること:
  1. 最新の main を pull したリポジトリで `node $KIT/cli/makefun.mjs bundle --repo . --out /tmp/b.tgz --dry-run` を実行し、何件のセッションが拾われるか見る（Mac の Claude Code で作業した分、`--claude-desktop-dir` が拾う分）。
  2. 拾われていないセッションがあれば、どこにあるか（claude.ai のクラウドセッションの履歴など）を確認する。持ってこられない場合は、次の提出までにどう扱うかを決める。
- **これからのクラウドのセッションは、終わる前にその場で `bundle` を作って保存する**運用にするかも決める（コンテナが消えるとトランスクリプトも消える）。

### 1-2. 提出（CLAUDE.md §7・docs/SUBMIT.md）
- make.fun アカウントとウォレットの連携、署名、提出コマンドの実行。Claude は提出コマンドを実行しない。
- GitHub リポジトリの設定（public、About に make.fun のリンク、topic `makefun`、README のバッジ）。
- 提出は1日5回・合計10回まで。最後の提出が最終エントリ。
- 提出するコミットでは、`bash scripts/check-binary.sh` が全部 OK であること（Mac 版バイナリを混ぜない）。

### 1-3. run record の空欄を埋める（docs/RUN_RECORD.md）
- `claude --version`、使ったモデル・effort・MCP サーバ。Mac の Claude Code で作業した分はそちらの値を書く。

## 2. Mac でやらなくてよいこと（クラウドでやる）

- 2コア・1024 MB・256 プロセス制限での測定 → クラウドで docker ドライバ（手順は §3）
- バイナリのビルド・採点・決定性の確認 → クラウドで `scripts/build.sh`・`scripts/grade.sh`
- Mac 用のバイナリ `forge-darwin-arm64` → Mac で動作を確認したいときだけ（`bash scripts/build.sh dev`。コミット禁止）

## 3. クラウドで docker ドライバの採点を回す手順（次のセッション向け）

```
bash scripts/fetch-kit.sh                      # キット（make.fun を許可した環境「makefun-forge」が必要）
rustup target add x86_64-unknown-linux-musl
(dockerd >/tmp/dockerd.log 2>&1 &); sleep 5; docker info | grep 'Server Version'
cd ../github-core-clone-01-kit
node cli/makefun.mjs grade-local --kit . --bot ../github-core-clone-01 --driver docker   # HEAD のコミットを採点
```

- `--bot` にリポジトリを渡すと HEAD のコミットが採点される。未コミットの変更は入らないので、コミットしてから回す。
- ベンチ値: `/root/.makefun/trials/github-core-clone-01/<run>/trial-0/state/perf.json` の `workloads[].p50_ms`。

## 4. いまの状態（docker ドライバでの初回結果）

2026-11-18 16:31 JST、コミット 9ae97dd（自作アロケータ入り）、practice trial-0:

| ベンチ | p50 | 満点ライン（≈T_ref/4） | 点 |
|---|---|---|---|
| push | 219.3 ms | 410 ms | 50 |
| clone | **84.6 ms** | 66.7 ms | **46** |
| read | 654.1 ms | 690 ms | 50（余裕は小さい） |
| write | **108.7 ms** | 87.2 ms | **46** |

- 合計 **992点**（feature 600・agent 200・perf 192）。replay はバイト一致。
- process ドライバ（制限なし）ではずっと 1000点だったので、**本番相当の条件では perf が満点を割っている**。
- 次の perf の作業は、docker ドライバの数値を基準に clone・write（と余裕の小さい read）を詰めること。
- 本番の実行環境は gVisor（`program/env/src/wire.ts:22`）。docker ドライバの runtime は runc なので、システムコールのコストは本番より軽い可能性がある。gVisor（runsc）で測るなら、ダウンロード元のホストを環境で許可する必要がある（未確認）。
