# HANDOFF_MAC — Mac の Claude Code が引き継ぐための指示書

最終更新: 2026-11-18 17:31 JST（08:31 UTC）。締切: 2026-11-21 06:30 UTC（JST 11/21 15:30）。

**Mac の Claude へ: るりあさんは「めんどいしわからん」状態です。コマンドはあなたが実行し、るりあさんに頼むのは §3 の「本人にしかできない操作」だけにしてください。1回に頼むことは1つ、手順は短く。**

## 0. 前提（このファイルを読んだら最初に）

- リポジトリ: `/Users/idenomako/Desktop/Developer/makeFun/github-core-clone-01`（作業ブランチ `claude/amazing-thompson-ar7i8w`）
- キット: `/Users/idenomako/Desktop/Developer/makeFun/github-core-clone-01-kit`（`export KIT=...` で使う）
- 作業記録（jsonl）の置き場: `~/makefun-transcripts/`（リポジトリの外。**絶対にリポジトリへ入れない・コミットしない**。リポジトリは public）
- 手元の古い変更は `git stash`（名前 `mac-old-local-2026-11-18`）に退避済み。消さない。
- Claude は make.fun への `submit` を実行しない（CLAUDE.md §7）。`login` も本人の承認が要るので、本人に頼む。
- 最初に `git pull` で作業ブランチを最新にする。

## 1. いまの状態

`check`（提出前チェック）の結果（17:29 JST、コミット 3ed06c2）:
- 通過: ライセンス、エントリポイント、サイズ、秘密情報、記録の形式、時刻、ターン数、料金、run record など 14 項目
- **失敗は1つだけ: `thinking_reproduces`**。記録がカバーする行が 1,564 / 5,566（28.0%）で、必要な 30% まであと約 106 行。
- 原因の見込み: クラウドのセッション「**セッション引き継ぎ準備**」（11/18 03:44 UTC〜。REST・git・MCP・GraphQL の大部分を書いた）の記録が `~/makefun-transcripts/` に入っていない。
- いま入っている記録: このプロジェクトのクラウドセッションの本体3つ（`864768a7…`・`66de7ff8…`・`84a5d15d…`＝「Perf 調整」）とサブエージェント6つ。Mac の Claude Code のセッション（`f4d0b44b…`）は `~/.claude/projects/` から読む。

## 2. やること（この順）

### 2-1. 足りない記録をそろえる（本人の操作が要る → §3-A）
1. るりあさんに §3-A を頼む。届いたファイルを `~/makefun-transcripts/handoff/`（サブエージェントは `~/makefun-transcripts/handoff/subagents/`）に置く。ダウンロードフォルダに入るので、移すのはあなたがやる。
2. すでに入っているファイルと同じ名前（UUID）なら、その記録はもうあった、ということ。その場合は §4 を見る。

### 2-2. 束を作り直してチェックする（あなたが実行）
```
cd /Users/idenomako/Desktop/Developer/makeFun/github-core-clone-01
git pull
export KIT="$(cd ../github-core-clone-01-kit && pwd)"
args=(); for f in ~/makefun-transcripts/**/*.jsonl ~/.claude/projects/-Users-idenomako-Desktop-Developer-makeFun-github-core-clone-01/*.jsonl; do args+=(--session "$f"); done
node "$KIT/cli/makefun.mjs" bundle --repo . "${args[@]}" \
  --mcp claude-code-remote=https://api.anthropic.com/v2/ccr-sessions/-/meta/mcp \
  --mcp github=https://api.anthropic.com/v2/ccr-sessions/cse_01KG3BcgZZiThox7tPuDod7c/github/mcp \
  --mcp ccd_turn=builtin \
  --out ~/transcripts.tar.gz 2>&1 | grep -E 'session file|whole file|lines kept|wrote|missing'
node "$KIT/cli/makefun.mjs" check --kit "$KIT" --repo . --transcripts ~/transcripts.tar.gz 2>&1 | grep -E '^(FAIL|pass +thinking|[0-9]+ passed|not ready)'
```
- Mac のセッションを `--session` で明示的に渡すのは必須。自動で見つけさせると一部の行が除外され、`thinking_wellformed` で落ちる（実際に落ちた）。
- `--mcp` の3行も必須。無いと run record（作業記録の申告欄）が不足して束が作られない。
- 束は `~/transcripts.tar.gz`（リポジトリの外）。

