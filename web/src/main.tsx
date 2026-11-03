import { MermaidDiagram } from './MermaidDiagram';
import { remarkFlowTables } from './flowTables';
import React, { Suspense,lazy,useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import Markdown, {type Components} from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { remarkWiki, sameTarget,rehypeHeadingIds,headingId,displayMarkdown } from './wiki';
import './style.css';
import { read as api } from './transport';
import { WatchManager } from './WatchManager';
import { ProjectManager, type Project } from './ProjectManager';
const GraphView=lazy(()=>import('./GraphView').then(module=>({default:module.GraphView})));


type Hit = { path: string; title: string };
type Backlink = Hit & {call:boolean};
type Doc = Hit & { markdown: string };
type Link = { source: string; target: string; resolved: string | null; status: string; resolved_title?:string|null };
type Page<T> = {items:T[];next_offset:number|null};
type LinkPage = { items: Link[]; next_offset: number | null };
const WikiPre: NonNullable<Components['pre']> = ({children,node: _node,...props}) => {
  const code=React.Children.toArray(children)[0];
  if(React.isValidElement<{className?:string;children?:React.ReactNode}>(code)&&code.props.className==='language-mermaid'){
    return <MermaidDiagram source={String(code.props.children??'').trim()}/>;
  }
  return <pre {...props}>{children}</pre>;
};
type HistoryEntry = { query:string; searchSource:string; source:string; path:string; anchor:string; scroll:number; view:'wiki'|'graph' };
type WikiView = {document:Doc;backlinks:Page<Backlink>;links:LinkPage};
function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [project, setProject] = useState('');
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<Hit[]>([]);
  const [nextHit,setNextHit]=useState<number|null>(null);
  const [nextBack,setNextBack]=useState<number|null>(null);
  const [searched,setSearched]=useState('');
  const [searchedSource,setSearchedSource]=useState('');
  const [scope,setScope]=useState<'project'|'source'>('project');
  const [doc, setDoc] = useState<Doc | null>(null);
  const [backlinks, setBacklinks] = useState<Backlink[]>([]);
  const [links, setLinks] = useState<Link[]>([]);
  const [nextLinkOffset, setNextLinkOffset] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [readBusy, setBusy] = useState(false);
  const [adminBusy,setAdminBusy]=useState(false);
  const busy=readBusy||adminBusy;
  const [capabilities,setCapabilities]=useState<{admin:boolean;folder_picker:boolean;desktop?:boolean}>({admin:false,folder_picker:false});
  const [stats,setStats]=useState<{documents:number;links:number;missing:number;missing_heading:number;ambiguous:number;isolated:number;duplicate_names:number}|null>(null);
  const [expanded,setExpanded]=useState(false);
  const [graphExpanded,setGraphExpanded]=useState(false);
  const leftPane=useRef<HTMLDivElement>(null);
  const centralPane=useRef<HTMLElement>(null);
  const [anchor,setAnchor]=useState('');
  useEffect(()=>{if(anchor&&doc){let id=anchor;try{id=decodeURIComponent(anchor);}catch{};(window.document.getElementById(id)||window.document.getElementById(headingId(id)))?.scrollIntoView();}},[doc,anchor]);
  useEffect(()=>{if(!anchor)centralPane.current?.scrollTo(0,0);},[doc?.path,anchor]);
  useEffect(()=>{
    if(!expanded&&!graphExpanded)return;
    const exit=(event:KeyboardEvent)=>{
      if(event.key!=='Escape'||event.defaultPrevented)return;
      if(graphExpanded)setGraphExpanded(false);
      else setExpanded(false);
    };
    window.addEventListener('keydown',exit);
    return ()=>window.removeEventListener('keydown',exit);
  },[expanded,graphExpanded]);
  const [view,setView]=useState<'wiki'|'graph'>('wiki');
  useEffect(()=>{if(view!=='graph')setGraphExpanded(false);},[view]);
  const [revision,setRevision]=useState(0);
  const generation = useRef(0);
  const [history,setHistory]=useState<{entries:HistoryEntry[];position:number}>({entries:[],position:-1});
  const pendingScroll=useRef<number|null>(null);
  useEffect(()=>{
    if(pendingScroll.current===null)return;
    centralPane.current?.scrollTo(0,pendingScroll.current);
    pendingScroll.current=null;
  },[history.position,doc,view]);

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
    setHistory({entries:[],position:-1}); pendingScroll.current=null;
    setDoc(null); setExpanded(false); setGraphExpanded(false); setAnchor(''); setBacklinks([]); setLinks([]); setNextLinkOffset(null); setNextHit(null);setNextBack(null);setHits([]); setQ('');setSearched('');setSearchedSource('');setScope('project'); setError('');setStats(null);setView('wiki');
    if (project) {
      setBusy(true);
      Promise.all([api<Page<Hit>>('search-page', project,{limit:'50'}),api<typeof stats>('stats',project)]).then(async ([h,s]) => {
        if(!active)return;
        setHits(h.items);setNextHit(h.next_offset);setSearched('');setStats(s);
        if(h.items[0]) {
          const path=h.items[0].path;
          const {document:d,backlinks:b,links:page}=await api<WikiView>('wiki-view',project,{path});
          if(active){setDoc(d);setBacklinks(b.items);setNextBack(b.next_offset);setLinks(page.items);setNextLinkOffset(page.next_offset);}
        }
      })
        .catch(e => { if (active) setError(String(e)); }).finally(() => { if (active) setBusy(false); });
    }
    return () => { active = false; };
  }, [project,revision]);
  function capture():HistoryEntry {
    return {query:searched,searchSource:searchedSource,source,path:doc?.path??'',anchor,scroll:centralPane.current?.scrollTop??0,view};
  }
  function record(before:HistoryEntry,after:HistoryEntry){
    setHistory(previous=>{
      const entries=previous.entries.slice(0,previous.position);
      entries.push(before,after);
      const bounded=entries.slice(-60);
      return {entries:bounded,position:bounded.length-1};
    });
  }
  function applyWiki(wiki:WikiView|null){
    setDoc(wiki?.document??null);
    setBacklinks(wiki?.backlinks.items??[]);
    setNextBack(wiki?.backlinks.next_offset??null);
    setLinks(wiki?.links.items??[]);
    setNextLinkOffset(wiki?.links.next_offset??null);
  }
  async function search(clear=false) {
    if(busy)return;
    const before=capture();
    const query=clear?'':q.trim();
    const searchSource=query&&scope==='source'?source:'';
    const current=++generation.current;
    setError('');setBusy(true);
    try{
      const found=await api<Page<Hit>>('search-page',project,{q:query,source:searchSource,limit:'50'});
      if(current!==generation.current)return;
      setHits(found.items);setNextHit(found.next_offset);setSearched(query);setSearchedSource(searchSource);setQ(query);
      record(before,{...before,query,searchSource});
      leftPane.current?.scrollTo(0,0);
    }catch(e){if(current===generation.current)setError(String(e));}
    finally{if(current===generation.current)setBusy(false);}
  }
  async function open(path:string,fragment=''){
    if(busy)return;
    const before=capture();
    const nextSource=sourceFor(path);
    const after:HistoryEntry={...before,source:nextSource,path,anchor:fragment,scroll:0,view:'wiki'};
    if(path===doc?.path){
      if(fragment!==anchor||view!=='wiki')record(before,after);
      setView('wiki');setAnchor(fragment);setSource(nextSource);
      if(!fragment)centralPane.current?.scrollTo(0,0);
      return;
    }
    const current=++generation.current;
    setError('');setBusy(true);
    try{
      const wiki=await api<WikiView>('wiki-view',project,{path});
      if(current!==generation.current)return;
      applyWiki(wiki);setSource(nextSource);setAnchor(fragment);setView('wiki');
      record(before,after);
    }catch(e){if(current===generation.current)setError(String(e));}
    finally{if(current===generation.current)setBusy(false);}
  }
  async function moveHistory(delta:number){
    const position=history.position+delta;
    const entry=history.entries[position];
    if(busy||!entry)return;
    const before=capture();
    const current=++generation.current;
    setError('');setBusy(true);
    try{
      const [found,wiki]=await Promise.all([
        entry.query===searched&&entry.searchSource===searchedSource?null:api<Page<Hit>>('search-page',project,{q:entry.query,source:entry.searchSource,limit:'50'}),
        entry.path?api<WikiView>('wiki-view',project,{path:entry.path}):null,
      ]);
      if(current!==generation.current)return;
      if(found){setHits(found.items);setNextHit(found.next_offset);}
      setQ(entry.query);setSearched(entry.query);setSearchedSource(entry.searchSource);setScope(entry.searchSource?'source':'project');setSource(entry.source);
      applyWiki(wiki);setAnchor(entry.anchor);setView(entry.view);
      pendingScroll.current=entry.scroll;
      const entries=history.entries.slice();
      entries[history.position]=before;
      setHistory({entries,position});
    }catch(e){if(current===generation.current)setError(String(e));}
    finally{if(current===generation.current)setBusy(false);}
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
      if(kind==='hits') {const page=await api<Page<Hit>>('search-page',project,{q:searched,source:searchedSource,offset:String(offset),limit:'50'});if(current===generation.current){setHits(v=>[...v,...page.items]);setNextHit(page.next_offset);}}
      else if(doc){const page=await api<Page<Backlink>>('backlink-details',project,{path:doc.path,offset:String(offset),limit:'100'});if(current===generation.current){setBacklinks(v=>[...v,...page.items]);setNextBack(page.next_offset);}}
    }catch(e){if(current===generation.current)setError(String(e));}finally{if(current===generation.current)setBusy(false);}
  }
  async function navigate(target:string){
    if(!doc||busy)return;const current=generation.current;
    try{const link=await api<Link|null>('link-target',project,{path:doc.path,target});if(current!==generation.current)return;if(link?.resolved)await open(link.resolved,link.target.split('#').slice(1).join('#'));else setError(link?.status==='missing-heading'?'参照先の見出しがありません。':'リンク先が見つからないか、同名候補が複数あります。');}catch(e){if(current===generation.current)setError(String(e));}
  }
  const [sources,setSources]=useState<Hit[]>([]);
  const [source,setSource]=useState('');
  const [sourcePages,setSourcePages]=useState<Hit[]>([]);
  useEffect(()=>{
    let active=true;
    setSource('');
    setSources([]);
    if(project) void api<Hit[]>('oml-catalog',project).then(items=>{
      if(active)setSources(items);
    }).catch(()=>{});
    return ()=>{active=false;};
  },[project,revision]);
  useEffect(()=>{
    let active=true;
    setSourcePages([]);
    if(project&&source)void api<Hit[]>('oml-catalog',project,{path:source}).then(items=>{
      if(active)setSourcePages(items);
    }).catch(e=>{if(active)setError(String(e));});
    return ()=>{active=false;};
  },[project,source,revision]);
  const sourceTitles=useMemo(()=>new Map(sources.map(item=>[item.path,item.title])),[sources]);
  function referenceTitle(path:string,title:string){
    const slash=path.indexOf('/');
    const original=slash<0?undefined:sourceTitles.get(path.slice(0,slash)+'.md');
    return original?`（${original}）${title}`:title;
  }
  function sourceFor(path:string){
    const folder=path.split('/')[0];
    const match=sources.find(item=>item.path===path||item.path===folder+'.md');
    return match?.path??'';
  }
  useEffect(()=>{
    const pane=leftPane.current;
    const selected=pane?.querySelector<HTMLElement>('[aria-current="page"]');
    if(!pane||!selected)return;
    const bounds=pane.getBoundingClientRect();
    const row=selected.getBoundingClientRect();
    if(row.top<bounds.top)pane.scrollTop+=row.top-bounds.top;
    else if(row.bottom>bounds.bottom)pane.scrollTop+=row.bottom-bounds.bottom;
  },[doc?.path,sourcePages,hits,expanded]);
  const listed=searched?hits:source?sourcePages:hits;
  const visibleHits=doc&&!searched&&!source&&!listed.some(hit=>hit.path===doc.path)?[doc,...listed]:listed;
  const groups=new Map<string,Hit[]>();
  for(const hit of visibleHits){
    const kind=searched?'検索結果':source?(hit.path===source?'元仕様書':hit.title.split(' · ')[0]):'文書';
    const items=groups.get(kind)??[];
    items.push(hit);
    groups.set(kind,items);
  }
  const outgoing = links.filter(l => l.source === doc?.path);
  const wikiNavigation=useRef({open,navigate});
  wikiNavigation.current={open,navigate};
  const wikiBody=useMemo(()=>{
    if(!doc)return null;
    const outgoing=links.filter(link=>link.source===doc.path);
    return <Markdown skipHtml remarkPlugins={[remarkGfm,remarkWiki,remarkFlowTables]} rehypePlugins={[rehypeHeadingIds]} components={{
        pre: WikiPre,
        a: ({ href, children }) => {
          const link = outgoing.find(l => sameTarget(href, l.target));
          if (link?.resolved) return <button className="inline" onClick={() => void wikiNavigation.current.open(link.resolved!,link.target.split('#').slice(1).join('#'))}>{children}</button>;
          if (href?.startsWith('https://') || href?.startsWith('http://')) return <a href={href} target="_blank" rel="noreferrer">{children}</a>;
          if(href&&!link)return <button className="inline" onClick={()=>void wikiNavigation.current.navigate(href)}>{children}</button>;
          return <span title="リンク先または見出しがありません">{children}</span>;
        },
        img: ({ alt }) => <span className="image-placeholder">[画像: {alt || '画像'}]</span>
      }}>{displayMarkdown(doc.markdown)}</Markdown>;
  },[doc,links]);
  return <div className={`app ${expanded?'wiki-expanded':''} ${graphExpanded&&view==='graph'?'graph-expanded':''}`}>
    <header><div className="brand">MK<span>H</span></div><div><h1>Markdown Knowledge Hub</h1><p>プロジェクトの知識を、つながりから読む{capabilities.desktop?' · Desktop':''}</p></div><span className="badge">原本Markdownは読み取り専用</span></header>
    <section className="toolbar" aria-label="プロジェクト選択">
      <label>プロジェクト<select disabled={busy} value={project} onChange={e => setProject(e.target.value)}><option value="" disabled>プロジェクトを選択</option>{projects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>

    </section>
    <details className="workspace-settings"><summary>プロジェクト管理・監視</summary><ProjectManager current={projects.find(p=>p.id===project)} enabled={capabilities.admin} folderPicker={capabilities.folder_picker} onChanged={refreshProjects} onBusy={setAdminBusy}/>
    {capabilities.desktop&&project&&<WatchManager key={project} project={project} onRefresh={()=>setRevision(v=>v+1)}/>}</details>
    {stats&&<div className="project-stats"><span>文書 <strong>{stats.documents.toLocaleString()}</strong></span><span>リンク <strong>{stats.links.toLocaleString()}</strong></span><span>リンク先なし <strong>{stats.missing.toLocaleString()}</strong></span><span>同名候補あり <strong>{stats.ambiguous.toLocaleString()}</strong></span><span>見出しなし <strong>{stats.missing_heading.toLocaleString()}</strong></span><span title="他の文書との解決済みリンクがない文書">孤立文書 <strong>{stats.isolated.toLocaleString()}</strong></span><span>同名文書群 <strong>{stats.duplicate_names.toLocaleString()}</strong></span></div>}
    {project&&<div className="view-tabs" aria-label="Wiki閲覧操作">
      <button type="button" disabled={busy||history.position<=0} onClick={()=>void moveHistory(-1)}>‹ 戻る</button>
      <button type="button" disabled={busy||history.position>=history.entries.length-1} onClick={()=>void moveHistory(1)}>進む ›</button>
      <div className="wiki-tabs" role="tablist" aria-label="表示切替">
        <button role="tab" aria-selected={view==='wiki'} onClick={()=>setView('wiki')}>Wiki</button>
        <button role="tab" aria-selected={view==='graph'} onClick={()=>setView('graph')}>リンク図</button>
      </div>
      {doc&&view==='graph'&&<button onClick={()=>setDoc(null)}>プロジェクト全体を見る</button>}
      <form className="wiki-search" onSubmit={event=>{event.preventDefault();void search();}}>
        <input aria-label="全文検索" value={q} onChange={event=>setQ(event.target.value)} placeholder="Wikiを全文検索"/>
        <select aria-label="検索範囲" value={source?scope:'project'} onChange={event=>setScope(event.target.value as typeof scope)}>
          <option value="project">プロジェクト全体</option>
          <option value="source" disabled={!source}>選択中の元仕様書</option>
        </select>
        <button disabled={busy||!q.trim()}>検索</button>
      </form>
      <button className="expand-workspace" type="button" aria-pressed={expanded} onClick={()=>setExpanded(value=>!value)}>{expanded?'通常表示に戻す（Esc）':'閲覧エリアを全画面に拡大'}</button>
    </div>}
    {error && <p role="alert" className="error">{error}</p>}
    {!projects.length && !error && <p className="empty">「新規プロジェクト」からMarkdownフォルダを登録してください。</p>}
    <div className="workspace" aria-busy={busy}>
      <nav className="source-navigation" aria-label="文書一覧">
        {sources.length>0&&<label className="source-filter">元仕様書<select aria-label="元仕様書" value={source} onChange={e=>setSource(e.target.value)}>
          <option value="">すべての仕様書</option>
          {sources.map(item=><option key={item.path} value={item.path}>{item.title}</option>)}
        </select></label>}
        {searched&&<div className="search-summary">
          <span>「{searched}」 · {searchedSource?(sourceTitles.get(searchedSource)??'元仕様書'):'プロジェクト全体'}</span>
          <button type="button" disabled={busy} onClick={()=>void search(true)}>検索結果を閉じる</button>
        </div>}
        <div className="source-list" ref={leftPane}>
        {[...groups].map(([kind,items])=><section key={kind}>
          <h2>{kind} <small>{items.length}件</small></h2>
          {items.map(hit=><button disabled={busy} className={`hit ${doc?.path===hit.path?'selected':''}`} key={hit.path} aria-current={doc?.path===hit.path?'page':undefined} onClick={()=>void open(hit.path)}>
            <strong>{source&&!searched&&hit.path!==source?hit.title.split(' · ').slice(1).join(' · ')||hit.title:hit.title}</strong>
            {(!source||searched)&&<span>{hit.path}</span>}
          </button>)}
        </section>)}
        {(searched||!source)&&nextHit!==null&&<button disabled={busy} onClick={()=>void more('hits')}>文書をさらに読み込む</button>}
        {project&&!visibleHits.length&&<p>該当する文書がありません。</p>}
        </div>
      </nav>
      <main ref={centralPane}>

      {view==='graph'&&project?<Suspense fallback={<p>リンク図を準備しています…</p>}><GraphView key={`${project}-${revision}`} project={project} center={doc?.path} expanded={graphExpanded} onToggleExpand={()=>setGraphExpanded(value=>!value)} onOpen={path=>void open(path)}/></Suspense>:doc ? <><div className="docpath">{doc.path}</div>{wikiBody}</> : <div className="empty"><span className="symbol">↔</span><h2>知識のつながりを見つける</h2><p>文書を選ぶと、本文と参照元を表示します。</p></div>}</main>
      <aside>
        <h2>参照元</h2>
        {backlinks.map(item=><button className="hit" key={item.path} disabled={busy} onClick={()=>void open(item.path)}>
          <strong>{referenceTitle(item.path,item.title)}</strong><span>{item.call?'呼び出し元':'文書参照'} · {item.path}</span>
        </button>)}
        {nextBack!==null&&<button disabled={busy} onClick={()=>void more('backlinks')}>参照元をさらに読み込む</button>}
        {!backlinks.length&&<p>参照元はありません。</p>}
        <h2>参照先</h2>
        {outgoing.filter(link=>link.resolved!==doc?.path).map((link,index)=><div className="link" key={index}>
          {link.resolved?<button className="hit" disabled={busy} onClick={()=>void open(link.resolved!,link.target.split('#').slice(1).join('#'))}>
            <strong>{referenceTitle(link.resolved,link.resolved_title??link.target)}</strong><span>{link.target.endsWith('#oml-call')?'呼び出し先':'文書参照'}</span>
          </button>:<span>{link.target}<small> {link.status==='ambiguous'?'同名候補あり':link.status==='missing-heading'?'見出しなし':'リンク先なし'}</small></span>}
        </div>)}
        {nextLinkOffset!==null&&<button disabled={busy} onClick={()=>void moreLinks()}>参照先をさらに読み込む</button>}
      </aside>
    </div><footer>入力Markdownは読み取り専用です。変更後は「再作成」または監視で反映できます。生成データの保存先は「プロジェクトの保存先・設定」で確認・変更できます。</footer>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
