use crate::{Hub, search::Search};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const NAMESPACE: &str = "MarkdownKnowledgeHub";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    id: String,
    name: String,
    input: String,
    token: String,
}
#[derive(Debug, Serialize)]
pub struct MigrationReport {
    pub old_output: String,
    pub new_output: String,
}
#[derive(Debug, Serialize)]
pub struct RetiredOutput {
    pub key: String,
    pub project: String,
    pub path: String,
}

pub(crate) fn initialize_registry(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS project_outputs(project TEXT PRIMARY KEY,path TEXT NOT NULL,token TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS retired_outputs(key TEXT PRIMARY KEY,project TEXT NOT NULL,path TEXT NOT NULL,token TEXT NOT NULL,legacy INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS preferences(key TEXT PRIMARY KEY,value TEXT NOT NULL);")?;
    Ok(())
}
pub(crate) fn overlaps(a: &Path, b: &Path) -> bool {
    // Canonical paths preserve case on Windows; comparison must still be case-insensitive.
    #[cfg(windows)]
    {
        let a = PathBuf::from(a.to_string_lossy().to_lowercase());
        let b = PathBuf::from(b.to_string_lossy().to_lowercase());
        a.starts_with(&b) || b.starts_with(&a)
    }
    #[cfg(not(windows))]
    {
        a.starts_with(b) || b.starts_with(a)
    }
}
fn path_text(path: &Path) -> Result<String> {
    Ok(path
        .to_str()
        .context("Unicodeのフォルダーを指定してください")?
        .to_string())
}
fn stored_input(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute()
            && !path
                .components()
                .any(|p| matches!(p, Component::ParentDir | Component::CurDir)),
        "入力フォルダーの絶対パスが不正です"
    );
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    if path.parent().is_none() {
        return Ok(path.to_path_buf());
    }
    let name = path.file_name().context("入力フォルダーが不正です")?;
    Ok(stored_input(path.parent().context("入力フォルダーが不正です")?)?.join(name))
}
fn read_manifest(path: &Path) -> Result<Manifest> {
    ensure!(
        path.is_dir() && path.canonicalize()? == path,
        "出力先の別名やリンクは使用できません"
    );
    let file = path.join("project.json");
    ensure!(
        file.canonicalize()? == file && fs::metadata(&file)?.len() <= 65536,
        "プロジェクト情報が不正です"
    );
    let m: Manifest = serde_json::from_slice(&fs::read(file)?)
        .context("出力フォルダーのプロジェクト情報を確認してください")?;
    Hub::validate_id(&m.id)?;
    ensure!(
        m.format == 1 && uuid::Uuid::parse_str(&m.token).is_ok(),
        "対応していないプロジェクト情報です"
    );
    ensure!(
        path.file_name() == Some(std::ffi::OsStr::new(&m.id))
            && path.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new(NAMESPACE)),
        "専用出力フォルダーを選択してください"
    );
    ensure!(
        !m.name.trim().is_empty() && m.name.len() <= 300,
        "プロジェクト名が不正です"
    );
    Ok(m)
}
fn write_manifest(path: &Path, m: &Manifest) -> Result<()> {
    let temp = path.join(format!(".project.json.tmp-{}", uuid::Uuid::new_v4()));
    use std::io::Write;
    let mut file = fs::File::options()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(m)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path.join("project.json"))?;
    Ok(())
}
/// Reject symlinks and unrelated files before recursive cleanup.
pub(crate) fn validate_tree(path: &Path) -> Result<()> {
    static SEGMENT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let segment = SEGMENT.get_or_init(|| {
        regex::Regex::new(r"^[0-9a-f]{32}(?:\.\d+)?\.(?:idx|pos|term|fast|fieldnorm|store|del)$")
            .expect("constant regex")
    });
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "出力内のリンクを検出したため操作を中断しました"
        );
        ensure!(
            entry.path().canonicalize()? == entry.path(),
            "出力内の別名を検出しました"
        );
        let relative = entry.path().strip_prefix(path)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let parts = relative
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let name = parts.last().context("出力構成が不正です")?;
        let search = parts.len() >= 2 && parts[parts.len() - 2] == "search";
        let valid = if entry.file_type().is_dir() {
            (parts.len() == 1 && matches!(name.as_str(), "search" | "generations" | "converted"))
                || (parts.len() == 2 && parts[0] == "converted" && name == "pages")
                || (parts.len() == 2 && parts[0] == "generations" && valid_generation(name))
                || (parts.len() == 3
                    && parts[0] == "generations"
                    && valid_generation(&parts[1])
                    && name == "search")
        } else if search {
            matches!(
                name.as_str(),
                "meta.json" | ".managed.json" | ".tantivy-meta.lock" | ".tantivy-writer.lock"
            ) || segment.is_match(name)
        } else if parts.len() == 2 && parts[0] == "converted" {
            matches!(name.as_str(), "index.md" | "log.md" | "file-wiki.json")
        } else if parts.len() == 3 && parts[0] == "converted" && parts[1] == "pages" {
            crate::convert::valid_converted_page(name)
        } else {
            parts.len() == 1
                && (matches!(
                    name.as_str(),
                    "project.json"
                        | "metadata.sqlite"
                        | "metadata.sqlite-wal"
                        | "metadata.sqlite-shm"
                        | "metadata.sqlite-journal"
                ) || name
                    .strip_prefix(".project.json.tmp-")
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok()))
        };
        ensure!(
            valid,
            "アプリ以外のファイルがあります。移動・削除を中断しました: {}",
            relative.display()
        );
    }
    Ok(())
}
fn copy_converted_bundle(source_output: &Path, target: &Path) -> Result<()> {
    let from = source_output.join("converted");
    if !from.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(&from).follow_links(false) {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "変換ページ内のリンクは移動できません"
        );
        let relative = entry.path().strip_prefix(source_output)?;
        let dest = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        ensure!(entry.file_type().is_file(), "変換ページの構成が不正です");
        fs::create_dir_all(dest.parent().context("変換ページの保存先が不正です")?)?;
        fs::copy(entry.path(), dest)?;
    }
    Ok(())
}
fn valid_generation(s: &str) -> bool {
    s.starts_with("g-")
        && s.bytes()
            .all(|b| b.is_ascii_digit() || b == b'g' || b == b'-')
}
fn ensure_capacity(parent: &Path, bytes: u64) -> Result<()> {
    ensure!(
        fs4::available_space(parent)? >= bytes.saturating_add(64 * 1024 * 1024),
        "出力先の空き容量が不足しています"
    );
    Ok(())
}

