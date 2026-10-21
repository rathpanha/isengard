//! Recursive file watching for the open workspace roots.
//!
//! A thin wrapper over `notify`: raw change paths go into a flume channel and
//! the app debounces / applies them (`app/watch.rs`). Build and VCS
//! directories are filtered out so a compile does not wake the UI.

use std::path::{Path, PathBuf};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Path components whose changes are never reported.
const SKIP_COMPONENTS: &[&str] = &[".git", "node_modules", "target", ".DS_Store"];

/// One changed path.
#[derive(Debug, Clone)]
pub struct FsChange {
    pub path: PathBuf,
    /// Created / removed / renamed (the parent's listing changed).
    pub structural: bool,
}

/// Keeps the OS watcher alive; dropping it stops watching.
pub struct FsWatcher {
    _watcher: RecommendedWatcher,
    events: flume::Receiver<FsChange>,
}

impl FsWatcher {
    pub fn new(roots: &[PathBuf]) -> notify::Result<Self> {
        let (tx, events) = flume::unbounded();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else {
                return;
            };
            if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
                return;
            }
            // Create / remove / rename change a directory listing; plain
            // content writes do not.
            let structural = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(notify::event::ModifyKind::Name(_))
            );
            for path in event.paths {
                if !is_ignored(&path) {
                    let _ = tx.send(FsChange { path, structural });
                }
            }
        })?;
        for root in roots {
            if let Err(err) = watcher.watch(root, RecursiveMode::Recursive) {
                log::warn!("watch {}: {err}", root.display());
            }
        }
        Ok(Self {
            _watcher: watcher,
            events,
        })
    }

    pub fn events(&self) -> flume::Receiver<FsChange> {
        self.events.clone()
    }
}

fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| {
        SKIP_COMPONENTS
            .iter()
            .any(|skip| c.as_os_str() == std::ffi::OsStr::new(skip))
    })
}

#[cfg(test)]
mod tests {
    use super::{FsWatcher, is_ignored};
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn ignores_build_and_vcs_dirs() {
        assert!(is_ignored(Path::new("/p/.git/index")));
        assert!(is_ignored(Path::new("/p/target/debug/x")));
        assert!(is_ignored(Path::new("/p/web/node_modules/a/b.js")));
        assert!(!is_ignored(Path::new("/p/src/main.rs")));
    }

    #[test]
    fn reports_writes_under_a_root() {
        let tmp = tempfile::tempdir().unwrap();
        // macOS temp dirs are symlinks; watch the canonical path.
        let root = tmp.path().canonicalize().unwrap();
        let watcher = FsWatcher::new(std::slice::from_ref(&root)).unwrap();
        std::fs::write(root.join("a.txt"), "hi").unwrap();
        let got = watcher.events().recv_timeout(Duration::from_secs(5));
        assert!(got.is_ok(), "no event within 5s");
    }
}
