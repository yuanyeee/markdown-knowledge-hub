import React, {
  Suspense,
  lazy,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { createRoot } from 'react-dom/client';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  displayMarkdown,
  extractHeadings,
  headingId,
  highlightParts,
  rehypeHeadingIds,
  remarkWiki,
  sameTarget,
} from './wiki';
import './style.css';
import { read as api } from './transport';
import { WatchManager } from './WatchManager';
import { ProjectManager, type Project } from './ProjectManager';

const GraphView = lazy(() =>
  import('./GraphView').then((module) => ({ default: module.GraphView })),
);

type Hit = {
  path: string;
  title: string;
  snippet?: string;
};

type Doc = Hit & { markdown: string };

type Link = {
  source: string;
  target: string;
  resolved: string | null;
  status: string;
};

type Page<T> = {
  items: T[];
  next_offset: number | null;
};

type LinkPage = {
  items: Link[];
  next_offset: number | null;
};

type DiagnosticKind =
  | 'missing'
  | 'ambiguous'
  | 'missing-heading'
  | 'isolated'
  | 'duplicate-names';

type DiagnosticItem = {
  path: string;
  title: string;
  detail?: string;
};

type ProjectStats = {
  documents: number;
  links: number;
  missing: number;
  missing_heading: number;
  ambiguous: number;
  isolated: number;
  duplicate_names: number;
};

const DIAGNOSTIC_LABELS: Record<DiagnosticKind, string> = {
  missing: 'リンク先なし',
  ambiguous: '同名候補あり',
  'missing-heading': '見出しなし',
  isolated: '孤立文書',
  'duplicate-names': '同名文書群',
};

function scrollElementByAnchor(rawFragment: string) {
  if (!rawFragment) return;
  let id = rawFragment;
  try {
    id = decodeURIComponent(rawFragment);
  } catch {
    // Keep raw fragment if percent-decoding fails
  }
  const element =
    window.document.getElementById(id) ||
    window.document.getElementById(headingId(id));
  element?.scrollIntoView();
}

function HighlightedSnippet({
  text,
  query,
}: {
  text: string;
  query: string;
}) {
  const parts = useMemo(() => highlightParts(text, query), [text, query]);
  return (
    <span className="snippet">
      {parts.map((part, idx) =>
        part.match ? <mark key={idx}>{part.text}</mark> : part.text,
      )}
    </span>
  );
}

