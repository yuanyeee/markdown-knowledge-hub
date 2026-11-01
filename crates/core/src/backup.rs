use crate::{Hub, search::Search};
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
        if destination.starts_with(&self.data) || self.data.starts_with(&destination) {
            bail!("バックアップ先を稼働データと分離してください");
        }
        for project in self.projects()? {
            let root = Path::new(&project.root);
            if destination.starts_with(root) || root.starts_with(&destination) {
                bail!("バックアップ先を原本フォルダと分離してください");
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
        let projects = snapshot.projects()?;
        let _locks = projects
            .iter()
            .map(|p| self.project_lock(&p.id, true))
            .collect::<Result<Vec<_>>>()?;
        // Snapshot's generation names are fixed. Lifecycle locks prevent their collection.
        for project in projects {
            let base = snapshot.dir(&project.id)?;
            fs::create_dir_all(&base)?;
            let metadata = self.db(&project.id)?;
            let source = self.index_dir_in_snapshot(&project.id, &metadata)?;
            let relative = source.strip_prefix(&self.data)?;
            let expected = destination.join(relative);
            metadata.backup("main", base.join("metadata.sqlite"), None)?;
            let search = source.join("search");
            fs::create_dir_all(expected.join("search"))?;
            for entry in fs::read_dir(search)? {
                let entry = entry?;
                if entry.file_type()?.is_symlink() {
                    bail!("索引のシンボリックリンクは未対応です");
                }
                if entry.file_type()?.is_file() {
                    fs::copy(
                        entry.path(),
                        expected.join("search").join(entry.file_name()),
                    )?;
                }
            }
            Search::open(&expected.join("search"))?;
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
