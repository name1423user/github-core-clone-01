# 全チェック一覧（120件）

`spec-analyst` が作成、`grader` が状態欄を更新する。根拠はキット内のパス:行（`env/` = `program/env/src/`）。

- 件数（`suiteChecks()` を実行して数えた結果、env/main.ts:622-624）: git 30・repos 15・issues 15・pulls 20・protocol 5・graphql 20・mcp 15 = 120。competition.yaml:60-62 と env/manifest.ts:65-73 に一致。`validateCheckSpecs` の指摘なし。
- 実行順はこの表の順（git → repos → issues → pulls → protocol → graphql → mcp、1件ずつ逐次）（env/main.ts:622-624, :697-735）。
- タイムアウト: git・graphql は各チェック指定値、REST・MCP は既定 15 s。いずれも 5〜15 s に clamp（env/manifest.ts:199-205, env/wire.ts:316-320）。
- 重み: 1件 = feature 5 点（600/120）。mcp は feature 5 点 + agent 13.33 点（200/15）= **18.33 点**（env/manifest.ts:13-19）。
- 担当: git / rest / graphql / mcp / lead（lead = ルーター・認証・共通ヘッダなど横断部分）。
- 共通前提（全 REST 系）: `Authorization: Bearer` 必須、JSON、`X-GitHub-Request-Id`、owner = `adaiversen`（docs/INTERFACES.md §4）。git 系はすべて REST の `POST /api/v3/user/repos` でリポジトリを作ってから始まる（checks-git.ts:258-269）。

## git（30）— 担当 git、重み各 5