function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [project, setProject] = useState('');
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<Hit[]>([]);
  const [nextHit, setNextHit] = useState<number | null>(null);
  const [nextBack, setNextBack] = useState<number | null>(null);
  const [searched, setSearched] = useState('');
  const [doc, setDoc] = useState<Doc | null>(null);
  const [backlinks, setBacklinks] = useState<string[]>([]);
  const [links, setLinks] = useState<Link[]>([]);
  const [nextLinkOffset, setNextLinkOffset] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [readBusy, setBusy] = useState(false);
  const [adminBusy, setAdminBusy] = useState(false);
  const busy = readBusy || adminBusy;
  const [capabilities, setCapabilities] = useState<{
    admin: boolean;
    folder_picker: boolean;
    desktop?: boolean;
  }>({ admin: false, folder_picker: false });
  const [stats, setStats] = useState<ProjectStats | null>(null);
  const [anchor, setAnchor] = useState('');
  const [view, setView] = useState<'wiki' | 'graph'>('wiki');
  const [revision, setRevision] = useState(0);
  const [history, setHistory] = useState<{ entries: string[]; index: number }>({
    entries: [],
    index: -1,
  });
  const [diagKind, setDiagKind] = useState<DiagnosticKind | null>(null);
  const [diagItems, setDiagItems] = useState<DiagnosticItem[]>([]);
  const [diagNextOffset, setDiagNextOffset] = useState<number | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    if (anchor && doc) {
      scrollElementByAnchor(anchor);
    }
  }, [doc, anchor]);

  useEffect(() => {
    api<Project[]>('projects')
      .then((p) => {
        setProjects(p);
        setProject(p[0]?.id || '');
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    api<typeof capabilities>('capabilities')
      .then(setCapabilities)
      .catch((e) => setError(String(e)));
  }, []);

  async function refreshProjects(preferred?: string) {
    generation.current++;
    const list = await api<Project[]>('projects');
    setProjects(list);
    setProject(
      preferred && list.some((p) => p.id === preferred)
        ? preferred
        : list[0]?.id || '',
    );
    setRevision((v) => v + 1);
  }

  useEffect(() => {
    generation.current++;
    let active = true;
    setDoc(null);
    setBacklinks([]);
    setLinks([]);
    setNextLinkOffset(null);
    setNextHit(null);
    setNextBack(null);
    setHits([]);
    setQ('');
    setSearched('');
    setError('');
    setStats(null);
    setView('wiki');
    setHistory({ entries: [], index: -1 });
    setDiagKind(null);
    setDiagItems([]);
    setDiagNextOffset(null);

    if (project) {
      setBusy(true);
      Promise.all([
        api<Page<Hit>>('search-page', project, { limit: '50' }),
        api<ProjectStats>('stats', project),
      ])
        .then(async ([h, s]) => {
          if (!active) return;
          setHits(h.items);
          setNextHit(h.next_offset);
          setSearched('');
          setStats(s);
          if (h.items[0]) {
            const path = h.items[0].path;
            const [d, b, page] = await Promise.all([
              api<Doc>('document', project, { path }),
              api<Page<string>>('backlinks-page', project, {
                path,
                limit: '100',
              }),
              api<LinkPage>('document-links', project, { path }),
            ]);
            if (active) {
              setDoc(d);
              setBacklinks(b.items);
              setNextBack(b.next_offset);
              setLinks(page.items);
              setNextLinkOffset(page.next_offset);
              setHistory({ entries: [path], index: 0 });
            }
          }
        })
        .catch((e) => {
          if (active) setError(String(e));
        })
        .finally(() => {
          if (active) setBusy(false);
        });
    }
    return () => {
      active = false;
    };
  }, [project, revision]);

  async function runSearch(queryText: string) {
    const current = ++generation.current;
    setError('');
    setBusy(true);
    try {
      const found = await api<Page<Hit>>('search-page', project, {
        q: queryText,
        limit: '50',
      });
      if (current === generation.current) {
        setHits(found.items);
        setNextHit(found.next_offset);
        setSearched(queryText);
      }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  function jumpToSameDocAnchor(fragment: string) {
    setAnchor(fragment);
    setView('wiki');
    scrollElementByAnchor(fragment);
  }

  async function open(path: string, fragment = '', recordHistory = true) {
    if (busy) return;
    if (doc && doc.path === path && view === 'wiki') {
      if (fragment) {
        jumpToSameDocAnchor(fragment);
      }
      return;
    }
    const current = ++generation.current;
    setError('');
    setBusy(true);
    try {
      const [d, b, page] = await Promise.all([
        api<Doc>('document', project, { path }),
        api<Page<string>>('backlinks-page', project, { path, limit: '100' }),
        api<LinkPage>('document-links', project, { path }),
      ]);
      if (current === generation.current) {
        setDoc(d);
        setBacklinks(b.items);
        setNextBack(b.next_offset);
        setLinks(page.items);
        setNextLinkOffset(page.next_offset);
        setAnchor(fragment);
        setView('wiki');
        if (recordHistory) {
          setHistory((prev) => {
            const base = prev.entries.slice(0, prev.index + 1);
            if (base[base.length - 1] === path) return prev;
            const nextEntries = [...base, path].slice(-50);
            return { entries: nextEntries, index: nextEntries.length - 1 };
          });
        }
      }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  async function stepHistory(delta: -1 | 1) {
    const nextIdx = history.index + delta;
    if (busy || nextIdx < 0 || nextIdx >= history.entries.length) return;
    const targetPath = history.entries[nextIdx];
    setHistory((prev) => ({ ...prev, index: nextIdx }));
    await open(targetPath, '', false);
  }

  async function toggleDiagnostics(kind: DiagnosticKind) {
    if (busy) return;
    if (diagKind === kind) {
      setDiagKind(null);
      setDiagItems([]);
      setDiagNextOffset(null);
      return;
    }
    const current = ++generation.current;
    setError('');
    setBusy(true);
    try {
      const page = await api<Page<DiagnosticItem>>(
        'diagnostics-page',
        project,
        { kind, limit: '50' },
      );
      if (current === generation.current) {
        setDiagKind(kind);
        setDiagItems(page.items);
        setDiagNextOffset(page.next_offset);
      }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  async function moreDiagnostics() {
    if (!diagKind || diagNextOffset === null || busy) return;
    const current = generation.current;
    setError('');
    setBusy(true);
    try {
      const page = await api<Page<DiagnosticItem>>(
        'diagnostics-page',
        project,
        {
          kind: diagKind,
          offset: String(diagNextOffset),
          limit: '50',
        },
      );
      if (current === generation.current) {
        setDiagItems((prev) => [...prev, ...page.items]);
        setDiagNextOffset(page.next_offset);
      }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  async function moreLinks() {
    if (!doc || nextLinkOffset === null || busy) return;
    const current = generation.current;
    setError('');
    setBusy(true);
    try {
      const page = await api<LinkPage>('document-links', project, {
        path: doc.path,
        offset: String(nextLinkOffset),
      });
      if (current === generation.current) {
        setLinks((l) => [...l, ...page.items]);
        setNextLinkOffset(page.next_offset);
      }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  async function more(kind: 'hits' | 'backlinks') {
    const offset = kind === 'hits' ? nextHit : nextBack;
    if (offset === null || busy) return;
    const current = generation.current;
    setBusy(true);
    setError('');
    try {
      if (kind === 'hits') {
        const page = await api<Page<Hit>>('search-page', project, {
          q: searched,
          offset: String(offset),
          limit: '50',
        });
        if (current === generation.current) {
          setHits((v) => [...v, ...page.items]);
          setNextHit(page.next_offset);
        }
      } else if (doc) {
        const page = await api<Page<string>>('backlinks-page', project, {
          path: doc.path,
          offset: String(offset),
          limit: '100',
        });
        if (current === generation.current) {
          setBacklinks((v) => [...v, ...page.items]);
          setNextBack(page.next_offset);
        }
      }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  async function navigate(target: string) {
    if (!doc || busy) return;
    if (target.startsWith('#')) {
      jumpToSameDocAnchor(target.slice(1));
      return;
    }
    const current = generation.current;
    try {
      const link = await api<Link | null>('link-target', project, {
        path: doc.path,
        target,
      });
      if (current !== generation.current) return;
      if (link?.resolved) {
        const fragment = link.target.split('#').slice(1).join('#');
        await open(link.resolved, fragment);
      } else {
        setError(
          link?.status === 'missing-heading'
            ? '参照先の見出しがありません。'
            : 'リンク先が見つからないか、同名候補が複数あります。',
        );
      }
    } catch (e) {
      if (current === generation.current) setError(String(e));
    }
  }

  const outgoing = links.filter((l) => l.source === doc?.path);
  const headings = useMemo(
    () => (doc ? extractHeadings(doc.markdown) : []),
    [doc],
  );
  const canGoBack = history.index > 0;
  const canGoForward =
    history.index >= 0 && history.index < history.entries.length - 1;

  function renderDiagnosticStat(
    kind: DiagnosticKind,
    count: number,
    tooltip?: string,
  ) {
    const label = DIAGNOSTIC_LABELS[kind];
    if (count === 0) {
      return (
        <span title={tooltip}>
          {label} <strong>0</strong>
        </span>
      );
    }
    return (
      <button
        type="button"
        className={`stat-filter ${diagKind === kind ? 'active' : ''}`}
        aria-pressed={diagKind === kind}
        disabled={busy}
        title={tooltip || `${label}の一覧を表示`}
        onClick={() => void toggleDiagnostics(kind)}
      >
        {label} <strong>{count.toLocaleString()}</strong>
      </button>
    );
  }

  return (
    <div className="app">
      <header>
        <div className="brand">
          MK<span>H</span>
        </div>
        <div>
          <h1>Markdown Knowledge Hub</h1>
          <p>
            プロジェクトの知識を、つながりから読む
            {capabilities.desktop ? ' · Desktop' : ''}
          </p>
        </div>
        <span className="badge">原本Markdownは読み取り専用</span>
      </header>

      <section className="toolbar" aria-label="検索条件">
        <label>
          プロジェクト
          <select
            disabled={busy}
            value={project}
            onChange={(e) => setProject(e.target.value)}
          >
            <option value="" disabled>
              プロジェクトを選択
            </option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void runSearch(q);
          }}
        >
          <label htmlFor="query">全文検索</label>
          <div className="search">
            <input
              id="query"
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="キーワードを入力（日本語・中文・English）"
            />
            {(q || searched) && (
              <button
                type="button"
                className="secondary-btn"
                disabled={!project || busy}
                onClick={() => {
                  setQ('');
                  if (searched) void runSearch('');
                }}
              >
                クリア
              </button>
            )}
            <button type="submit" disabled={!project || busy}>
              検索
            </button>
          </div>
        </form>
      </section>

      <ProjectManager
        current={projects.find((p) => p.id === project)}
        enabled={capabilities.admin}
        folderPicker={capabilities.folder_picker}
        onChanged={refreshProjects}
        onBusy={setAdminBusy}
      />

      {capabilities.desktop && project && (
        <WatchManager
          key={project}
          project={project}
          onRefresh={() => setRevision((v) => v + 1)}
        />
      )}

      {stats && (
        <div className="project-stats">
          <span>
            文書 <strong>{stats.documents.toLocaleString()}</strong>
          </span>
          <span>
            リンク <strong>{stats.links.toLocaleString()}</strong>
          </span>
          {renderDiagnosticStat('missing', stats.missing)}
          {renderDiagnosticStat('ambiguous', stats.ambiguous)}
          {renderDiagnosticStat('missing-heading', stats.missing_heading)}
          {renderDiagnosticStat(
            'isolated',
            stats.isolated,
            '他の文書との解決済みリンクがない文書の一覧を表示',
          )}
          {renderDiagnosticStat('duplicate-names', stats.duplicate_names)}
        </div>
      )}

      {diagKind && (
        <section
          className="diagnostics-panel"
          aria-label={`${DIAGNOSTIC_LABELS[diagKind]}の診断一覧`}
        >
          <div className="diagnostics-header">
            <h2>
              診断：{DIAGNOSTIC_LABELS[diagKind]}{' '}
              <small>{diagItems.length}件表示</small>
            </h2>
            <button
              type="button"
              onClick={() => {
                setDiagKind(null);
                setDiagItems([]);
                setDiagNextOffset(null);
              }}
            >
              閉じる
            </button>
          </div>
          <div className="diagnostics-list">
            {diagItems.map((item, idx) => (
              <button
                key={`${item.path}-${item.detail || ''}-${idx}`}
                type="button"
                className="hit"
                disabled={busy}
                onClick={() => void open(item.path)}
              >
                <strong>{item.title}</strong>
                <span>
                  {item.path}
                  {item.detail ? ` — 対象: ${item.detail}` : ''}
                </span>
              </button>
            ))}
          </div>
          {diagNextOffset !== null && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void moreDiagnostics()}
            >
              診断項目をさらに読み込む
            </button>
          )}
        </section>
      )}

      {project && (
        <div className="view-tabs" role="tablist" aria-label="表示切替">
          <button
            role="tab"
            aria-selected={view === 'wiki'}
            onClick={() => setView('wiki')}
          >
            Wiki
          </button>
          <button
            role="tab"
            aria-selected={view === 'graph'}
            onClick={() => setView('graph')}
          >
            リンク図
          </button>
          {doc && view === 'graph' && (
            <button onClick={() => setDoc(null)}>プロジェクト全体を見る</button>
          )}
        </div>
      )}

      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}

      {!projects.length && !error && (
        <p className="empty">
          「新規プロジェクト」からMarkdownフォルダを登録してください。
        </p>
      )}

      <div className="workspace" aria-busy={busy}>
        <nav aria-label="文書一覧">
          <h2>
            {searched ? '検索結果' : '文書'} <small>{hits.length}件</small>
          </h2>
          {hits.map((h) => (
            <button
              disabled={busy}
              className={`hit ${doc?.path === h.path ? 'selected' : ''}`}
              key={h.path}
              onClick={() => void open(h.path)}
            >
              <strong>{h.title}</strong>
              <span>{h.path}</span>
              {h.snippet && (
                <HighlightedSnippet text={h.snippet} query={searched} />
              )}
            </button>
          ))}
          {nextHit !== null && (
            <button
              className="more-btn"
              disabled={busy}
              onClick={() => void more('hits')}
            >
              文書をさらに読み込む
            </button>
          )}
          {project && !hits.length && <p>該当する文書がありません。</p>}
        </nav>

        <main>
          {view === 'graph' && project ? (
            <Suspense fallback={<p>リンク図を準備しています…</p>}>
              <GraphView
                key={`${project}-${revision}`}
                project={project}
                center={doc?.path}
                onOpen={(path) => void open(path)}
              />
            </Suspense>
          ) : doc ? (
            <>
              <div className="doc-header">
                <div className="docpath">{doc.path}</div>
                {history.entries.length > 1 && (
                  <div className="history-nav" aria-label="閲覧履歴">
                    <button
                      type="button"
                      disabled={busy || !canGoBack}
                      onClick={() => void stepHistory(-1)}
                      title="前の文書へ戻る"
                    >
                      ← 戻る
                    </button>
                    <button
                      type="button"
                      disabled={busy || !canGoForward}
                      onClick={() => void stepHistory(1)}
                      title="次の文書へ進む"
                    >
                      進む →
                    </button>
                  </div>
                )}
              </div>
              <Markdown
                skipHtml
                remarkPlugins={[remarkGfm, remarkWiki]}
                rehypePlugins={[rehypeHeadingIds]}
                components={{
                  a: ({ href, children }) => {
                    if (href?.startsWith('#')) {
                      return (
                        <button
                          type="button"
                          className="inline"
                          onClick={() => jumpToSameDocAnchor(href.slice(1))}
                        >
                          {children}
                        </button>
                      );
                    }
                    const link = outgoing.find((l) =>
                      sameTarget(href, l.target),
                    );
                    if (link?.resolved) {
                      const fragment = link.target
                        .split('#')
                        .slice(1)
                        .join('#');
                      return (
                        <button
                          type="button"
                          className="inline"
                          onClick={() => void open(link.resolved!, fragment)}
                        >
                          {children}
                        </button>
                      );
                    }
                    if (
                      href?.startsWith('https://') ||
                      href?.startsWith('http://')
                    ) {
                      return (
                        <a href={href} target="_blank" rel="noreferrer">
                          {children}
                        </a>
                      );
                    }
                    if (href && !link) {
                      return (
                        <button
                          type="button"
                          className="inline"
                          onClick={() => void navigate(href)}
                        >
                          {children}
                        </button>
                      );
                    }
                    return (
                      <span title="リンク先または見出しがありません">
                        {children}
                      </span>
                    );
                  },
                  img: ({ alt }) => (
                    <span className="image-placeholder">
                      [画像: {alt || '画像'}]
                    </span>
                  ),
                }}
              >
                {displayMarkdown(doc.markdown)}
              </Markdown>
            </>
          ) : (
            <div className="empty">
              <span className="symbol">↔</span>
              <h2>知識のつながりを見つける</h2>
              <p>文書を選ぶと、本文と参照元を表示します。</p>
            </div>
          )}
        </main>

        <aside>
          {headings.length >= 2 && (
            <details open className="toc">
              <summary>
                目次 <small>{headings.length}見出し</small>
              </summary>
              <div className="toc-list">
                {headings.map((h) => (
                  <button
                    key={h.id}
                    type="button"
                    className={`toc-item level-${Math.min(h.level, 4)}`}
                    onClick={() => jumpToSameDocAnchor(h.id)}
                  >
                    {h.text}
                  </button>
                ))}
              </div>
            </details>
          )}
          <h2>Backlinks</h2>
          <p className="hint">この文書への参照</p>
          {backlinks.map((p) => (
            <button
              className="hit"
              key={p}
              onClick={() => void open(p)}
            >
              {p}
            </button>
          ))}
          {nextBack !== null && (
            <button
              className="more-btn"
              disabled={busy}
              onClick={() => void more('backlinks')}
            >
              参照元をさらに読み込む
            </button>
          )}
          {!backlinks.length && <p>参照元はありません。</p>}

          <h2>出リンク</h2>
          {outgoing.map((l, i) => (
            <div className="link" key={i}>
              {l.resolved ? (
                <button
                  type="button"
                  className="inline"
                  onClick={() =>
                    void open(
                      l.resolved!,
                      l.target.split('#').slice(1).join('#'),
                    )
                  }
                >
                  {l.target}
                </button>
              ) : (
                <span>
                  {l.target}
                  <small>
                    {' '}
                    {l.status === 'ambiguous'
                      ? '同名候補あり'
                      : l.status === 'missing-heading'
                        ? '見出しなし'
                        : 'リンク先なし'}
                  </small>
                </span>
              )}
            </div>
          ))}
          {nextLinkOffset !== null && (
            <button
              className="more-btn"
              disabled={busy}
              onClick={() => void moreLinks()}
            >
              出リンクをさらに読み込む
            </button>
          )}
          <p className="hint">
            出リンクは文書ごとに100件ずつ取得します。リンク図タブで文書間の関係を確認できます。
          </p>
        </aside>
      </div>

      <footer>
        入力Markdownは読み取り専用です。変更後は「再作成」または監視で反映できます。生成データの保存先は「プロジェクトの保存先・設定」で確認・変更できます。
      </footer>
    </div>
  );
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
