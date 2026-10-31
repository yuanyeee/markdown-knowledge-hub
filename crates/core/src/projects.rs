use crate::{Hub, SearchHit};
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::Serialize;
use std::{collections::HashSet, fs::File};

#[derive(Debug, Serialize)]
pub struct ProjectStats {
    pub documents: usize,
    pub links: usize,
    pub missing: usize,
    pub ambiguous: usize,
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

impl Hub {
    pub(crate) fn project_lock(&self, id: &str, shared: bool) -> Result<File> {
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
        let _lock = self.project_lock(id, false)?;
        self.remove_unlocked(id)
    }
    pub(crate) fn remove_unlocked(&mut self, id: &str) -> Result<()> {
        let dir = self.dir(id)?;
        if dir.exists() {
            let expected_parent = self.data.join("indexes").canonicalize()?;
            let resolved = dir.canonicalize()?;
            if resolved.parent() != Some(expected_parent.as_path())
                || resolved.file_name() != Some(std::ffi::OsStr::new(id))
                || std::fs::symlink_metadata(&dir)?.file_type().is_symlink()
            {
                bail!("生成索引の保存先が不正なため削除を中断しました");
            }
            // The resolved target is exactly indexes/<validated-id>, never the MD root.
            std::fs::remove_dir_all(&resolved)
                .context("索引の削除に失敗しました。watchを停止して再試行してください")?;
        }
        self.registry
            .execute("DELETE FROM projects WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn stats(&self, id: &str) -> Result<ProjectStats> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
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
            ambiguous: db.query_row(
                "SELECT COUNT(*) FROM links WHERE status='ambiguous'",
                [],
                |r| Ok(r.get::<_, i64>(0)? as usize),
            )?,
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
            let mut s=db.prepare_cached("SELECT DISTINCT resolved FROM links WHERE source=?1 AND resolved IS NOT NULL LIMIT 1001")?;
            let targets = s
                .query_map([path], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            truncated |= targets.len() > 1000;
            for target in targets {
                if selected.contains(&target) && edge_keys.insert((path.clone(), target.clone())) {
                    if edges.len() == 1000 {
                        truncated = true;
                        break;
                    }
                    edges.push(GraphEdge {
                        source: path.clone(),
                        target,
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
