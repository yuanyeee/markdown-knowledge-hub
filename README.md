# Markdown Knowledge Hub

Rust + Tantivyの読み取り専用Markdown Wiki・全文検索・Backlinks・リンク図。Windowsデスクトップ版と、このPC内で動くWeb版を提供します。Dockerは不要です。**ユーザー指定によりネットワークへ公開しません。**

## Windowsで使う

`Markdown-Knowledge-Hub.exe`を起動するか、Windows用セットアップexeでインストールします。PowerShell・ブラウザ・サーバーの手動起動は不要です。

1. 「新規プロジェクト」から名前・入力フォルダー・出力の親フォルダーを指定します。「今回、新しく作成する保存先」を確認して作成します。
2. 自動でWiki・検索索引・リンク関係を作成します。
3. Wiki、全文検索、Backlinks、リンク図で閲覧します。文書一覧・検索・参照元・出リンクは追加取得できます。
4. 「監視を開始」で原本変更を索引に反映します。「表示を更新」で画面を読み直します。アプリ終了で監視も停止します。
5. 「再作成」で全原本を読み直します。「プロジェクトを削除」で登録と生成索引を削除します。原本は残るため、同じフォルダーを再登録できます。

原本Markdownは変更しません。入力は読むMarkdownのあるフォルダー、出力は本文コピー・検索索引・リンク情報を保存するフォルダーです。新規作成では出力先の選択が必須です。選択した親フォルダーの`MarkdownKnowledgeHub/<project-id>`だけを管理し、親の既存ファイルは変更しません。

「プロジェクトの保存先・設定」で入力の再指定と出力先の変更ができます。出力をコピー・検証してから切り替え、旧出力は保管します。新しい場所で閲覧を確認してから「旧出力を削除」で整理します。「既存プロジェクトを開く」は、新規作成で指定する親ではなく、`MarkdownKnowledgeHub/<project-id>`の中の`project.json`がある専用出力を選択します。入力が見つからなくても保存済みの本文を閲覧できます。

アプリ設定と出力先の参照情報は`%LOCALAPPDATA%\com.yuanyeee.markdownknowledgehub`に保存します。パス内のWindowsユーザー名は、このPC内の設定の場所を示すもので、外部送信先ではありません。従来の本文コピー・索引は移行するまで元の場所に残ります。Web版の設定は既定`.hub`、別の保存先は`--data-dir "<保存先の絶対パス>"`で指定します。

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
cargo run -p hub-app -- --data .hub add demo "Demo" .\notes --output .\hub-output
cargo run -p hub-app -- --data .hub scan demo
cargo run -p hub-app -- --data .hub watch demo
# 別端末で本番ビルドをローカル配信（Vite不要）
cargo run -p hub-app -- --data .hub serve --web-dir web/dist --admin
```

先に`web`で`npm.cmd run build`を実行してください。URLは`http://127.0.0.1:3847/`。管理操作が不要なら`--admin`を省略します。APIのbind先は127.0.0.1固定で、外部向けのlisten設定は提供しません。開発用Viteは`web`で`npm.cmd run dev`（127.0.0.1:5173）です。

`update PROJECT docs/a.md`は対象ファイルをハッシュ検証して更新します。`rebuild PROJECT`は完全再作成です。通常のscanはサイズとmtimeを使用し、両方不変の変更を確認する場合は`scan PROJECT --verify`を使います。

## Claude / Gemini CLI / Antigravity から使う

ローカルのstdio MCPを同梱しています。公開するのは `search`（path・title・短い抜粋、最大5件）と `read`（1文書、約8KB）だけです。リンク図・管理操作・索引の全件出力はありません。通信は親プロセスとの標準入出力だけで、待受ポートは開きません。検索そのものはこのPCの索引で完結します。

モデルへ渡る使い方は、接続時の `instructions` と各ツールの説明に入れています。抜粋で判断し、足りない1件だけ読み、長い文書は見出しか `next_offset` で続ける、という手順です。

デスクトップ版は、起動中のアプリと同じ登録を見ます。保存先は `%LOCALAPPDATA%\com.yuanyeee.markdownknowledgehub` です。別の索引を使うときだけ `--data-dir` に絶対パスを足します。開発用 `hub-app mcp` は、`--data` を省略すると同じデスクトップの保存先を使います。他の `hub-app` コマンドは従来どおり `.hub` です。

インストールしたexeの設定例は、次でこのPCのパスが入ったJSONが出ます。

```powershell
& "$env:LOCALAPPDATA\Markdown Knowledge Hub\markdown-knowledge-hub.exe" mcp --print-config
```

開発中は `cargo run -p hub-app -- mcp --print-config` です。出力の `mcpServers` を、使うクライアントの設定へ足します。

| クライアント | 設定ファイル |
| --- | --- |
| Claude Code | 対象フォルダーの `.mcp.json`、または `claude mcp add` |
| Claude Desktop | `%APPDATA%\Claude\claude_desktop_config.json` |
| Gemini CLI | `~/.gemini/settings.json` または対象フォルダーの `.gemini/settings.json` |
| Antigravity | `~/.gemini/config/mcp_config.json`（全体）、`.agents/mcp_config.json`（対象フォルダー） |

形は共通です。`serverUrl` はリモート用なので使いません。

```json
{
  "mcpServers": {
    "markdown-knowledge-hub": {
      "command": "C:\\Users\\<ユーザー>\\AppData\\Local\\Markdown Knowledge Hub\\markdown-knowledge-hub.exe",
      "args": ["mcp"]
    }
  }
}
```

`mcp` を付けて起動すると画面は開かず、親プロセスの入出力だけで応答します。設定を保存したあとは、各クライアントを起動し直してください。アプリを開いたままでも検索できます。索引を更新したいときは、アプリの「監視を開始」か「再作成」を使います。MCPは原本も索引も書き換えません。

## 構成

```text
crates/core/       解析・リンク解決・検索・原子的更新・監視・バックアップ
crates/service/    Web/Desktop共用の明示的操作と、stdio MCP
crates/app/        CLIとloopback HTTP API、本番静的ファイル配信
web/src/          共通React UI
web/src-tauri/    Tauri Windowsホスト・監視管理・NSIS
scripts/          合成文書の測定と、追跡ファイルの確認
docs/             設計・計画・検証・運用
```

索引には入力Markdownのコピーが含まれます。入力ファイルと生成した索引はGitへ追加しません。性能の数値は集計だけを残します。push と pull request のたびに `scripts/check_public_surface.py` が、追跡ファイルの種類と大きさ、およびこの README の語彙を確認します。ライセンスは未設定で、公開・再配布許諾はありません。
