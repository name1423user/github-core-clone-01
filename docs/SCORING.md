# 採点式

`spec-analyst` が `program/scorer/*`・`env/manifest.ts`・`env/bench.ts`・`env/calibration.json`・RULES.txt・competition.yaml から検証・補完した（`env/` = `program/env/src/`）。リード記載の旧版の値はすべてソースと一致した。補足と訂正は「補足」と書いた。

## 1. スコア式（0〜1000、1 試行）

scorer は `state.json` と `perf.json` だけを読む。`/reference` は読まない（program/scorer/main.ts:262-356, score.ts:166-194）。

| 列 | 式 | 根拠 |
|---|---|---|
| feature（0〜600） | `round(600 × counts.passed / counts.total)`。total は常に 120 | score.ts:89-93, :173; env/manifest.ts:261-265, :497-512 |
| agent（0〜200） | `round(200 × byGroup.mcp.passed / byGroup.mcp.total)`（total 15） | score.ts:124-136, :174 |
| perf（0〜200） | perf.json の `points_total`（env が計算済み） | score.ts:152-158, :176 |
| score | feature + agent + perf | score.ts:175-177 |
| perf_p50_us | perf.json の `perf_p50_us` | score.ts:157, :180 |
| working | state.working = 1 かつ passed > 0 なら 1 | score.ts:181; env/wire.ts:706-708 |

- 丸めは `Math.round`（0.5 は切り上げ）（score.ts:85-93）。feature は 120 件なので **1件 = ちょうど 5 点**。agent は 1件 ≒ 13.33 点（例: 14/15 → 187、13/15 → 173）。
- **mcp の1件は feature 5 ＋ agent 13.33 = 約 18.3 点**。他の1件は 5 点（env/manifest.ts:18-19）。
- export が壊れている・無い場合は全列 0（score.ts:166-194）。state.json の counts が checks のビットと矛盾すると 0（score.ts:119-123。env が書くのでフォージ側では起きない）。
- working は bootOk（登録＋/healthz 2回 200）が必要（env/export.ts:127, env/main.ts:644-665）。working は順位に関係しない（RULES.txt:51）。

## 2. perf（4 ワークロード）

- 1 ワークロードの点: `round(clamp(25 − 12.5 × log2(t / T_ref), 0, 50))`。t は 3 ラウンドの中央値（ms）（env/manifest.ts:448-453, env/bench.ts:228-246）。
- 各ワークロード: 暖機 5 操作（計測外）→ 3 ラウンド。入力は計測前に全部作る。1 ワークロードの予算 60 s、phase 全体 240 s。途中で失敗・時間切れ・1 ラウンドでも欠けたら p50 = 1e9（センチネル）で **0 点**（env/manifest.ts:337-388, env/bench.ts:23-41, :228-246）。
- 順番は push → clone → read → write（env/manifest.ts:351-388, env/bench.ts:829-863）。
- T_ref（calibration.json の各ワークロード `p50_ms` = ハウス参照フォージの 3 回の実行の中央値。T_ref 自体は計測環境 dsh-makefun の makefun-dev、split レイアウト）（env/calibration.json:239-243, :310-315; env/manifest.ts:405-424）:

| ベンチ | 内容 | T_ref (ms) | 50点（t ≤ T/3.89） | 38点（T/2） | 25点（T） | 13点（2T） | 0点（t ≥ 3.89T） |
|---|---|---|---|---|---|---|---|
| push | 1 KiB の変更を200回逐次 push（1回 = GET info/refs + POST） | 1640.82 | ≤ 421.8 | 820.4 | 1640.8 | 3281.6 | ≥ 6383 |
| clone | depth 1 の shallow clone を50並列、200 コミット（1回 = GET×3 + POST×2） | 266.941 | ≤ 68.6 | 133.5 | 266.9 | 533.9 | ≥ 1038 |
| read | GET issue を2,000回逐次、1本の keep-alive | 2759.715 | ≤ 709.5 | 1379.9 | 2759.7 | 5519.4 | ≥ 10735 |
| write | issue 作成を200回逐次 | 348.965 | ≤ 89.7 | 174.5 | 349.0 | 697.9 | ≥ 1357 |

  補足: 50 点の境界は「生の値 ≥ 49.5」なので t ≤ T × 2^(−1.96) ≈ T/3.89（旧版の T/4 はやや厳しめの目安）。1 点刻みの境界は 1 点 = log2 で 0.08 = 約 5.7% の時間差。
