# 採点式

`spec-analyst` が `program/scorer/*`・`bench.ts`・`calibration.json` から補完する。下の値はリードが再構築時に RULES.txt・competition.yaml・calibration.json で確かめたもの。

## スコア式（0〜1000）
- feature（600）: `round(600 × 合格数 / 総数)`。チェックは最大120: git 30・repos 15・issues 15・pulls 20・graphql 20・mcp 15・protocol 5。
- agent（200）: `round(200 × MCP 合格数 / MCP 総数)`（MCP 15件がもう一度効く）。
- perf（200）: 4ベンチの合計。各 `round(clamp(25 − 12.5·log2(t / T_ref), 0, 50))`、t は3ラウンドの中央値（ms）。
- タイブレーク: `perf_p50_us = round(1000 × 4ベンチの中央値の幾何平均)`、小さいほど上。その次は受付が早い方。

## 各ベンチの基準値（calibration.json、ハウスの3回の中央値）
| ベンチ | 内容 | T_ref (ms) | 満点 50 の目安（T_ref/4） | 25点 |
|---|---|---|---|---|
| push | 1 KiB の変更を200回、逐次 push | 1640.82 | ≤ 410 | 1640 |
| clone | depth 1 の shallow clone を50並列（200コミット） | 266.941 | ≤ 66.7 | 267 |
| read | GET issue を2,000回、1本の keep-alive | 2759.715 | ≤ 690 | 2760 |
| write | issue 作成を200回、逐次 | 348.965 | ≤ 87.2 | 349 |

各ベンチは5回の暖機のあと3ラウンド。入力はすべて計測前に作られる。60 s を超えると 0 点。

## ランキングとハウス超えの判定
- 本番の3インスタンスの平均で順位。賞はハウスより**厳密に高く**、かつインスタンスごとの差の 90% paired t-interval が 0 より上であること（区間が 0 にかかれば統計的な引き分けで、賞なし）。
- 決定性: 本番は live と replay の2回走り、state.json がバイト一致しないと unranked。

## 優先順位の提案
1. feature 600 と agent 200 を満点にする（上位は全員満点。MCP 15件は feature と agent の両方に効くので1件あたり最大）。
2. perf: 満点から遠いベンチから詰める。上位は perf で差がつく。
3. spec-analyst が CHECKS.md を作った後に、チェックごとの重み（1件 = 600/120 = 5点、MCP は +200/15 ≈ 13.3点）で更新する。
