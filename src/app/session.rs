//! Persist/restore workspace session and window geometry.
use super::*;
use super::tab::TabBody;
use crate::acp;

impl IsengardApp {
    pub(super) fn restart_terminal_for_workspace(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Drop shells from the previous workspace; `restore_session` respawns
        // saved tabs for the new key (if any).
        self.terminal.update(cx, |term, cx| {
            term.clear_sessions(cx);
            term.hide(cx);
        });
    }

    /// Debounced write of window size/position into `config.json`.
    pub(super) fn schedule_persist_window_geometry(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.window_geometry_generation = self.window_geometry_generation.wrapping_add(1);
        let geometry_gen = self.window_geometry_generation;
        // Capture geometry now — by the time the timer fires the Window borrow is gone.
        let snapshot = window_geometry_snapshot(window);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            this.update(cx, |this, _| {
                if this.window_geometry_generation != geometry_gen {
                    return;
                }
                if this.apply_window_geometry_snapshot(snapshot) {
                    this.config.save();
                }
            })
            .ok()
        })
        .detach();
    }

    pub(super) fn persist_window_geometry(&mut self, window: &Window) {
        if self.apply_window_geometry_snapshot(window_geometry_snapshot(window)) {
            self.config.save();
        }
    }

    pub(super) fn apply_window_geometry_snapshot(&mut self, snap: WindowGeometrySnapshot) -> bool {
        // Skip fullscreen: restore size is already in Maximized/Windowed variants.
        if snap.fullscreen {
            return false;
        }
        self.config.set_window_geometry(
            snap.width,
            snap.height,
            snap.x,
            snap.y,
            snap.maximized,
        )
    }

    /// Debounced session write (dirty buffers, search, git UI, layout).
    pub(super) fn schedule_persist_session(&mut self, cx: &mut Context<Self>) {
        if self.restoring_session {
            return;
        }
        self.session_persist_generation = self.session_persist_generation.wrapping_add(1);
        let persist_gen = self.session_persist_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            this.update(cx, |this, cx| {
                if this.session_persist_generation != persist_gen {
                    return;
                }
                this.persist_session(cx);
            })
            .ok()
        })
        .detach();
    }

    /// Writes tabs + layout + sidebar strip + search + git UI + dirty text +
    /// terminal layout/cwds under this workspace's key.
    pub(super) fn persist_session(&mut self, cx: &App) {
        if self.restoring_session {
            return;
        }
        let Some(key) = self.workspace.session_key() else {
            return;
        };
        let open_files = self
            .tabs
            .iter()
            .filter_map(|tab| session_open_entry(tab, cx))
            .collect();
        let terminal = self.terminal.read(cx);
        let active = self.tabs.active().map(|t| t.path.clone());
        let mut session = WorkspaceSession {
            open_files,
            active,
            expanded: self.file_tree.expanded_paths(),
            terminal_cwds: terminal.session_cwds(),
            terminal_active: terminal.active_index(),
            terminal_visible: terminal.is_visible(),
            sidebar_width: self.sidebar_width,
            sidebar_visible: self.sidebar_visible,
            sidebar_panel: self.sidebar_view.into(),
            terminal_height: self.terminal_height,
            agent_visible: self.agent_visible,
            agent_width: self.agent_width,
            agent: None,
            agent_chats: Vec::new(),
            agent_active: None,
            agent_prefs: Default::default(),
            agent_slots: Default::default(),
            search: self.search.read(cx).session_state(cx),
            git_repos: self.git.read(cx).snapshot_session_ui(cx),
        };
        // Keep other agents' chat indexes + remembered agent; only refresh the
        // active slot (and the remembered id when not on the picker).
        if let Some(existing) = self.config.session(&key) {
            session.agent = existing.agent.clone();
            session.agent_chats = existing.agent_chats.clone();
            session.agent_active = existing.agent_active.clone();
            session.agent_prefs = existing.agent_prefs.clone();
            session.agent_slots = existing.agent_slots.clone();
        }
        if let Some(agent) = &self.agent {
            let agent = agent.read(cx);
            if !agent.is_awaiting_pick() {
                let slot = acp::agent_slot_id(agent.settings());
                session.agent = Some(slot.clone());
                session.set_agent_slot(
                    &slot,
                    crate::config::AgentSlotSession {
                        chats: agent.chats().to_vec(),
                        active: agent.active_chat().map(str::to_owned),
                        prefs: agent.prefs().clone(),
                    },
                );
            }
        }
        self.config.put_session(key, session);
        self.config.save();
    }

    pub(super) fn reset_layout_defaults(&mut self) {
        self.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
        self.sidebar_visible = true;
        self.sidebar_view = SidebarView::Files;
        self.terminal_height = DEFAULT_TERMINAL_HEIGHT;
        self.agent_visible = false;
        self.agent_width = DEFAULT_AGENT_WIDTH;
    }

    /// Reopens saved tabs (with cursor/scroll), expands dirs, and restores terminals.
    pub(super) fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.workspace.session_key() else {
            self.reset_layout_defaults();
            return;
        };
        let Some(session) = self.config.session(&key).cloned() else {
            self.reset_layout_defaults();
            return;
        };

        self.restoring_session = true;
        self.sidebar_width = if session.sidebar_width >= 100. {
            session.sidebar_width
        } else {
            DEFAULT_SIDEBAR_WIDTH
        };
        self.sidebar_visible = session.sidebar_visible;
        self.sidebar_view = session.sidebar_panel.into();
        self.terminal_height = if session.terminal_height >= 100. {
            session.terminal_height
        } else {
            DEFAULT_TERMINAL_HEIGHT
        };
        self.agent_width = if session.agent_width >= 200. {
            session.agent_width
        } else {
            DEFAULT_AGENT_WIDTH
        };
        self.agent_visible = session.agent_visible && !self.workspace.folders().is_empty();
        if self.agent_visible
            && let Some(agent) = &self.agent
        {
            agent.update(cx, |agent, cx| agent.ensure_started(cx));
        }
        self.file_tree.expand_paths(&session.expanded, cx);
        let git_statuses = session
            .open_files
            .iter()
            .any(|e| document::is_git_preview(e.path()))
            .then(|| load_workspace_status(self.workspace.folders()));
        for entry in &session.open_files {
            let path = entry.path();
            if document::is_git_preview(path) {
                if self.restore_git_preview_tab(path, git_statuses.as_deref(), window, cx) {
                    self.apply_diff_view(path, &entry.view(), window, cx);
                }
                continue;
            }
            if document::is_untitled(path) {
                self.restore_untitled_from_session(entry, window, cx);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            self.open_file(path, true, window, cx);
            self.apply_file_view(path, &entry.view(), window, cx);
            self.apply_unsaved_text(path, &entry.view(), window, cx);
        }
        if let Some(active) = &session.active
            && let Some(ix) = self.tabs.position(|t| t.path == *active)
        {
            self.activate_tab(ix, window, cx);
            if let Some(entry) = session.open_files.iter().find(|e| e.path() == active.as_path())
            {
                // Re-apply scroll after activate; cursor move-to can shift the viewport.
                if document::is_git_preview(active) {
                    self.apply_diff_view(active, &entry.view(), window, cx);
                } else {
                    self.apply_file_view(active, &entry.view(), window, cx);
                }
            }
        }
        self.terminal.update(cx, |term, cx| {
            term.restore_sessions(
                &session.terminal_cwds,
                session.terminal_active,
                session.terminal_visible,
                window,
                cx,
            );
        });
        if !session.search.is_empty() {
            let query = self.search.update(cx, |panel, cx| {
                panel.restore_session_state(&session.search, window, cx);
                panel.query(cx)
            });
            self.schedule_search(query, cx);
        }
        self.git.update(cx, |git, cx| {
            git.restore_session_ui(session.git_repos.clone(), window, cx);
        });
        self.restoring_session = false;
        self.persist_session(cx);
    }

    fn restore_untitled_from_session(
        &mut self,
        entry: &OpenFileEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = entry.view();
        let text = view.unsaved_text.clone().unwrap_or_default();
        let path = if self.tabs.iter().any(|t| t.path == *entry.path()) {
            let mut id = 1u64;
            loop {
                let candidate = document::untitled_path(id);
                if !self.tabs.iter().any(|t| t.path == candidate) {
                    break candidate;
                }
                id += 1;
            }
        } else {
            entry.path().to_path_buf()
        };
        let restored_path = path.clone();
        self.open_untitled_at(path, view.untitled_save_dir.clone(), &text, window, cx);
        self.apply_file_view(&restored_path, &view, window, cx);
    }

    /// Apply dirty text after open (disk load already set `saved` baseline).
    fn apply_unsaved_text(
        &mut self,
        path: &Path,
        view: &FileViewState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = view.unsaved_text.as_ref() else {
            return;
        };
        let Some(ix) = self.tabs.position(|t| t.path == path) else {
            return;
        };
        let Some(state) = self.tabs.get(ix).and_then(|t| t.editor_state().cloned()) else {
            return;
        };
        state.update(cx, |state, cx| {
            state.set_value(text.clone(), window, cx);
        });
        if let Some(tab) = self.tabs.get_mut(ix) {
            tab.refresh_modified(cx);
        }
        self.lsp.did_change(path, text);
        cx.notify();
    }

    pub(super) fn apply_file_view(
        &self,
        path: &Path,
        view: &FileViewState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self
            .tabs
            .iter()
            .find(|t| t.path == path)
            .and_then(|t| t.editor_state())
            .cloned()
        else {
            return;
        };
        Self::apply_editor_view(&state, view, window, cx);
    }

    /// Restore cursor/scroll on both Diff panes (scroll is locked).
    pub(super) fn apply_diff_view(
        &self,
        path: &Path,
        view: &FileViewState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((old, new)) = self.tabs.iter().find(|t| t.path == path).and_then(|t| {
            match &t.body {
                TabBody::Diff { old, new, .. } => Some((old.clone(), new.clone())),
                _ => None,
            }
        }) else {
            return;
        };
        Self::apply_editor_view(&old, view, window, cx);
        Self::apply_editor_view(&new, view, window, cx);
    }

    pub(super) fn apply_editor_view(
        state: &Entity<EditorState>,
        view: &FileViewState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        state.update(cx, |state, cx| {
            // gpui prelude's `Position` is CSS; the editor uses LSP Position from Kit.
            state.set_cursor_position(
                gpui_kit::component::input::Position {
                    line: view.cursor_line,
                    character: view.cursor_character,
                },
                window,
                cx,
            );
            state.set_scroll_offset(point(px(view.scroll_x), px(view.scroll_y)), cx);
        });
    }

    /// Reopen a saved `isengard-diff:` / `isengard-conflict:` tab if still dirty.
    ///
    /// Returns false when the path is no longer a change (resolved / gone).
    pub(super) fn restore_git_preview_tab(
        &mut self,
        virtual_path: &Path,
        statuses: Option<&[RepoStatus]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(file) = document::git_preview_worktree_path(virtual_path) else {
            return false;
        };
        let Some(statuses) = statuses else {
            return false;
        };
        let Some((repo, change)) = change_at_path(statuses, &file) else {
            return false;
        };
        if change.status == 'U' {
            self.open_git_conflict(repo, file, change.rel_path, false, window, cx);
            return true;
        }
        // Conflict cleared but still dirty → change diff; otherwise reopen change.
        self.open_git_diff(
            repo,
            file,
            change.rel_path,
            change.status,
            change.old_rel_path,
            false,
            window,
            cx,
        );
        true
    }

    pub(super) fn remember_sidebar_width(&mut self, state: &Entity<ResizableState>, cx: &App) {
        let sizes = state.read(cx).sizes();
        if let Some(&w) = sizes.first() {
            let w = f32::from(w).clamp(100., 800.);
            if (self.sidebar_width - w).abs() > 0.5 {
                self.sidebar_width = w;
                self.persist_session(cx);
            }
        }
    }

    pub(super) fn remember_agent_width(&mut self, state: &Entity<ResizableState>, cx: &App) {
        let sizes = state.read(cx).sizes();
        if let Some(&w) = sizes.get(1) {
            let w = f32::from(w).clamp(200., 1200.);
            if (self.agent_width - w).abs() > 0.5 {
                self.agent_width = w;
                self.persist_session(cx);
            }
        }
    }

    pub(super) fn remember_terminal_height(&mut self, state: &Entity<ResizableState>, cx: &App) {
        let sizes = state.read(cx).sizes();
        if let Some(&h) = sizes.get(1) {
            let h = f32::from(h).clamp(100., 800.);
            if (self.terminal_height - h).abs() > 0.5 {
                self.terminal_height = h;
                self.persist_session(cx);
            }
        }
    }
}

