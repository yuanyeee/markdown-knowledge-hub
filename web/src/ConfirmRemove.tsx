import { useEffect,useRef } from 'react';
export function ConfirmRemove({name,onConfirm,onCancel}:{name:string;onConfirm:()=>void;onCancel:()=>void}) {
  const dialog=useRef<HTMLDialogElement>(null);
  useEffect(()=>{const element=dialog.current;if(element&&!element.open)element.showModal();return()=>element?.close();},[]);
  return <dialog ref={dialog} className="confirm-remove" aria-labelledby="remove-heading" onCancel={e=>{e.preventDefault();onCancel();}}><h2 id="remove-heading">プロジェクトを削除</h2><p>「{name}」の登録と生成索引を削除します。</p><p><strong>原本Markdownは削除しません。</strong>同じフォルダを再登録できます。</p><div><button autoFocus onClick={onCancel}>キャンセル</button><button className="danger" onClick={onConfirm}>削除する</button></div></dialog>;
}
