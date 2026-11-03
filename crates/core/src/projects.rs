use crate::{Hub, SearchHit};
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::Serialize;
use std::{collections::HashSet, fs::File, path::Path};
pub(crate) struct ProjectLock {
    _files: Vec<File>,
}

#[derive(Debug, Serialize)]
pub struct ProjectStats {
    pub documents: usize,
    pub links: usize,
    pub missing: usize,
    pub missing_heading: usize,
    pub ambiguous: usize,
    pub isolated: usize,
    pub duplicate_names: usize,
}
#[derive(Debug, Serialize)]
pub struct GraphEdge {
    pub call: bool,
    pub source: String,
    pub target: String,
}
#[derive(Debug, Serialize)]
pub struct ProjectGraph {
    pub nodes: Vec<SearchHit>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
}

impl Hub {
    pub(crate) fn project_lock(&self, id: &str, shared: bool) -> Result<ProjectLock> {
        let mut files = vec![self.central_lock(id, shared)?];
        if let Some((path, token)) = self.managed_output(id)?
            && path.exists()
        {
            self.validate_owned(id, &path, &token)?;
            files.push(Self::lock_output(&path, id, shared, false)?);
        }
        Ok(ProjectLock { _files: files })
    }
    pub(crate) fn writer_lock(&self, id: &str) -> Result<ProjectLock> {
        Self::validate_id(id)?;
        let parent = self.data.join("writers");
        std::fs::create_dir_all(&parent)?;
        if parent.canonicalize()? != parent {
            bail!("書込ロック保存先が不正です");
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join(format!("{id}.lock")))?;
        file.try_lock()?;
        let mut files = vec![file];
        if let Some((path, token)) = self.managed_output(id)?
            && path.exists()
        {
            self.validate_owned(id, &path, &token)?;
            files.push(Self::lock_output(&path, id, false, true)?);
        }
        Ok(ProjectLock { _files: files })
    }
    pub(crate) fn lock_output(path: &Path, id: &str, shared: bool, writer: bool) -> Result<File> {
        Self::validate_id(id)?;
        let parent = path.parent().context("出力先が不正です")?.join(".locks");
        std::fs::create_dir_all(&parent)?;
        if parent.canonicalize()? != parent {
            bail!("出力ロックの別名は使用できません");
        }
        let file_path = parent.join(format!(
            "{id}.{}.lock",
            if writer { "writer" } else { "lifecycle" }
        ));
        if file_path.exists() && file_path.canonicalize()? != file_path {
            bail!("出力ロックが不正です");
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(file_path)?;
        if shared {
            file.try_lock_shared().context("出力を更新中です")?;
        } else {
            file.try_lock().context("出力を別の処理で使用中です")?;
        }
        Ok(file)
    }
    pub(crate) fn central_lock(&self, id: &str, shared: bool) -> Result<File> {
        Self::validate_id(id)?;
        let dir = self.data.join("locks");
        std::fs::create_dir_all(&dir)?;
        if dir.canonicalize()? != dir {
            bail!("ロック保存先の別名は使用できません");
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(format!("{id}.lock")))?;
        if shared {
            file.try_lock_shared()
                .context("プロジェクトを更新中です。完了後に再試行してください")?;
        } else {
            file.try_lock()
                .context("プロジェクトを使用中です。閲覧やwatchの処理完了後に再試行してください")?;
        }
        Ok(file)
    }
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let _writer = self.writer_lock(id)?;
        let _lock = self.project_lock(id, false)?;
        self.remove_unlocked(id)
    }
    pub(crate) fn remove_unlocked(&mut self, id: &str) -> Result<()> {
        let managed = self.managed_output(id)?;
        let dir = if let Some((path, _)) = &managed {
            path.clone()
        } else {
            self.dir(id)?
        };
        if dir.exists() {
            self.delete_output(id, &dir, managed.as_ref().map(|(_, token)| token.as_str()))?;
        }
        self.registry
            .execute("DELETE FROM project_outputs WHERE project=?1", [id])?;
        self.registry
            .execute("DELETE FROM projects WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn stats(&self, id: &str) -> Result<ProjectStats> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut names = std::collections::HashMap::<String, usize>::new();
        let mut statement = db.prepare("SELECT path FROM docs")?;
        for path in statement.query_map([], |r| r.get::<_, String>(0))? {
            if let Some(name) = std::path::Path::new(&path?)
                .file_stem()
                .and_then(|p| p.to_str())
            {
                *names.entry(name.to_string()).or_default() += 1;
            }
        }
        Ok(ProjectStats {
            documents: db.query_row("SELECT COUNT(*) FROM docs", [], |r| {
                Ok(r.get::<_, i64>(0)? as usize)
            })?,
            links: db.query_row("SELECT COUNT(*) FROM links", [], |r| {
                Ok(r.get::<_, i64>(0)? as usize)
            })?,
            missing: db.query_row(
                "SELECT COUNT(*) FROM links WHERE status='missing'",
                [],
                |r| Ok(r.get::<_, i64>(0)? as usize),
            )?,
            missing_heading:db.query_row("SELECT COUNT(*) FROM links WHERE status='missing-heading'",[],|r|Ok(r.get::<_,i64>(0)? as usize))?,
            ambiguous: db.query_row(
                "SELECT COUNT(*) FROM links WHERE status='ambiguous'",
                [],
                |r| Ok(r.get::<_, i64>(0)? as usize),
            )?,
            isolated: db.query_row("SELECT COUNT(*) FROM docs d WHERE NOT EXISTS(SELECT 1 FROM links WHERE source=d.path AND resolved IS NOT NULL AND resolved<>d.path) AND NOT EXISTS(SELECT 1 FROM links WHERE resolved=d.path AND source<>d.path)",[],|r|Ok(r.get::<_,i64>(0)? as usize))?,
            duplicate_names: names.values().filter(|n|**n>1).count(),
        })
    }
    /// Folder aggregation reads every resolved edge; document rendering caps do not apply.
    pub fn graph_summary(&self, id: &str) -> Result<ProjectGraph> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut groups = std::collections::BTreeMap::<String, usize>::new();
        let folder = |path: &str| {
            path.rsplit_once('/')
                .map(|(p, _)| p.split('/').next().unwrap_or("/"))
                .unwrap_or("/")
                .to_string()
        };
        let mut statement = db.prepare("SELECT path FROM docs")?;
        for path in statement.query_map([], |r| r.get::<_, String>(0))? {
            *groups.entry(folder(&path?)).or_default() += 1;
        }
        let mut statement = db.prepare(
            "SELECT path, title FROM docs WHERE path GLOB 'oml-*.md' AND instr(path, '/') = 0",
        )?;
        let source_titles = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;
        let truncated = groups.len() > 200;
        let mut nodes = groups
            .iter()
            .take(200)
            .map(|(name, count)| {
                let label = if name == "/" {
                    "ルート"
                } else {
                    source_titles
                        .get(&format!("{name}.md"))
                        .map_or(name.as_str(), String::as_str)
                };
                SearchHit {
                    path: name.clone(),
                    title: format!("{label} ({count}文書)"),
                }
            })
            .collect::<Vec<_>>();
        let selected = nodes.iter().map(|n| n.path.clone()).collect::<HashSet<_>>();
        if truncated {
            nodes.push(SearchHit {
                path: "[その他]".into(),
                title: format!(
                    "その他 ({}文書)",
                    groups
                        .iter()
                        .filter(|(p, _)| !selected.contains(*p))
                        .map(|(_, n)| n)
                        .sum::<usize>()
                ),
            });
        }
        let mut keys = std::collections::BTreeSet::new();
        let mut statement =
            db.prepare("SELECT source,resolved FROM links WHERE resolved IS NOT NULL")?;
        for row in
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (a, b) = row?;
            let group = |p: &str| {
                let f = folder(p);
                if selected.contains(&f) {
                    f
                } else {
                    "[その他]".to_string()
                }
            };
            keys.insert((group(&a), group(&b)));
        }
        let edges = keys
            .into_iter()
            .map(|(source, target)| GraphEdge {
                source,
                target,
                call: false,
            })
            .collect();
        Ok(ProjectGraph {
            nodes,
            edges,
            truncated,
        })
    }
    pub fn graph(&self, id: &str, center: Option<&str>, limit: usize) -> Result<ProjectGraph> {
        if !(1..=500).contains(&limit) {
            bail!("グラフの表示件数は1～500件です");
        }
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let mut paths = vec![];
        let mut truncated = false;
        if let Some(center) = center.filter(|s| !s.is_empty()) {
            db.query_row("SELECT path FROM docs WHERE path=?1", [center], |r| {
                r.get::<_, String>(0)
            })?;
            paths.push(center.to_string());
            let mut s=db.prepare("SELECT resolved AS neighbor FROM links WHERE source=?1 AND resolved IS NOT NULL AND resolved<>?1 UNION SELECT source FROM links WHERE resolved=?1 AND source<>?1 ORDER BY neighbor LIMIT ?2")?;
            let neighbors = s
                .query_map(params![center, limit as i64], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            truncated = neighbors.len() >= limit;
            paths.extend(neighbors.into_iter().take(limit - 1));
        } else {
            let mut s = db.prepare("SELECT path FROM docs ORDER BY path LIMIT ?1")?;
            paths = s
                .query_map([(limit + 1) as i64], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
            if paths.len() > limit {
                truncated = true;
                paths.truncate(limit);
            }
        }
        let mut nodes = vec![];
        for path in &paths {
            nodes.push(
                db.query_row("SELECT path,title FROM docs WHERE path=?1", [path], |r| {
                    Ok(SearchHit {
                        path: r.get(0)?,
                        title: r.get(1)?,
                    })
                })?,
            );
        }
        let selected: HashSet<_> = paths.iter().cloned().collect();
        let mut edges = vec![];
        let mut edge_keys = HashSet::new();
        for path in &paths {
            let mut s=db.prepare_cached("SELECT resolved, MAX(target LIKE '%#oml-call') FROM links WHERE source=?1 AND resolved IS NOT NULL AND resolved<>source GROUP BY resolved LIMIT 1001")?;
            let targets = s
                .query_map([path], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            truncated |= targets.len() > 1000;
            for (target, call) in targets {
                if selected.contains(&target) && edge_keys.insert((path.clone(), target.clone())) {
                    if edges.len() == 1000 {
                        truncated = true;
                        break;
                    }
                    edges.push(GraphEdge {
                        source: path.clone(),
                        target,
                        call,
                    });
                }
            }
        }
        Ok(ProjectGraph {
            nodes,
            edges,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_in_use_cannot_be_deleted() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        let lock = hub.project_lock("demo", true).unwrap();
        assert!(hub.remove("demo").is_err());
        assert_eq!(hub.projects().unwrap().len(), 1);
        drop(lock);
        hub.remove("demo").unwrap();
    }
}
