# Windowsデスクトップ版 実装計画

## 目的・構成

Tauri 2で既存ReactをWindowsアプリとして配布する。DesktopはHTTPサーバーを立てず、IPCから共通Rust Coreを呼び出す。Webは従来のHTTP経路を維持し、UIは共有する。原本Markdownへの書き込み機能は作らない。

`crates/service`は検索・閲覧・リンク・グラフ・作成／再作成／削除の明示的な操作だけを受け付ける。`web/src/transport.ts`がWeb HTTPとDesktop IPCを切り替える。`web/src-tauri`は専用のCargo workspaceとして分離し、Windows CIで検証する。LinuxのCore/API CIにGUI依存を追加しない。

0.3.0では、新規プロジェクトの本文コピー・索引を画面で選択した出力親フォルダーの`MarkdownKnowledgeHub/<project-id>`に保存する。LocalAppDataには登録先などの設定を保存する。従来索引は互換経路で読み込み、移行するまで保存場所を維持する。検証・既存Web索引の再利用には`--data-dir <絶対パス>`で保存先を指定できる。Webの既定`.hub`とDesktopの既定保存先は別。初回は新規プロジェクトから登録する。

## 実装と受入条件

- [x] Service操作のテストを先に作り、未知の操作拒否・作成→検索→リンク→再作成→削除と原本不変を確認。
- [x] 共通UIの通信を分離し、Desktop時にfetchを呼ばずIPCへ送るテスト、WebのGET／管理POST互換テストを追加。
- [x] TauriのローカルコンテンツにだけCore呼び出しを許可。CSPを設定し、外部の画面へ遷移しない。
- [x] デスクトップで新規フォルダ選択、Wiki、検索、リンク図、再作成・削除確認を動かす。削除確認はWeb／Desktop共通のHTMLダイアログにする。
- [x] Windows release exeとNSISインストーラーをビルド。PowerShell・ブラウザを手動起動せず動作し、再起動後もプロジェクトを再利用できることを実機確認。
- [x] Windows CIでDesktopをビルドし、配布ファイルをartifactとして保存する。README・残作業・検証記録を更新。反映とCI結果はGitHub履歴／Actionsを参照。

## 0.2.0

監視の開始／停止、ページ取得、別名・見出し、フォルダー集約図、原子的更新、実文書評価を追加。現在の結果は[0.2.0検証記録](RELEASE-0.2.0.ja.md)を参照。ネットワーク公開と外部更新配信はユーザー指示により対象外。配布は署名証明書未準備のため未署名。

一次資料: https://v2.tauri.app/develop/calling-rust/ 、 https://v2.tauri.app/reference/config/ 、 https://v2.tauri.app/distribute/windows-installer/

## 0.3.0

入力・出力選択、保存先プレビュー、入力再指定、コピー検証後の出力先切替、旧出力の確認付き整理、既存出力の再登録を追加。移行・入力変更の前に対象監視を停止し、処理後に元の監視状態を復元する。設定保存先の絶対パスは通常画面に常時表示しない。[保存先機能の検証](RELEASE-0.3.0.ja.md)を参照。
