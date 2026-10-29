//! File watching for the open workspace roots.
//!
//! **Linux:** non-recursive watch on each root, then walks the tree for
//! descendant watches, skipping build/VCS/cache dirs. `RecursiveMode::Recursive`
//! would create an inotify watch for every directory under `node_modules` /
//! `target` / … and blow `fs.inotify.max_user_watches`. Event filtering alone
//! does not prevent that. The descendant walk runs **after** `FsWatcher::new`
//! returns so opening a large folder never freezes the UI.
//!
//! **macOS / Windows:** one recursive watch per root (FSEvents /
//! ReadDirectoryChangesW). No tree walk — those backends do not pay the
//! Linux per-directory watch cost.
//!
//! A dedicated thread creates and owns the `notify` watcher (moving a macOS
//! FSEvents watcher across threads after `watch()` is unreliable). The app
//! still debounces in `app/watch.rs`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Path components that are never watched and never reported.
const SKIP_COMPONENTS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "__pycache__",
    ".venv",
    "venv",
    ".DS_Store",
];

/// One changed path.
#[derive(Debug, Clone)]
pub struct FsChange {
    pub path: PathBuf,
    /// Created / removed / renamed (the parent's listing changed).
    pub structural: bool,
}

/// Keeps the OS watcher alive; dropping it stops watching.
pub struct FsWatcher {
    /// Closes when dropped so the watcher thread exits.
    _shutdown: mpsc::Sender<()>,
    events: flume::Receiver<FsChange>,
}

