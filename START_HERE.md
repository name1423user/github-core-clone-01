# はじめに（るりあ用の手順）

所要時間は 20 分ほど（Rust のクロスビルド環境の準備を含む）。Mac（Apple Silicon）のターミナルで進める。

## 1. Node 22 を確認（ランチャーとキットの採点用）
```sh
node -v   # v22.x.x であること
```
22 でなければ `brew install node@22` などで入れる。

## 2. Rust ツールチェーンと musl ターゲット
```sh
rustup --version || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
cargo --version                               # 1.85 以上（edition 2024）
rustup target add x86_64-unknown-linux-musl   # 提出用バイナリのターゲット
```

## 3. クロスコンパイルのリンカ（cargo-zigbuild を使う）
Mac から Linux amd64 の静的バイナリを作るためのリンカ。musl-cross ではなく cargo-zigbuild に統一する。
```sh
brew install zig
cargo install cargo-zigbuild
cargo zigbuild --version
```
（Linux x64 のマシンやクラウドの Claude Code では不要。rustc 同梱の musl で `cargo build --target x86_64-unknown-linux-musl` が通る）

## 4. 展開してキットを取る
```sh
mkdir -p ~/dev && cd ~/dev
unzip ~/Downloads/github-core-clone-01.zip   # → ~/dev/github-core-clone-01/
cd github-core-clone-01
bash scripts/fetch-kit.sh                    # → ~/dev/github-core-clone-01-kit/ ができる
```
フォルダ名を変えたいなら、この時点で変える（GitHub のリポジトリ名と揃える）。

## 5. ビルドを確認
```sh
bash scripts/build.sh dev   # → ./forge-darwin-arm64（Mac のローカル採点用。コミットしない）
bash scripts/build.sh       # → ./forge-linux-x64（提出用。x86-64 の静的 ELF か自動で検査される）
bash scripts/grade.sh       # ローカル採点。working 1 が出れば起動・登録は OK
```
- `node main.ts` は、マシンに合ったバイナリ（Mac なら `forge-darwin-arm64`）を起動するだけ。
- **採点は必ず `bash scripts/grade.sh` で回す。** `grade-local --worktree` を直接使うと、.gitignore された Mac 用バイナリが入らず「起動しない」ように見える。

## 6. GitHub に公開リポジトリを作る
```sh
git init -b main
git add -A && git commit -m "scaffold"
gh repo create github-core-clone-01 --public --source . --push
gh repo edit --add-topic makefun --homepage "https://make.fun/bounty/github-core-clone-01"
```
（gh を使わない場合は Web で作成し、About に make.fun のリンクと topic `makefun` を設定する）
`git add -A` の前に `git status` で `forge-darwin-arm64` や `target/` が入っていないことを確認する（.gitignore で除外済み）。

## 7. Claude Code で始める
```sh
claude
```
起動したら `/model` でいちばん強いモデルを選び、次を入力する:
```
/kickoff
```
あとは Claude がキットを読んで仕様をまとめ、Rust の土台（HTTP・JSON・ストア）を作るところまで進める。

## 8. その後の回し方
| コマンド | いつ |
|---|---|
| `/parallel` | 4つの担当（git・REST・GraphQL・MCP）を同時に動かして、落ちているチェックを潰す。合格率100%になるまで繰り返す |
| `/grade` | 採点だけしたいとき |
| `/perf` | 合格率100%になってから、速度を詰める（上位は perf で競っている） |
| `/presubmit` | 提出の前（バイナリが x86-64 の静的 ELF か・ソースより新しいかも検査する） |

## 大事なこと
- **Claude Code は必ずこのフォルダで動かす。** 提出には作業記録（`~/.claude/projects/` のセッションログ）が必要で、キットの `bundle` がここから拾う。提出が終わるまでログを消さない。
- コードを手で大量に書き換えない（「30% 以上が AI のセッション由来」の審査があるため。少しの手直しは問題ない）。外部クレートを vendor しないのも同じ理由。
- **ソースを直したら `bash scripts/build.sh` で `forge-linux-x64` を作り直し、一緒にコミットする。** 古いバイナリを提出すると古い実装が採点される。
- ウォレット連携・署名・提出は自分で操作する。Claude には提出コマンドを実行させない。
- 締切は **11/21（土）15:30（日本時間）**。前日のうちに1回提出して受付審査を通しておくと安全。最後の提出が最終エントリになる。

## 目安のスケジュール
- 11/18 朝: `/kickoff`（仕様抽出と Rust の土台）
- 11/18〜19: `/parallel` を回して全チェックを通す
- 11/19 夜: 合格率 100%、practice 3インスタンスで確認（`bash scripts/grade.sh --instances 3`）
- 11/20: `/perf` → `/presubmit` → 1回目の提出
- 11/21 午前: 最終調整と最終提出（15:30 まで。ハウスは 11:30 に入る）