| ID | 面 | 要求 | 期待 | 重み | 状態 | 担当 | 根拠 |
|---|---|---|---|---|---|---|---|
| git.advertise-v2 | git | 初回 push 後、GET info/refs?service=git-upload-pack（Git-Protocol: version=2） | 200、v2 広告（`version 2`）、`fetch` と `ls-refs` を含む、object-format は無いか sha1 | 5 | TODO | git | checks-git.ts:394-408 |
| git.advertise-receive-pack | git | GET info/refs?service=git-receive-pack | v0 広告、refs/heads/main を tip で列挙、caps に report-status・side-band-64k・delete-refs | 5 | TODO | git | checks-git.ts:410-426 |
| git.advertise-headers | git | v0 upload-pack（Git-Protocol なし）・v2 upload-pack・receive-pack の info/refs | 3つとも 200、content-type `application/x-git-upload-pack-advertisement` / `…-receive-pack-advertisement`。v0 の1行目 `# service=git-upload-pack` か `version 2`、receive-pack の1行目 `# service=git-receive-pack`、v2 の1行目 `version 2`（`# service=…` でも本チェックは通る） | 5 | TODO | git | checks-git.ts:428-463 |
| git.unknown-repo-404 | git | `gitconf-unknown-repo-missing` の両サービス info/refs | 両方 404、その後既知リポジトリは 200 | 5 | TODO | git | checks-git.ts:465-475 |
| git.ls-refs | git | main 更新＋annotated tag v1 を push → v2 ls-refs（peel, symrefs） | main = 新 tip、HEAD `symref-target:refs/heads/main`、refs/tags/v1 = tag オブジェクト、`peeled:` = commit | 5 | TODO | git | checks-git.ts:477-506 |
| git.ls-refs-prefix | git | ls-refs を ref-prefix `refs/heads/`・`refs/tags/`・`refs/does-not-exist/` で3回 | サーバ側で絞り込み（HEAD も除外）、該当なしは 0 行 | 5 | TODO | git | checks-git.ts:508-528 |
| git.clone-empty | git | 空リポジトリの広告・ls-refs・clone | `refs/` の ref なし、オブジェクト 0、pack なし | 5 | TODO | git | checks-git.ts:530-545 |
| git.push-first-commit | git | 空リポジトリへ 0→tip で refs/heads/main を push | unpack ok・ok、ls-refs で main=tip、HEAD→refs/heads/main | 5 | TODO | git | checks-git.ts:547-559 |
| git.clone-object-hashes | git | 3コミット履歴をフルクローン | 全オブジェクトの SHA-1 一致、tip の到達集合が完全 | 5 | TODO | git | checks-git.ts:561-586 |
| git.clone-blob-bytes | git | 4 KiB blob を含む2コミットをクローン | 到達オブジェクトがバイト一致 | 5 | TODO | git | checks-git.ts:588-606 |
| git.clone-tree-entries | git | `config/token.css` 等の入れ子パスを含むツリーをクローン | ルート/サブツリーの名前・mode 40000・バイト一致 | 5 | TODO | git | checks-git.ts:608-647 |
| git.clone-commit-bytes | git | 2コミット目をクローン | commit バイト一致、parent・message・時刻 `+0000`・作者メール | 5 | TODO | git | checks-git.ts:649-675 |
| git.clone-annotated-tag | git | tag v1 を push しクローン（tag oid を want） | tag オブジェクトのバイト一致、ls-refs で peeled=commit | 5 | TODO | git | checks-git.ts:677-697 |
| git.fetch-incremental | git | `want 新tip, have 旧tip, done` | 新オブジェクトのみ（旧 tip と既存オブジェクトを含めない） | 5 | TODO | git | checks-git.ts:699-730 |
| git.fetch-nothing-new | git | `want tip, have tip, done` | オブジェクト 0（空 pack か pack なし） | 5 | TODO | git | checks-git.ts:732-744 |
| git.fetch-shallow-depth1 | git | 3コミット → clone depth 1（`deepen 1`） | tip とその tree・blob のみ、親・祖父なし、shallow-info に `shallow <tip>` | 5 | TODO | git | checks-git.ts:746-765 |
| git.fetch-shallow-deepen | git | `want T, have T, shallow T, deepen 2` | 親 P とその tree を送る、祖父なし、`shallow P` か `unshallow T` | 5 | TODO | git | checks-git.ts:767-795 |
| git.fetch-shallow-new-tip | git | 新 tip push 後 `want N, have P, shallow P, deepen 1` | N のみ、P と祖父を送らない | 5 | TODO | git | checks-git.ts:797-818 |
| git.push-update-ref | git | fast-forward 更新 | ok、ls-refs と clone に反映 | 5 | TODO | git | checks-git.ts:820-833 |
| git.push-create-branch | git | 既存コミットで refs/heads/feature 作成（空 pack 付き） | ok、ls-refs と clone の refs に出る | 5 | TODO | git | checks-git.ts:835-848 |
| git.push-delete-branch | git | feature を作成→削除（new=0×40、pack なし） | ok、feature 消滅、main 無傷、clone 可 | 5 | TODO | git | checks-git.ts:850-863 |
| git.push-non-fast-forward | git | 兄弟コミットで main を更新（既存オブジェクト込みの pack） | unpack ok、`ng refs/heads/main non-fast-forward`、main 不動、以後 clone 可 | 5 | TODO | git | checks-git.ts:865-888 |
| git.push-mixed-report | git | 1回の push で main は FF、side は非 FF | ref 行ちょうど2行（ok main / ng side non-fast-forward）、main だけ前進 | 5 | TODO | git | checks-git.ts:890-925 |
| git.push-delete-tag | git | refs/tags/v1 作成→削除 | ok、tag 消滅 | 5 | TODO | git | checks-git.ts:927-938 |
| git.push-100-files | git | 100ファイル1コミットを push→clone | 全パスのバイト一致 | 5 | TODO | git | checks-git.ts:940-977 |
| git.pack-integrity | git | 2 KiB×2コミット、`want tip` を parse なしで fetch | trailer 一致、declaredCount = entry 数 = 一意オブジェクト数（重複禁止）、version 2/3 | 5 | TODO | git | checks-git.ts:979-1001 |
| git.pack-deltas | git | 20 KB ファイルの4リビジョンを push→clone | 全オブジェクトのハッシュ・バイト一致（デルタは任意） | 5 | TODO | git | checks-git.ts:1003-1023 |
| git.sideband-framing | git | fetch 応答の全 pkt を観察 | pkt ≤ 65520、packfile セクションに sideband pkt ≥1、band 3 なし、band 1 の連結が有効な pack | 5 | TODO | git | checks-git.ts:1025-1077 |
| git.push-sideband-report | git | 通常 push | side-band-64k を使用、band 1 内の report で unpack ok・ok | 5 | TODO | git | checks-git.ts:1079-1087 |
| git.repo-survives-errors | git | 404・非 FF・不正 POST（`zzzznot-a-pkt-line`）の後 | 既知リポジトリは 200、main 存在、clone 完全 | 5 | TODO | git | checks-git.ts:1089-1133 |

## repos（15）— REST、重み各 5

