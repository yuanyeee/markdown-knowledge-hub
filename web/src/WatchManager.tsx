import {useEffect,useState} from 'react';
import {read,write} from './transport';
type Status={running:boolean;error:string|null;updates:number};
export function WatchManager({project,onRefresh}:{project:string;onRefresh:()=>void}) {
  const [status,setStatus]=useState<Status>({running:false,error:null,updates:0});
  const [error,setError]=useState('');
  const [busy,setBusy]=useState(false);
  useEffect(()=>{
    let active=true;
    const poll=()=>read<Status>('watch/status',project).then(s=>{if(active)setStatus(s);}).catch(e=>{if(active)setError(String(e));});
    void poll();const timer=setInterval(()=>void poll(),2000);
    return()=>{active=false;clearInterval(timer);};
  },[project]);
  async function toggle(){setBusy(true);setError('');try{setStatus(await write(status.running?'watch-stop':'watch-start',{project}) as Status);}catch(e){setError(String(e));}finally{setBusy(false);}}
  return <section className="manager" aria-label="変更監視"><span role="status">変更監視：{status.running?'実行中':'停止中'}（反映 {status.updates}回）</span> <button disabled={busy} onClick={()=>void toggle()}>{status.running?'監視を停止':'監視を開始'}</button> <button onClick={onRefresh}>表示を更新</button><p className="hint">アプリを終了すると監視も停止します。変更反映後は「表示を更新」で読み直してください。</p>{(error||status.error)&&<p role="alert" className="error">{error||status.error}</p>}</section>;
}
