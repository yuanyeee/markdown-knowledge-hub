# 検証記録

測定日: 2026-10-31。Windows、AMD Ryzen AI MAX+ 395（16コア/32スレッド）、メモリ約32GiB。Rust 1.99.1 MSVC。Dockerは使用していません。

## 実施済み

- Rust Core統合テスト4件とHTTP結合テスト1件: 成功。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`: 成功。
- React/Vitestテスト2件、本番ビルド: 成功。npm audit: 既知の脆弱性0件（検査時点）。
- 実ブラウザ: 文書一覧、日本語WikiLinkによる顧客への移動と復帰、通常MDリンク、Backlinks、中国語検索、コード中WikiLink非解決を確認。
- Windows notify: 一時Markdownを書き換え、watchログのchanged=1と新語の検索結果を確認。
- 永続登録、プロジェクト分離、更新・削除・無変更スキャン、原本不変、検索索引dirtyからの復旧、不正パス・Windows予約名拒否をテスト。
- 不正UTF-8を含む追加ファイルでscanが失敗しても既存の検索結果を維持することを確認。

## 合成データ性能

releaseビルド、1ファイル約449バイト、1リンク、本文の多くが共通する合成MD。ファイル生成・一時データ削除時間は索引生成時間に含めません。検索とBacklinksは各100回、OSキャッシュはウォーム、検索Readerはリクエストごとに開く実装。検索結果上限50件。測定値は独立した1回ずつの試行で、分散の評価は未実施です。

|件数|原文容量|初回索引|無変更走査|単一ファイル差分|検索P95|Backlinks P95|
|---:|---:|---:|---:|---:|---:|---:|
|10,000|4.47MB|1.333秒|42.0ms|211.1ms|11.55ms|3.22ms|
|50,000|22.43MB|5.604秒|196.1ms|545.7ms|36.73ms|3.82ms|
|100,000|44.88MB|10.828秒|444.7ms|1209.6ms|48.90ms|3.37ms|

10万件の索引容量は約123.8MB。実行中に観測したプロセスのPeakWorkingSetは約390.5MBでした（監視方式による参考値）。10万件の差分1秒目標は未達です。

この合成データは計画の平均8KiB基準より非常に小さく、語彙やリンク分布も偏っています。実文書10万件・平均8KiBに対する性能、同時利用50～100人、コールド起動、ネットワーク共有の性能は未検証です。上表から実運用の性能を保証できません。

再現:

```powershell
cargo run --release -p hub-core --example benchmark -- 10000
cargo run --release -p hub-core --example benchmark -- 50000
cargo run --release -p hub-core --example benchmark -- 100000
```

5万・10万件の生JSONもこのディレクトリに保存しています。

## 未完了・制約

- Tauri 2アプリ、Windowsインストーラー、IISチーム公開、SSO/ACL/監査、TLS、同時利用試験。
- Sigma.js + Graphologyによる局所／全体グラフ、同名／孤立文書の集計画面。
- Frontmatter aliases、見出しIDの存在検証、画像・添付ファイル表示、`.markdown`拡張子、横断検索、AI/MCP。
- 検索Readerキャッシュ、イベント対象だけの更新、解析バッチによるメモリ上限制御、世代単位の原子的公開、ジョブ管理、完全再構築CLI。
- 元MDはUTF-8、1文書16MiB以下。読取失敗時はscan全体を中断。通常スキャンではサイズとmtimeが両方不変の変更を検出できないため`scan PROJECT --verify`を使用。
- 文書一覧／検索は50件、Backlinksは500件、リンクAPIは5,000件まで。ページングは未実装。現在のUIはリンクAPIの範囲外の本文リンクを移動できません。
- SQLiteとTantivy間に単一トランザクションはありません。クラッシュ後は次回scanで検索索引を再構築。scan完了前の閲覧に一時的不整合があり得ます。
- 現時点は開発用Viteとloopback APIの組み合わせ。API単体はReactの本番ビルドを配信しません。チーム配信には静的配信、認証認可、レート制限と接続プールを追加する必要があります。
- CIはWindows/Linux Rust検査とLinux React検査を定義。実行状況はGitHub Actions参照。