| ID | 面 | 要求 | 期待 | 重み | 状態 | 担当 | 根拠 |
|---|---|---|---|---|---|---|---|
| repos.pagination | rest | scenario の30リポジトリを POST /user/repos（422 可）＋各 GET → GET /user/repos?per_page=10 を Link で追跡 | 各ページ ≤10、Link next は同一 origin、重複なし、30件すべて、3ページ以上・20ページ以内。この時点で git の `gitconf-*` 約30件も存在 | 5 | TODO | rest | checks-rest.ts:649-685 |
| repos.root | rest | GET /api/v3/ | 200 JSON オブジェクト、X-OAuth-Scopes ヘッダ | 5 | TODO | rest | checks-rest.ts:593-598 |
| repos.meta | rest | GET /api/v3/meta | 200 `{installed_version: 非空}` | 5 | TODO | rest | checks-rest.ts:600-603 |
| repos.create | rest | POST /user/repos `{name, private:true, description}` → GET ×2 | 201 `{name, private:true}`、GET 200 `{name, private:true, default_branch 非空}`、2回で default_branch 同じ | 5 | TODO | rest | checks-rest.ts:605-624 |
| repos.not-found | rest | GET 存在しないリポジトリ | 404 `{message}` | 5 | TODO | rest | checks-rest.ts:626-631 |
| repos.patch | rest | PATCH `{description}` | 200 で更新値、後の GET も | 5 | TODO | rest | checks-rest.ts:633-639 |
| repos.delete | rest | DELETE repo → GET | 204、その後 404 `{message}` | 5 | TODO | rest | checks-rest.ts:641-647 |
| repos.branches | rest | POST git/refs でブランチ作成 → GET /branches | 201 `{ref, object:{sha}}`、一覧に作成ブランチ（初回コミットの sha）と既定ブランチ | 5 | TODO | rest | checks-rest.ts:687-701, :461-472 |
| repos.commits | rest | ブランチに PUT contents → GET /commits?sha=branch と GET /commits | branch 側にコミットあり（message 完全一致）、既定側には無い | 5 | TODO | rest | checks-rest.ts:703-726 |
| repos.unauthorized | rest | token なし GET repo | 401 `{message}` JSON | 5 | TODO | lead | checks-rest.ts:728-732 |
| repos.foreign-token | rest | 自分のリポジトリを作成後、同名を `olgabauer` で GET | 自分は 200、他人は 404 `{message}` | 5 | TODO | rest | checks-rest.ts:734-744 |
| repos.contents-create | rest | PUT 新規ファイル → GET | 201 `{content:{sha, encoding:"base64"}, commit:{sha}}`、GET の content デコード一致・sha 一致・size=バイト数 | 5 | TODO | rest | checks-rest.ts:746-756 |
| repos.contents-directory | rest | `config/seed.txt` で初期化、`config/one.txt`・`two.txt` 追加 → GET contents/config | 200 配列 `[{type, name, path, sha}]` に3件 | 5 | TODO | rest | checks-rest.ts:758-775 |
| repos.contents-stale-sha | rest | 正しい sha で更新 → 古い sha で再 PUT | 更新 200/201 で sha 変化、古い sha は 409 `{message}` | 5 | TODO | rest | checks-rest.ts:777-785 |
| repos.contents-delete | rest | DELETE contents `{message, sha}` → GET | 200 `{commit:{sha}}`、その後 404 | 5 | TODO | rest | checks-rest.ts:787-795 |

全 REST チェックの準備 `prepareRepo` = POST /user/repos（201）→ **空リポジトリへの PUT contents（201）** → GET repo の default_branch（checks-rest.ts:396-409）。

## issues（15）— REST、重み各 5

準備 `prepareIssueRepo` = prepareRepo ＋ POST /labels `{name:"bug", color}` → 201 `{name}`（checks-rest.ts:552-558）。issue の shape = `{number≥1, title, body(文字列), state, labels:[{name}]}`（checks-rest.ts:538-545）。

