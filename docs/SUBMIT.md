# 提出チェックリスト

`/presubmit` で Claude が確認する項目と、るりあ本人が操作する項目。

## Claude が確認する
- [ ] `node scripts/compliance.mjs` が OK（サイズ上限・ランチャーの import・8080 なし・依存なし・開発用バイナリの混入なし・秘密情報なし）
- [ ] **`bash scripts/check-binary.sh` が全部 OK**（HEAD の `forge-linux-x64` が `file` で x86-64 の静的 ELF、8 MiB 以下、100755、ソースより古くない。Mac 版の誤提出防止）
- [ ] 最新のローカル採点で全チェック合格、決定性 OK（`docs/PROGRESS.md` の先頭行）
- [ ] practice の3インスタンスすべてで合格
- [ ] git 履歴の全体に秘密情報がない
- [ ] `node $KIT/cli/makefun.mjs bundle --dry-run` で、このフォルダの全セッション（サブエージェント含む）が拾われている
- [ ] `Cargo.toml` の `version` と台帳 ruria-versions-registry が一致している
- [ ] `docs/RUN_RECORD.md` が埋まっている

## るりあが操作する
- [ ] GitHub リポジトリが public で、2026-11-14 06:30Z 以降に作成したもの
- [ ] GitHub の About に make.fun へのリンク、topic に `makefun`
- [ ] README のバッジがキットのテンプレどおり（OWNER/REPO を自分のものに）で、表示される
- [ ] make.fun のアカウントとウォレットの連携（自分で承認）
- [ ] 作業記録の束を作る: `node $KIT/cli/makefun.mjs bundle`（個人情報だけ伏せてよい。コード・コマンド・ツールの出力・推論は伏せない）
- [ ] 提出（repo_url・40文字の commit_sha・bundle・run record）。CLI の提出コマンドはキットの RULES.txt / skill.md の手順に従う
- [ ] 提出は1日5回・合計10回まで。**最後の提出が最終エントリ**。締切: 2026-11-21 06:30 UTC（日本時間 15:30）

## 提出後
- [ ] 受付審査で弾かれた項目があれば、`docs/PROGRESS.md` に記録して直す
