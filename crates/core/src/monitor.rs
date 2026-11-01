use crate::Hub;
use anyhow::Result;
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Debug, Serialize)]
pub struct WatchStatus {
    pub running: bool,
    pub error: Option<String>,
    pub updates: usize,
}
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<WatchStatus>>,
    worker: Option<JoinHandle<()>>,
}
impl WatchHandle {
    pub fn start(data: &Path, id: &str) -> Result<Self> {
        let mut hub = Hub::open(data)?;
        hub.root(id)?;
        let id = id.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(WatchStatus {
            running: true,
            error: None,
            updates: 0,
        }));
        let signal = stop.clone();
        let output = status.clone();
        let worker = thread::spawn(move || {
            let result = crate::watch::run_controlled(&mut hub, &id, &signal, &|error| {
                let mut s = output.lock().unwrap_or_else(|e| e.into_inner());
                s.error = error;
                s.updates += 1;
            });
            let mut s = output.lock().unwrap_or_else(|e| e.into_inner());
            s.running = false;
            if let Err(error) = result {
                s.error = Some(format!("{error:#}"));
            }
        });
        Ok(Self {
            stop,
            status,
            worker: Some(worker),
        })
    }
    pub fn status(&self) -> WatchStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monitor_reconciles_changes_and_stops() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# Before").unwrap();
        let mut hub = Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        let mut handle = WatchHandle::start(state.path(), "demo").unwrap();
        let wait = |condition: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !condition() {
                assert!(std::time::Instant::now() < deadline);
                thread::sleep(std::time::Duration::from_millis(50));
            }
        };
        wait(&|| handle.status().updates > 0);
        std::fs::write(source.path().join("a.md"), "# After").unwrap();
        wait(&|| hub.search("demo", "After").is_ok_and(|v| !v.is_empty()));
        handle.stop();
        assert!(!handle.status().running);
        hub.remove("demo").unwrap();
        assert!(source.path().join("a.md").exists());
    }
}
