# Markdown Knowledge Hub

Rust + Tantivyの読み取り専用Markdown Wiki・全文検索・Backlinks・リンク図。Windowsデスクトップ版と、このPC内で動くWeb版を提供します。Dockerは不要です。**ユーザー指定によりネットワークへ公開しません。**

## Windowsで使う

`Markdown-Knowledge-Hub.exe`を起動するか、Windows用セットアップexeでインストールします。PowerShell・ブラウザ・サーバーの手動起動は不要です。

1. 「新規プロジェクト」から名前・入力フォルダー・出力フォルダーを指定します。実際の生成先を確認して作成します。
2. 自動でWiki・検索索引・リンク関係を作成します。
3. Wiki、全文検索、Backlinks、リンク図で閲覧します。文書一覧・検索・参照元・出リンクは追加取得できます。
4. 「監視を開始」で原本変更を索引に反映します。「表示を更新」で画面を読み直します。アプリ終了で監視も停止します。
5. 「再作成」で全原本を読み直します。「プロジェクトを削除」で登録と生成索引を削除します。原本は残るため、同じフォルダーを再登録できます。

原本Markdownは変更しません。入力はお客様が用意したMarkdownのある場所、出力はアプリが本文コピー・検索索引・リンク情報を保存する場所です。新規作成では出力先の選択が必須です。選択した親フォルダーの`MarkdownKnowledgeHub/<project-id>`だけを管理し、親の既存ファイルは変更しません。

「プロジェクトの保存先・設定」で入力の再指定と出力先の変更ができます。出力をコピー・検証してから切り替え、旧出力は保管します。新しい場所で閲覧を確認してから「旧出力を削除」で整理します。「既存プロジェクトを開く」は`project.json`のある専用出力を選択します。入力が見つからなくても保存済みの本文を閲覧できます。

アプリ設定と出力先の参照情報は`%LOCALAPPDATA%\com.yuanyeee.markdownknowledgehub`に保存します。パス内のWindowsユーザー名は、このPC内の設定の場所を示すもので、外部送信先ではありません。従来の本文コピー・索引は移行するまで元の場所に残ります。Web版の設定は既定`.hub`、検証・既存設定の再利用は`--data-dir "D:\HubState"`で指定します。

0.1.0の索引を引き継いだ場合は、一度「再作成」するか監視を開始してください。別名・見出し・位置情報付き検索索引へ更新します。旧版のwatchと新バージョンを同じ索引に対して併用しないでください。

## 実装済み

- マルチプロジェクト登録・削除・再作成・再起動後の再利用。
- Rayonの16文書単位の並列解析、pulldown-cmark、notifyの対象パス更新・定期照合。
- SQLiteとTantivyの公開先を同時確定する原子的更新。公開直前の子プロセス停止・復旧試験。
- 日本語・中文・英語の位置情報付き部分文字列検索。空白区切りの語はAND条件。
- WikiLink、通常Markdownリンク、frontmatter aliases、見出しID・安全な空HTMLアンカー、Backlinks。
- ReactのMarkdown表表示、文書／検索／参照元／出リンクのページ取得、未取得の本文リンクの個別解決。
- Sigma.jsの文書近傍図・フォルダー集約図、同名・孤立・未解決の集計。
- CLIのバックアップ・未使用保存先への復元、本番Webビルドのローカル配信。
- Windows/Linux Rust CI、React CI、Windows Desktopビルド・配布artifact。

[0.3.0保存先機能の検証](docs/RELEASE-0.3.0.ja.md)・[0.2.0性能検証結果](docs/RELEASE-0.2.0.ja.md)・[アーキテクチャ](docs/ARCHITECTURE.ja.md)・[運用／バックアップ](docs/OPERATIONS.ja.md)・[制約と対象外](TODOS.md)を参照してください。

## 開発環境

Rust stable（WindowsはMSVCとC++ Build Tools）、Node.js 22以上、WebView2 Runtimeを使用します。SQLiteはbundledビルドです。配布物は証明書未準備のため未署名です。NSISはWebView2未導入時に公式bootstrapperを取得する設定です。

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cd web
npm.cmd ci
npm.cmd test
npm.cmd run desktop:dev
# 配布用exe・NSISを生成
npm.cmd run desktop:build -- --ci -- --locked
```

WindowsでcargoがPATHにない場合は`& "$env:USERPROFILE\.cargo\bin\cargo.exe"`を使用します。再ビルドや`npm ci`の前に、古いアプリ・watch・Viteを停止してください。実行中のexeはWindowsで置き換えられません。

## ローカルWeb版とCLI

```powershell
cargo run -p hub-app -- --data D:\HubState add system-a "システムA" D:\Specifications\SystemA --output D:\MarkdownHubOutputs
cargo run -p hub-app -- --data D:\HubState scan system-a
cargo run -p hub-app -- --data D:\HubState watch system-a
# 別端末で本番ビルドをローカル配信（Vite不要）
cargo run -p hub-app -- --data D:\HubState serve --web-dir web/dist --admin
```

先に`web`で`npm.cmd run build`を実行してください。URLは`http://127.0.0.1:3847/`。管理操作が不要なら`--admin`を省略します。APIのbind先は127.0.0.1固定で、外部向けのlisten設定は提供しません。開発用Viteは`web`で`npm.cmd run dev`（127.0.0.1:5173）です。

`update PROJECT docs/a.md`は対象ファイルをハッシュ検証して更新します。`rebuild PROJECT`は完全再作成です。通常のscanはサイズとmtimeを使用し、両方不変の変更を確認する場合は`scan PROJECT --verify`を使います。

## 構成

```text
crates/core/       解析・リンク解決・検索・原子的更新・監視・バックアップ
crates/service/    Web/Desktop共用の明示的操作
crates/app/        CLIとloopback HTTP API、本番静的ファイル配信
web/src/          共通React UI
web/src-tauri/    Tauri Windowsホスト・監視管理・NSIS
scripts/          合成文書の性能／メモリ測定
docs/             設計・計画・検証・運用
```

索引には原文のコピーが含まれます。原本と同等のアクセス権で保護し、機密文書や生成索引をGitへ追加しないでください。実文書の性能報告は集計値だけを記録します。ライセンスは未設定で、公開・再配布許諾はありません。