fn session_open_entry(tab: &super::tab::EditorTab, cx: &App) -> Option<OpenFileEntry> {
    // Diff panes: prefer the left editor's cursor/scroll (same as before).
    let mut view = tab
        .editor_state()
        .or(match &tab.body {
            super::tab::TabBody::Diff { old, .. } => Some(old),
            _ => None,
        })
        .map(|state| {
            let state = state.read(cx);
            let cursor = state.cursor_position();
            let scroll = state.scroll_offset();
            FileViewState {
                cursor_line: cursor.line,
                cursor_character: cursor.character,
                scroll_x: scroll.x.into(),
                scroll_y: scroll.y.into(),
                unsaved_text: None,
                untitled_save_dir: None,
            }
        })
        .unwrap_or_default();

    if document::is_untitled(&tab.path) {
        let text = tab
            .editor_state()
            .map(|state| state.read(cx).value().to_string())
            .unwrap_or_default();
        if text.len() > MAX_SESSION_UNSAVED_BYTES {
            return None;
        }
        view.unsaved_text = Some(text);
        view.untitled_save_dir = tab.untitled_save_dir.clone();
        return Some(OpenFileEntry::with_view(tab.path.clone(), view));
    }

    if document::is_git_preview(&tab.path) {
        return Some(OpenFileEntry::with_view(tab.path.clone(), view));
    }

    if tab.is_modified
        && let Some(state) = tab.editor_state()
    {
        let text = state.read(cx).value().to_string();
        if text.len() <= MAX_SESSION_UNSAVED_BYTES {
            view.unsaved_text = Some(text);
        }
    }
    Some(OpenFileEntry::with_view(tab.path.clone(), view))
}