| ID | 面 | 要求 | 期待 | 重み | 状態 | 担当 | 根拠 |
|---|---|---|---|---|---|---|---|
| issues.create | rest | POST issues（labels:[bug]） | 201、state "open" | 5 | TODO | rest | checks-rest.ts:800-804 |
| issues.create-title-body | rest | POST issues | title・body をそのまま返す | 5 | TODO | rest | checks-rest.ts:806-811 |
| issues.create-labels | rest | POST issues（labels:[bug]） | labels に `{name:"bug"}` | 5 | TODO | rest | checks-rest.ts:813-818 |
| issues.numbers-increment | rest | 連続2件作成 | 2件目 = 1件目 + 1 | 5 | TODO | rest | checks-rest.ts:820-825 |
| issues.list-open | rest | open 1件・closed 1件 → GET ?state=open&per_page=100 | open を含み、全件 state "open" | 5 | TODO | rest | checks-rest.ts:827-837 |
| issues.list-open-excludes-closed | rest | closed 1件 → GET ?state=open | closed を含まない | 5 | TODO | rest | checks-rest.ts:839-845 |
| issues.list-sort-asc | rest | 3件 → ?state=open&sort=created&direction=asc | 作成順 | 5 | TODO | rest | checks-rest.ts:847-858 |
| issues.list-sort-desc | rest | 3件 → ?…&direction=desc | 逆順 | 5 | TODO | rest | checks-rest.ts:860-871 |
| issues.list-shape | rest | GET ?state=open&per_page=10 | 非空、全要素 issue shape | 5 | TODO | rest | checks-rest.ts:873-882 |
| issues.get-one | rest | GET issues/{n} | 200 shape、number・title・body 一致 | 5 | TODO | rest | checks-rest.ts:884-895 |
| issues.patch-close | rest | PATCH `{state:"closed"}` | 200 `{number, state:"closed"}` | 5 | TODO | rest | checks-rest.ts:897-903, :575-580 |
| issues.patch-close-persists | rest | close 後 GET | state "closed" | 5 | TODO | rest | checks-rest.ts:905-915 |
| issues.comment-create | rest | POST issues/{n}/comments `{body}` | 201 `{id≥1, body}` | 5 | TODO | rest | checks-rest.ts:917-931 |
| issues.comments-list | rest | コメント作成 → GET comments | 200 `[{id, body}]` に同じ id・body | 5 | TODO | rest | checks-rest.ts:933-955 |
| issues.bad-label | rest | labels:["bug-not-a-label"] | 422 `{message}` | 5 | TODO | rest | checks-rest.ts:957-961 |

## pulls（20）— REST、重み各 5

準備 `preparePull` = prepareRepo → POST git/refs で `develop-<slug>` を初回コミットに作成 → そのブランチに PUT contents（新規ファイル、201）→ POST /pulls `{head, base:default_branch, title, body}` 201、shape `{number, state, title, body, head{ref,sha}, base{ref,sha}}`（checks-rest.ts:481-524）。merge は `PUT …/merge {merge_method:"squash"}`（checks-rest.ts:498-500）。

