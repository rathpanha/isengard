//! Disk changes under the workspace roots: reload clean tabs, refresh the
//! tree and Source Control. Unsaved edits are never overwritten.
use std::collections::HashSet;

use super::tab::TabBody;
use super::*;
use crate::watch::{FsChange, FsWatcher};

/// Wait this long after the first event so a burst (checkout, formatter,
/// agent turn) becomes one refresh.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// What a changed path means for an open tab.
enum TabAction {
    /// Disk matches the buffer's saved text (our own save, or no real change).
    InSync,
    /// Clean buffer, different disk text.
    Reload,
    /// Unsaved edits and different disk text.
    Conflict,
    /// File gone from disk; buffer had no unsaved edits.
    GoneClean,
    /// File gone from disk; keep the dirty buffer and warn once.
    GoneDirty,
}

impl IsengardApp {
    /// (Re)start watching the current roots. Called from `workspace_changed`.
    pub(super) fn restart_fs_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.fs_watch = None;
        self.fs_watch_gen = self.fs_watch_gen.wrapping_add(1);
        self.disk_conflicts.clear();
        // Tests drive the disk themselves; a live OS watcher would race them.
        if cfg!(test) || self.workspace.is_empty() {
            return;
        }
        let watcher = match FsWatcher::new(self.workspace.folders()) {
            Ok(watcher) => watcher,
            Err(err) => {
                log::warn!("file watching unavailable: {err}");
                return;
            }
        };
        let events = watcher.events();
        let generation = self.fs_watch_gen;
        self.fs_watch = Some(watcher);
        cx.spawn_in(window, async move |this, window| {
            // Async recv: no pool thread parked per wait.
            while let Ok(first) = events.recv_async().await {
                window.background_executor().timer(DEBOUNCE).await;
                let mut batch = vec![first];
                batch.extend(events.try_iter());
                let alive = this
                    .update_in(window, |this, window, cx| {
                        if this.fs_watch_gen == generation {
                            this.apply_fs_changes(batch, window, cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Re-check every on-disk text tab against the filesystem (focus / wake).
    /// Covers changes the OS watcher missed while the app was inactive.
    pub(super) fn reconcile_open_tabs_with_disk(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let batch: Vec<FsChange> = self
            .tabs
            .iter()
            .filter(|t| {
                matches!(t.body, TabBody::Text { .. })
                    && !document::is_untitled(&t.path)
                    && !document::is_diff_preview(&t.path)
                    && !document::is_conflict_preview(&t.path)
            })
            .map(|t| FsChange {
                path: t.path.clone(),
                structural: false,
            })
            .collect();
        if !batch.is_empty() {
            self.apply_fs_changes(batch, window, cx);
        }
    }

    pub(super) fn apply_fs_changes(
        &mut self,
        batch: Vec<FsChange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: HashSet<PathBuf> = batch.iter().map(|c| c.path.clone()).collect();
        let mut close: Vec<PathBuf> = Vec::new();

        for path in &paths {
            match self.tab_action_for(path) {
                None | Some(TabAction::InSync) => {
                    self.disk_conflicts.remove(path);
                }
                Some(TabAction::Reload) => {
                    self.disk_conflicts.remove(path);
                    self.reload_open_file_from_disk(path, window, cx);
                }
                Some(TabAction::Conflict) => {
                    // One warning per divergence, not per event.
                    if self.disk_conflicts.insert(path.clone()) {
                        notify_warning(
                            format!(
                                "{} changed on disk — kept your unsaved edits",
                                document::file_name(path)
                            ),
                            window,
                            cx,
                        );
                    }
                }
                Some(TabAction::GoneClean) => {
                    self.disk_conflicts.remove(path);
                    close.push(path.clone());
                }
                Some(TabAction::GoneDirty) => {
                    if self.disk_conflicts.insert(path.clone()) {
                        notify_warning(
                            format!(
                                "{} was deleted on disk — kept your unsaved edits",
                                document::file_name(path)
                            ),
                            window,
                            cx,
                        );
                    }
                }
            }
        }

        if !close.is_empty() {
            self.close_tabs(&close, window, cx);
        }

        // Directory listings: only parents of created / removed / renamed paths.
        let dirs: HashSet<&Path> = batch
            .iter()
            .filter(|c| c.structural)
            .filter_map(|c| c.path.parent())
            .collect();
        self.file_tree.refresh_loaded_dirs(dirs, cx);

        self.git.update(cx, |git, cx| git.refresh(window, cx));
        cx.notify();
    }

    /// `None` when `path` is not an open text tab (or is unreadable while still
    /// present: the buffer stays as is).
    fn tab_action_for(&self, path: &Path) -> Option<TabAction> {
        if document::is_untitled(path)
            || document::is_diff_preview(path)
            || document::is_conflict_preview(path)
        {
            return None;
        }
        let tab = self.tabs.iter().find(|t| t.path == path)?;
        let TabBody::Text { saved, .. } = &tab.body else {
            return None;
        };
        if !path.exists() {
            return Some(if tab.is_modified {
                TabAction::GoneDirty
            } else {
                TabAction::GoneClean
            });
        }
        let disk = document::read_text(path).ok()?;
        Some(if disk == *saved {
            TabAction::InSync
        } else if tab.is_modified {
            TabAction::Conflict
        } else {
            TabAction::Reload
        })
    }
}
