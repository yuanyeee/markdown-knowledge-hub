# Windowsデスクトップ版 実装計画

## 目的・構成

Tauri 2で既存ReactをWindowsアプリとして配布する。DesktopはHTTPサーバーを立てず、IPCから共通Rust Coreを呼び出す。Webは従来のHTTP経路を維持し、UIは共有する。原本Markdownへの書き込み機能は作らない。

`crates/service`は検索・閲覧・リンク・グラフ・作成／再作成／削除の明示的な操作だけを受け付ける。`web/src/transport.ts`がWeb HTTPとDesktop IPCを切り替える。`web/src-tauri`は専用のCargo workspaceとして分離し、Windows CIで検証する。LinuxのCore/API CIにGUI依存を追加しない。

Desktopは標準でOSのLocalAppData配下のアプリデータフォルダに永続索引を保存する。検証・既存Web索引の再利用には`--data-dir <絶対パス>`で保存先を指定できる。Webの既定`.hub`とDesktopの既定保存先は別。初回は新規プロジェクトから登録する。

## 実装と受入条件

- [x] Service操作のテストを先に作り、未知の操作拒否・作成→検索→リンク→再作成→削除と原本不変を確認。
- [x] 共通UIの通信を分離し、Desktop時にfetchを呼ばずIPCへ送るテスト、WebのGET／管理POST互換テストを追加。
- [x] TauriのローカルコンテンツにだけCore呼び出しを許可。CSPを設定し、外部の画面へ遷移しない。
- [x] デスクトップで新規フォルダ選択、Wiki、検索、リンク図、再作成・削除確認を動かす。削除確認はWeb／Desktop共通のHTMLダイアログにする。
- [x] Windows release exeとNSISインストーラーをビルド。PowerShell・ブラウザを手動起動せず動作し、再起動後もプロジェクトを再利用できることを実機確認。
- [x] Windows CIでDesktopをビルドし、配布ファイルをartifactとして保存する。README・残作業・検証記録を更新。反映とCI結果はGitHub履歴／Actionsを参照。

## 今後の残作業

巨大グラフの集約、frontmatter aliases、検索等のページング、安定世代の原子的公開、実文書性能評価、チーム認証／IIS運用は別マイルストーン。未署名の初期Windows配布物を作成し、自動更新・コード署名は後続工程とする。

一次資料: https://v2.tauri.app/develop/calling-rust/ 、 https://v2.tauri.app/reference/config/ 、 https://v2.tauri.app/distribute/windows-installer/