| ID | 面 | 要求 | 期待 | 重み | 状態 | 担当 | 根拠 |
|---|---|---|---|---|---|---|---|
| pulls.create | rest | preparePull → GET pulls/{n} | state "open"、head.ref・base.ref 一致 | 5 | TODO | rest | checks-rest.ts:966-975 |
| pulls.create-body | rest | GET pulls/{n} | title・body 一致 | 5 | TODO | rest | checks-rest.ts:977-985 |
| pulls.create-head-sha | rest | GET pulls/{n} | head.sha = head ブランチ tip | 5 | TODO | rest | checks-rest.ts:987-994 |
| pulls.create-base-sha | rest | GET pulls/{n} | base.sha = 既定ブランチ tip（初回コミット） | 5 | TODO | rest | checks-rest.ts:996-1003 |
| pulls.get-one | rest | GET pulls/{n} | number・title 一致 | 5 | TODO | rest | checks-rest.ts:1005-1013 |
| pulls.files-list | rest | GET pulls/{n}/files | 変更パスを含む、status added/modified/changed、additions ≥1 | 5 | TODO | rest | checks-rest.ts:1032-1039 |
| pulls.files-shape | rest | GET files | 非空、各要素 `{filename, status, additions, deletions, changes}`（整数 ≥0） | 5 | TODO | rest | checks-rest.ts:1041-1048, :1015-1022 |
| pulls.files-multiple | rest | 2ファイル（2コミット）の PR → files | 両パスを含む | 5 | TODO | rest | checks-rest.ts:1050-1061 |
| pulls.merge-status | rest | PUT merge | 200 | 5 | TODO | rest | checks-rest.ts:1073-1077 |
| pulls.merge-body | rest | PUT merge | 200 `{merged:true, sha(40hex), message(文字列)}` | 5 | TODO | rest | checks-rest.ts:1063-1082 |
| pulls.merge-closes | rest | merge → GET pulls/{n} | state "closed"、merged true、merged_at 非空 | 5 | TODO | rest | checks-rest.ts:1084-1098 |
| pulls.merge-commit-on-default | rest | merge → GET commits?sha=base | マージ sha を含む | 5 | TODO | rest | checks-rest.ts:1100-1107 |
| pulls.merge-content-on-default | rest | merge → GET contents/{変更パス}?ref=base | 内容 = head 側の内容 | 5 | TODO | rest | checks-rest.ts:1109-1114 |
| pulls.merge-commit-sha | rest | merge → GET pulls/{n} | merge_commit_sha = マージ sha | 5 | TODO | rest | checks-rest.ts:1116-1123 |
| pulls.merge-unknown | rest | PUT pulls/999999999/merge | 404 `{message}` | 5 | TODO | rest | checks-rest.ts:1125-1129 |
| pulls.merge-conflict | rest | 2ブランチが同じ `config/token.css` を別内容に更新（sha 付き PUT）→ PR 2件 → 1件目 merge → 2件目 merge | 1件目 200、2件目 405 `{message}` | 5 | TODO | rest | checks-rest.ts:1131-1146 |
| pulls.list-state-all | rest | マージ済み1件＋open 1件（旧 base から分岐）→ GET pulls?state=all&per_page=100 | 両方含む | 5 | TODO | rest | checks-rest.ts:1148-1158, :527-533 |
| pulls.list-default-open | rest | 同上 → GET pulls?per_page=100 | open のみ（マージ済みを含まない） | 5 | TODO | rest | checks-rest.ts:1160-1169 |
| pulls.list-shape | rest | GET pulls?state=all&per_page=10 | 非空、全要素 pull shape | 5 | TODO | rest | checks-rest.ts:1171-1182 |
| pulls.get-unknown | rest | GET pulls/999999999 | 404 `{message}` | 5 | TODO | rest | checks-rest.ts:1184-1188 |

## protocol（5）— REST 横断、重み各 5

| ID | 面 | 要求 | 期待 | 重み | 状態 | 担当 | 根拠 |
|---|---|---|---|---|---|---|---|
| protocol.content-type-json | rest | GET /api/v3/、/meta、repo、repo/issues、未知ルート、token なし repo | 全部 application/json、HTML でない（本文が `<` で始まらない） | 5 | TODO | lead | checks-rest.ts:1193-1209 |
| protocol.unknown-route-json-404 | rest | GET /api/v3/no-such-route/amber-thistle | 404 `{message}` JSON | 5 | TODO | lead | checks-rest.ts:1211-1216 |
| protocol.malformed-body-400 | rest | POST /user/repos、raw `{"name":` | 400 `{message}` JSON | 5 | TODO | lead | checks-rest.ts:1218-1222 |
| protocol.request-id | rest | GET /api/v3/、repo、未知ルート、token なし repo | 全部に非空 X-GitHub-Request-Id | 5 | TODO | lead | checks-rest.ts:1224-1237 |
| protocol.error-message | rest | 存在しない repo（404）、token なし（401）、未知ラベル（422） | 各 `{message: 非空文字列}` | 5 | TODO | lead | checks-rest.ts:1239-1250 |

## graphql（20）— 担当 graphql、重み各 5

fixture（1 run で共有・遅延生成）: `amber-thistle-graphql` を REST 作成（201/200/422/409 可）→ GET repo 200 → GraphQL で id・databaseId 取得。base ブランチ main・head 3本は **env の git クライアントで push**。issue は REST（open 3・closed 1）、PR は REST（checks-graphql.ts:682-796, env/main.ts:446-507）。

