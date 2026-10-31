import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import Markdown from 'react-markdown';
import { remarkWiki, sameTarget } from './wiki';
import './style.css';

type Project = { id: string; name: string };
type Hit = { path: string; title: string };
type Doc = Hit & { markdown: string };
type Link = { source: string; target: string; resolved: string | null; status: string };
type LinkPage = { items: Link[]; next_offset: number | null };
async function api<T>(endpoint: string, project = '', extras: Record<string, string> = {}): Promise<T> {
  const response = await fetch(`/api/${endpoint}?${new URLSearchParams({ project, ...extras })}`);
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || '接続できませんでした');
  return value as T;
}
function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [project, setProject] = useState('');
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<Hit[]>([]);
  const [doc, setDoc] = useState<Doc | null>(null);
  const [backlinks, setBacklinks] = useState<string[]>([]);
  const [links, setLinks] = useState<Link[]>([]);
  const [nextLinkOffset, setNextLinkOffset] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  useEffect(() => { api<Project[]>('projects').then(p => { setProjects(p); setProject(p[0]?.id || ''); }).catch(e => setError(String(e))); }, []);
  useEffect(() => {
    generation.current++;
    let active = true;
    setDoc(null); setBacklinks([]); setLinks([]); setNextLinkOffset(null); setHits([]); setQ(''); setError('');
    if (project) {
      setBusy(true);
      api<Hit[]>('search', project).then(h => { if (active) setHits(h); })
        .catch(e => { if (active) setError(String(e)); }).finally(() => { if (active) setBusy(false); });
    }
    return () => { active = false; };
  }, [project]);
  async function search() {
    const current = ++generation.current;
    setError(''); setBusy(true);
    try { const found = await api<Hit[]>('search', project, { q }); if (current === generation.current) setHits(found); }
    catch (e) { if (current === generation.current) setError(String(e)); }
    finally { if (current === generation.current) setBusy(false); }
  }
  async function open(path: string) {
    const current = ++generation.current;
    setError(''); setBusy(true);
    try {
      const [d, b, page] = await Promise.all([api<Doc>('document', project, { path }), api<string[]>('backlinks', project, { path }), api<LinkPage>('document-links', project, { path })]);
      if (current === generation.current) { setDoc(d); setBacklinks(b); setLinks(page.items); setNextLinkOffset(page.next_offset); }
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
  const outgoing = links.filter(l => l.source === doc?.path);
  return <div className="app">
    <header><div className="brand">MK<span>H</span></div><div><h1>Markdown Knowledge Hub</h1><p>プロジェクトの知識を、つながりから読む</p></div><span className="badge">読み取り専用</span></header>
    <section className="toolbar" aria-label="検索条件">
      <label>プロジェクト<select value={project} onChange={e => setProject(e.target.value)}>{projects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
      <form onSubmit={e => { e.preventDefault(); void search(); }}><label htmlFor="query">全文検索</label><div className="search"><input id="query" value={q} onChange={e => setQ(e.target.value)} placeholder="キーワードを入力（日本語・中文・English）"/><button disabled={!project || busy}>検索</button></div></form>
    </section>
    {error && <p role="alert" className="error">{error}</p>}
    {!projects.length && !error && <p className="empty">管理CLIでMarkdownフォルダを登録し、スキャンしてください。</p>}
    <div className="workspace" aria-busy={busy}>
      <nav aria-label="文書一覧"><h2>文書 <small>{hits.length}件 / 上限50件</small></h2>{hits.map(h => <button className={`hit ${doc?.path === h.path ? 'selected' : ''}`} key={h.path} onClick={() => void open(h.path)}><strong>{h.title}</strong><span>{h.path}</span></button>)}{project && !hits.length && <p>該当する文書がありません。</p>}</nav>
      <main>{doc ? <><div className="docpath">{doc.path}</div><Markdown remarkPlugins={[remarkWiki]} components={{
        a: ({ href, children }) => {
          const link = outgoing.find(l => sameTarget(href, l.target));
          if (link?.resolved) return <button className="inline" onClick={() => void open(link.resolved!)}>{children}</button>;
          if (href?.startsWith('https://') || href?.startsWith('http://')) return <a href={href} target="_blank" rel="noreferrer">{children}</a>;
          return <span title="未解決リンク、または見出し参照">{children}</span>;
        },
        img: ({ alt }) => <span className="image-placeholder">[画像: {alt || '画像'}]</span>
      }}>{doc.markdown}</Markdown></> : <div className="empty"><span className="symbol">↔</span><h2>知識のつながりを見つける</h2><p>文書を選ぶと、本文と参照元を表示します。</p></div>}</main>
      <aside><h2>Backlinks</h2><p className="hint">この文書への参照（上限500件）</p>{backlinks.map(p => <button className="hit" key={p} onClick={() => void open(p)}>{p}</button>)}{!backlinks.length && <p>参照元はありません。</p>}<h2>出リンク</h2>{outgoing.map((l, i) => <div className="link" key={i}>{l.resolved ? <button className="inline" onClick={() => void open(l.resolved!)}>{l.target}</button> : <span>{l.target}<small> {l.status === 'ambiguous' ? '同名候補あり' : 'リンク先なし'}</small></span>}</div>)}{nextLinkOffset !== null && <button disabled={busy} onClick={() => void moreLinks()}>出リンクをさらに読み込む</button>}<p className="hint">出リンクは文書ごとに100件ずつ取得します。全体グラフ・同名／孤立文書の集計は後続工程です。</p></aside>
    </div><footer>原本を変更せず、索引から閲覧しています。更新は管理CLIまたは監視プロセスで反映します。</footer>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
