# 運用・バックアップ・チーム配信

## デスクトップ

「監視を開始」で選択プロジェクトの原本変更を反映する。アプリ終了で監視も停止する。監視状態・反映回数・エラーを画面に表示する。反映後の画面は「表示を更新」で読み直す。監視の起動設定は保存せず、再起動時は停止状態から開始する。削除時は対象の監視を停止してから登録と生成索引を削除する。

## 公開世代と復旧

本文・リンクはSQLite WALに保持し、閲覧操作ごとに読取トランザクションを固定する。更新中は別ディレクトリでTantivyの検索世代を作る。検索commit・ファイル同期の完了後、本文・リンク・検索世代の参照先を**同じSQLiteトランザクション**でcommitする。失敗時は本文と公開先の更新がrollbackされ、旧索引を使用する。旧検索世代は次の更新開始時、閲覧者がいない場合のみ回収する。Windowsでファイル使用中なら回収を次回に延期する。

プロセス強制終了試験は公開直前で終了コード77を発生させ、次回起動時に旧本文・旧検索結果が一致することを検証する。ストレージ故障や電源断でOSの永続化保証を超えた破損はバックアップから復旧する。

## バックアップと復元

```powershell
.\hub-app.exe --data D:\HubState backup D:\HubBackups\backup-20261101
.\hub-app.exe --data D:\HubState restore D:\HubBackups\backup-20261101 D:\HubRestored
.\hub-app.exe --data D:\HubRestored projects
.\hub-app.exe --data D:\HubRestored search sales 顧客
```

保存先は既存ではない絶対パスにする。登録情報と完成済み索引のバックアップであり、原本MD自体のバックアップではない。索引には原文が含まれるので、原本と同等のNTFS権限・保管ルールを設定する。稼働データや原本の内側へは保存できない。監査ログはサーバー停止中に別途保全する。復元後のscanには元の原本パスが必要。同じパスがない場合は「プロジェクトの保存先・設定」で入力先を指定し直し、再作成する。

## Webの本番ビルド配信

```powershell
cd web
npm.cmd run build
cd ..
.\hub-app.exe --data D:\HubState serve --web-dir .\web\dist
```

Vite不要で127.0.0.1:3847から配信できる。これはこのPC内のローカル閲覧用。ネットワーク公開は行わない。

## 公開範囲

ユーザー指示によりネットワークへ公開しない。APIは127.0.0.1のみへbindする。IIS、LAN向け公開、SSO、外部更新サーバー、自動更新配信は今回の対象外。実行ファイル／インストーラーを手動で差し替える。コード署名は証明書が未準備のため未署名。

## 入力・出力の管理（0.3.0）

新規作成で入力と出力親フォルダーを別に選択する。出力先の変更はコピー移行であり、親全体を移動しない。旧出力は自動削除しない。保管中の旧出力はバックアップの対象外なので、必要なら整理前に別途保管する。バックアップ／復元で稼働中の外部出力への参照は引き継がず、復元先だけで閲覧できる。

```powershell
.\hub-app.exe --data D:\HubState move-output sales D:\NewOutputs
.\hub-app.exe --data D:\HubState retired-outputs
.\hub-app.exe --data D:\HubState cleanup-output <旧出力のkey>
.\hub-app.exe --data D:\OtherSettings open-output D:\NewOutputs\MarkdownKnowledgeHub\sales
.\hub-app.exe --data D:\HubState change-input sales D:\NewSpecifications
```

入力が見つからない時はキャッシュ閲覧を継続できる。再作成・監視には有効な入力先が必要。専用出力を手動で動かす場合はアプリ／監視を停止し、別名リンクを使わず`MarkdownKnowledgeHub/ID`の構造と`project.json`を保ったうえで「既存プロジェクトを開く」で再登録する。同じIDが登録済みの場合は上書きせず拒否する。通常の移行にはアプリの「出力先を変更」を使う。
