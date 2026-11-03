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
        self.search_page_scoped(id, q, "", offset, limit)
    }

    pub fn search_page_scoped(
        &self,
        id: &str,
        q: &str,
        source: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Page<SearchHit>> {
        if !source.is_empty()
            && !source
                .strip_prefix("oml-")
                .and_then(|path| path.strip_suffix(".md"))
                .is_some_and(|key| {
                    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        {
            bail!("検索対象の元仕様書が不正です");
        }
        validate(offset, limit)?;
        if q.len() > 1024 {
            bail!("検索語が長すぎます");
        }
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut items = vec![];
        if q.trim().is_empty() {
            let mut s =
                db.prepare("SELECT path,title FROM docs WHERE ?3='' OR path=?3 OR substr(path,1,length(?4))=?4 ORDER BY path LIMIT ?1 OFFSET ?2")?;
            items = s
                .query_map(
                    params![
                        (limit + 1) as i64,
                        offset as i64,
                        source,
                        format!("{}/", source.trim_end_matches(".md"))
                    ],
                    |r| {
                        Ok(SearchHit {
                            path: r.get(0)?,
                            title: r.get(1)?,
                        })
                    },
                )?
                .collect::<rusqlite::Result<_>>()?;
        } else {
            // Resolve both stores from the same generation, even if publication occurs now.
            let dir = self.index_dir_in_snapshot(id, &db)?;
            for path in Search::cached(&dir.join("search"))?.query_page_scoped(
                q,
                source,
                offset,
                limit + 1,
            )? {
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

#[derive(Debug, Serialize)]
pub struct BacklinkDetail {
    pub path: String,
    pub title: String,
    pub call: bool,
}

impl Hub {
    pub fn backlink_details(
        &self,
        id: &str,
        path: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Page<BacklinkDetail>> {
        validate(offset, limit)?;
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut statement = db.prepare(
            "SELECT d.path,d.title,MAX(l.target LIKE '%#oml-call') FROM links l JOIN docs d ON d.path=l.source
             WHERE l.resolved=?1 AND l.source<>?1 GROUP BY d.path,d.title ORDER BY d.title,d.path LIMIT ?2 OFFSET ?3",
        )?;
        let items = statement
            .query_map(params![path, (limit + 1) as i64, offset as i64], |row| {
                Ok(BacklinkDetail {
                    path: row.get(0)?,
                    title: row.get(1)?,
                    call: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(page(items, offset, limit))
    }

    pub fn oml_catalog(&self, id: &str, source: &str) -> Result<Vec<SearchHit>> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let prefix = format!("{}/", source.trim_end_matches(".md"));
        let mut statement = db.prepare(
            "SELECT path,title FROM docs WHERE
             (?1='' AND path GLOB 'oml-*.md' AND instr(path,'/')=0)
             OR (?1<>'' AND (path=?1 OR substr(path,1,length(?2))=?2))
             ORDER BY title,path",
        )?;
        let items = statement
            .query_map(params![source, prefix], |row| {
                Ok(SearchHit {
                    path: row.get(0)?,
                    title: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(items)
    }
}

#[cfg(test)]
mod scoped_search_tests {
    use super::*;
    use std::fs;

    #[test]
    fn source_scope_is_applied_before_pagination_and_cached_separately() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let input = temp.path().join("input");
        let source = format!("oml-{}", "a".repeat(64));
        let other = format!("oml-{}", "b".repeat(64));
        fs::create_dir_all(input.join(&source))?;
        fs::create_dir_all(input.join(&other))?;
        let root = format!("{source}.md");
        fs::write(input.join(&root), "# Needle source")?;
        fs::write(input.join(format!("{source}/1.md")), "# Needle child")?;
        fs::write(input.join(format!("{other}/1.md")), "# Needle elsewhere")?;
        let mut hub = Hub::open(&temp.path().join("state"))?;
        hub.register("demo", "Demo", &input)?;
        hub.scan("demo")?;
        let all = hub.search_page("demo", "Needle", 0, 10)?;
        assert_eq!(all.items.len(), 3);
        let first = hub.search_page_scoped("demo", "Needle", &root, 0, 1)?;
        assert_eq!(first.next_offset, Some(1));
        let next = hub.search_page_scoped("demo", "Needle", &root, 1, 1)?;
        assert_eq!(next.next_offset, None);
        let paths = [first.items[0].path.as_str(), next.items[0].path.as_str()];
        assert!(paths.contains(&root.as_str()));
        assert!(paths.contains(&format!("{source}/1.md").as_str()));
        assert!(
            hub.search_page_scoped("demo", "Needle", "../data.md", 0, 1)
                .is_err()
        );
        Ok(())
    }
}