| ID | 面 | 要求 | 期待 | 重み | 状態 | 担当 | 根拠 |
|---|---|---|---|---|---|---|---|
| graphql.viewer-login | graphql | `query ViewerLogin { viewer { login } }` | `adaiversen` | 5 | TODO | graphql | checks-graphql.ts:843-850 |
| graphql.repository-fields | graphql | RepositoryFields（main を git push 後） | id・databaseId が fixture 値、name、owner.login、defaultBranchRef.name = main、viewerPermission ∈ ADMIN/MAINTAIN/WRITE/TRIAGE/READ、hasIssuesEnabled true | 5 | TODO | graphql | checks-graphql.ts:853-878 |
| graphql.introspection-issue | graphql | `Issue: __type(name:"Issue"){fields(includeDeprecated:true){name}}` | id, number, title, state, body を含む | 5 | TODO | graphql | checks-graphql.ts:881-889 |
| graphql.introspection-types | graphql | PullRequest・Repository の fields、SearchType の enumValues | PR: id, number, headRefName, baseRefName, mergeable, state, merged, files。Repo: id, databaseId, name, nameWithOwner, owner, defaultBranchRef, viewerPermission, hasIssuesEnabled, issue, issues, pullRequests, pullRequest。SearchType に ISSUE | 5 | TODO | graphql | checks-graphql.ts:892-924 |
| graphql.issues-open-order | graphql | issues(first:10, states:[OPEN], orderBy:{CREATED_AT, DESC}){nodes{number title state id}} | ≤10、全 OPEN、number 降順、closed なし、open 3件を含む | 5 | TODO | graphql | checks-graphql.ts:927-954 |
| graphql.issue-by-number | graphql | issue(number:) | number・state OPEN・title `… open 1`・body・id | 5 | TODO | graphql | checks-graphql.ts:957-974 |
| graphql.create-issue | graphql | createIssue(input:{repositoryId,title,body}){issue{number id}} | REST GET で同じ title・open | 5 | TODO | graphql | checks-graphql.ts:977-993 |
| graphql.close-issue | graphql | REST 作成 → id 取得 → closeIssue(input:{issueId}) | issue.state CLOSED、REST で closed | 5 | TODO | graphql | checks-graphql.ts:996-1016 |
| graphql.pull-requests-list | graphql | pullRequests(first:5){nodes{number headRefName baseRefName mergeable state}} | ≤5、fixture PR を含み head/base 一致、state OPEN、mergeable ∈ MERGEABLE/CONFLICTING/UNKNOWN | 5 | TODO | graphql | checks-graphql.ts:1019-1038 |
| graphql.pull-request-files | graphql | pullRequest(number:){files(first:10){nodes{path}}} | `config/token.css` を含む | 5 | TODO | graphql | checks-graphql.ts:1041-1062 |
| graphql.create-pull-request | graphql | createPullRequest(input:{repositoryId, baseRefName, headRefName, title}) | number・id・state OPEN・head/base 一致、REST GET で head.ref/base.ref 一致 | 5 | TODO | graphql | checks-graphql.ts:1065-1094 |
| graphql.merge-pull-request | graphql | mergePullRequest(input:{pullRequestId, mergeMethod:SQUASH}) | merged true、REST で merged true・closed | 5 | TODO | graphql | checks-graphql.ts:1097-1111 |
| graphql.add-comment | graphql | addComment(input:{subjectId, body}){commentEdge{node{id body}}} | body 一致・id 非空、REST comments に出る | 5 | TODO | graphql | checks-graphql.ts:1114-1136 |
| graphql.invalid-field | graphql | viewer { login noSuchFieldOnViewer } | HTTP 200、errors のメッセージに `noSuchFieldOnViewer` | 5 | TODO | graphql | checks-graphql.ts:1139-1149 |
| graphql.unauthenticated | graphql | token なし POST | **正確に 401** | 5 | TODO | graphql | checks-graphql.ts:1152-1158 |
| graphql.node-repository | graphql | node(id:){id ... on Repository{name nameWithOwner}} | id・name・nameWithOwner `adaiversen/amber-thistle-graphql` | 5 | TODO | graphql | checks-graphql.ts:1161-1173 |
| graphql.unknown-repository | graphql | repository(name:"amber-thistle-graphql-missing") | data.repository = null、errors なし | 5 | TODO | graphql | checks-graphql.ts:1176-1182 |
| graphql.batched-operations | graphql | JSON 配列で2操作（viewer、Issue introspection） | 200、配列2要素を順に | 5 | TODO | graphql | checks-graphql.ts:1185-1198 |
| graphql.global-id-stability | graphql | repository id・issue id を2回ずつ | 同値、id ≠ String(databaseId)、issue id ≠ String(number) | 5 | TODO | graphql | checks-graphql.ts:1201-1223 |
| graphql.issues-pagination | graphql | `amber-thistle-graphql-pages` に7件 → issues(first:5, after:null) → after:endCursor | 1ページ目5件・hasNextPage true・endCursor 非空、2ページ目2件・hasNextPage false、重複なし・7件 | 5 | TODO | graphql | checks-graphql.ts:1226-1269 |

