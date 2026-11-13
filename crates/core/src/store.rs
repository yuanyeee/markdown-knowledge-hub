use crate::{parser, search::Search};
use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

#[derive(Debug, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: String,
    pub output: String,
    pub legacy_output: bool,
}
#[derive(Debug, Serialize, Default)]
pub struct ScanReport {
    pub changed: usize,
    pub deleted: usize,
    pub unchanged: usize,
    pub recovered: bool,
}
#[derive(Debug, Serialize)]
pub struct Document {
    pub path: String,
    pub title: String,
    pub markdown: String,
}
#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub resolved: Option<String>,
    pub status: String,
}
#[derive(Debug, Serialize)]
pub struct LinkPage {
    pub items: Vec<Link>,
    pub next_offset: Option<usize>,
}
pub struct Hub {
    pub(crate) data: PathBuf,
    pub(crate) registry: Connection,
    pub(crate) working: Option<crate::generations::WorkingGeneration>,
}
struct Candidate {
    path: String,
    full: PathBuf,
    size: u64,
    mtime: String,
    previous: Option<String>,
}
struct Change {
    path: String,
    size: u64,
    mtime: String,
    hash: String,
    raw: String,
    parsed: Option<parser::Parsed>,
}

impl Hub {
    pub fn open(data: &Path) -> Result<Self> {
        std::fs::create_dir_all(data)?;
        let data = data.canonicalize()?;
        let registry = Connection::open(data.join("registry.sqlite"))?;
        registry.busy_timeout(Duration::from_secs(10))?;
        registry.execute_batch("CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY, name TEXT NOT NULL, root TEXT NOT NULL);")?;
        crate::outputs::initialize_registry(&registry)?;
        Ok(Self {
            data,
            registry,
            working: None,
        })
    }
    pub fn register(&mut self, id: &str, name: &str, root: &Path) -> Result<()> {
        Self::validate_id(id)?;
        let _writer = self.writer_lock(id)?;
        let _lock = self.project_lock(id, false)?;
        if name.trim().is_empty() || name.len() > 300 {
            bail!("プロジェクト名は1～300バイトで指定してください");
        }
        let root = root.canonicalize().context("MDルートが存在しません")?;
        if !root.is_dir() || root.starts_with(&self.data) || self.data.starts_with(&root) {
            bail!("原本ルートと索引ディレクトリは独立させてください");
        }
        self.validate_input(&root)?;
        self.registry.execute(
            "INSERT INTO projects VALUES(?1,?2,?3)",
            params![
                id,
                name,
                root.to_str().context("ルートはUnicodeパスが必要です")?
            ],
        )?;
        let initialized = self.db(id).and_then(|_| self.search_index(id).map(|_| ()));
        if let Err(error) = initialized {
            if let Err(cleanup) = self.remove_unlocked(id) {
                eprintln!("登録失敗後の索引削除に失敗: {cleanup:#}");
                self.registry
                    .execute("DELETE FROM projects WHERE id=?1", [id])?;
            }
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn validate_id(id: &str) -> Result<()> {
        if id.is_empty()
            || id.len() > 80
            || !id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
            || matches!(id, "con" | "prn" | "aux" | "nul")
            || (id.len() == 4
                && (id.starts_with("com") || id.starts_with("lpt"))
                && matches!(id.as_bytes()[3], b'1'..=b'9'))
        {
            bail!("プロジェクトIDは小文字英数字・-・_のみ。Windows予約名は指定できません");
        }
        Ok(())
    }
    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut statement=self.registry.prepare("SELECT p.id,p.name,p.root,o.path FROM projects p LEFT JOIN project_outputs o ON p.id=o.project ORDER BY p.name")?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, name, root, output)| {
                let legacy_output = output.is_none();
                let root = self.manifest_input(&id)?.unwrap_or(root);
                Ok(Project {
                    output: output.unwrap_or_else(|| {
                        self.data
                            .join("indexes")
                            .join(&id)
                            .to_string_lossy()
                            .into_owned()
                    }),
                    id,
                    name,
                    root,
                    legacy_output,
                })
            })
            .collect()
    }
    pub fn root(&self, id: &str) -> Result<PathBuf> {
        Self::validate_id(id)?;
        if let Some(input) = self.manifest_input(id)? {
            return Ok(PathBuf::from(input));
        }
        Ok(PathBuf::from(self.registry.query_row(
            "SELECT root FROM projects WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )?))
    }
    pub(crate) fn dir(&self, id: &str) -> Result<PathBuf> {
        self.root(id)?;
        if let Some((path, token)) = self.managed_output(id)? {
            self.validate_owned(id, &path, &token)?;
            return Ok(path);
        }
        let parent = self.data.join("indexes");
        let dir = parent.join(id);
        if (parent.exists() && parent.canonicalize()? != parent)
            || (dir.exists() && dir.canonicalize()? != dir)
        {
            bail!("索引保存先のシンボリックリンク・別名は使用できません");
        }
        Ok(dir)
    }
    pub(crate) fn db(&self, id: &str) -> Result<Connection> {
        let dir = self.dir(id)?;
        let db_path = dir.join("metadata.sqlite");
        let db = if db_path.is_file() {
            let conn = Connection::open(&db_path)?;
            conn.busy_timeout(Duration::from_secs(10))?;
            conn
        } else {
            self.write_db(id)?
        };
        db.execute_batch("BEGIN DEFERRED;")?;
        // Pin the metadata snapshot before resolving its paired search generation.
        db.query_row("SELECT COUNT(*) FROM state", [], |r| r.get::<_, i64>(0))?;
        Ok(db)
    }
    pub(crate) fn write_db(&self, id: &str) -> Result<Connection> {
        let dir = self.dir(id)?;
        std::fs::create_dir_all(&dir)?;
        let db = Connection::open(dir.join("metadata.sqlite"))?;
        db.busy_timeout(Duration::from_secs(10))?;
        db.execute_batch("PRAGMA journal_mode=WAL;
          CREATE TABLE IF NOT EXISTS docs(path TEXT PRIMARY KEY,size INTEGER,mtime TEXT,hash TEXT,title TEXT,body TEXT,raw TEXT);
          CREATE TABLE IF NOT EXISTS doc_stems(path TEXT PRIMARY KEY,stem TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS doc_stems_stem ON doc_stems(stem);
          CREATE TABLE IF NOT EXISTS link_keys(source TEXT, name TEXT);
          CREATE INDEX IF NOT EXISTS link_keys_name ON link_keys(name);
          CREATE INDEX IF NOT EXISTS link_keys_source ON link_keys(source);
          CREATE TABLE IF NOT EXISTS aliases(path TEXT, name TEXT);
          CREATE INDEX IF NOT EXISTS aliases_name ON aliases(name);
          CREATE TABLE IF NOT EXISTS headings(path TEXT, name TEXT);
          CREATE INDEX IF NOT EXISTS headings_path ON headings(path);
          CREATE INDEX IF NOT EXISTS headings_path_name ON headings(path, name);
          CREATE TABLE IF NOT EXISTS links(source TEXT,target TEXT,resolved TEXT,status TEXT);
          CREATE INDEX IF NOT EXISTS links_resolved ON links(resolved);
          CREATE INDEX IF NOT EXISTS links_source ON links(source);
          CREATE INDEX IF NOT EXISTS links_status ON links(status);
          CREATE TABLE IF NOT EXISTS state(key TEXT PRIMARY KEY,value INTEGER);
          CREATE TABLE IF NOT EXISTS state_text(key TEXT PRIMARY KEY,value TEXT);
          INSERT OR IGNORE INTO state VALUES('dirty',0);
          INSERT OR IGNORE INTO state VALUES('parser_version',0);")?;
        Ok(db)
    }
    fn search_index(&self, id: &str) -> Result<Search> {
        Search::open(&self.active_dir(id)?.join("search"))
    }
    pub fn scan(&mut self, id: &str) -> Result<ScanReport> {
        self.scan_verified(id, false)
    }
    pub fn scan_verified(&mut self, id: &str, verify: bool) -> Result<ScanReport> {
        self.scan_scope(id, verify, None, false)
    }
    /// Reconcile exact relative Markdown file paths supplied by a file event.
    /// Unlike metadata-only scans, event updates always verify the content hash.
    pub fn scan_paths(&mut self, id: &str, paths: &[String]) -> Result<ScanReport> {
        let mut paths = paths
            .iter()
            .map(|p| p.replace('\\', "/"))
            .collect::<Vec<_>>();
        for path in &paths {
            validate_relative_md(path)?;
        }
        paths.sort();
        paths.dedup();
        if paths.len() > 500 {
            return self.scan_verified(id, true);
        }
        if let Ok(root) = self.root(id)
            && let Ok(db) = self.db(id)
        {
            for path in &paths {
                if !root.join(path).exists() {
                    let prefix = format!("{path}/%");
                    let nested: bool = db
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM docs WHERE path LIKE ?1)",
                            [prefix],
                            |r| r.get(0),
                        )
                        .unwrap_or(false);
                    if nested {
                        return self.scan_verified(id, false);
                    }
                }
            }
        }
        self.scan_scope(id, true, Some(&paths), false)
    }
    pub fn rebuild(&mut self, id: &str) -> Result<ScanReport> {
        self.scan_scope(id, true, None, true)
    }
    fn scan_scope(
        &mut self,
        id: &str,
        verify: bool,
        paths: Option<&[String]>,
        rebuild: bool,
    ) -> Result<ScanReport> {
        self.publish_scan(id, verify, paths, rebuild, || Ok(()))
    }
    pub(crate) fn scan_staged(
        &mut self,
        id: &str,
        verify: bool,
        paths: Option<&[String]>,
        rebuild: bool,
        before_publish: impl FnOnce() -> Result<()>,
    ) -> Result<ScanReport> {
        let root = self.root(id)?;
        if !root.is_dir() {
            bail!("原本ルートにアクセスできません");
        }
        let mut db = self.write_db(id)?;
        let index = self.search_index(id)?;
        let dirty: bool = db.query_row("SELECT value FROM state WHERE key='dirty'", [], |r| {
            r.get(0)
        })?;
        let mut report = ScanReport {
            recovered: dirty,
            ..Default::default()
        };
        if dirty && !rebuild {
            let mut s = db.prepare("SELECT path,title,body FROM docs")?;
            let all = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            index.update(true, &[], all.map(|r| Ok(r?)))?;
        }
        let version: i64 = db.query_row(
            "SELECT value FROM state WHERE key='parser_version'",
            [],
            |r| r.get(0),
        )?;
        let reparse = rebuild || version < 5;
        let verify = verify || reparse;
        let mut old = HashMap::new();
        {
            let sql = match paths {
                Some(paths) => format!(
                    "SELECT path,size,mtime,hash FROM docs WHERE path IN ({})",
                    vec!["?"; paths.len()].join(",")
                ),
                None => "SELECT path,size,mtime,hash FROM docs".to_string(),
            };
            let mut s = db.prepare(&sql)?;
            for row in s.query_map(rusqlite::params_from_iter(paths.unwrap_or_default()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (
                        r.get::<_, i64>(1)? as u64,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ),
                ))
            })? {
                let (p, v) = row?;
                old.insert(p, v);
            }
        }
        let mut seen = HashSet::new();
        let mut candidates = vec![];
        let mut files = vec![];
        if let Some(paths) = paths {
            for path in paths {
                if let Some(file) = event_file(&root, path)? {
                    files.push(file);
                }
            }
        } else {
            for entry in walkdir::WalkDir::new(&root).follow_links(false) {
                let entry = entry?;
                if entry.file_type().is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                {
                    files.push((entry.path().to_path_buf(), entry.metadata()?));
                }
            }
        }
        for (full, m) in files {
            let path = full
                .strip_prefix(&root)?
                .to_str()
                .context("Unicodeパスが必要です")?
                .replace('\\', "/");
            let mtime = m
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_nanos()
                .to_string();
            seen.insert(path.clone());
            if !verify
                && old
                    .get(&path)
                    .is_some_and(|(size, time, _)| *size == m.len() && time == &mtime)
            {
                report.unchanged += 1;
                continue;
            }
            let previous = if reparse {
                None
            } else {
                old.get(&path).map(|v| v.2.clone())
            };
            candidates.push(Candidate {
                path,
                full,
                size: m.len(),
                mtime,
                previous,
            });
        }
        let deleted: Vec<String> = old.keys().filter(|p| !seen.contains(*p)).cloned().collect();
        report.deleted = deleted.len();
        if candidates.is_empty() && deleted.is_empty() {
            let tx = db.transaction()?;
            tx.execute("UPDATE state SET value=0 WHERE key='dirty'", [])?;
            tx.execute("UPDATE state SET value=5 WHERE key='parser_version'", [])?;
            self.commit_generation(id, &tx, before_publish)?;
            tx.commit()?;
            return Ok(report);
        }
        let tx = db.transaction()?;
        tx.execute("UPDATE state SET value=1 WHERE key='dirty'", [])?;
        let mut affected_names = HashSet::new();
        for path in &deleted {
            affected_names.insert(link_key(path));
            let mut statement = tx.prepare("SELECT name FROM aliases WHERE path=?1")?;
            for alias in statement.query_map([path], |r| r.get::<_, String>(0))? {
                affected_names.insert(alias?);
            }
            tx.execute("DELETE FROM link_keys WHERE source=?1", [path])?;
            tx.execute("DELETE FROM docs WHERE path=?1", [path])?;
            tx.execute("DELETE FROM doc_stems WHERE path=?1", [path])?;
            tx.execute("DELETE FROM aliases WHERE path=?1", [path])?;
            tx.execute("DELETE FROM headings WHERE path=?1", [path])?;
            tx.execute("DELETE FROM links WHERE source=?1", [path])?;
        }
        let mut documents = vec![];
        // Bound raw/parsed payloads to 16 documents, including full initial scans.
        for batch in candidates.chunks(16) {
            let changes: Vec<Change> = batch
                .par_iter()
                .map(|c| -> Result<_> {
                    if c.size > 16 * 1024 * 1024 {
                        bail!("16MiBを超えるMDは未対応: {}", c.path);
                    }
                    let raw = std::fs::read_to_string(&c.full)
                        .with_context(|| format!("読み取り失敗: {}", c.path))?;
                    let hash = blake3::hash(raw.as_bytes()).to_hex().to_string();
                    let parsed = if c.previous.as_ref() == Some(&hash) {
                        None
                    } else {
                        Some(parser::parse(&raw, &c.path)?)
                    };
                    Ok(Change {
                        path: c.path.clone(),
                        size: c.size,
                        mtime: c.mtime.clone(),
                        hash,
                        raw,
                        parsed,
                    })
                })
                .collect::<Result<_>>()?;
            for c in &changes {
                if let Some(p) = &c.parsed {
                    for (table, values) in [("aliases", &p.aliases), ("headings", &p.headings)] {
                        let mut statement = tx.prepare(&format!(
                            "SELECT name FROM {table} WHERE path=?1 ORDER BY name"
                        ))?;
                        let previous = statement
                            .query_map([&c.path], |r| r.get::<_, String>(0))?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        let mut new = values.clone();
                        new.sort();
                        new.dedup();
                        if previous != new {
                            affected_names.insert(link_key(&c.path));
                            affected_names.extend(p.aliases.iter().cloned());
                            if table == "aliases" {
                                affected_names.extend(previous.iter().cloned());
                                affected_names.extend(new.iter().cloned());
                            }
                        }
                        tx.execute(&format!("DELETE FROM {table} WHERE path=?1"), [&c.path])?;
                        for value in new {
                            tx.execute(
                                &format!("INSERT INTO {table} VALUES(?1,?2)"),
                                params![c.path, value],
                            )?;
                        }
                    }

                    tx.execute(
                        "INSERT OR REPLACE INTO docs VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        params![
                            c.path,
                            c.size as i64,
                            c.mtime,
                            c.hash,
                            p.title,
                            p.text,
                            c.raw
                        ],
                    )?;
                    if let Some(stem) = Path::new(&c.path).file_stem().and_then(|s| s.to_str()) {
                        tx.execute(
                            "INSERT OR REPLACE INTO doc_stems VALUES(?1,?2)",
                            params![c.path, stem],
                        )?;
                    }
                    if !old.contains_key(&c.path) {
                        affected_names.insert(link_key(&c.path));
                    }
                    tx.execute("DELETE FROM link_keys WHERE source=?1", [&c.path])?;
                    tx.execute("DELETE FROM links WHERE source=?1", [&c.path])?;
                    for target in &p.links {
                        tx.execute(
                            "INSERT INTO link_keys VALUES(?1,?2)",
                            params![c.path, link_key(target)],
                        )?;
                        tx.execute(
                            "INSERT INTO links VALUES(?1,?2,NULL,'missing')",
                            params![c.path, target],
                        )?;
                    }
                    documents.push(c.path.clone());
                    report.changed += 1;
                } else {
                    report.unchanged += 1;
                    tx.execute(
                        "UPDATE docs SET size=?2,mtime=?3 WHERE path=?1",
                        params![c.path, c.size as i64, c.mtime],
                    )?;
                }
            }
        }
        let content_changed = report.changed > 0 || !deleted.is_empty();
        if content_changed {
            ensure_doc_stems_synced(&tx)?;
            let mut sources = documents.iter().cloned().collect::<HashSet<_>>();
            let keys = affected_names.into_iter().collect::<Vec<_>>();
            for batch in keys.chunks(100) {
                let mut statement = tx.prepare(&format!(
                    "SELECT DISTINCT source FROM link_keys WHERE name IN ({})",
                    vec!["?"; batch.len()].join(",")
                ))?;
                for path in statement
                    .query_map(rusqlite::params_from_iter(batch), |r| r.get::<_, String>(0))?
                {
                    sources.insert(path?);
                }
            }
            resolve(&tx, Some(&sources.into_iter().collect::<Vec<_>>()))?;
        }
        tx.execute("UPDATE state SET value=5 WHERE key='parser_version'", [])?;
        if content_changed {
            let mut statement = tx.prepare("SELECT path,title,body FROM docs WHERE path=?1")?;
            index.update(
                false,
                &deleted,
                documents.iter().map(|path| {
                    Ok(statement.query_row([path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?)
                }),
            )?;
        }
        tx.execute("UPDATE state SET value=0 WHERE key='dirty'", [])?;
        self.commit_generation(id, &tx, before_publish)?;
        tx.commit()?;
        Ok(report)
    }
    pub fn search(&self, id: &str, q: &str) -> Result<Vec<SearchHit>> {
        let _lock = self.project_lock(id, true)?;
        if q.len() > 1024 {
            bail!("検索語が長すぎます");
        }
        let db = self.db(id)?;
        if q.trim().is_empty() {
            let mut s = db.prepare("SELECT path,title FROM docs ORDER BY path LIMIT 50")?;
            return Ok(s
                .query_map([], |r| {
                    Ok(SearchHit {
                        path: r.get(0)?,
                        title: r.get(1)?,
                        snippet: None,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?);
        }
        let dir = self.index_dir_in_snapshot(id, &db)?;
        let mut doc_stmt =
            db.prepare("SELECT path,title,substr(body,1,16384) FROM docs WHERE path=?1")?;
        Search::cached(&dir.join("search"))?
            .query(q)?
            .into_iter()
            .map(|p| {
                let (path, title, body): (String, String, String) =
                    doc_stmt.query_row([p], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                Ok(SearchHit {
                    snippet: extract_snippet(&body, q),
                    path,
                    title,
                })
            })
            .collect()
    }
    pub fn document(&self, id: &str, path: &str) -> Result<Document> {
        let _lock = self.project_lock(id, true)?;
        if Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            bail!("不正な文書パス");
        }
        Ok(self.db(id)?.query_row(
            "SELECT path,title,raw FROM docs WHERE path=?1",
            [path],
            |r| {
                Ok(Document {
                    path: r.get(0)?,
                    title: r.get(1)?,
                    markdown: r.get(2)?,
                })
            },
        )?)
    }
    pub fn backlinks(&self, id: &str, path: &str) -> Result<Vec<String>> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut s = db.prepare(
            "SELECT DISTINCT source FROM links WHERE resolved=?1 ORDER BY source LIMIT 500",
        )?;
        Ok(s.query_map([path], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn links(&self, id: &str) -> Result<Vec<Link>> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut s = db.prepare(
            "SELECT source,target,resolved,status FROM links ORDER BY source,target LIMIT 5000",
        )?;
        Ok(s.query_map([], |r| {
            Ok(Link {
                source: r.get(0)?,
                target: r.get(1)?,
                resolved: r.get(2)?,
                status: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn links_page(
        &self,
        id: &str,
        source: &str,
        offset: usize,
        limit: usize,
    ) -> Result<LinkPage> {
        let _lock = self.project_lock(id, true)?;
        if !(1..=200).contains(&limit) || offset > i64::MAX as usize - limit {
            bail!("リンクのページ指定が不正です");
        }
        validate_relative_md(source)?;
        let db = self.db(id)?;
        let mut s = db.prepare("SELECT source,target,resolved,status FROM links WHERE source=?1 ORDER BY target,rowid LIMIT ?2 OFFSET ?3")?;
        let mut items = s
            .query_map(params![source, (limit + 1) as i64, offset as i64], |r| {
                Ok(Link {
                    source: r.get(0)?,
                    target: r.get(1)?,
                    resolved: r.get(2)?,
                    status: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_offset = (items.len() > limit).then_some(offset + limit);
        items.truncate(limit);
        Ok(LinkPage { items, next_offset })
    }
    pub fn link_target(&self, id: &str, source: &str, target: &str) -> Result<Option<Link>> {
        if target.len() > 4096 {
            bail!("リンクが長すぎます");
        }
        validate_relative_md(source)?;
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut statement =
            db.prepare("SELECT source,target,resolved,status FROM links WHERE source=?1")?;
        let decoded = percent_encoding::percent_decode_str(target).decode_utf8_lossy();
        for row in statement.query_map([source], |r| {
            Ok(Link {
                source: r.get(0)?,
                target: r.get(1)?,
                resolved: r.get(2)?,
                status: r.get(3)?,
            })
        })? {
            let link = row?;
            if percent_encoding::percent_decode_str(&link.target).decode_utf8_lossy() == decoded {
                return Ok(Some(link));
            }
        }
        Ok(None)
    }
    pub fn watch(&mut self, id: &str) -> Result<()> {
        crate::watch::run(self, id)
    }
}

/// Apply the same filesystem guard before either targeted updates or migration fallback.
pub(crate) fn event_file(root: &Path, path: &str) -> Result<Option<(PathBuf, std::fs::Metadata)>> {
    let mut full = root.to_path_buf();
    for component in Path::new(path).components() {
        full.push(component);
        match std::fs::symlink_metadata(&full) {
            Ok(m) if m.file_type().is_symlink() => {
                bail!("シンボリックリンクは索引対象にできません: {path}")
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    }
    let metadata = std::fs::metadata(&full)?;
    if !metadata.is_file() {
        bail!("ファイル以外の変更は全体scanを使用してください: {path}");
    }
    if full.canonicalize()? != full {
        bail!("実ファイルと同じ表記の相対パスを指定してください: {path}");
    }
    Ok(Some((full, metadata)))
}

fn normalize(path: &str) -> Option<String> {
    let mut parts = vec![];
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}
fn validate_relative_md(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains([':', '\0'])
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || !Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
    {
        bail!("ルート内のMarkdown相対パスを指定してください");
    }
    Ok(())
}
fn ensure_doc_stems_synced(db: &Connection) -> Result<()> {
    let docs_count: i64 = db.query_row("SELECT COUNT(*) FROM docs", [], |r| r.get(0))?;
    let stems_count: i64 = db.query_row("SELECT COUNT(*) FROM doc_stems", [], |r| r.get(0))?;
    if docs_count != stems_count {
        db.execute("DELETE FROM doc_stems", [])?;
        let all_paths = {
            let mut s = db.prepare("SELECT path FROM docs")?;
            s.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut ins = db.prepare("INSERT INTO doc_stems VALUES(?1,?2)")?;
        for path in all_paths {
            if let Some(stem) = Path::new(&path).file_stem().and_then(|s| s.to_str()) {
                ins.execute(params![path, stem])?;
            }
        }
    }
    Ok(())
}

type PendingLinkRow = (i64, String, String, Option<String>, String);

fn resolve(db: &Connection, sources: Option<&[String]>) -> Result<()> {
    if let Some(sources) = sources {
        if sources.is_empty() {
            return Ok(());
        }
        db.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolving_sources(path TEXT PRIMARY KEY); DELETE FROM resolving_sources;")?;
        let mut insert = db.prepare_cached("INSERT OR IGNORE INTO resolving_sources VALUES(?1)")?;
        for source in sources {
            insert.execute([source])?;
        }
    }
    let sql = if sources.is_some() {
        "SELECT rowid,source,target,resolved,status FROM links WHERE source IN (SELECT path FROM resolving_sources)"
    } else {
        "SELECT rowid,source,target,resolved,status FROM links"
    };
    let links: Vec<PendingLinkRow> = {
        let mut s = db.prepare(sql)?;
        s.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if links.is_empty() {
        return Ok(());
    }
    if sources.is_some() && links.len() <= 2_000 {
        return resolve_scoped(db, links);
    }
    resolve_bulk(db, links)
}

fn parse_link_target(
    source: &str,
    raw_target: &str,
) -> (bool, String, Option<String>, Option<String>, Option<String>) {
    let (base, anchor) = raw_target
        .split_once('#')
        .map(|(a, b)| (a, Some(b)))
        .unwrap_or((raw_target, None));
    let self_link = base.is_empty();
    let anchor = anchor.map(|a| {
        percent_encoding::percent_decode_str(a)
            .decode_utf8_lossy()
            .into_owned()
    });
    let target = percent_encoding::percent_decode_str(base)
        .decode_utf8_lossy()
        .replace('\\', "/");
    let mut target = target.trim().to_string();
    if !target.to_lowercase().ends_with(".md") {
        target.push_str(".md");
    }
    let parent = source.rsplit_once('/').map(|v| v.0).unwrap_or("");
    let relative = normalize(&format!("{parent}/{target}"));
    let absolute = normalize(&target);
    (self_link, target, anchor, relative, absolute)
}

/// Incremental link resolution using indexed point lookups instead of loading all docs/headings.
fn resolve_scoped(db: &Connection, links: Vec<PendingLinkRow>) -> Result<()> {
    let mut path_cache: HashMap<String, bool> = HashMap::new();
    let mut stem_cache: HashMap<String, Vec<String>> = HashMap::new();
    let mut heading_cache: HashMap<(String, String), bool> = HashMap::new();

    let mut path_exists_stmt =
        db.prepare_cached("SELECT EXISTS(SELECT 1 FROM docs WHERE path=?1)")?;
    let mut candidates_stmt = db.prepare_cached(
        "SELECT path FROM doc_stems WHERE stem=?1 UNION SELECT path FROM aliases WHERE name=?1 LIMIT 2",
    )?;
    let mut heading_exists_stmt = db.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM headings WHERE path=?1 AND (name=?2 OR name=?3))",
    )?;
    let mut update = db.prepare_cached("UPDATE links SET resolved=?2,status=?3 WHERE rowid=?1")?;

    let mut doc_exists = |candidate: &str| -> Result<bool> {
        if let Some(&exists) = path_cache.get(candidate) {
            return Ok(exists);
        }
        let exists: bool = path_exists_stmt.query_row([candidate], |r| r.get(0))?;
        path_cache.insert(candidate.to_string(), exists);
        Ok(exists)
    };

    for (row, source, raw_target, previous, previous_status) in links {
        let (self_link, target, anchor, relative, absolute) =
            parse_link_target(&source, &raw_target);
        let exact = if self_link {
            Some(source.clone())
        } else if let Some(rel) = relative
            && doc_exists(&rel)?
        {
            Some(rel)
        } else if let Some(abs) = absolute
            && doc_exists(&abs)?
        {
            Some(abs)
        } else {
            None
        };

        let candidates: Option<&[String]> = if exact.is_some() || target.contains('/') {
            // Note: even when `exact` is None and `target` has no '/', compute stem candidates.
            None
        } else if let Some(stem) = Path::new(&target).file_stem().and_then(|s| s.to_str()) {
            if !stem_cache.contains_key(stem) {
                let found = candidates_stmt
                    .query_map([stem], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                stem_cache.insert(stem.to_string(), found);
            }
            stem_cache.get(stem).map(Vec::as_slice)
        } else {
            None
        };

        let mut resolved =
            exact.or_else(|| candidates.filter(|v| v.len() == 1).map(|v| v[0].clone()));
        let missing_heading = if let (Some(path), Some(anchor_str)) = (&resolved, &anchor) {
            if anchor_str.is_empty() {
                false
            } else {
                let key = (path.clone(), anchor_str.clone());
                let present = if let Some(&cached) = heading_cache.get(&key) {
                    cached
                } else {
                    let slug = parser::heading_id(anchor_str);
                    let exists: bool = heading_exists_stmt
                        .query_row(params![path, anchor_str, slug], |r| r.get(0))?;
                    heading_cache.insert(key, exists);
                    exists
                };
                !present
            }
        } else {
            false
        };

        let status = if missing_heading {
            resolved = None;
            "missing-heading"
        } else if resolved.is_some() {
            "resolved"
        } else if candidates.is_some_and(|v| v.len() > 1) {
            "ambiguous"
        } else {
            "missing"
        };
        if previous != resolved || previous_status != status {
            update.execute(params![row, resolved, status])?;
        }
    }
    Ok(())
}

fn resolve_bulk(db: &Connection, links: Vec<PendingLinkRow>) -> Result<()> {
    let paths: HashSet<String> = {
        let mut s = db.prepare("SELECT path FROM docs")?;
        s.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut stems: HashMap<String, Vec<String>> = HashMap::new();
    for p in &paths {
        if let Some(stem) = Path::new(p).file_stem().and_then(|s| s.to_str()) {
            stems.entry(stem.to_string()).or_default().push(p.clone());
        }
    }
    {
        let mut s = db.prepare("SELECT path,name FROM aliases")?;
        for row in s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (path, name) = row?;
            let candidates = stems.entry(name).or_default();
            if !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }
    let mut headings: HashMap<String, HashSet<String>> = HashMap::new();
    {
        let mut s = db.prepare("SELECT path,name FROM headings")?;
        for row in s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (path, name) = row?;
            headings.entry(path).or_default().insert(name);
        }
    }
    let mut update = db.prepare_cached("UPDATE links SET resolved=?2,status=?3 WHERE rowid=?1")?;
    for (row, source, raw_target, previous, previous_status) in links {
        let (self_link, target, anchor, relative, absolute) =
            parse_link_target(&source, &raw_target);
        let exact = if self_link {
            Some(source.clone())
        } else {
            relative
                .filter(|p| paths.contains(p))
                .or_else(|| absolute.filter(|p| paths.contains(p)))
        };
        let candidates = if target.contains('/') {
            None
        } else {
            Path::new(&target)
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| stems.get(s))
        };
        let mut resolved =
            exact.or_else(|| candidates.filter(|v| v.len() == 1).map(|v| v[0].clone()));
        let missing_heading =
            resolved
                .as_ref()
                .zip(anchor.as_ref())
                .is_some_and(|(path, anchor)| {
                    !anchor.is_empty()
                        && !headings.get(path).is_some_and(|h| {
                            h.contains(anchor) || h.contains(&parser::heading_id(anchor))
                        })
                });
        let status = if missing_heading {
            resolved = None;
            "missing-heading"
        } else if resolved.is_some() {
            "resolved"
        } else if candidates.is_some_and(|v| v.len() > 1) {
            "ambiguous"
        } else {
            "missing"
        };
        if previous != resolved || previous_status != status {
            update.execute(params![row, resolved, status])?;
        }
    }
    Ok(())
}

fn link_key(target: &str) -> String {
    let value = percent_encoding::percent_decode_str(target.split('#').next().unwrap_or(""))
        .decode_utf8_lossy()
        .replace('\\', "/");
    let value = value.trim().rsplit('/').next().unwrap_or("");
    if value.to_ascii_lowercase().ends_with(".md") {
        value[..value.len() - 3].to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn extract_snippet(body: &str, query: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if terms.is_empty() {
        return None;
    }

    let match_byte = if trimmed.is_ascii() {
        let lower = trimmed.to_ascii_lowercase();
        terms.iter().filter_map(|t| lower.find(t.as_str())).min()
    } else {
        let lower = trimmed.to_lowercase();
        if lower.len() == trimmed.len() {
            terms
                .iter()
                .filter_map(|t| lower.find(t.as_str()))
                .filter(|&idx| trimmed.is_char_boundary(idx))
                .min()
        } else {
            let char_terms: Vec<Vec<char>> = terms.iter().map(|t| t.chars().collect()).collect();
            trimmed
                .char_indices()
                .find(|&(idx, _)| {
                    char_terms
                        .iter()
                        .any(|term| starts_with_lowercase_chars(&trimmed[idx..], term))
                })
                .map(|(idx, _)| idx)
        }
    };

    let center_byte = match_byte.unwrap_or(0);
    let start_byte = if center_byte == 0 {
        0
    } else {
        trimmed[..center_byte]
            .char_indices()
            .rev()
            .nth(36)
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    };
    let end_byte = trimmed[center_byte..]
        .char_indices()
        .nth(96)
        .map(|(idx, _)| center_byte + idx)
        .unwrap_or(trimmed.len());

    let mut collapsed = String::with_capacity(160);
    if start_byte > 0 {
        collapsed.push('…');
    }
    let mut prev_space = false;
    for ch in trimmed[start_byte..end_byte].chars() {
        if ch.is_whitespace() {
            if !prev_space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            prev_space = true;
        } else {
            collapsed.push(ch);
            prev_space = false;
        }
    }
    let mut result = collapsed.trim_end().to_string();
    if result.is_empty() {
        return None;
    }
    if end_byte < trimmed.len() {
        result.push('…');
    }
    Some(result)
}

fn starts_with_lowercase_chars(text: &str, needle_lower: &[char]) -> bool {
    let mut lower_iter = text.chars().flat_map(char::to_lowercase);
    for &expected in needle_lower {
        if lower_iter.next() != Some(expected) {
            return false;
        }
    }
    true
}
