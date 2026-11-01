use crate::Hub;
use anyhow::Result;
use notify::{
    EventKind, Watcher,
    event::{CreateKind, ModifyKind, RemoveKind},
};
use std::{
    collections::HashSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Batch {
    full: bool,
    paths: HashSet<String>,
}
impl Batch {
    fn add(&mut self, root: &Path, event: notify::Result<notify::Event>) {
        let event = match event {
            Ok(event) => event,
            Err(e) => {
                eprintln!("監視エラー（全体照合に切替）: {e}");
                self.full = true;
                return;
            }
        };
        if event.need_rescan() {
            self.full = true;
            return;
        }
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let exact_file = matches!(
            event.kind,
            EventKind::Create(CreateKind::File)
                | EventKind::Remove(RemoveKind::File)
                | EventKind::Modify(ModifyKind::Data(_))
        );
        let existing_file = matches!(
            event.kind,
            EventKind::Modify(ModifyKind::Any | ModifyKind::Metadata(_))
                | EventKind::Create(CreateKind::Any)
        ) && !event.paths.is_empty()
            && event.paths.iter().all(|p| p.is_file());
        if (!exact_file && !existing_file) || event.paths.is_empty() {
            self.full = true;
            return;
        }
        for path in event.paths {
            let Some(relative) = relative_event_path(root, &path) else {
                self.full = true;
                continue;
            };
            if Path::new(&relative)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            {
                self.paths.insert(relative);
            }
        }
    }
}
fn relative_event_path(root: &Path, path: &Path) -> Option<String> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Some(relative.to_str()?.replace('\\', "/"));
    }
    // ReadDirectoryChangesW may omit the verbatim prefix returned by canonicalize.
    let root_text = root.to_str()?.trim_start_matches(r"\\?\");
    let path_text = path.to_str()?.trim_start_matches(r"\\?\");
    Some(
        Path::new(path_text)
            .strip_prefix(Path::new(root_text))
            .ok()?
            .to_str()?
            .replace('\\', "/"),
    )
}
pub(crate) fn run(hub: &mut Hub, id: &str) -> Result<()> {
    run_controlled(hub, id, &AtomicBool::new(false), &|_| {})
}
pub(crate) fn run_controlled(
    hub: &mut Hub,
    id: &str,
    stop: &AtomicBool,
    status: &dyn Fn(Option<String>),
) -> Result<()> {
    let root = hub.root(id)?;
    let (send, recv) = mpsc::sync_channel(4096);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = overflow.clone();
    let mut watcher = notify::recommended_watcher(move |event| {
        if let Err(mpsc::TrySendError::Full(_)) = send.try_send(event) {
            callback_overflow.store(true, Ordering::Release);
        }
    })?;
    watcher.watch(&root, notify::RecursiveMode::Recursive)?;
    hub.scan(id)?;
    status(None);
    let interval = Duration::from_secs(60);
    let mut next_full = Instant::now() + interval;
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut batch = Batch::default();
        match recv.recv_timeout(
            next_full
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(250)),
        ) {
            Ok(event) => batch.add(&root, event),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() < next_full {
                    continue;
                }
                batch.full = true;
            }
            Err(e) => return Err(e.into()),
        }
        let deadline = (Instant::now() + Duration::from_millis(250)).min(next_full);
        while Instant::now() < deadline {
            match recv.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(event) => batch.add(&root, event),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(e) => return Err(e.into()),
            }
        }
        batch.full |= overflow.swap(false, Ordering::AcqRel) || Instant::now() >= next_full;
        if !batch.full && batch.paths.is_empty() {
            continue;
        }
        anyhow::ensure!(
            hub.root(id)? == root,
            "入力フォルダーが変更されました。監視を再開してください"
        );
        let full = batch.full;
        let result = if full {
            hub.scan(id)
        } else {
            hub.scan_paths(id, &batch.paths.into_iter().collect::<Vec<_>>())
        };
        match result {
            Ok(r) => {
                status(None);
                println!("{}", serde_json::to_string(&r)?);
            }
            Err(e) => {
                status(Some(format!("{e:#}")));
                eprintln!("差分更新失敗（定期照合で再試行）: {e:#}");
            }
        }
        if full {
            next_full = Instant::now() + interval;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
    use std::path::PathBuf;
    #[test]
    fn file_events_are_scoped_and_folder_or_rename_events_fall_back() {
        let root = Path::new("/specs");
        let mut batch = Batch::default();
        batch.add(
            root,
            Ok(notify::Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(root.join("a.md"))),
        );
        batch.add(
            root,
            Ok(notify::Event::new(EventKind::Create(CreateKind::File)).add_path(root.join("b.md"))),
        );
        batch.add(
            root,
            Ok(notify::Event::new(EventKind::Remove(RemoveKind::File)).add_path(root.join("c.md"))),
        );
        assert!(!batch.full);
        assert_eq!(batch.paths.len(), 3);
        batch.add(
            root,
            Ok(
                notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                    .add_path(root.join("a.md"))
                    .add_path(root.join("b.md")),
            ),
        );
        assert!(batch.full);
        let mut batch = Batch::default();
        batch.add(
            root,
            Ok(notify::Event::new(EventKind::Create(CreateKind::Folder))
                .add_path(root.join("folder"))),
        );
        assert!(batch.full);
    }
    #[test]
    fn rescan_errors_and_unmappable_paths_cannot_be_silently_lost() {
        let root = Path::new("/specs");
        let mut batch = Batch::default();
        batch.add(root, Err(notify::Error::generic("overflow")));
        assert!(batch.full);
        let mut batch = Batch::default();
        batch.add(
            root,
            Ok(notify::Event::new(EventKind::Any).set_flag(notify::event::Flag::Rescan)),
        );
        assert!(batch.full);
        let mut batch = Batch::default();
        batch.add(
            root,
            Ok(
                notify::Event::new(EventKind::Create(notify::event::CreateKind::File))
                    .add_path(PathBuf::from("/elsewhere/a.md")),
            ),
        );
        assert!(batch.full);
    }
}
