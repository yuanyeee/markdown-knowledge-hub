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
    pub source: String,
    pub target: String,
}
#[derive(Debug, Serialize)]
pub struct ProjectGraph {
    pub nodes: Vec<SearchHit>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
}

fn try_shared_with_backoff(file: &File) -> std::result::Result<(), std::fs::TryLockError> {
    for delay_ms in [2_u64, 4, 8, 16, 32, 64, 100] {
        match file.try_lock_shared() {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            Err(err) => return Err(err),
        }
    }
    file.try_lock_shared()
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
            try_shared_with_backoff(&file).context("出力を更新中です")?;
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
            try_shared_with_backoff(&file)
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
        let documents = db.query_row("SELECT COUNT(*) FROM docs", [], |r| {
            Ok(r.get::<_, i64>(0)? as usize)
        })?;
        let mut links = 0usize;
        let mut missing = 0usize;
        let mut missing_heading = 0usize;
        let mut ambiguous = 0usize;
        {
            let mut stmt = db.prepare("SELECT status, COUNT(*) FROM links GROUP BY status")?;
            for row in stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
            })? {
                let (status, count) = row?;
                links += count;
                match status.as_str() {
                    "missing" => missing = count,
                    "missing-heading" => missing_heading = count,
                    "ambiguous" => ambiguous = count,
                    _ => {}
                }
            }
        }
        let has_stems_table: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='doc_stems')",
            [],
            |r| r.get(0),
        )?;
        let duplicate_names = if has_stems_table
            && db.query_row("SELECT COUNT(*) FROM doc_stems", [], |r| {
                Ok(r.get::<_, i64>(0)? as usize)
            })? == documents
        {
            db.query_row(
                "SELECT COUNT(*) FROM (SELECT 1 FROM doc_stems GROUP BY stem HAVING COUNT(*) > 1)",
                [],
                |r| Ok(r.get::<_, i64>(0)? as usize),
            )?
        } else {
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
            names.values().filter(|n| **n > 1).count()
        };
        let isolated = db.query_row(
            "SELECT COUNT(*) FROM docs d WHERE NOT EXISTS(SELECT 1 FROM links WHERE source=d.path AND resolved IS NOT NULL AND resolved<>d.path) AND NOT EXISTS(SELECT 1 FROM links WHERE resolved=d.path AND source<>d.path)",
            [],
            |r| Ok(r.get::<_, i64>(0)? as usize),
        )?;
        Ok(ProjectStats {
            documents,
            links,
            missing,
            missing_heading,
            ambiguous,
            isolated,
            duplicate_names,
        })
    }
    /// Folder aggregation reads every resolved edge; document rendering caps do not apply.
    pub fn graph_summary(&self, id: &str) -> Result<ProjectGraph> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let all_paths = {
            let mut statement = db.prepare("SELECT path FROM docs")?;
            statement
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let folder_at_depth = |path: &str, depth: usize| -> String {
            let Some((dir, _)) = path.rsplit_once('/') else {
                return "/".to_string();
            };
            let parts: Vec<&str> = dir.split('/').take(depth).collect();
            if parts.is_empty() || parts[0].is_empty() {
                "/".to_string()
            } else {
                parts.join("/")
            }
        };
        let mut groups = std::collections::BTreeMap::<String, usize>::new();
        for path in &all_paths {
            *groups.entry(folder_at_depth(path, 1)).or_default() += 1;
        }
        let depth = if groups.len() == 1 && !groups.contains_key("/") {
            let mut deeper = std::collections::BTreeMap::<String, usize>::new();
            for path in &all_paths {
                *deeper.entry(folder_at_depth(path, 2)).or_default() += 1;
            }
            if deeper.len() > 1 {
                groups = deeper;
                2
            } else {
                1
            }
        } else {
            1
        };
        let truncated = groups.len() > 200;
        let mut nodes = groups
            .iter()
            .take(200)
            .map(|(name, count)| SearchHit {
                path: name.clone(),
                title: format!("{name} ({count}文書)"),
                snippet: None,
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
                snippet: None,
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
                let f = folder_at_depth(p, depth);
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
            .map(|(source, target)| GraphEdge { source, target })
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
            let mut s = db.prepare("SELECT resolved AS neighbor FROM links WHERE source=?1 AND resolved IS NOT NULL AND resolved<>?1 UNION SELECT source FROM links WHERE resolved=?1 AND source<>?1 ORDER BY neighbor LIMIT ?2")?;
            let neighbors = s
                .query_map(params![center, limit as i64], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            truncated = neighbors.len() >= limit;
            paths.extend(neighbors.into_iter().take(limit - 1));
        } else {
            let wanted = limit + 1;
            let mut seen_paths = HashSet::new();
            let mut ranked = db.prepare(
                "SELECT path FROM (
                    SELECT resolved AS path, COUNT(*) AS deg FROM links WHERE resolved IS NOT NULL AND source<>resolved GROUP BY resolved
                    UNION ALL
                    SELECT source AS path, COUNT(*) AS deg FROM links WHERE resolved IS NOT NULL AND source<>resolved GROUP BY source
                ) WHERE EXISTS(SELECT 1 FROM docs d WHERE d.path=path)
                GROUP BY path ORDER BY SUM(deg) DESC, path ASC LIMIT ?1",
            )?;
            for row in ranked.query_map([wanted as i64], |r| r.get::<_, String>(0))? {
                let p = row?;
                if seen_paths.insert(p.clone()) {
                    paths.push(p);
                }
            }
            if paths.len() < wanted {
                let mut fallback = db.prepare("SELECT path FROM docs ORDER BY path LIMIT ?1")?;
                for row in fallback.query_map([wanted as i64], |r| r.get::<_, String>(0))? {
                    let p = row?;
                    if seen_paths.insert(p.clone()) {
                        paths.push(p);
                        if paths.len() == wanted {
                            break;
                        }
                    }
                }
            }
            if paths.len() > limit {
                truncated = true;
                paths.truncate(limit);
            }
        }
        if paths.is_empty() {
            return Ok(ProjectGraph {
                nodes: vec![],
                edges: vec![],
                truncated,
            });
        }
        let mut title_map = std::collections::HashMap::<String, String>::with_capacity(paths.len());
        for chunk in paths.chunks(250) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let mut s = db.prepare(&format!(
                "SELECT path, title FROM docs WHERE path IN ({placeholders})"
            ))?;
            for row in s.query_map(rusqlite::params_from_iter(chunk), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (p, title) = row?;
                title_map.insert(p, title);
            }
        }
        let nodes = paths
            .iter()
            .filter_map(|p| {
                title_map.get(p).map(|title| SearchHit {
                    path: p.clone(),
                    title: title.clone(),
                    snippet: None,
                })
            })
            .collect::<Vec<_>>();
        let selected: HashSet<_> = paths.iter().cloned().collect();
        let mut edges = vec![];
        let mut edge_keys = HashSet::new();
        let mut per_source_count = std::collections::HashMap::<String, usize>::new();
        for chunk in paths.chunks(250) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let mut s = db.prepare(&format!(
                "SELECT DISTINCT source, resolved FROM links WHERE resolved IS NOT NULL AND source IN ({placeholders}) ORDER BY source, resolved"
            ))?;
            for row in s.query_map(rusqlite::params_from_iter(chunk), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (source, target) = row?;
                let count = per_source_count.entry(source.clone()).or_default();
                *count += 1;
                if *count > 1000 {
                    truncated = true;
                    continue;
                }
                if selected.contains(&target) && edge_keys.insert((source.clone(), target.clone()))
                {
                    if edges.len() == 1000 {
                        truncated = true;
                    } else {
                        edges.push(GraphEdge { source, target });
                    }
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

    #[test]
    fn shared_lock_waits_for_brief_exclusive_lock_release() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        let exclusive = hub.project_lock("demo", false).unwrap();
        let state_path = state.path().to_path_buf();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            drop(exclusive);
        });
        let reader = Hub::open(&state_path).unwrap();
        let shared = reader.project_lock("demo", true);
        assert!(shared.is_ok());
        releaser.join().unwrap();
    }
}