### 2-3. 全部 pass になったら、提出の準備（本人の操作 → §3-B・§3-C）
- `check` の最後に `at submission` と出る5項目は、提出時にサイトが確かめるもの。§3-B で満たす。
- 提出するコミットは、`git log -1 --format=%H` で出る、作業ブランチの最新のコミットにする（origin に push 済みであること）。

## 3. 本人にしかできない操作（るりあさんに1つずつ頼む）

### 3-A. 足りない記録を送ってもらう
claude.ai でクラウドのセッション「セッション引き継ぎ準備」を開き、次の文をそのまま送ってもらう:
> `~/.claude/projects/` にあるこのセッションの `*.jsonl`（`subagents/` の中も）を SendUserFile で送って。コミットや push はしないで。

届いたファイルはダウンロードフォルダに入る。

### 3-B. GitHub の設定（ブラウザ）
- リポジトリ `name1423user/github-core-clone-01` を public にする。
- About の Website に `https://make.fun/bounty/github-core-clone-01`、Topics に `makefun`。
- make.fun のアカウントに GitHub の `name1423user` が結び付いていない場合は、`.makefun/owner.json` が要る（`check` の repo_owner の説明を参照）。結び付いているかを本人に聞く。

### 3-C. ログインと提出（ターミナル。あなたはコマンドを書いて渡すだけ）
```
node "$KIT/cli/makefun.mjs" login --origin https://make.fun
```
（表示されたリンクを本人が開いて承認。「Connected」と出るまで繰り返す）
```
node "$KIT/cli/makefun.mjs" submit --competition github-core-clone-01 \
  --repo-url https://github.com/name1423user/github-core-clone-01 \
  --commit <作業ブランチの最新のコミット（40文字）> \
  --title "Rust forge" --bundle ~/transcripts.tar.gz
```
- 提出は1日5回・合計10回まで。**最後の提出が最終エントリ**。今の版を保険として今日1回出し、perf を詰めた版を締切の数時間前に出し直す方針。
- 提出の前に `bash scripts/check-binary.sh` が全部 OK であること（Mac でも動く。Mac 版バイナリをコミットしない）。

## 4. 30% にまだ届かないとき

- 「セッション引き継ぎ準備」の記録が、すでに入っていたファイルと同じだった場合や、足しても足りない場合は、残りの手段は2つ。
  1. ほかのクラウドのセッション（「開発開始」11/17 22:14 UTC〜、「Session start」11/17 22:50 UTC〜）にも §3-A と同じことを頼み、まだ無い記録がないか確かめる。
  2. これからの perf の作業（クラウドで行う。§5）で書くコードが、そのセッションの記録として加わる。
- `check` の fix に従い、記録の書き換えや作り直しは絶対にしない（失格になる）。

## 5. Mac でやらなくてよいこと（クラウドでやる）

- perf の作業: 本番と同じ制限（2 CPU・1024MB・256 プロセス）をかけた docker の採点で、clone 84.6ms・write 108.7ms が満点のラインを超えて 46点ずつ（合計 992点）。これを満点に戻すのが次の作業（`docs/PROGRESS.md` の作業キュー）。
  - 計測はクラウドの x86-64 で、`dockerd` を起動して `grade-local --driver docker` で行う。Apple Silicon の Docker では x86-64 のバイナリがエミュレーションで動くので、数値が歪む。
- バイナリのビルド: `scripts/build.sh` で `forge-linux-x64` を作る。
- クラウドのセッションを終える前には、毎回そのセッションの jsonl を SendUserFile でるりあさんに送る（コンテナが消えると記録も消えるため）。
