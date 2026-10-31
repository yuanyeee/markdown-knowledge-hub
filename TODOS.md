# TODOS

## Core

- **Priority:** P2 — 追加・削除で影響を受けるリンクだけ再解決する。現在は追加・削除・フォルダ操作で全体再評価。
- **Priority:** P1 — 安定世代の原子的公開とクラッシュ注入試験。
- **Priority:** P1 — 実文書1万/5万/10万件、8KiB基準で性能・検索精度・メモリを評価。
- **Priority:** P2 — Readerキャッシュ、解析バッチ、検索／文書一覧／Backlinksのページング。
- **Priority:** P2 — frontmatter aliases、見出しID、安定世代による完全再構築。

## Viewer / Desktop

- **Priority:** P2 — 同名・孤立文書の集計、巨大グラフの集約表示、レイアウト改善。
- **Priority:** P2 — Windowsコード署名、自動更新、インストール／アンインストール試験、Desktop内の常駐監視管理。

## Team

- **Priority:** P1 — IIS静的配信・リバースプロキシ、TLS、SSO、プロジェクトACL、監査。
- **Priority:** P2 — 50～100人の混合閲覧負荷試験、バックアップ復旧手順。

## Completed

- Tauri 2 Windowsデスクトップ版、Core直接呼び出し（HTTP不要）、Webと共通React、単体exe・NSISインストーラー生成。
- Desktopのフォルダ選択、Wiki、全文検索、リンク図、再作成、削除確認キャンセル、終了・再起動後の登録再利用を実機確認。
- Web/Desktop共通のHTML削除確認ダイアログ、通信経路切替テスト、ローカルコンテンツに限定したDesktop操作。

- 画面からの新規プロジェクト登録、Windows標準フォルダ選択、自動Wiki表示、再作成、登録・生成索引の削除と再登録。
- Sigma.js/Graphologyによる近傍／先頭文書群のリンク図、文書・リンク・未解決件数の表示。
- 管理モード限定API、Origin／専用ヘッダー検証、削除と更新の競合防止、作成失敗時の取消。

- 日本語要件・アーキテクチャ・マイルストーン・性能目標。
- Rust Core、永続プロジェクト、Tantivy/SQLite、並列MD解析、WikiLink/Backlinks、差分更新、notify。
- 管理CLI、閲覧API、React閲覧UI、多言語検索、テスト、Windows/Linux CI定義。
- 合成1万/5万/10万ファイルのrelease実測。
- ファイルイベントの対象パス更新、本文だけの変更時は該当文書の出リンクだけ再解決、監視キュー上限・定期照合。
- 文書別出リンクのページングAPIとReact追加読み込み。全体5,000リンクの制限を閲覧経路から除去。
- M2合成10万件: 対象更新P95約470ms、全体scan単一差分約844ms（Core測定）。