impl Hub {
    pub(crate) fn manifest_input(&self, id: &str) -> Result<Option<String>> {
        if let Some((path, token)) = self.managed_output(id)?
            && path.exists()
        {
            let m = read_manifest(&path)?;
            ensure!(
                m.id == id && m.token == token,
                "出力の所有情報が一致しません"
            );
            stored_input(Path::new(&m.input))?;
            return Ok(Some(m.input));
        }
        Ok(None)
    }
    pub(crate) fn managed_output(&self, id: &str) -> Result<Option<(PathBuf, String)>> {
        Ok(self
            .registry
            .query_row(
                "SELECT path,token FROM project_outputs WHERE project=?1",
                [id],
                |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get(1)?)),
            )
            .optional()?)
    }
    pub(crate) fn validate_owned(&self, id: &str, path: &Path, token: &str) -> Result<()> {
        let m = read_manifest(path)?;
        ensure!(
            m.id == id && m.token == token,
            "出力先の所有情報が一致しません。操作を中断しました"
        );
        Ok(())
    }
    pub(crate) fn validate_input(&self, input: &Path) -> Result<()> {
        ensure!(
            !overlaps(input, &self.data),
            "入力フォルダーとアプリ設定の保存先を分離してください"
        );
        for p in self.projects()? {
            ensure!(
                !overlaps(input, Path::new(&p.output))
                    && (p.legacy_output
                        || !overlaps(
                            input,
                            Path::new(&p.output).parent().context("出力先が不正です")?
                        )),
                "入力フォルダーと既存の出力先が重なっています"
            );
        }
        for p in self.retired_outputs()? {
            ensure!(
                !overlaps(input, Path::new(&p.path))
                    && (Path::new(&p.path).parent().and_then(Path::file_name)
                        != Some(std::ffi::OsStr::new(NAMESPACE))
                        || !overlaps(
                            input,
                            Path::new(&p.path).parent().context("旧出力先が不正です")?
                        )),
                "入力フォルダーと旧出力先が重なっています"
            );
        }
        Ok(())
    }
    fn destination(&self, id: &str, parent: &Path, input: &Path) -> Result<PathBuf> {
        ensure!(
            parent.is_absolute(),
            "出力フォルダーの絶対パスを指定してください"
        );
        let parent = parent
            .canonicalize()
            .context("出力フォルダーが見つかりません")?;
        ensure!(parent.is_dir(), "出力先にはフォルダーを選択してください");
        let namespace = parent.join(NAMESPACE);
        if namespace.exists() {
            ensure!(
                namespace.canonicalize()? == namespace && namespace.is_dir(),
                "専用出力領域に別名やファイルがあります"
            );
        }
        let dest = namespace.join(id);
        ensure!(
            !overlaps(&namespace, &self.data) && !overlaps(&namespace, input),
            "入力と出力のフォルダーを分離してください"
        );
        for p in self.projects()? {
            ensure!(
                !overlaps(&namespace, Path::new(&p.root)),
                "出力先が入力フォルダーと重なっています"
            );
            ensure!(
                !overlaps(&dest, Path::new(&p.output)),
                "出力先が既存の出力と重なっています"
            );
        }
        for p in self.retired_outputs()? {
            ensure!(
                !overlaps(&dest, Path::new(&p.path)),
                "出力先が保管中の旧出力と重なっています"
            );
        }
        ensure!(
            !dest.exists(),
            "同じプロジェクトの出力が既にあります。「既存プロジェクトを開く」を使用してください"
        );
        Ok(dest)
    }
    pub fn last_output_parent(&self) -> Result<Option<String>> {
        Ok(self
            .registry
            .query_row(
                "SELECT value FROM preferences WHERE key='last_output'",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }
    fn remember_output(&self, parent: &Path) -> Result<()> {
        self.registry.execute("INSERT INTO preferences VALUES('last_output',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[path_text(parent)?])?;
        Ok(())
    }
    pub fn register_with_output(
        &mut self,
        id: &str,
        name: &str,
        input: &Path,
        parent: &Path,
    ) -> Result<()> {
        Self::validate_id(id)?;
        ensure!(
            !name.trim().is_empty() && name.len() <= 300,
            "プロジェクト名を指定してください"
        );
        let _writer = self.writer_lock(id)?;
        let _lock = self.project_lock(id, false)?;
        ensure!(
            !self.projects()?.iter().any(|p| p.id == id),
            "このIDは登録済みです"
        );
        let input = input
            .canonicalize()
            .context("入力フォルダーが見つかりません")?;
        ensure!(input.is_dir(), "入力フォルダーを選択してください");
        self.validate_input(&input)?;
        let dest = self.destination(id, parent, &input)?;
        let mut bytes = 0u64;
        for entry in walkdir::WalkDir::new(&input).follow_links(false) {
            let e = entry?;
            if e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("md"))
            {
                bytes = bytes.saturating_add(e.metadata()?.len());
            }
        }
        ensure_capacity(&parent.canonicalize()?, bytes.saturating_mul(4))?;
        fs::create_dir_all(dest.parent().context("出力先が不正です")?)?;
        fs::create_dir(&dest).context("出力先へ書き込めません")?;
        let _external_writer = Self::lock_output(&dest, id, false, true)?;
        let _external_lifecycle = Self::lock_output(&dest, id, false, false)?;
        let m = Manifest {
            format: 1,
            id: id.to_string(),
            name: name.to_string(),
            input: path_text(&input)?,
            token: uuid::Uuid::new_v4().to_string(),
        };
        let initialized = (|| -> Result<()> {
            write_manifest(&dest, &m)?;
            let tx = self.registry.transaction()?;
            tx.execute(
                "INSERT INTO projects VALUES(?1,?2,?3)",
                params![id, name, m.input],
            )?;
            tx.execute(
                "INSERT INTO project_outputs VALUES(?1,?2,?3)",
                params![id, path_text(&dest)?, m.token],
            )?;
            tx.commit()?;
            self.db(id)?;
            Search::open(&dest.join("search"))?;
            self.remember_output(&parent.canonicalize()?)?;
            Ok(())
        })();
        if initialized.is_err() {
            self.registry
                .execute("DELETE FROM project_outputs WHERE project=?1", [id])?;
            self.registry
                .execute("DELETE FROM projects WHERE id=?1", [id])?;
            if validate_tree(&dest).is_ok() {
                let _ = fs::remove_dir_all(&dest);
            }
        }
        initialized
    }
    pub fn move_output(&mut self, id: &str, parent: &Path) -> Result<MigrationReport> {
        self.move_output_inner(id, parent, || Ok(()))
    }
    fn move_output_inner(
        &mut self,
        id: &str,
        parent: &Path,
        before_switch: impl FnOnce() -> Result<()>,
    ) -> Result<MigrationReport> {
        let _writer = self.writer_lock(id)?;
        let _lock = self.project_lock(id, false)?;
        let p = self
            .projects()?
            .into_iter()
            .find(|p| p.id == id)
            .context("プロジェクトがありません")?;
        let old = self.dir(id)?;
        let source_owner = self.managed_output(id)?;
        let dest = self.destination(id, parent, Path::new(&p.root))?;
        let bytes =
            walkdir::WalkDir::new(&old)
                .into_iter()
                .try_fold(0u64, |sum, e| -> Result<u64> {
                    let e = e?;
                    Ok(sum.saturating_add(if e.file_type().is_file() {
                        e.metadata()?.len()
                    } else {
                        0
                    }))
                })?;
        ensure_capacity(&parent.canonicalize()?, bytes)?;
        fs::create_dir_all(dest.parent().context("出力先が不正です")?)?;
        fs::create_dir(&dest)?;
        let _external_writer = Self::lock_output(&dest, id, false, true)?;
        let _external_lifecycle = Self::lock_output(&dest, id, false, false)?;
        let token = uuid::Uuid::new_v4().to_string();
        let m = Manifest {
            format: 1,
            id: id.to_string(),
            name: p.name,
            input: p.root,
            token: token.clone(),
        };
        let copied = (|| -> Result<()> {
            write_manifest(&dest, &m)?;
            self.copy_snapshot(id, &dest)?;
            before_switch()?;
            let tx = self.registry.transaction()?;
            tx.execute(
                "INSERT INTO retired_outputs VALUES(?1,?2,?3,?4,?5)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    id,
                    path_text(&old)?,
                    source_owner.as_ref().map(|(_, t)| t.as_str()).unwrap_or(""),
                    source_owner.is_none()
                ],
            )?;
            tx.execute("INSERT INTO project_outputs VALUES(?1,?2,?3) ON CONFLICT(project) DO UPDATE SET path=excluded.path,token=excluded.token",params![id,path_text(&dest)?,token])?;
            tx.execute("INSERT INTO preferences VALUES('last_output',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[path_text(dest.parent().and_then(Path::parent).context("出力先が不正です")?)?])?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = copied {
            if validate_tree(&dest).is_ok() {
                let _ = fs::remove_dir_all(&dest);
            }
            return Err(error);
        }
        Ok(MigrationReport {
            old_output: path_text(&old)?,
            new_output: path_text(&dest)?,
        })
    }
    pub(crate) fn copy_snapshot(&self, id: &str, target: &Path) -> Result<()> {
        let metadata = self.db(id)?;
        let source = self.index_dir_in_snapshot(id, &metadata)?;
        let base = self.dir(id)?;
        let relative = source.strip_prefix(&base)?;
        metadata.backup("main", target.join("metadata.sqlite"), None)?;
        let search = target.join(relative).join("search");
        fs::create_dir_all(&search)?;
        for entry in fs::read_dir(source.join("search"))? {
            let e = entry?;
            ensure!(
                e.file_type()?.is_file() && !e.file_type()?.is_symlink(),
                "検索索引に不明なファイルがあります"
            );
            fs::copy(e.path(), search.join(e.file_name()))?;
        }
        copy_converted_bundle(&base, target)?;
        let db = Connection::open_with_flags(
            target.join("metadata.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        ensure!(
            db.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))? == "ok",
            "コピー先のデータ検証に失敗しました"
        );
        let documents: u64 = db.query_row("SELECT COUNT(*) FROM docs", [], |r| {
            r.get::<_, i64>(0).map(|v| v as u64)
        })?;
        let index = Search::open(&search)?;
        ensure!(
            index.document_count()? == documents,
            "検索索引の件数が一致しません。再作成してから移行してください"
        );
        let title: Option<String> = db
            .query_row("SELECT title FROM docs ORDER BY path LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(title) = title {
            let q = title.chars().take(32).collect::<String>();
            if !q.trim().is_empty() {
                index.query(&q)?;
            }
        }
        for entry in walkdir::WalkDir::new(target) {
            let e = entry?;
            if e.file_type().is_file() {
                fs::File::options()
                    .read(true)
                    .write(true)
                    .open(e.path())?
                    .sync_all()?;
            }
        }
        Ok(())
    }
    pub fn import_output(&mut self, path: &Path) -> Result<String> {
        let path = path.canonicalize()?;
        let m = read_manifest(&path)?;
        let _writer = self.writer_lock(&m.id)?;
        let _lock = self.project_lock(&m.id, false)?;
        ensure!(
            !self.projects()?.iter().any(|p| p.id == m.id),
            "同じプロジェクトIDが登録済みです"
        );
        let input = stored_input(Path::new(&m.input))?;
        self.validate_input(&input)?;
        let namespace = path.parent().context("出力先が不正です")?;
        ensure!(
            !overlaps(&input, namespace) && !overlaps(namespace, &self.data),
            "入力と出力の構成が不正です"
        );
        for p in self.projects()? {
            ensure!(
                !overlaps(namespace, Path::new(&p.root)) && !overlaps(&path, Path::new(&p.output)),
                "既存プロジェクトの領域と重なっています"
            );
        }
        let _external_writer = Self::lock_output(&path, &m.id, false, true)?;
        let _external_lifecycle = Self::lock_output(&path, &m.id, false, false)?;
        let current = read_manifest(&path)?;
        ensure!(
            current.token == m.token && current.input == m.input && current.id == m.id,
            "出力情報が変更されました"
        );
        validate_tree(&path)?;
        let db = Connection::open_with_flags(
            path.join("metadata.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        ensure!(
            db.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))? == "ok",
            "出力のデータが不正です"
        );
        let count: i64 = db.query_row("SELECT COUNT(*) FROM docs", [], |r| r.get(0))?;
        let generation: Option<String> = db
            .query_row(
                "SELECT value FROM state_text WHERE key='search_generation'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let search = if let Some(g) = generation {
            ensure!(valid_generation(&g), "世代名が不正です");
            path.join("generations").join(g).join("search")
        } else {
            path.join("search")
        };
        ensure!(search.is_dir(), "検索索引がありません");
        ensure!(
            Search::open(&search)?.document_count()? == count as u64,
            "索引の件数が一致しません"
        );
        let tx = self.registry.transaction()?;
        tx.execute(
            "INSERT INTO projects VALUES(?1,?2,?3)",
            params![m.id, m.name, path_text(&input)?],
        )?;
        tx.execute(
            "INSERT INTO project_outputs VALUES(?1,?2,?3)",
            params![m.id, path_text(&path)?, m.token],
        )?;
        tx.commit()?;
        Ok(m.id)
    }
    pub fn change_input(&mut self, id: &str, input: &Path) -> Result<()> {
        let _writer = self.writer_lock(id)?;
        let _lock = self.project_lock(id, false)?;
        let input = input.canonicalize()?;
        ensure!(input.is_dir(), "入力フォルダーを選択してください");
        self.validate_input(&input)?;
        ensure!(
            self.projects()?.iter().any(|p| p.id == id),
            "プロジェクトがありません"
        );
        let managed = self.managed_output(id)?;
        let mut previous = None;
        if let Some((path, token)) = &managed {
            self.validate_owned(id, path, token)?;
            let mut m = read_manifest(path)?;
            previous = Some(read_manifest(path)?);
            m.input = path_text(&input)?;
            write_manifest(path, &m)?;
        }
        if let Err(error) = self.registry.execute(
            "UPDATE projects SET root=?2 WHERE id=?1",
            params![id, path_text(&input)?],
        ) {
            if let (Some((path, _)), Some(m)) = (&managed, previous) {
                write_manifest(path, &m).context("入力情報の復旧に失敗しました")?;
            }
            return Err(error.into());
        }
        Ok(())
    }
    pub fn retired_outputs(&self) -> Result<Vec<RetiredOutput>> {
        let mut s = self
            .registry
            .prepare("SELECT key,project,path FROM retired_outputs ORDER BY key")?;
        Ok(s.query_map([], |r| {
            Ok(RetiredOutput {
                key: r.get(0)?,
                project: r.get(1)?,
                path: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn cleanup_retired(&mut self, key: &str) -> Result<()> {
        let (id, path, token, legacy): (String, String, String, bool) = self.registry.query_row(
            "SELECT project,path,token,legacy FROM retired_outputs WHERE key=?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let _central = self.central_lock(&id, false)?;
        let path = PathBuf::from(path);
        ensure!(
            !self
                .projects()?
                .iter()
                .any(|p| overlaps(Path::new(&p.output), &path)),
            "使用中の出力は削除できません"
        );
        if path.exists() {
            let _output = if legacy {
                None
            } else {
                Some(Self::lock_output(&path, &id, false, false)?)
            };
            self.delete_output(&id, &path, if legacy { None } else { Some(&token) })?;
        }
        self.registry
            .execute("DELETE FROM retired_outputs WHERE key=?1", [key])?;
        Ok(())
    }
    pub(crate) fn delete_output(&self, id: &str, path: &Path, token: Option<&str>) -> Result<()> {
        if let Some(token) = token {
            self.validate_owned(id, path, token)?;
        } else {
            ensure!(
                path == self.data.join("indexes").join(id) && path.canonicalize()? == path,
                "旧出力の保存先が不正です"
            );
        }
        for project in self.projects()? {
            ensure!(
                !overlaps(path, Path::new(&project.root)),
                "原本領域は削除できません"
            );
        }
        validate_tree(path)?;
        Search::evict(path);
        fs::remove_dir_all(path)
            .context("出力が使用中のため削除できません。監視や別のアプリを停止してください")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_respects_external_writer_and_lifecycle_locks() {
        let source = tempfile::tempdir().unwrap();
        let settings = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::write(source.path().join("a.md"), "# Keep").unwrap();
        let mut h = Hub::open(settings.path()).unwrap();
        h.register_with_output("demo", "Demo", source.path(), output.path())
            .unwrap();
        h.scan("demo").unwrap();
        let path = h.dir("demo").unwrap();
        let mut second = Hub::open(other.path()).unwrap();
        for writer in [true, false] {
            let lock = Hub::lock_output(&path, "demo", false, writer).unwrap();
            assert!(second.import_output(&path).is_err());
            assert!(second.projects().unwrap().is_empty());
            drop(lock);
        }
        second.import_output(&path).unwrap();
    }
    #[test]
    fn input_change_restores_manifest_when_registry_update_fails() {
        let source = tempfile::tempdir().unwrap();
        let settings = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let replacement = tempfile::tempdir().unwrap();
        let mut h = Hub::open(settings.path()).unwrap();
        h.register_with_output("demo", "Demo", source.path(), output.path())
            .unwrap();
        let old = h.root("demo").unwrap();
        h.registry.execute_batch("CREATE TRIGGER prevent_input_change BEFORE UPDATE ON projects BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        assert!(h.change_input("demo", replacement.path()).is_err());
        assert_eq!(h.root("demo").unwrap(), old);
    }
    #[test]
    fn failed_migration_keeps_source_and_registration() {
        let source = tempfile::tempdir().unwrap();
        let settings = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        fs::write(source.path().join("a.md"), "# Keep").unwrap();
        let mut h = Hub::open(settings.path()).unwrap();
        h.register("demo", "Demo", source.path()).unwrap();
        h.scan("demo").unwrap();
        let old = h.dir("demo").unwrap();
        assert!(
            h.move_output_inner("demo", output.path(), || anyhow::bail!(
                "injected copy failure"
            ))
            .is_err()
        );
        assert_eq!(h.dir("demo").unwrap(), old);
        assert_eq!(h.search("demo", "Keep").unwrap().len(), 1);
        assert!(h.retired_outputs().unwrap().is_empty());
        assert!(!output.path().join(NAMESPACE).join("demo").exists());
        assert!(ensure_capacity(output.path(), u64::MAX).is_err());
    }
}
