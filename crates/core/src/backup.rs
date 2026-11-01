use crate::Hub;
use anyhow::{Context, Result, bail};
use std::{fs, path::Path};

impl Hub {
    /// A consistent backup of registered indexes, not a backup of original MD files.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        if !destination.is_absolute() || destination.exists() {
            bail!("バックアップ先には未使用の絶対パスを指定してください");
        }
        let parent = destination
            .parent()
            .context("保存先が不正です")?
            .canonicalize()?;
        let destination = parent.join(destination.file_name().context("保存先が不正です")?);
        if crate::outputs::overlaps(&destination, &self.data) {
            bail!("バックアップ先を稼働データと分離してください");
        }
        for project in self.projects()? {
            let root = Path::new(&project.root);
            if crate::outputs::overlaps(&destination, root) {
                bail!("バックアップ先を原本フォルダと分離してください");
            }
        }
        for project in self.projects()? {
            if crate::outputs::overlaps(&destination, Path::new(&project.output)) {
                bail!("バックアップ先を出力フォルダーと分離してください");
            }
        }
        for retired in self.retired_outputs()? {
            if crate::outputs::overlaps(&destination, Path::new(&retired.path)) {
                bail!("バックアップ先を旧出力フォルダーと分離してください");
            }
        }
        let initial = self.projects()?;
        let _initial_locks = initial
            .iter()
            .map(|p| self.project_lock(&p.id, true))
            .collect::<Result<Vec<_>>>()?;
        fs::create_dir(&destination)?;
        self.registry
            .backup("main", destination.join("registry.sqlite"), None)?;
        let snapshot = Hub::open(&destination)?;
        // Backups are self-contained; never retain pointers to live external outputs.
        snapshot.registry.execute_batch(
            "DELETE FROM project_outputs; DELETE FROM retired_outputs; DELETE FROM preferences;",
        )?;
        // The shared manifest may have been updated by another settings profile.
        for project in self.projects()? {
            snapshot.registry.execute(
                "UPDATE projects SET root=?2 WHERE id=?1",
                rusqlite::params![project.id, project.root],
            )?;
        }
        let projects = snapshot.projects()?;
        let _locks = projects
            .iter()
            .map(|p| self.project_lock(&p.id, true))
            .collect::<Result<Vec<_>>>()?;
        // Snapshot's generation names are fixed. Lifecycle locks prevent their collection.
        for project in projects {
            let base = snapshot.dir(&project.id)?;
            fs::create_dir_all(&base)?;
            self.copy_snapshot(&project.id, &base)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_can_be_restored_without_touching_sources() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        fs::write(source.path().join("a.md"), "# BackupTest").unwrap();
        let mut hub = Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        let backup = output.path().join("backup");
        hub.backup(&backup).unwrap();
        assert!(hub.backup(&backup).is_err());
        let restored = output.path().join("restored");
        Hub::open(&backup).unwrap().backup(&restored).unwrap();
        assert_eq!(
            Hub::open(&restored)
                .unwrap()
                .search("demo", "BackupTest")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            fs::read_to_string(source.path().join("a.md")).unwrap(),
            "# BackupTest"
        );
    }
}
