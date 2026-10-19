//! Debounced watch on the machine index, so actions run from a terminal (or by a headless
//! `boxdeck-mcp`) show up without polling.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::error::{Error, Result};

const DEBOUNCE: Duration = Duration::from_millis(500);

/// Keeps the watch alive; dropping it stops watching.
pub struct FileWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

/// Calls `on_change` (debounced) when `file` is created, written or removed.
///
/// Watches the parent directory rather than the file, so the watch survives the file being
/// replaced or created later. If the parent doesn't exist yet (Vagrant never ran), the nearest
/// existing ancestor is watched recursively instead.
pub fn watch_file(file: &Path, on_change: impl Fn() + Send + 'static) -> Result<FileWatcher> {
    let name = file
        .file_name()
        .ok_or_else(|| Error::invalid("watched file", "has no file name"))?
        .to_owned();
    let parent = file
        .parent()
        .ok_or_else(|| Error::invalid("watched file", "has no parent folder"))?;
    let (root, mode) = match existing_ancestor(parent) {
        Some(dir) if dir == parent => (dir, RecursiveMode::NonRecursive),
        Some(dir) => (dir, RecursiveMode::Recursive),
        None => return Err(Error::invalid("watched file", "no existing parent folder")),
    };
    let target = file.to_owned();
    let watched = root.clone();
    let mut debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
        let Ok(events) = result else { return };
        // inotify only watches a new folder after its create event, so `mkdir -p` plus a write
        // can finish first and the file's own event never arrives. Count the folders on the way.
        let relevant = events.iter().any(|e| {
            e.paths.iter().any(|p| {
                p == &target
                    || p.file_name() == Some(name.as_os_str())
                    || (p != &watched && target.starts_with(p))
            })
        });
        if relevant {
            on_change();
        }
    })
    .map_err(|e| Error::Watch(e.to_string()))?;
    debouncer
        .watch(&root, mode)
        .map_err(|e| Error::Watch(e.to_string()))?;
    Ok(FileWatcher {
        _debouncer: debouncer,
    })
}

/// Calls `on_change` (debounced) when anything in the existing folder `dir` changes.
pub fn watch_dir(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<FileWatcher> {
    let mut debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
        if result.is_ok_and(|events| !events.is_empty()) {
            on_change();
        }
    })
    .map_err(|e| Error::Watch(e.to_string()))?;
    debouncer
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|e| Error::Watch(e.to_string()))?;
    Ok(FileWatcher {
        _debouncer: debouncer,
    })
}

fn existing_ancestor(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|a| a.is_dir()).map(Path::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn expect_change(rx: &mpsc::Receiver<()>) {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("change notification");
    }

    #[test]
    fn notices_writes_to_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("index");
        std::fs::write(&file, "{}").unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch_file(&file, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(&file, r#"{"version":1}"#).unwrap();
        expect_change(&rx);
    }

    #[test]
    fn notices_new_files_in_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch_dir(dir.path(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.path().join("1-2-3.request.json"), "{}").unwrap();
        expect_change(&rx);
    }

    #[test]
    fn notices_the_file_appearing_in_a_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data").join("machine-index").join("index");
        let (tx, rx) = mpsc::channel();
        let _w = watch_file(&file, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{}").unwrap();
        expect_change(&rx);
    }
}
