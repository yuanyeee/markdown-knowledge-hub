import { useEffect, useRef } from 'react';

export function ConfirmRemove({
  name,
  retired = false,
  onConfirm,
  onCancel,
}: {
  name: string;
  retired?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) {
      element.showModal();
    }
    return () => element?.close();
  }, []);

  return (
    <dialog
      ref={dialog}
      className="confirm-remove"
      aria-labelledby="remove-heading"
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
    >
      <h2 id="remove-heading">
        {retired ? '旧出力を削除' : 'プロジェクトを削除'}
      </h2>
      <p>
        「{name}」
        {retired
          ? 'の保管中の専用出力を削除します。'
          : 'の登録と現在の専用出力を削除します。'}
      </p>
      <p>
        <strong>入力Markdownは削除しません。</strong>
        {retired
          ? '現在の出力は残ります。'
          : '親フォルダーの既存ファイルと保管中の旧出力は残ります。'}
      </p>
      <div>
        <button autoFocus onClick={onCancel}>
          キャンセル
        </button>
        <button className="danger" onClick={onConfirm}>
          削除する
        </button>
      </div>
    </dialog>
  );
}
