# はじめに（るりあ用の手順）

所要時間は 10 分ほど。Mac のターミナルで進める。

## 1. Node 22 を確認
```sh
node -v   # v22.x.x であること
```
22 でなければ `brew install node@22` などで入れる。

## 2. 展開してキットを取る
```sh
mkdir -p ~/dev && cd ~/dev
unzip ~/Downloads/github-core-clone-01.zip   # → ~/dev/github-core-clone-01/
cd github-core-clone-01
bash scripts/fetch-kit.sh                    # → ~/dev/github-core-clone-01-kit/ ができる
```
フォルダ名を変えたいなら、この時点で変える（GitHub のリポジトリ名と揃える）。

## 3. GitHub に公開リポジトリを作る
```sh
git init -b main
git add -A && git commit -m "scaffold"
gh repo create github-core-clone-01 --public --source . --push
gh repo edit --add-topic makefun --homepage "https://make.fun/bounty/github-core-clone-01"
```
（gh を使わない場合は Web で作成し、About に make.fun のリンクと topic `makefun` を設定する）

## 4. Claude Code で始める
```sh
claude
```
起動したら `/model` でいちばん強いモデルを選び、次を入力する:
```
/kickoff
```
あとは Claude がキットを読んで仕様をまとめ、土台を作るところまで進める。

## 5. その後の回し方
| コマンド | いつ |
|---|---|
| `/parallel` | 4つの担当（git・REST・GraphQL・MCP）を同時に動かして、落ちているチェックを潰す。合格率100%になるまで繰り返す |
| `/grade` | 採点だけしたいとき |
| `/perf` | 合格率100%になってから、速度を詰める |
| `/presubmit` | 提出の前 |

## 大事なこと
- **Claude Code は必ずこのフォルダで動かす。** 提出には作業記録（`~/.claude/projects/` のセッションログ）が必要で、キットの `bundle` がここから拾う。提出が終わるまでログを消さない。
- コードを手で大量に書き換えない（「30% 以上が AI のセッション由来」の審査があるため。少しの手直しは問題ない）。
- ウォレット連携・署名・提出は自分で操作する。Claude には提出コマンドを実行させない。
- 締切は **11/21（土）15:30（日本時間）**。前日のうちに1回提出して受付審査を通しておくと安全。最後の提出が最終エントリになる。

## 目安のスケジュール
- 11/17 夜: `/kickoff` まで
- 11/18: `/parallel` を回して大半のチェックを通す
- 11/19: 合格率 100%、practice 3インスタンスで確認
- 11/20: `/perf` → `/presubmit` → 1回目の提出
- 11/21 午前: 最終調整と最終提出（15:30 まで）
