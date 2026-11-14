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
        self.search_page_scoped(id, q, "", offset, limit, "")
    }

    pub fn search_page_scoped(
        &self,
        id: &str,
        q: &str,
        source: &str,
        offset: usize,
        limit: usize,
        kind: &str,
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
        if kind.chars().count() > 80 {
            bail!("種別の指定が長すぎます");
        }
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut items = vec![];
        if q.trim().is_empty() {
            let prefix = format!("{}/", source.trim_end_matches(".md"));
            let has_okf = crate::okf::table_exists(&db)?;
            if !kind.is_empty() && !has_okf {
                return Ok(page(items, offset, limit));
            }
            let sql = if has_okf {
                "SELECT d.path,d.title FROM docs d LEFT JOIN okf o ON o.path=d.path WHERE (?3='' OR d.path=?3 OR substr(d.path,1,length(?4))=?4) AND (?5='' OR o.kind=?5) ORDER BY d.path LIMIT ?1 OFFSET ?2"
            } else {
                "SELECT path,title FROM docs WHERE (?3='' OR path=?3 OR substr(path,1,length(?4))=?4) AND ?5='' ORDER BY path LIMIT ?1 OFFSET ?2"
            };
            let mut s = db.prepare(sql)?;
            items = s
                .query_map(
                    params![(limit + 1) as i64, offset as i64, source, prefix, kind],
                    |r| {
                        Ok(SearchHit {
                            path: r.get(0)?,
                            title: r.get(1)?,
                            snippet: None,
                        })
                    },
                )?
                .collect::<rusqlite::Result<_>>()?;
        } else if !kind.is_empty() {
            let allowed = crate::okf::paths_of_kind(&db, kind)?;
            let dir = self.index_dir_in_snapshot(id, &db)?;
            let prefix = format!("{}/", source.trim_end_matches(".md"));
            let mut doc_stmt =
                db.prepare("SELECT path,title,substr(body,1,16384) FROM docs WHERE path=?1")?;
            for path in Search::cached(&dir.join("search"))?
                .query(q)?
                .into_iter()
                .filter(|path| {
                    allowed.contains(path)
                        && (source.is_empty() || path == source || path.starts_with(&prefix))
                })
                .skip(offset)
                .take(limit + 1)
            {
                let (p, title, body): (String, String, String) =
                    doc_stmt.query_row([&path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                items.push(SearchHit {
                    snippet: crate::store::extract_snippet(&body, q),
                    path: p,
                    title,
                });
            }
        } else {
            // Resolve both stores from the same generation, even if publication occurs now.
            let dir = self.index_dir_in_snapshot(id, &db)?;
            let mut doc_stmt =
                db.prepare("SELECT path,title,substr(body,1,16384) FROM docs WHERE path=?1")?;
            for path in Search::cached(&dir.join("search"))?.query_page_scoped(
                q,
                source,
                offset,
                limit + 1,
            )? {
                let (p, title, body): (String, String, String) =
                    doc_stmt.query_row([path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                items.push(SearchHit {
                    snippet: crate::store::extract_snippet(&body, q),
                    path: p,
                    title,
                });
            }
        }
        crate::okf::fill_descriptions(&db, &mut items)?;
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
            "stale" | "missing-type" | "frontmatter-invalid" => {
                if !crate::okf::table_exists(&db)? {
                    Vec::new()
                } else {
                    let now = crate::okf::utc_now();
                    let (sql, binds): (&str, Vec<&str>) = match kind {
                        "stale" => (
                            "SELECT d.path, d.title, o.stale_after
                             FROM okf o JOIN docs d ON d.path=o.path
                             WHERE o.stale_after IS NOT NULL AND o.stale_after<=?1
                             ORDER BY o.stale_after, d.path LIMIT ?2 OFFSET ?3",
                            vec![now.as_str()],
                        ),
                        "missing-type" => (
                            "SELECT d.path, d.title, NULL
                             FROM docs d JOIN okf o ON o.path=d.path
                             WHERE o.concept=1 AND (o.kind IS NULL OR o.kind='')
                               AND (o.frontmatter_error IS NULL OR o.frontmatter_error='')
                               AND EXISTS(SELECT 1 FROM state_text WHERE key='okf_version' AND value<>'')
                             ORDER BY d.path LIMIT ?1 OFFSET ?2",
                            vec![],
                        ),
                        _ => (
                            "SELECT d.path, d.title, o.frontmatter_error
                             FROM okf o JOIN docs d ON d.path=o.path
                             WHERE o.frontmatter_error IS NOT NULL AND o.frontmatter_error<>''
                             ORDER BY d.path LIMIT ?1 OFFSET ?2",
                            vec![],
                        ),
                    };
                    let mut statement = db.prepare(sql)?;
                    let mut values: Vec<rusqlite::types::Value> = binds
                        .into_iter()
                        .map(|value| rusqlite::types::Value::Text(value.to_string()))
                        .collect();
                    values.push(rusqlite::types::Value::Integer(fetch_limit));
                    values.push(rusqlite::types::Value::Integer(fetch_offset));
                    let params = rusqlite::params_from_iter(values);
                    statement
                        .query_map(params, |row| {
                            Ok(DiagnosticItem {
                                path: row.get(0)?,
                                title: row.get(1)?,
                                detail: row.get(2)?,
                            })
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                }
            }
            _ => bail!("未対応の診断種別です: {kind}"),
        };
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
                    snippet: None,
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
        let first = hub.search_page_scoped("demo", "Needle", &root, 0, 1, "")?;
        assert_eq!(first.next_offset, Some(1));
        let next = hub.search_page_scoped("demo", "Needle", &root, 1, 1, "")?;
        assert_eq!(next.next_offset, None);
        let paths = [first.items[0].path.as_str(), next.items[0].path.as_str()];
        assert!(paths.contains(&root.as_str()));
        assert!(paths.contains(&format!("{source}/1.md").as_str()));
        assert!(
            hub.search_page_scoped("demo", "Needle", "../data.md", 0, 1, "")
                .is_err()
        );
        Ok(())
    }
}
