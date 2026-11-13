use crate::{Hub, SearchHit, search::Search};
use anyhow::{Result, bail};
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticItem {
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
fn validate(offset: usize, limit: usize) -> Result<()> {
    if !(1..=200).contains(&limit) || offset > 1_000_000 {
        bail!("ページ指定が不正です（1～200件）");
    }
    Ok(())
}
fn page<T>(mut items: Vec<T>, offset: usize, limit: usize) -> Page<T> {
    let next_offset = (items.len() > limit).then_some(offset + limit);
    items.truncate(limit);
    Page { items, next_offset }
}
impl Hub {
    pub fn search_page(
        &self,
        id: &str,
        q: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Page<SearchHit>> {
        validate(offset, limit)?;
        if q.len() > 1024 {
            bail!("検索語が長すぎます");
        }
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut items = vec![];
        if q.trim().is_empty() {
            let mut s =
                db.prepare("SELECT path,title FROM docs ORDER BY path LIMIT ?1 OFFSET ?2")?;
            items = s
                .query_map(params![(limit + 1) as i64, offset as i64], |r| {
                    Ok(SearchHit {
                        path: r.get(0)?,
                        title: r.get(1)?,
                        snippet: None,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        } else {
            // Resolve both stores from the same generation, even if publication occurs now.
            let dir = self.index_dir_in_snapshot(id, &db)?;
            let mut doc_stmt =
                db.prepare("SELECT path,title,substr(body,1,16384) FROM docs WHERE path=?1")?;
            for path in Search::cached(&dir.join("search"))?.query_page(q, offset, limit + 1)? {
                let (p, title, body): (String, String, String) =
                    doc_stmt.query_row([path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                items.push(SearchHit {
                    snippet: crate::store::extract_snippet(&body, q),
                    path: p,
                    title,
                });
            }
        }
        Ok(page(items, offset, limit))
    }
    pub fn backlinks_page(
        &self,
        id: &str,
        path: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Page<String>> {
        validate(offset, limit)?;
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut s = db.prepare("SELECT DISTINCT source FROM links WHERE resolved=?1 ORDER BY source LIMIT ?2 OFFSET ?3")?;
        let items = s
            .query_map(params![path, (limit + 1) as i64, offset as i64], |r| {
                r.get(0)
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(page(items, offset, limit))
    }

    pub fn diagnostics_page(
        &self,
        id: &str,
        kind: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Page<DiagnosticItem>> {
        validate(offset, limit)?;
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let fetch_limit = (limit + 1) as i64;
        let fetch_offset = offset as i64;
        let items = match kind {
            "missing" | "ambiguous" | "missing-heading" => {
                let mut s = db.prepare(
                    "SELECT l.source, COALESCE(d.title, l.source), l.target
                     FROM links l
                     LEFT JOIN docs d ON d.path = l.source
                     WHERE l.status = ?1
                     ORDER BY l.source, l.target, l.rowid
                     LIMIT ?2 OFFSET ?3",
                )?;
                s.query_map(params![kind, fetch_limit, fetch_offset], |r| {
                    Ok(DiagnosticItem {
                        path: r.get(0)?,
                        title: r.get(1)?,
                        detail: Some(r.get(2)?),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
            "isolated" => {
                let mut s = db.prepare(
                    "SELECT d.path, d.title
                     FROM docs d
                     WHERE NOT EXISTS(SELECT 1 FROM links WHERE source=d.path AND resolved IS NOT NULL AND resolved<>d.path)
                       AND NOT EXISTS(SELECT 1 FROM links WHERE resolved=d.path AND source<>d.path)
                     ORDER BY d.path
                     LIMIT ?1 OFFSET ?2",
                )?;
                s.query_map(params![fetch_limit, fetch_offset], |r| {
                    Ok(DiagnosticItem {
                        path: r.get(0)?,
                        title: r.get(1)?,
                        detail: None,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
            "duplicate-names" => {
                let has_stems: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='doc_stems')",
                    [],
                    |r| r.get(0),
                )?;
                let docs_count: i64 =
                    db.query_row("SELECT COUNT(*) FROM docs", [], |r| r.get(0))?;
                if has_stems
                    && db.query_row("SELECT COUNT(*) FROM doc_stems", [], |r| r.get::<_, i64>(0))?
                        == docs_count
                {
                    let mut s = db.prepare(
                        "SELECT s.path, COALESCE(d.title, s.path), s.stem
                         FROM doc_stems s
                         LEFT JOIN docs d ON d.path = s.path
                         WHERE s.stem IN (SELECT stem FROM doc_stems GROUP BY stem HAVING COUNT(*) > 1)
                         ORDER BY s.stem, s.path
                         LIMIT ?1 OFFSET ?2",
                    )?;
                    s.query_map(params![fetch_limit, fetch_offset], |r| {
                        Ok(DiagnosticItem {
                            path: r.get(0)?,
                            title: r.get(1)?,
                            detail: Some(r.get(2)?),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                } else {
                    let mut all_docs = db.prepare("SELECT path, title FROM docs ORDER BY path")?;
                    let rows = all_docs
                        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    let mut counts = std::collections::HashMap::<String, usize>::new();
                    for (p, _) in &rows {
                        if let Some(stem) =
                            std::path::Path::new(p).file_stem().and_then(|s| s.to_str())
                        {
                            *counts.entry(stem.to_string()).or_default() += 1;
                        }
                    }
                    let mut dupes: Vec<(String, String, String)> = rows
                        .into_iter()
                        .filter_map(|(p, title)| {
                            let stem = std::path::Path::new(&p)
                                .file_stem()
                                .and_then(|s| s.to_str())?
                                .to_string();
                            (counts.get(&stem).copied().unwrap_or(0) > 1)
                                .then_some((stem, p, title))
                        })
                        .collect();
                    dupes.sort();
                    dupes
                        .into_iter()
                        .skip(offset)
                        .take(limit + 1)
                        .map(|(stem, path, title)| DiagnosticItem {
                            path,
                            title,
                            detail: Some(stem),
                        })
                        .collect()
                }
            }
            _ => bail!("未対応の診断種別です: {kind}"),
        };
        Ok(page(items, offset, limit))
    }
}