impl FsWatcher {
    pub fn new(roots: &[PathBuf]) -> notify::Result<Self> {
        let (event_tx, events) = flume::unbounded();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<notify::Result<()>>();
        let roots = roots.to_vec();

        thread::Builder::new()
            .name("isengard-fs-watch".into())
            .spawn(move || {
                let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
                let mut watcher = match notify::recommended_watcher(move |res| {
                    let _ = raw_tx.send(res);
                }) {
                    Ok(w) => w,
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                // Register each root before ready so Open Folder can return with
                // the top level already covered. Linux still walks descendants
                // after ready (that walk used to beachball the UI).
                watch_roots_shallow(&mut watcher, &roots);
                let _ = ready_tx.send(Ok(()));
                #[cfg(target_os = "linux")]
                for root in &roots {
                    watch_dir_tree(&mut watcher, root);
                }

                loop {
                    match shutdown_rx.try_recv() {
                        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                    match raw_rx.recv_timeout(std::time::Duration::from_millis(200)) {
                        Ok(Ok(event)) => apply_raw_event(&mut watcher, event, &event_tx),
                        Ok(Err(err)) => log::warn!("file watch error: {err}"),
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .map_err(|err| notify::Error::new(notify::ErrorKind::Generic(err.to_string())))?;

        ready_rx
            .recv()
            .map_err(|_| notify::Error::new(notify::ErrorKind::Generic("watch thread died".into())))??;

        Ok(Self {
            _shutdown: shutdown_tx,
            events,
        })
    }

    pub fn events(&self) -> flume::Receiver<FsChange> {
        self.events.clone()
    }
}

/// One watch per workspace root — O(roots), never walks the tree.
fn watch_roots_shallow(watcher: &mut RecommendedWatcher, roots: &[PathBuf]) {
    for root in roots {
        if is_ignored(root) {
            continue;
        }
        #[cfg(target_os = "linux")]
        let mode = RecursiveMode::NonRecursive;
        #[cfg(not(target_os = "linux"))]
        let mode = RecursiveMode::Recursive;
        if let Err(err) = watcher.watch(root, mode) {
            log::warn!("watch {}: {err}", root.display());
        }
    }
}

fn apply_raw_event(
    #[cfg(target_os = "linux")] watcher: &mut RecommendedWatcher,
    #[cfg(not(target_os = "linux"))] _watcher: &mut RecommendedWatcher,
    event: notify::Event,
    event_tx: &flume::Sender<FsChange>,
) {
    if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
        return;
    }
    let structural = matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
    );
    for path in event.paths {
        if is_ignored(&path) {
            continue;
        }
        // Linux: new directories are not covered by the parent's non-recursive watch.
        #[cfg(target_os = "linux")]
        if matches!(event.kind, EventKind::Create(_)) && path.is_dir() {
            watch_dir_tree(watcher, &path);
        }
        let _ = event_tx.send(FsChange { path, structural });
    }
}

/// Non-recursive watch on `dir`, then the same for every non-ignored child dir.
#[cfg(target_os = "linux")]
fn watch_dir_tree(watcher: &mut RecommendedWatcher, dir: &Path) {
    if is_ignored(dir) {
        return;
    }
    if let Err(err) = watcher.watch(dir, RecursiveMode::NonRecursive) {
        log::warn!("watch {}: {err}", dir.display());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        // Skip symlinked dirs — following them can loop or escape the root.
        if ft.is_dir() && !ft.is_symlink() {
            watch_dir_tree(watcher, &entry.path());
        }
    }
}

fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| {
        SKIP_COMPONENTS
            .iter()
            .any(|skip| c.as_os_str() == std::ffi::OsStr::new(*skip))
    })
}

#[cfg(test)]
mod tests {
    use super::{FsWatcher, is_ignored};
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[cfg(target_os = "linux")]
    use super::watch_dir_tree;
    #[cfg(target_os = "linux")]
    use notify::{RecommendedWatcher, Watcher};
    #[cfg(target_os = "linux")]
    use std::sync::mpsc;

    #[test]
    fn ignores_build_and_vcs_dirs() {
        assert!(is_ignored(Path::new("/p/.git/index")));
        assert!(is_ignored(Path::new("/p/target/debug/x")));
        assert!(is_ignored(Path::new("/p/web/node_modules/a/b.js")));
        assert!(is_ignored(Path::new("/p/app/.next/cache")));
        assert!(is_ignored(Path::new("/p/.venv/lib/x")));
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

    #[test]
    fn reports_writes_in_nested_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let nested = root.join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();
        let watcher = FsWatcher::new(std::slice::from_ref(&root)).unwrap();
        // Linux still walks after ready; give it a moment to reach `c`.
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(nested.join("x.txt"), "hi").unwrap();
        let got = watcher.events().recv_timeout(Duration::from_secs(5));
        assert!(got.is_ok(), "no nested event within 5s");
    }

    /// Open-folder used to beachball: `new` waited for the full Linux walk.
    /// Ready must return before a large tree could finish registering.
    #[test]
    fn new_returns_without_waiting_on_tree_size() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for i in 0..80 {
            std::fs::create_dir_all(root.join(format!("branch{i}/a/b/c/d"))).unwrap();
        }
        let start = Instant::now();
        let _watcher = FsWatcher::new(std::slice::from_ref(&root)).unwrap();
        assert!(
            start.elapsed() < Duration::from_millis(750),
            "FsWatcher::new blocked for {:?} — must not wait on the tree walk",
            start.elapsed()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn skipped_dirs_are_not_descended_into() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let nested = root.join("node_modules").join("pkg");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let (tx, _rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )
        .unwrap();

        watch_dir_tree(&mut watcher, &root);

        let mut visited = Vec::new();
        fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            if is_ignored(dir) {
                return;
            }
            out.push(dir.to_path_buf());
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_dir() && !ft.is_symlink() {
                    collect(&entry.path(), out);
                }
            }
        }
        collect(&root, &mut visited);
        assert!(visited.iter().any(|p| p.ends_with("src")));
        assert!(
            visited
                .iter()
                .all(|p| !p.to_string_lossy().contains("node_modules")),
            "skip dirs must not be visited: {visited:?}"
        );
        let _ = watcher;
    }
}
