//! Commit metadata and the paired immutable search generation atomically.
use crate::{Hub, ScanReport};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

impl Hub {
    pub(crate) fn active_dir(&self, id: &str) -> Result<PathBuf> {
        if let Some((project, dir)) = &self.working
            && project == id
        {
            return Ok(dir.clone());
        }
        self.index_dir_in_snapshot(id, &self.db(id)?)
    }
    pub(crate) fn index_dir_in_snapshot(&self, id: &str, db: &Connection) -> Result<PathBuf> {
        let base = self.dir(id)?;
        let name: Option<String> = db
            .query_row(
                "SELECT value FROM state_text WHERE key='search_generation'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(name) = name {
            if !name.starts_with("g-")
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'-' || b == b'g')
            {
                bail!("世代名が不正です");
            }
            let dir = base.join("generations").join(name);
            if dir.canonicalize()? != dir {
                bail!("世代保存先の別名は使用できません");
            }
            Ok(dir)
        } else {
            Ok(base)
        }
    }
    pub(crate) fn commit_generation(
        &self,
        id: &str,
        db: &Connection,
        before_publish: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let (_, stage) = self
            .working
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("更新世代がありません"))?;
        for entry in walkdir::WalkDir::new(stage) {
            let entry = entry?;
            if entry.file_type().is_file() {
                fs::File::options()
                    .read(true)
                    .write(true)
                    .open(entry.path())?
                    .sync_all()?;
            }
        }
        before_publish()?;
        let name = stage
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| anyhow::anyhow!("世代名が不正です"))?;
        Self::validate_id(id)?;
        db.execute("INSERT INTO state_text VALUES('search_generation',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![name])?;
        Ok(())
    }

    pub(crate) fn publish_scan(
        &mut self,
        id: &str,
        verify: bool,
        paths: Option<&[String]>,
        rebuild: bool,
        before_publish: impl FnOnce() -> Result<()>,
    ) -> Result<ScanReport> {
        Self::validate_id(id)?;
        // Readers share this lifecycle lock. Removal remains exclusive.
        let writer_dir = self.data.join("writers");
        fs::create_dir_all(&writer_dir)?;
        if writer_dir.canonicalize()? != writer_dir {
            bail!("書込ロック保存先が不正です");
        }
        let writer = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(writer_dir.join(format!("{id}.lock")))?;
        writer.try_lock()?;
        {
            let _read = self.project_lock(id, true)?;
            if paths.is_none()
                && !verify
                && !rebuild
                && let Some(unchanged) = self.unchanged_metadata(id)?
            {
                return Ok(ScanReport {
                    unchanged,
                    ..Default::default()
                });
            }
        }
        // Reclaim old generations only when no reader/remover is using this project.
        if let Ok(_exclusive) = self.project_lock(id, false) {
            crate::search::Search::evict(&self.dir(id)?);
            let active = self.active_dir(id)?;
            let parent = self.dir(id)?.join("generations");
            if parent.exists() {
                if parent.canonicalize()? != parent {
                    bail!("世代保存先が不正です");
                }
                for entry in fs::read_dir(&parent)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path != active
                        && entry.file_type()?.is_dir()
                        && !entry.file_type()?.is_symlink()
                        && entry.file_name().to_string_lossy().starts_with("g-")
                        && path.canonicalize()?.parent() == Some(parent.as_path())
                    {
                        // Windows may still have mapped index files open. Retry next scan.
                        if let Err(error) = fs::remove_dir_all(path) {
                            eprintln!("旧世代の回収を次回へ延期: {error}");
                        }
                    }
                }
            }
        }
        let _lifecycle = self.project_lock(id, true)?;
        let source = self.active_dir(id)?;
        let version: i64 = self.db(id)?.query_row(
            "SELECT value FROM state WHERE key='parser_version'",
            [],
            |r| r.get(0),
        )?;
        let paths = if version < 5 {
            if let Some(paths) = paths {
                let root = self.root(id)?;
                for path in paths {
                    crate::store::event_file(&root, path)?;
                }
            }
            None
        } else {
            paths
        };
        let name = format!(
            "g-{}-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            std::process::id()
        );
        let stage = self.dir(id)?.join("generations").join(&name);
        fs::create_dir_all(&stage)?;
        if stage.canonicalize()? != stage {
            bail!("世代保存先が不正です");
        }
        if !rebuild && crate::search::Search::current(&source.join("search"))? {
            copy_search(&source.join("search"), &stage.join("search"))?;
        }
        self.working = Some((id.to_string(), stage.clone()));
        let result = self.scan_staged(id, verify, paths, rebuild, before_publish);
        self.working = None;
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }

    fn unchanged_metadata(&self, id: &str) -> Result<Option<usize>> {
        let db = self.db(id)?;
        let ready:bool=db.query_row("SELECT (SELECT value FROM state WHERE key='dirty')=0 AND (SELECT value FROM state WHERE key='parser_version')>=5",[],|r|r.get(0))?;
        if !ready {
            return Ok(None);
        }
        let mut statement = db.prepare("SELECT path,size,mtime FROM docs")?;
        let old = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (r.get::<_, i64>(1)? as u64, r.get::<_, String>(2)?),
                ))
            })?
            .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;
        let root = self.root(id)?;
        let mut seen = 0;
        for entry in walkdir::WalkDir::new(&root).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            {
                let path = entry
                    .path()
                    .strip_prefix(&root)?
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Unicodeパスが必要です"))?
                    .replace('\\', "/");
                let metadata = entry.metadata()?;
                let stamp = metadata
                    .modified()?
                    .duration_since(UNIX_EPOCH)?
                    .as_nanos()
                    .to_string();
                if !old
                    .get(&path)
                    .is_some_and(|(size, time)| *size == metadata.len() && *time == stamp)
                {
                    return Ok(None);
                }
                seen += 1;
            }
        }
        Ok((seen == old.len()).then_some(seen))
    }

    #[cfg(test)]
    pub(crate) fn scan_before_publish(
        &mut self,
        id: &str,
        hook: impl FnOnce() -> Result<()>,
    ) -> Result<ScanReport> {
        self.publish_scan(id, true, None, false, hook)
    }
}

fn copy_search(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    if !source.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if entry.file_type()?.is_symlink() {
            bail!("検索索引のシンボリックリンクは未対応です");
        }
        if entry.file_type()?.is_file() && name.to_string_lossy() != ".tantivy-writer.lock" {
            // Tantivy segment files are immutable; only metadata/lock files must be copied.
            let immutable = entry
                .path()
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| {
                    matches!(
                        s,
                        "idx" | "pos" | "term" | "fast" | "fieldnorm" | "store" | "del"
                    )
                });
            if !immutable || fs::hard_link(entry.path(), target.join(&name)).is_err() {
                fs::copy(entry.path(), target.join(name))?;
            }
        }
    }
    Ok(())
}
