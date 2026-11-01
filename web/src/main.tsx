import React, { Suspense,lazy,useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { remarkWiki, sameTarget,rehypeHeadingIds,headingId,displayMarkdown } from './wiki';
import './style.css';
import { read as api } from './transport';
import { WatchManager } from './WatchManager';
import { ProjectManager } from './ProjectManager';
const GraphView=lazy(()=>import('./GraphView').then(module=>({default:module.GraphView})));

type Project = { id: string; name: string; root:string };
type Hit = { path: string; title: string };
type Doc = Hit & { markdown: string };
type Link = { source: string; target: string; resolved: string | null; status: string };
type Page<T> = {items:T[];next_offset:number|null};
type LinkPage = { items: Link[]; next_offset: number | null };
function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [project, setProject] = useState('');
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<Hit[]>([]);
  const [nextHit,setNextHit]=useState<number|null>(null);
  const [nextBack,setNextBack]=useState<number|null>(null);
  const [searched,setSearched]=useState('');
  const [doc, setDoc] = useState<Doc | null>(null);
  const [backlinks, setBacklinks] = useState<string[]>([]);
  const [links, setLinks] = useState<Link[]>([]);
  const [nextLinkOffset, setNextLinkOffset] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [readBusy, setBusy] = useState(false);
  const [adminBusy,setAdminBusy]=useState(false);
  const busy=readBusy||adminBusy;
  const [capabilities,setCapabilities]=useState<{admin:boolean;folder_picker:boolean;desktop?:boolean;data_dir?:string}>({admin:false,folder_picker:false});
  const [stats,setStats]=useState<{documents:number;links:number;missing:number;missing_heading:number;ambiguous:number;isolated:number;duplicate_names:number}|null>(null);
  const [anchor,setAnchor]=useState('');
  useEffect(()=>{if(anchor&&doc){let id=anchor;try{id=decodeURIComponent(anchor);}catch{};(window.document.getElementById(id)||window.document.getElementById(headingId(id)))?.scrollIntoView();}},[doc,anchor]);
  const [view,setView]=useState<'wiki'|'graph'>('wiki');
  const [revision,setRevision]=useState(0);
  const generation = useRef(0);
  useEffect(() => { api<Project[]>('projects').then(p => { setProjects(p); setProject(p[0]?.id || ''); }).catch(e => setError(String(e))); }, []);
  useEffect(()=>{api<typeof capabilities>('capabilities').then(setCapabilities).catch(e=>setError(String(e)));},[]);
  async function refreshProjects(preferred?:string) {
    generation.current++;
    const list=await api<Project[]>('projects');
    setProjects(list);setProject(preferred&&list.some(p=>p.id===preferred)?preferred:list[0]?.id||'');setRevision(v=>v+1);
  }
  useEffect(() => {
    generation.current++;
    let active = true;
    setDoc(null); setBacklinks([]); setLinks([]); setNextLinkOffset(null); setNextHit(null);setNextBack(null);setHits([]); setQ(''); setError('');setStats(null);setView('wiki');
    if (project) {
      setBusy(true);
      Promise.all([api<Page<Hit>>('search-page', project,{limit:'50'}),api<typeof stats>('stats',project)]).then(async ([h,s]) => {
        if(!active)return;
        setHits(h.items);setNextHit(h.next_offset);setSearched('');setStats(s);
        if(h.items[0]) {
          const path=h.items[0].path;
          const [d,b,page]=await Promise.all([api<Doc>('document',project,{path}),api<Page<string>>('backlinks-page',project,{path,limit:'100'}),api<LinkPage>('document-links',project,{path})]);
          if(active){setDoc(d);setBacklinks(b.items);setNextBack(b.next_offset);setLinks(page.items);setNextLinkOffset(page.next_offset);}
        }
      })
        .catch(e => { if (active) setError(String(e)); }).finally(() => { if (active) setBusy(false); });
    }
    return () => { active = false; };
  }, [project,revision]);
  async function search() {
    const current = ++generation.current;
    setError(''); setBusy(true);
    try { const found = await api<Page<Hit>>('search-page', project, { q,limit:'50' }); if (current === generation.current) {setHits(found.items);setNextHit(found.next_offset);setSearched(q);} }
    catch (e) { if (current === generation.current) setError(String(e)); }
    finally { if (current === generation.current) setBusy(false); }
  }
  async function open(path: string, fragment='') {
    if(busy)return;
    const current = ++generation.current;
    setError(''); setBusy(true);
    try {
      const [d, b, page] = await Promise.all([api<Doc>('document', project, { path }), api<Page<string>>('backlinks-page', project, { path,limit:'100' }), api<LinkPage>('document-links', project, { path })]);
      if (current === generation.current) { setDoc(d); setBacklinks(b.items);setNextBack(b.next_offset); setLinks(page.items); setNextLinkOffset(page.next_offset);setAnchor(fragment);setView('wiki'); }
    } catch (e) { if (current === generation.current) setError(String(e)); }
    finally { if (current === generation.current) setBusy(false); }
  }
  async function moreLinks() {
    if (!doc || nextLinkOffset === null || busy) return;
    const current = generation.current;
    setError(''); setBusy(true);
    try {
      const page = await api<LinkPage>('document-links', project, { path: doc.path, offset: String(nextLinkOffset) });
      if (current === generation.current) { setLinks(l => [...l, ...page.items]); setNextLinkOffset(page.next_offset); }
    } catch (e) { if (current === generation.current) setError(String(e)); }
    finally { if (current === generation.current) setBusy(false); }
  }
  async function more(kind:'hits'|'backlinks') {
    const offset=kind==='hits'?nextHit:nextBack;
    if(offset===null||busy)return;
    const current=generation.current;setBusy(true);setError('');
    try {
      if(kind==='hits') {const page=await api<Page<Hit>>('search-page',project,{q:searched,offset:String(offset),limit:'50'});if(current===generation.current){setHits(v=>[...v,...page.items]);setNextHit(page.next_offset);}}
      else if(doc){const page=await api<Page<string>>('backlinks-page',project,{path:doc.path,offset:String(offset),limit:'100'});if(current===generation.current){setBacklinks(v=>[...v,...page.items]);setNextBack(page.next_offset);}}
    }catch(e){if(current===generation.current)setError(String(e));}finally{if(current===generation.current)setBusy(false);}
  }
  async function navigate(target:string){
    if(!doc||busy)return;const current=generation.current;
    try{const link=await api<Link|null>('link-target',project,{path:doc.path,target});if(current!==generation.current)return;if(link?.resolved)await open(link.resolved,link.target.split('#').slice(1).join('#'));else setError(link?.status==='missing-heading'?'参照先の見出しがありません。':'リンク先が見つからないか、同名候補が複数あります。');}catch(e){if(current===generation.current)setError(String(e));}
  }
  const outgoing = links.filter(l => l.source === doc?.path);
  return <div className="app">
    <header><div className="brand">MK<span>H</span></div><div><h1>Markdown Knowledge Hub</h1><p>プロジェクトの知識を、つながりから読む{capabilities.desktop?' · Desktop':''}</p></div><span className="badge">原本Markdownは読み取り専用</span></header>
    <section className="toolbar" aria-label="検索条件">
      <label>プロジェクト<select disabled={busy} value={project} onChange={e => setProject(e.target.value)}><option value="" disabled>プロジェクトを選択</option>{projects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
      <form onSubmit={e => { e.preventDefault(); void search(); }}><label htmlFor="query">全文検索</label><div className="search"><input id="query" value={q} onChange={e => setQ(e.target.value)} placeholder="キーワードを入力（日本語・中文・English）"/><button disabled={!project || busy}>検索</button></div></form>
    </section>
    <ProjectManager current={projects.find(p=>p.id===project)} enabled={capabilities.admin} folderPicker={capabilities.folder_picker} onChanged={refreshProjects} onBusy={setAdminBusy}/>
    {capabilities.desktop&&project&&<WatchManager key={project} project={project} onRefresh={()=>setRevision(v=>v+1)}/>}
    {stats&&<div className="project-stats"><span>文書 <strong>{stats.documents.toLocaleString()}</strong></span><span>リンク <strong>{stats.links.toLocaleString()}</strong></span><span>リンク先なし <strong>{stats.missing.toLocaleString()}</strong></span><span>同名候補あり <strong>{stats.ambiguous.toLocaleString()}</strong></span><span>見出しなし <strong>{stats.missing_heading.toLocaleString()}</strong></span><span title="他の文書との解決済みリンクがない文書">孤立文書 <strong>{stats.isolated.toLocaleString()}</strong></span><span>同名文書群 <strong>{stats.duplicate_names.toLocaleString()}</strong></span></div>}
    {project&&<div className="view-tabs" role="tablist" aria-label="表示切替"><button role="tab" aria-selected={view==='wiki'} onClick={()=>setView('wiki')}>Wiki</button><button role="tab" aria-selected={view==='graph'} onClick={()=>setView('graph')}>リンク図</button>{doc&&view==='graph'&&<button onClick={()=>setDoc(null)}>プロジェクト全体を見る</button>}</div>}
    {error && <p role="alert" className="error">{error}</p>}
    {!projects.length && !error && <p className="empty">「新規プロジェクト」からMarkdownフォルダを登録してください。</p>}
    <div className="workspace" aria-busy={busy}>
      <nav aria-label="文書一覧"><h2>文書 <small>{hits.length}件</small></h2>{hits.map(h => <button disabled={busy} className={`hit ${doc?.path === h.path ? 'selected' : ''}`} key={h.path} onClick={() => void open(h.path)}><strong>{h.title}</strong><span>{h.path}</span></button>)}{nextHit!==null&&<button disabled={busy} onClick={()=>void more('hits')}>文書をさらに読み込む</button>}{project && !hits.length && <p>該当する文書がありません。</p>}</nav>
      <main>{view==='graph'&&project?<Suspense fallback={<p>リンク図を準備しています…</p>}><GraphView key={`${project}-${revision}`} project={project} center={doc?.path} onOpen={path=>void open(path)}/></Suspense>:doc ? <><div className="docpath">{doc.path}</div><Markdown skipHtml remarkPlugins={[remarkGfm,remarkWiki]} rehypePlugins={[rehypeHeadingIds]} components={{
        a: ({ href, children }) => {
          const link = outgoing.find(l => sameTarget(href, l.target));
          if (link?.resolved) return <button className="inline" onClick={() => void open(link.resolved!,link.target.split('#').slice(1).join('#'))}>{children}</button>;
          if (href?.startsWith('https://') || href?.startsWith('http://')) return <a href={href} target="_blank" rel="noreferrer">{children}</a>;
          if(href&&!link)return <button className="inline" onClick={()=>void navigate(href)}>{children}</button>;
          return <span title="リンク先または見出しがありません">{children}</span>;
        },
        img: ({ alt }) => <span className="image-placeholder">[画像: {alt || '画像'}]</span>
      }}>{displayMarkdown(doc.markdown)}</Markdown></> : <div className="empty"><span className="symbol">↔</span><h2>知識のつながりを見つける</h2><p>文書を選ぶと、本文と参照元を表示します。</p></div>}</main>
      <aside><h2>Backlinks</h2><p className="hint">この文書への参照</p>{backlinks.map(p => <button className="hit" key={p} onClick={() => void open(p)}>{p}</button>)}{nextBack!==null&&<button disabled={busy} onClick={()=>void more('backlinks')}>参照元をさらに読み込む</button>}{!backlinks.length && <p>参照元はありません。</p>}<h2>出リンク</h2>{outgoing.map((l, i) => <div className="link" key={i}>{l.resolved ? <button className="inline" onClick={() => void open(l.resolved!,l.target.split('#').slice(1).join('#'))}>{l.target}</button> : <span>{l.target}<small> {l.status === 'ambiguous' ? '同名候補あり' : l.status==='missing-heading'?'見出しなし':'リンク先なし'}</small></span>}</div>)}{nextLinkOffset !== null && <button disabled={busy} onClick={() => void moreLinks()}>出リンクをさらに読み込む</button>}<p className="hint">出リンクは文書ごとに100件ずつ取得します。リンク図タブで文書間の関係を確認できます。</p></aside>
    </div><footer>原本を変更せず、索引から閲覧しています。変更後は「再作成」または監視プロセスで反映できます。{capabilities.desktop&&<div>データ保存先: {capabilities.data_dir}</div>}</footer>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
