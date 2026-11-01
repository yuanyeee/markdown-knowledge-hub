use crate::{Hub, SearchHit, search::Search};
use anyhow::{Result, bail};
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_offset: Option<usize>,
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
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        } else {
            // Resolve both stores from the same generation, even if publication occurs now.
            let dir = self.index_dir_in_snapshot(id, &db)?;
            for path in Search::cached(&dir.join("search"))?.query_page(q, offset, limit + 1)? {
                items.push(db.query_row(
                    "SELECT path,title FROM docs WHERE path=?1",
                    [path],
                    |r| {
                        Ok(SearchHit {
                            path: r.get(0)?,
                            title: r.get(1)?,
                        })
                    },
                )?);
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
}
