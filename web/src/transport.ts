import { isTauri,invoke } from '@tauri-apps/api/core';
async function request<T>(operation:string,args:Record<string,unknown>,method:'GET'|'POST'):Promise<T> {
  if(isTauri())return invoke<T>('hub_request',{operation,args});
  const query=new URLSearchParams(Object.entries(args).map(([k,v])=>[k,String(v)]));
  const metadata=operation==='output-settings'||operation==='retired-outputs';
  const response=await fetch(method==='GET'?`/api/${operation}?${query}`:`/api/${operation}`,method==='POST'?{method,headers:{'Content-Type':'application/json','X-Hub-Admin':'1'},body:JSON.stringify(args)}:metadata?{headers:{'X-Hub-Admin':'1'}}:undefined);
  const value=await response.json();
  if(!response.ok)throw new Error(value.error||'処理を完了できませんでした');
  return value as T;
}
export function read<T>(endpoint:string,project='',extras:Record<string,string>={}) { return request<T>(endpoint,{project,...extras},'GET'); }
export function write(endpoint:string,args:Record<string,unknown>={}) { return request<Record<string,unknown>>(`admin/${endpoint}`,args,'POST'); }
