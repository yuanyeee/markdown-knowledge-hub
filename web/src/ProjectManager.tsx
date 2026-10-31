import { useState } from 'react';
type Project={id:string;name:string;root:string};
async function command(endpoint:string,body:unknown={}) {
  const response=await fetch(`/api/admin/${endpoint}`,{method:'POST',headers:{'Content-Type':'application/json','X-Hub-Admin':'1'},body:JSON.stringify(body)});
  const value=await response.json();
  if(!response.ok)throw new Error(value.error||'操作に失敗しました');
  return value;
}
export function ProjectManager({current,enabled,folderPicker,onChanged,onBusy}:{current?:Project;enabled:boolean;folderPicker:boolean;onChanged:(preferred?:string)=>Promise<void>;onBusy:(busy:boolean)=>void}) {
  const [show,setShow]=useState(false);
  const [name,setName]=useState('');
  const [root,setRoot]=useState('');
  const [pending,setPending]=useState(false);
  const [error,setError]=useState('');
  const [message,setMessage]=useState('');
  async function perform(action:()=>Promise<void>,label:string) {
    setPending(true);onBusy(true);setError('');setMessage(label);
    try{await action();}catch(e){setError(String(e));setMessage('');}
    finally{setPending(false);onBusy(false);}
  }
  if(!enabled)return <p className="admin-hint">新規作成・再作成・削除を使う場合は、APIを管理モード <code>serve --admin</code> で起動してください。</p>;
  return <section className="manager" aria-label="プロジェクト管理">
    <div className="manager-actions"><button disabled={pending} onClick={()=>{setShow(v=>!v);setError('');}}>新規プロジェクト</button><button disabled={!current||pending} onClick={()=>void perform(async()=>{await command('rebuild',{project:current!.id});await onChanged(current!.id);setMessage('Wiki・検索索引・リンク関係を再作成しました。');},'原本を読み直して再作成しています…')}>再作成</button><button className="danger" disabled={!current||pending} onClick={()=>{if(current&&window.confirm(`「${current.name}」の登録と生成索引を削除します。原本Markdownは削除しません。よろしいですか？`))void perform(async()=>{await command('remove',{project:current.id});await onChanged();setMessage('登録と生成索引を削除しました。原本Markdownは残っています。');},'生成索引を削除しています…');}}>プロジェクトを削除</button></div>
    {current&&<p className="hint source-path">原本フォルダ: {current.root.startsWith('\\\\?\\')?current.root.slice(4):current.root}</p>}
    {show&&<form className="project-form" onSubmit={e=>{e.preventDefault();void perform(async()=>{const id=`project-${Date.now().toString(36)}`;await command('create',{id,name,root});await onChanged(id);setShow(false);setName('');setRoot('');setMessage('Wiki・検索索引・リンク関係を作成しました。リンク図タブから確認できます。');},'Markdownを解析してWikiとリンク関係を作成しています…');}}>
      <h2>新規プロジェクト</h2><label>プロジェクト名<input required maxLength={100} value={name} disabled={pending} onChange={e=>setName(e.target.value)} placeholder="例：販売システム"/></label>
      <label>Markdownフォルダ<div className="folder-input"><input required value={root} disabled={pending} onChange={e=>setRoot(e.target.value)} placeholder="D:\Specifications\SystemA"/>{folderPicker&&<button type="button" disabled={pending} onClick={()=>void perform(async()=>{const result=await command('pick-folder');if(result.path)setRoot(result.path);setMessage('');},'フォルダ選択画面を開いています…')}>フォルダを選択</button>}</div></label>
      <p className="hint">原本Markdownは変更しません。指定フォルダ以下の.mdを読み込みます。</p><button disabled={pending||!name.trim()||!root.trim()} type="submit">作成してWikiを開く</button><button disabled={pending} type="button" onClick={()=>setShow(false)}>キャンセル</button>
    </form>}
    {message&&<p role="status">{message}</p>}{error&&<p className="error" role="alert">{error}</p>}
  </section>;
}