依存: repository-fields（defaultBranchRef）、pull-requests-list・pull-request-files・create-pull-request・merge-pull-request は **git push（v2 ls-refs・fetch・receive-pack）と REST の POST /user/repos 422 `already exists`** に依存（env/main.ts:446-507, env/git-client.ts:2096）。

## mcp（15）— 担当 mcp、重み各 18.33（feature 5 + agent 13.33）

すべて最初に `server/discover`（→ 失敗なら initialize）で接続（checks-mcp.ts 各 fn 冒頭、env/mcp-client.ts:432-468）。1 リクエスト 3 s 以内。

| ID | 面 | 要求 | 期待 | 重み | 状態 | 担当 | 根拠 |
|---|---|---|---|---|---|---|---|
| mcp.initialize | mcp | 接続 | serverInfo.name・version 非空、capabilities.tools がオブジェクト | 18.33 | TODO | mcp | checks-mcp.ts:537-557 |
| mcp.server-discover | mcp | 接続 | modern: supportedVersions に YYYY-MM-DD が1つ以上。legacy: 接続できれば pass | 18.33 | TODO | mcp | checks-mcp.ts:558-581 |
| mcp.tools-list | mcp | tools/list | 6個以上、必須6種を名前/別名/接頭辞で | 18.33 | TODO | mcp | checks-mcp.ts:582-607 |
| mcp.tool-schemas | mcp | tools/list | 全ツールに非空 description、inputSchema.type "object" | 18.33 | TODO | mcp | checks-mcp.ts:608-632 |
| mcp.create-issue | mcp | REST でリポジトリ作成 → create_issue `{owner, repo, title, body}` | 成功（isError でない）、REST issues?state=all に同じ title | 18.33 | TODO | mcp | checks-mcp.ts:633-662 |
| mcp.list-issues | mcp | REST で issue 作成 → list_issues `{owner, repo, state:"open"}` | 結果テキストに title か `"number": N` | 18.33 | TODO | mcp | checks-mcp.ts:663-694 |
| mcp.create-pull-request | mcp | REST で base コミット・head ブランチ準備 → create_pull_request | REST で head.ref・base.ref 一致（結果 JSON の number、無ければ一覧検索） | 18.33 | TODO | mcp | checks-mcp.ts:695-725, :452-478 |
| mcp.merge-pull-request | mcp | REST で PR 作成（body なし）→ merge_pull_request `{owner, repo, pull_number}` | REST GET pulls/{n} で merged true | 18.33 | TODO | mcp | checks-mcp.ts:726-758 |
| mcp.unknown-tool | mcp | tools/call `mcp_no_such_tool_6a09e667` | JSON-RPC エラー -32602 か -32601、HTTP 500 不可 | 18.33 | TODO | mcp | checks-mcp.ts:759-781 |
| mcp.parse-error | mcp | 閉じない JSON を POST | modern: HTTP 400 + -32700。legacy: 400 か 200 + -32700 | 18.33 | TODO | mcp | checks-mcp.ts:782-801 |
| mcp.missing-arguments | mcp | create_issue を `{}` で | isError:true か -32602 | 18.33 | TODO | mcp | checks-mcp.ts:802-827 |
| mcp.concurrent-calls | mcp | get_repo を2本同時（文字列 id） | 各応答が自分の id、各結果テキストに自分のリポジトリ名 | 18.33 | TODO | mcp | checks-mcp.ts:828-866 |
| mcp.tools-list-idempotent | mcp | tools/list ×2 | JSON.stringify 一致 | 18.33 | TODO | mcp | checks-mcp.ts:867-888 |
| mcp.rest-parity | mcp | create_repo `{owner, name, description, private:true}` → REST GET ×2 → create_issue → REST GET ×2 | repo の id が2回同じ、issue の id が2回同じ、title 一致 | 18.33 | TODO | mcp | checks-mcp.ts:889-955 |
| mcp.auth-required | mcp | Authorization なしの tools/call | HTTP 401 | 18.33 | TODO | mcp | checks-mcp.ts:956-979 |

依存: create-issue・list-issues・create-pull-request・merge-pull-request・concurrent-calls・rest-parity は REST（POST /user/repos 2xx、PUT contents、POST git/refs、issues、pulls、GET pulls/{n}）に依存（checks-mcp.ts:46-53）。
