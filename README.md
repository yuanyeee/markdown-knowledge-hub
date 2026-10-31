# Markdown Knowledge Hub

Rust + Tantivyによる、複数プロジェクトのMarkdown検索・WikiLink・Backlinks基盤。Windows上でDockerを使わず動作させ、チームの閲覧を中心に育てるプロジェクトです。原本Markdownを変更するAPIは提供しません。

## 決定済み要件

- 1プロジェクト10,000～100,000ファイル。複数ルートはプロジェクトとして個別登録。
- Rust CoreをCLI・Rust HTTP APIで共有し、React UIを接続。将来Tauri 2アダプターを追加。
- Rayon並列解析、pulldown-cmark、notify監視、Tantivy全文検索、SQLiteメタデータ／リンク。
- 索引は原本と重ならない別フォルダに保存。登録・切替・再起動で索引を再利用。
- 同名・リンク切れ・孤立文書を診断し、曖昧なリンクを誤接続しない。
- AI/MCP、システム横断検索、編集機能は将来の独立モジュール。

詳細は [設計・開発計画](docs/PLAN.ja.md)、実測結果と制約は [検証記録](docs/VERIFICATION.ja.md)。

初期実装済み: プロジェクト登録、差分更新、監視、全文検索、WikiLink/Backlinks、CLI、閲覧API、React UI。未実装: Tauri配布、Sigmaグラフ、認証認可付きチーム公開。[残作業](TODOS.md)を参照してください。

## Windowsで開始

Rust stable（MSVC、Visual Studio C++ Build Tools）とNode.js 22以上を用意します。SQLiteはbundledビルドのため別途導入不要です。

```powershell
cargo build --workspace
cargo run -p hub-app -- --data D:\KnowledgeHub\state add system-a "システムA" D:\Specifications\SystemA
cargo run -p hub-app -- --data D:\KnowledgeHub\state scan system-a
# 相対パスを指定して更新（変更・追加・削除済みファイル）
cargo run -p hub-app -- --data D:\KnowledgeHub\state update system-a docs/order.md
cargo run -p hub-app -- --data D:\KnowledgeHub\state search system-a 注文
cargo run -p hub-app -- --data D:\KnowledgeHub\state watch system-a
# watchとは別端末。索引更新プロセスは同時に1つだけ実行してください。
cargo run -p hub-app -- --data D:\KnowledgeHub\state serve
```

別端末で:

```powershell
cd web
npm.cmd ci
npm.cmd run dev
```

表示されるローカルURLを開きます。APIは127.0.0.1:3847、Viteは127.0.0.1:5173。プロジェクト追加・スキャンは管理者CLIのみです。HTTPは閲覧専用、外部公開前にはIIS側の認証・認可とTLSを実装してください。現段階のAPIにはユーザー認証がありません。

サンプルを試す場合は、リポジトリルートで`cargo run -p hub-app -- add demo "サンプル仕様" samples/specs`、続けて`cargo run -p hub-app -- scan demo`を実行できます。`serve`も同じデータディレクトリで起動してください。

監視のファイル変更イベントは対象パスだけを更新します。追加・削除は全体のリンク候補を再評価し、フォルダ操作・監視エラー・再走査要求では全体scanに戻ります。60秒ごとの全体照合も実施します。`update`はファイル名の大小文字も実ファイルと同じ相対パスで指定してください。最大500パスまで対象限定で処理し、それを超える場合は全件ハッシュ検証に切り替わります。

Reactの出リンクは文書別に100件ずつ取得するため、プロジェクト全体のリンク数が5,000件を超えても利用できます。「出リンクをさらに読み込む」で続きを表示します。本文内のリンク移動は取得済みの出リンクに対応します。

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd web
npm.cmd run build
```

## リポジトリ構成

```text
crates/core/       原本走査・解析・永続プロジェクト・リンク解決・検索
crates/app/        管理CLI、監視、閲覧HTTP API
web/              React + TypeScript閲覧UI（Web/Tauri共用候補）
scripts/          合成Markdown生成と性能測定
docs/             日本語計画・検証記録・運用上の制約
.github/workflows/ Windows/Linuxビルド・テスト
```

## 取り扱い

索引DBには本文のコピーが含まれます。原本と同じアクセス権で保護してください。機密文書や生成索引をGitに追加しないでください。ライセンスは未設定で、公開・再配布許諾はありません。