- 1 操作あたりの目安（50 点境界）: push 約 2.1 ms/回（HTTP 2 往復）、clone 1 ラウンド 68.6 ms（50 並列×5 往復）、read 約 0.35 ms/回、write 約 0.45 ms/回。
- perf_p50_us（タイブレーク）= `round(1000 × 計測できたワークロードの p50_ms の幾何平均)`、各 p50 は 0.001 ms で下限、失敗したワークロード（1e9）は平均から除外、全滅なら 1e9（env/manifest.ts:482-487, env/bench.ts:282-285）。小さいほど上。
- calibration の 3 回の実行間でも perf_p50_us は 26% ばらつく（env/calibration.json:268-278）。ハウス自身が中央値ちょうどなら perf 100/200（env/calibration.json:317）。

## 3. 順位とハウス超え

- 練習: 提出ごとに practice/trial-0 で 1 回（live のみ）（RULES.txt:63, :151）。
- 本番: 締切後に drand round 34146612 から 3 インスタンスを引く（rules/instance.yaml:30-36）。全員が 1 つ目を live で 1 回（予選）。上位 60 と 60 位タイは 3 インスタンスすべて、各試行を live＋replay（RULES.txt:62-68）。ハウスは常に 3 インスタンス・枠を取らない（RULES.txt:72）。
- 順位 = 3 インスタンスの**平均スコア**。同点は perf_p50_us の小さい方、次に受付の早い方（RULES.txt:66, :77, :224）。
- **ハウス超え（賞の条件）**: 3 インスタンスの平均がハウスより厳密に高く、かつインスタンスごとの差（自分 − ハウス）の **90% paired t-interval が 0 より上**。区間が 0 にかかれば統計的引き分けで賞なし（RULES.txt:73, competition.yaml:439-445）。
  - 区間が両側か片側かは RULES に書かれていない（**不明**、RULES.txt:73, :224）。両側 90%・自由度 2 なら t = 2.920 で、条件は「差の平均 > 2.920 × 差の標準偏差 / √3 ≈ 1.69 × 標準偏差」。差が 3 回とも同じ（標準偏差 0）なら平均 > 0 で足りる。→ **3 インスタンスすべてで安定してハウスを上回る**ことが必要。
- 決定性: contender の各試行は replay され、state.json がバイト一致しないと unranked（RULES.txt:67, :184）。state.json は合否ビットだけなので、**同じ入力に同じ合否**になれば良い（env/export.ts:9-16）。
- 全インスタンスで suite の入力データ（scenario）は同じ（SCENARIO_SEED は wire.ts の定数、env/wire.ts:751-754; competition.yaml:29-31; practice/trial-*/reference/scenario.json）。インスタンス間で変わるのはトークン等で、スコア差は主に perf の計測ばらつき。

## 4. ハウスの位置

- ハウス参照フォージは 120/120（git 30・repos 15・issues 15・pulls 20・graphql 20・mcp 15・protocol 5）、working 1（env/calibration.json:303-308）。→ ハウスは feature 600 + agent 200 = 800 を取る前提。
- ハウスの perf は T_ref の定義上、中央値の venue なら 100 点前後。本番 venue は production の `pod` レイアウトで、T_ref を測った split レイアウトと違う（env/calibration.json:242, :309）。本番でのハウスの perf は**不明**。
- よって **ハウス超え ＝ 800 点満点を取ったうえで、perf でハウスより安定して上**。1 件でもチェックを落とすと −5 点（mcp なら −18 点）で、perf で取り返す必要がある。

## 5. 点が多く乗る場所と優先順位の提案

点の配分: conformance 800（うち mcp 15 件で 275、git 30 件で 150、REST 55 件で 275、graphql 20 件で 100）、perf 200。

1. **土台（lead）**: 登録・/healthz・認証 401・JSON・`X-GitHub-Request-Id`・404/400・`POST /user/repos`（重複は 422 + `already exists`）・空リポジトリへの PUT contents。全面の前提（git も MCP も GraphQL fixture もここから始まる）。
2. **MCP（275 点、1件 18.3 点）**: 実装量は小さいのに点が最大。REST の repos・contents・git/refs・issues・pulls・merge が前提。modern（server/discover）で実装するなら parse-error は HTTP 400 が必須。
3. **git（150 点＋GraphQL の PR 4 件の前提）**: v2 ls-refs・fetch（shallow 含む）・v0 receive-pack（side-band 内 report-status）。bench の push・clone もこれ。
4. **REST の issues・pulls・contents（約 200 点）**: スカッシュマージとパス単位の衝突検出（405）、PR files の行数。
5. **GraphQL（100 点）**: 18 文書だけ通るパーサ・実行器。
6. **perf（200 点、勝負所）**: 120/120 になってから。read（2,000 回）と write は応答生成の固定費、push は 1 回 2 往復の固定費、clone は 50 並列の pack 生成（キャッシュ）。各ベンチ T_ref の約 1/3.9 で頭打ちなので、満点に届いたベンチより届いていないベンチを先に。
