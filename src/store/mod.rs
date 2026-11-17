//! インメモリのストア（リポジトリ・git オブジェクト・ref・issue・PR・コメント・ラベル）。持ち場: リード。
//! REST / GraphQL / MCP / git はすべてここの公開関数だけを呼ぶ。
//! 決定性: ID は単調カウンタ、時刻は MAKEFUN_TRIAL_NOW から、反復する集合は BTreeMap か Vec
//! （std の HashMap は反復順がプロセスごとに変わるので、反復するものには使わない）。
