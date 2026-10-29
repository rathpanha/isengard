//! Open / save / close tabs, untitled buffers, font size, rendered preview.
use super::*;
use super::tab::{EditorTab, TabBody};

impl IsengardApp {
    pub(super) fn open_file(
        &mut self,
        path: &Path,
        permanent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.tabs.position(|t| t.path == path) {
            if permanent {
                self.pin_tab(ix, cx);
            }
            if self.tabs.activate(ix) {
                self.file_tree.select_path(path, cx);
                self.persist_session(cx);
                cx.notify();
            }
            self.focus_after_tree_open(permanent, window, cx);
            return;
        }

        if !permanent {
            self.retire_preview_tab(window, cx);
        }

        if document::is_image(path) {
            if !path.is_file() {
                return notify_error(
                    format!("Image not found: {}", path.display()),
                    window,
                    cx,
                );
            }
            self.tabs.push(EditorTab {
                path: path.to_path_buf(),
                body: TabBody::Image,
                is_modified: false,
                preview: !permanent,
                untitled_save_dir: None,
            });
            self.file_tree.select_path(path, cx);
            self.focus_after_tree_open(permanent, window, cx);
            self.persist_session(cx);
            self.refresh_menus(cx);
            cx.notify();
            return;
        }

        let text = match document::read_text(path) {
            Ok(text) => text,
            Err(err) => return notify_error(format!("Could not open file: {err:#}"), window, cx),
        };

        let language = Language::from_path(path);
        let indent = self.editor_tab_size(&text);
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .line_number(true)
                .indent_guides(true)
                .soft_wrap(false)
                .tab_size(indent)
                .default_value(text)
        });
        let subscriptions = vec![
            cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let id = state.entity_id();
                    let path = this
                        .tabs
                        .iter()
                        .find(|t| t.editor_state().is_some_and(|s| s.entity_id() == id))
                        .map(|t| t.path.clone());
                    if let Some(tab) = this.tabs.find_mut(|t| {
                        t.editor_state()
                            .is_some_and(|s| s.entity_id() == id)
                    }) {
                        tab.refresh_modified(cx);
                        // Editing pins a preview tab (VS Code behaviour).
                        tab.preview = false;
                    }
                    if let Some(path) = path {
                        let text = state.read(cx).value().to_string();
                        this.lsp.did_change(&path, &text);
                    }
                    // Buffer changed: occurrences may move even if range stays.
                    this.selection_match_last_range.remove(&state.entity_id());
                    this.schedule_selection_matches(&state, cx);
                    this.schedule_persist_session(cx);
                    cx.notify();
                }
            }),
            // Cursor/selection → status bar Ln/Col + selection-match highlights.
            // Scroll/blink also notify the editor — ignore those no-ops.
            cx.observe(&state, |this, state, cx| {
                this.on_editor_state_observed(&state, cx);
            }),
            // Kit binds Tab → indent; accept the completion menu instead.
            install_tab_accepts_completion(&state, cx),
        ];

        self.ensure_selection_match_collection(&state, cx);

        let saved = state.read(cx).text().clone();
        self.tabs.push(EditorTab {
            path: path.to_path_buf(),
            body: TabBody::Text {
                language,
                state: state.clone(),
                saved,
                indent,
                rendered_preview: false,
                _subscriptions: subscriptions,
            },
            is_modified: false,
            preview: !permanent,
            untitled_save_dir: None,
        });
        self.lsp_attach_tab(path, language, &state, window, cx);
        self.file_tree.select_path(path, cx);
        self.focus_after_tree_open(permanent, window, cx);
        self.persist_session(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// After Accept Current/Incoming (or similar), refresh an open text tab from disk.
    /// Keeps the prior selection and scroll (clamped); undo history still resets
    /// because `EditorState::set_value` clears it.
    pub(super) fn reload_open_file_from_disk(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.tabs.position(|t| t.path == path) else {
            return;
        };
        let Some(state) = self.tabs.get(ix).and_then(|t| t.editor_state().cloned()) else {
            return;
        };
        let text = match document::read_text(path) {
            Ok(text) => text,
            Err(err) => {
                return notify_error(format!("Could not reload file: {err:#}"), window, cx);
            }
        };
        state.update(cx, |state, cx| {
            let range = state.selected_range();
            let scroll = state.scroll_offset();
            state.set_value(text, window, cx);
            let len = state.text().len();
            let start = range.start.min(len);
            let end = range.end.min(len);
            state.set_selected_range(start..end, cx);
            state.set_scroll_offset(scroll, cx);
        });
        if let Some(tab) = self.tabs.get_mut(ix) {
            tab.mark_saved(cx);
        }
        cx.notify();
    }

    pub(super) fn focus_file_tree(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree
            .state()
            .update(cx, |state, cx| state.focus(window, cx));
    }

    pub(super) fn focus_after_tree_open(
        &self,
        permanent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if permanent {
            self.focus_active_editor(window, cx);
        } else {
            self.focus_file_tree(window, cx);
        }
    }

    pub(super) fn pin_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(ix)
            && tab.preview
        {
            tab.preview = false;
            cx.notify();
        }
    }

    /// Drops a clean preview tab, or pins it if dirty, before opening another preview.
    pub(super) fn retire_preview_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.position(|t| t.preview) else {
            return;
        };
        let dirty = self.tabs.get(ix).is_some_and(|t| t.is_modified);
        if dirty {
            self.pin_tab(ix, cx);
        } else if let Some(path) = self.tabs.get(ix).map(|t| t.path.clone()) {
            self.close_tab(&path, window, cx);
        }
    }

    pub(super) fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(ix) {
            if let Some(path) = self.tabs.active().map(|t| t.path.clone()) {
                self.file_tree.select_path(&path, cx);
            }
            self.sync_git_active_from_tab(cx);
            self.focus_active_editor(window, cx);
            self.persist_session(cx);
            cx.notify();
        }
    }

    /// Drop `dragged` before the tab at `before` (left-border target).
    pub(super) fn drop_editor_tab_before(
        &mut self,
        dragged: &Path,
        before: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.tabs.position(|t| t.path == *dragged) else {
            return;
        };
        let Some(before_ix) = self.tabs.position(|t| t.path == *before) else {
            return;
        };
        let Some((from, to)) = tab_order::reorder_for_drop_before(from, before_ix) else {
            return;
        };
        self.tabs.reorder(from, to);
        self.activate_tab(to, window, cx);
    }

    /// Drop `dragged` at the end of the strip.
    pub(super) fn drop_editor_tab_at_end(
        &mut self,
        dragged: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.tabs.position(|t| t.path == *dragged) else {
            return;
        };
        let to = self.tabs.len().saturating_sub(1);
        if from == to {
            return;
        }
        self.tabs.reorder(from, to);
        self.activate_tab(to, window, cx);
    }

    /// Highlight the Source Control row for the active diff/conflict tab.
    pub(super) fn sync_git_active_from_tab(&mut self, cx: &mut Context<Self>) {
        let path = self
            .tabs
            .active()
            .and_then(|t| document::git_preview_worktree_path(&t.path));
        self.git.update(cx, |panel, cx| {
            panel.set_active_path(path, cx);
        });
    }

    pub(super) fn focus_active_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.active().and_then(|t| t.editor_state()) {
            Some(state) => state.update(cx, |state, cx| state.focus(window, cx)),
            None => self.focus_handle.focus(window, cx),
        }
    }

    /// After LSP catalog / install dialog chrome: park focus on the editor
    /// (or the app handle) so native File menu actions keep a live target.
    pub(super) fn restore_focus_after_lsp_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lsp_catalog_open = false;
        window.dismiss_tooltip(cx);
        self.focus_active_editor(window, cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Writes an on-disk tab. Untitled buffers must go through [`Self::request_save`].
    pub(super) fn save_tab(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        if document::is_untitled(path) {
            anyhow::bail!("untitled buffer needs a path");
        }
        let (saved_path, text) = {
            let Some(tab) = self.tabs.find_mut(|t| t.path == path) else {
                return Ok(());
            };
            let Some(state) = tab.editor_state() else {
                return Ok(());
            };
            let text = state.read(cx).value().to_string();
            let saved_path = tab.path.clone();
            document::write_text(&saved_path, &text)?;
            tab.mark_saved(cx);
            (saved_path, text)
        };
        self.lsp.did_save(&saved_path, Some(&text));
        self.reload_lsp_settings_if_config(&saved_path, window, cx);
        cx.notify();
        Ok(())
    }

    /// Saves `path`, prompting for a location when the buffer is untitled.
    /// `after` runs with the final on-disk path on success.
    /// `abort_message` toasts when the user cancels an untitled save dialog.
    pub(super) fn request_save(
        &mut self,
        path: PathBuf,
        after: impl Fn(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
        abort_message: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if document::is_untitled(&path) {
            self.prompt_save_untitled(path, Rc::new(after), None, abort_message, window, cx);
            return;
        }
        if self.wants_format_on_save(&path) {
            self.format_then_save(path, Rc::new(after), window, cx);
            return;
        }
        match self.save_tab(&path, window, cx) {
            Ok(()) => after(self, path, window, cx),
            Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
        }
    }

    /// `on_write_fail`: when set (Save All), a write error after the dialog
    /// continues the queue instead of stopping; cancel still aborts via
    /// `abort_message`.
    pub(super) fn prompt_save_untitled(
        &mut self,
        old_path: PathBuf,
        after: AfterSave,
        on_write_fail: Option<AfterSaveFail>,
        abort_message: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.tabs.iter().find(|t| t.path == old_path);
        let dir = tab
            .and_then(|t| t.untitled_save_dir.clone())
            .or_else(|| self.new_file_directory(cx))
            .or_else(|| self.workspace.folders().first().cloned())
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let suggested = if tab.is_some_and(|t| t.untitled_save_dir.is_some()) {
            document::unique_new_file_path(&dir)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("{}.txt", document::tab_label(&old_path)))
        } else {
            format!("{}.txt", document::tab_label(&old_path))
        };
        let prompt = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn_in(window, async move |this, window| {
            let new_path = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => {
                    if let Some(msg) = abort_message {
                        this.update_in(window, |_, window, cx| {
                            notify_error(msg, window, cx);
                        })
                        .ok();
                    }
                    return;
                }
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Save dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Save dialog was interrupted", window, cx);
                    })
                    .ok();
                    return;
                }
            };
            this.update_in(window, |this, window, cx| {
                match this.materialize_untitled(&old_path, new_path.clone(), window, cx) {
                    Ok(()) => after(this, new_path, window, cx),
                    Err(err) => {
                        if let Some(on_fail) = on_write_fail {
                            on_fail(this, err, window, cx);
                        } else {
                            notify_error(format!("Save failed: {err:#}"), window, cx);
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Writes an untitled buffer to `new_path` and rebinds the tab.
    pub(super) fn materialize_untitled(
        &mut self,
        old_path: &Path,
        new_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.write_tab_to_path(old_path, new_path, window, cx)
    }

    /// Write the text tab at `old_path` to `new_path` and rebind the tab
    /// (untitled Save and Save As). Leaves the original on-disk file alone
    /// when `old_path` was already a real path.
    pub(super) fn write_tab_to_path(
        &mut self,
        old_path: &Path,
        new_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let text = {
            let Some(tab) = self.tabs.iter().find(|t| t.path == *old_path) else {
                anyhow::bail!("tab was closed");
            };
            let Some(state) = tab.editor_state() else {
                anyhow::bail!("not a text tab");
            };
            state.read(cx).value()
        };
        document::write_text(&new_path, &text)?;
        self.rebind_tab_to_path(old_path, new_path, window, cx)
    }

    /// After the bytes are on disk: rematerialize the tab onto `new_path`.
    fn rebind_tab_to_path(
        &mut self,
        old_path: &Path,
        new_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        if old_path == new_path.as_path() {
            // Same path (Save As → overwrite self): just mark saved.
            if let Some(tab) = self.tabs.find_mut(|t| t.path == *old_path) {
                tab.mark_saved(cx);
            }
            self.lsp.did_save(old_path, None);
            self.persist_session(cx);
            cx.notify();
            return Ok(());
        }

        if let Some(existing) = self.tabs.position(|t| t.path == new_path) {
            if let Some(old_ix) = self.tabs.position(|t| t.path == *old_path) {
                self.tabs.remove(old_ix);
            }
            let ix = self
                .tabs
                .position(|t| t.path == new_path)
                .unwrap_or(existing);
            self.activate_tab(ix, window, cx);
        } else {
            let attach = if let Some(tab) = self.tabs.find_mut(|t| t.path == *old_path) {
                tab.path = new_path.clone();
                tab.mark_saved(cx);
                tab.untitled_save_dir = None;
                if let TabBody::Text {
                    language, state, ..
                } = &mut tab.body
                {
                    let lang = Language::from_path(&new_path);
                    *language = lang;
                    let name = lang.highlighter_name();
                    state.update(cx, |state, cx| {
                        state.set_highlighter(name, cx);
                    });
                    Some((lang, state.clone()))
                } else {
                    None
                }
            } else {
                None
            };
            self.lsp.did_close(old_path);
            if let Some((language, state)) = attach {
                self.lsp_attach_tab(&new_path, language, &state, window, cx);
            }
        }

        if self.workspace.is_empty()
            && let Some(parent) = new_path.parent().filter(|p| p.is_dir())
        {
            self.add_folders(&[parent.to_path_buf()], window, cx);
        } else if let Some(parent) = new_path.parent() {
            self.file_tree.refresh_dir(parent, cx);
        }
        self.file_tree.select_path(&new_path, cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    /// Save As to a concrete path (UI tests / dialog callback).
    pub(super) fn save_as_to(
        &mut self,
        old_path: &Path,
        new_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.write_tab_to_path(old_path, new_path, window, cx)
    }

    /// Saves every dirty tab (prompts for each untitled), then runs `after`.
    /// `abort_message` toasts when an untitled save dialog is cancelled mid-queue.
    pub(super) fn request_save_all(
        &mut self,
        after: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        abort_message: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dirty: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.is_modified)
            .map(|t| t.path.clone())
            .collect();
        self.request_save_queue(dirty, Vec::new(), Rc::new(after), abort_message, window, cx);
    }

    /// Saves `paths` in order. Write failures (on-disk or untitled after the
    /// Save dialog) are collected and the queue moves on; any failure still
    /// blocks `after` (close / quit / switch) and is reported in one toast.
    /// Cancelling an untitled Save dialog still aborts the queue.
    pub(super) fn request_save_queue(
        &mut self,
        mut paths: Vec<PathBuf>,
        mut failures: Vec<String>,
        after: AfterSaveAll,
        abort_message: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        while let Some(path) = paths.first().cloned() {
            let still_dirty = self
                .tabs
                .iter()
                .any(|t| t.path == path && t.is_modified);
            if !still_dirty {
                paths.remove(0);
                continue;
            }
            if document::is_untitled(&path) {
                let rest = paths[1..].to_vec();
                let after_ok = after.clone();
                let after_fail = after.clone();
                let failures_ok = failures.clone();
                let failures_fail = failures.clone();
                let rest_ok = rest.clone();
                let rest_fail = rest;
                let label = document::tab_label(&path);
                self.prompt_save_untitled(
                    path,
                    Rc::new(move |this, _, window, cx| {
                        // materialize already succeeded inside prompt_save_untitled.
                        this.request_save_queue(
                            rest_ok.clone(),
                            failures_ok.clone(),
                            after_ok.clone(),
                            abort_message,
                            window,
                            cx,
                        );
                    }),
                    Some(Rc::new(move |this, err, window, cx| {
                        let mut failures = failures_fail.clone();
                        failures.push(format!("{label}: {err:#}"));
                        this.request_save_queue(
                            rest_fail.clone(),
                            failures,
                            after_fail.clone(),
                            abort_message,
                            window,
                            cx,
                        );
                    })),
                    abort_message,
                    window,
                    cx,
                );
                return;
            }
            if self.wants_format_on_save(&path) {
                let rest = paths[1..].to_vec();
                let after = after.clone();
                self.format_then_save(
                    path,
                    Rc::new(move |this, _, window, cx| {
                        this.request_save_queue(
                            rest.clone(),
                            failures.clone(),
                            after.clone(),
                            abort_message,
                            window,
                            cx,
                        );
                    }),
                    window,
                    cx,
                );
                return;
            }
            if let Err(err) = self.save_tab(&path, window, cx) {
                failures.push(format!("{}: {err:#}", document::tab_label(&path)));
            }
            paths.remove(0);
        }
        if !failures.is_empty() {
            return notify_error(save_failures_message(&failures), window, cx);
        }
        after(self, window, cx);
    }

    pub(super) fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.is_modified)
    }

    /// Closes the tab for `path`, asking first if it has unsaved changes.
    pub(super) fn request_close(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.path == path) else {
            return;
        };
        if tab.is_modified {
            self.confirm_close_tab(path.to_path_buf(), window, cx);
        } else {
            self.close_tab(path, window, cx);
        }
    }

    /// Closes many tabs at once; one confirmation if any are dirty.
    pub(super) fn request_close_many(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let dirty = paths.iter().any(|path| {
            self.tabs
                .iter()
                .any(|t| t.path == *path && t.is_modified)
        });
        if !dirty {
            self.close_tabs(&paths, window, cx);
            return;
        }
        let paths_discard = paths.clone();
        self.open_choice_dialog(
            "Unsaved changes",
            "Some files have unsaved changes. Save them before closing?",
            "Don't Save",
            "Save All",
            move |this, window, cx| this.close_tabs(&paths_discard, window, cx),
            move |this, window, cx| {
                let paths = paths.clone();
                this.request_save_all(
                    move |this, window, cx| this.close_tabs(&paths, window, cx),
                    Some("Save cancelled — tabs still open"),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    pub(super) fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tabs(&[path.to_path_buf()], window, cx);
    }

    pub(super) fn close_tabs(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        // Tab × tooltips lose their trigger on remove (incl. ⌘W while hovered).
        window.dismiss_tooltip(cx);
        // Remove from the end so earlier indexes stay valid while we scan.
        let mut indexes: Vec<usize> = paths
            .iter()
            .filter_map(|path| self.tabs.position(|t| t.path == *path))
            .collect();
        indexes.sort_unstable();
        indexes.dedup();
        for ix in indexes.into_iter().rev() {
            if let Some(tab) = self.tabs.get(ix) {
                self.lsp.did_close(&tab.path);
                if let Some(state) = tab.editor_state() {
                    self.clear_selection_matches(state.entity_id(), cx);
                }
            }
            self.tabs.remove(ix);
        }
        self.sync_git_active_from_tab(cx);
        self.focus_active_editor(window, cx);
        self.persist_session(cx);
        self.refresh_menus(cx);
        cx.notify();
    }

    // ---- Dialogs -----------------------------------------------------------

    /// Opens a confirmation dialog with Cancel, a secondary choice, and the
    /// primary (safe default) choice. Choices run on the app after the dialog closes.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_choice_dialog(
        &self,
        title: &'static str,
        message: impl Into<SharedString>,
        secondary_label: &'static str,
        primary_label: &'static str,
        on_secondary: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        on_primary: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        let message: SharedString = message.into();
        let on_secondary = Rc::new(on_secondary);
        let on_primary = Rc::new(on_primary);
        window.open_dialog(cx, move |dialog, window, _| {
            let (app_secondary, app_primary) = (app.clone(), app.clone());
            let (on_secondary, on_primary) = (on_secondary.clone(), on_primary.clone());
            center_dialog(
                dialog.title(title).child(message.clone()).footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("dialog-cancel")
                                .cursor_pointer()
                                .outline()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            // Secondary is always the discard path today
                            // ("Don't Save", "Quit Without Saving").
                            Button::new("dialog-secondary")
                                .cursor_pointer()
                                .danger()
                                .outline()
                                .label(secondary_label)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = app_secondary
                                        .update(cx, |this, cx| on_secondary(this, window, cx));
                                }),
                        )
                        .child(
                            Button::new("dialog-primary")
                                .cursor_pointer()
                                .primary()
                                .label(primary_label)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = app_primary
                                        .update(cx, |this, cx| on_primary(this, window, cx));
                                }),
                        ),
                ),
                window,
            )
        });
    }

    pub(super) fn confirm_close_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path_discard = path.clone();
        self.open_choice_dialog(
            "Unsaved changes",
            format!(
                "Save changes to {} before closing?",
                document::tab_label(&path)
            ),
            "Don't Save",
            "Save",
            move |this, window, cx| this.close_tab(&path_discard, window, cx),
            move |this, window, cx| {
                this.request_save(
                    path.clone(),
                    |this, saved, window, cx| this.close_tab(&saved, window, cx),
                    Some("Save cancelled — tab still open"),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }
    pub(super) fn confirm_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.allow_quit {
            self.persist_session(cx);
            return true;
        }
        let agent_running = self
            .agent
            .as_ref()
            .is_some_and(|agent| agent.read(cx).is_turn_running());
        let busy_terminals = self.terminal.read(cx).busy_tab_count();
        if agent_running || busy_terminals > 0 {
            let message = match (agent_running, busy_terminals) {
                (true, 0) => "The agent is still running. Quitting now will stop it mid-task.".to_string(),
                (false, 1) => "A process is still running in the terminal. Quitting now will stop it.".to_string(),
                (false, n) => format!("{n} terminals have running processes. Quitting now will stop them."),
                (true, _) => "The agent and a terminal process are still running. Quitting now will stop them.".to_string(),
            };
            self.open_choice_dialog(
                "Quit Isengard?",
                message,
                "Quit Anyway",
                "Keep Working",
                |this, window, cx| {
                    if this.confirm_quit_prompts(window, cx) {
                        this.quit(cx);
                    }
                },
                |_, _, _| {},
                window,
                cx,
            );
            return false;
        }
        self.confirm_quit_prompts(window, cx)
    }

    /// Untitled-workspace and unsaved-file prompts of the quit flow.
    fn confirm_quit_prompts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // Same order as workspace switch: offer to persist an untitled
        // multi-root workspace before asking about dirty files.
        if self.workspace.is_untitled_multi_root() {
            self.open_choice_dialog(
                "Save workspace?",
                "Save the folders in this workspace to a workspace file so you can reopen it later?",
                "Don't Save",
                "Save…",
                |this, window, cx| {
                    if this.confirm_quit_unsaved(window, cx) {
                        this.quit(cx);
                    }
                },
                |this, window, cx| {
                    this.save_workspace_as(AfterWorkspaceSave::ContinueQuit, window, cx);
                },
                window,
                cx,
            );
            return false;
        }
        self.confirm_quit_unsaved(window, cx)
    }

    /// Unsaved-file half of quit (after any untitled-workspace prompt).
    pub(super) fn confirm_quit_unsaved(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.has_unsaved() {
            self.persist_session(cx);
            return true;
        }
        self.open_choice_dialog(
            "Quit Isengard?",
            "Some files have unsaved changes.",
            "Quit Without Saving",
            "Save All & Quit",
            |this, _, cx| this.quit(cx),
            |this, window, cx| {
                this.request_save_all(
                    |this, _, cx| this.quit(cx),
                    Some("Save cancelled — quit aborted"),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
        false
    }

    pub(super) fn quit(&mut self, cx: &mut Context<Self>) {
        self.persist_session(cx);
        // Don't leave servers (and their ports) behind after the window closes.
        self.terminal.update(cx, |term, _| term.terminate_all());
        self.allow_quit = true;
        if let Some(exe) = self.relaunch_exe.take() {
            self.relaunch(&exe);
        }
        cx.quit();
    }

    // ---- Actions -----------------------------------------------------------

    pub(super) fn on_new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        let save_dir = if self.workspace.is_empty() {
            None
        } else {
            match self.new_file_directory(cx) {
                Some(dir) => Some(dir),
                None => {
                    return notify_error(
                        "No folder available for a new file.",
                        window,
                        cx,
                    );
                }
            }
        };
        self.open_untitled(save_dir, window, cx);
    }

    /// In-memory buffer until Save; `save_dir` seeds the save dialog in a workspace.
    pub(super) fn open_untitled(
        &mut self,
        save_dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut id = 1u64;
        let path = loop {
            let candidate = document::untitled_path(id);
            if !self.tabs.iter().any(|t| t.path == candidate) {
                break candidate;
            }
            id += 1;
        };
        self.open_untitled_at(path, save_dir, "", window, cx);
    }

    /// Restore or create an untitled tab at a specific `untitled:N` path.
    pub(super) fn open_untitled_at(
        &mut self,
        path: PathBuf,
        save_dir: Option<PathBuf>,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = Language::from_path(Path::new("untitled.txt"));
        let indent = self.editor_tab_size(initial);
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .line_number(true)
                .indent_guides(true)
                .soft_wrap(false)
                .tab_size(indent)
                .default_value(initial.to_owned())
        });
        let subscriptions = vec![
            cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let entity_id = state.entity_id();
                    if let Some(tab) = this.tabs.find_mut(|t| {
                        t.editor_state()
                            .is_some_and(|s| s.entity_id() == entity_id)
                    }) {
                        tab.refresh_modified(cx);
                        tab.preview = false;
                    }
                    this.selection_match_last_range.remove(&state.entity_id());
                    this.schedule_selection_matches(&state, cx);
                    this.schedule_persist_session(cx);
                    cx.notify();
                }
            }),
            cx.observe(&state, |this, state, cx| {
                this.on_editor_state_observed(&state, cx);
            }),
            install_tab_accepts_completion(&state, cx),
        ];
        self.ensure_selection_match_collection(&state, cx);
        // Empty `saved` baseline so any restored text counts as dirty.
        let saved = Rope::new();
        let is_modified = !initial.is_empty();
        self.tabs.push(EditorTab {
            path,
            body: TabBody::Text {
                language,
                state,
                saved,
                indent,
                rendered_preview: false,
                _subscriptions: subscriptions,
            },
            is_modified,
            preview: false,
            untitled_save_dir: save_dir,
        });
        self.focus_active_editor(window, cx);
        self.refresh_menus(cx);
        self.schedule_persist_session(cx);
        cx.notify();
    }

    /// Directory for a new file on Save: tree selection (or its parent), else the
    /// active root (multi-root) / sole root.
    pub(super) fn new_file_directory(&self, cx: &App) -> Option<PathBuf> {
        let folders = self.workspace.folders();
        if folders.is_empty() {
            return None;
        }
        if let Some(selected) = self.file_tree.selected_path(cx) {
            let dir = if selected.is_dir() {
                selected
            } else {
                selected
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .unwrap_or(selected)
            };
            if folders.iter().any(|root| dir.starts_with(root)) {
                return Some(dir);
            }
        }
        if let Some(path) = self.tabs.active().map(|t| &t.path)
            && !document::is_untitled(path)
            && let Some(parent) = path.parent()
            && folders.iter().any(|root| parent.starts_with(root))
        {
            return Some(parent.to_path_buf());
        }
        self.active_root(cx)
    }

    /// Root that contains the tree selection or active file; otherwise the first root.
    pub(super) fn on_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(|t| t.path.clone()) else {
            return;
        };
        self.request_save(path, |_, _, _, _| {}, None, window, cx);
    }

    pub(super) fn on_save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if tab.editor_state().is_none() {
            return;
        }
        let path = tab.path.clone();
        if document::is_untitled(&path) {
            self.request_save(path, |_, _, _, _| {}, None, window, cx);
            return;
        }
        self.prompt_save_as(path, window, cx);
    }

    fn prompt_save_as(
        &mut self,
        old_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = old_path
            .parent()
            .map(Path::to_path_buf)
            .or_else(|| self.new_file_directory(cx))
            .or_else(|| self.workspace.folders().first().cloned())
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let suggested = old_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled.txt".into());
        let prompt = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn_in(window, async move |this, window| {
            let new_path = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return,
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Save dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Save dialog was interrupted", window, cx);
                    })
                    .ok();
                    return;
                }
            };
            this.update_in(window, |this, window, cx| {
                if let Err(err) = this.save_as_to(&old_path, new_path, window, cx) {
                    notify_error(format!("Save As failed: {err:#}"), window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn on_save_all(&mut self, _: &SaveAll, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_unsaved() {
            return;
        }
        self.request_save_all(|_, _, _| {}, None, window, cx);
    }

    pub(super) fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.tabs.active().map(|t| t.path.clone()) {
            self.request_close(&path, window, cx);
        }
    }

    pub(super) fn on_close_tab_at(
        &mut self,
        action: &CloseTabAt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_close(&action.0, window, cx);
    }

    pub(super) fn on_close_other_tabs(
        &mut self,
        action: &CloseOtherTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep = &action.0;
        // Activate the right-clicked tab so it stays selected after others close.
        if let Some(ix) = self.tabs.position(|t| t.path == *keep) {
            self.tabs.activate(ix);
        }
        let paths: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.path != *keep)
            .map(|t| t.path.clone())
            .collect();
        self.request_close_many(paths, window, cx);
    }

    pub(super) fn on_close_all_tabs(
        &mut self,
        _: &CloseAllTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<PathBuf> = self.tabs.iter().map(|t| t.path.clone()).collect();
        self.request_close_many(paths, window, cx);
    }

    pub(super) fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(true);
        self.focus_active_editor(window, cx);
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn on_prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(false);
        self.focus_active_editor(window, cx);
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn on_increase_font(
        &mut self,
        _: &IncreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.change_font_size(1.0);
        self.apply_config(window, cx);
    }

    pub(super) fn on_decrease_font(
        &mut self,
        _: &DecreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.change_font_size(-1.0);
        self.apply_config(window, cx);
    }

    pub(super) fn on_reset_font(&mut self, _: &ResetFontSize, window: &mut Window, cx: &mut Context<Self>) {
        self.config.font_size = DEFAULT_EDITOR_FONT_SIZE;
        self.apply_config(window, cx);
    }

    pub(super) fn on_toggle_preview(
        &mut self,
        _: &TogglePreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if !tab.supports_rendered_preview() {
            return;
        }
        let next = !tab.rendered_preview();
        self.set_rendered_preview(next, window, cx);
    }

    pub(super) fn set_rendered_preview(
        &mut self,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.tabs.active_index() else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        let TabBody::Text {
            language,
            rendered_preview,
            ..
        } = &mut tab.body
        else {
            return;
        };
        if !language.supports_rendered_preview() || *rendered_preview == preview {
            return;
        }
        *rendered_preview = preview;
        if preview {
            self.focus_handle.focus(window, cx);
        } else {
            self.focus_active_editor(window, cx);
        }
        cx.notify();
    }

    pub(super) fn apply_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        theme::apply(&self.effective_config(), Some(window), cx);
        self.config.save();
        cx.notify();
    }

    /// User `config.json` plus overlay from the open `.isengard-workspace` settings.
    pub(super) fn effective_config(&self) -> AppConfig {
        let mut config = self.config.clone();
        crate::workspace::apply_to_config(self.workspace.settings(), &mut config);
        config
    }

    /// Indent for a buffer: detected from `text`, else the configured tab size.
    pub(super) fn editor_tab_size(&self, text: &str) -> TabSize {
        crate::editor::indent::detect_indent(text).unwrap_or(TabSize {
            tab_size: self.effective_config().tab_size.max(1) as usize,
            hard_tabs: false,
        })
    }
}

/// One toast for every file Save All could not write.
fn save_failures_message(failures: &[String]) -> String {
    match failures {
        [one] => format!("Save failed: {one}"),
        many => format!("Could not save {} files:\n{}", many.len(), many.join("\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::save_failures_message;

    #[test]
    fn single_failure_keeps_save_failed_prefix() {
        let msg = save_failures_message(&["a.rs: permission denied".into()]);
        assert_eq!(msg, "Save failed: a.rs: permission denied");
    }

    #[test]
    fn multiple_failures_are_listed_together() {
        let msg = save_failures_message(&["a.rs: denied".into(), "b.rs: disk full".into()]);
        assert_eq!(msg, "Could not save 2 files:\na.rs: denied\nb.rs: disk full");
    }
}
