# TODOS

## Core

- **Priority:** P1 — イベント対象だけの差分更新とリンク再解決。10万件の単一差分1秒以内を目指す。
- **Priority:** P1 — 安定世代の原子的公開とクラッシュ注入試験。
- **Priority:** P1 — 実文書1万/5万/10万件、8KiB基準で性能・検索精度・メモリを評価。
- **Priority:** P2 — Readerキャッシュ、解析バッチ、検索／文書／リンクのページング。
- **Priority:** P2 — frontmatter aliases、見出しID、完全再構築コマンド。

## Viewer / Desktop

- **Priority:** P2 — 同名・孤立文書の集計、Sigma.js/Graphology局所グラフ。
- **Priority:** P2 — Tauri 2アダプター、フォルダ選択、Windows配布。

## Team

- **Priority:** P1 — IIS静的配信・リバースプロキシ、TLS、SSO、プロジェクトACL、監査。
- **Priority:** P2 — 50～100人の混合閲覧負荷試験、バックアップ復旧手順。

## Completed

- 日本語要件・アーキテクチャ・マイルストーン・性能目標。
- Rust Core、永続プロジェクト、Tantivy/SQLite、並列MD解析、WikiLink/Backlinks、差分更新、notify。
- 管理CLI、閲覧API、React閲覧UI、多言語検索、テスト、Windows/Linux CI定義。
- 合成1万/5万/10万ファイルのrelease実測。
