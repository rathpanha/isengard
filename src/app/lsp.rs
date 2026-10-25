//! LSP attach, install/remove prompts, and the diagnostics event pump.
use super::*;

/// Worst-first color for the status-bar LSP badge (error > starting > running > idle).
pub(super) fn lsp_aggregate_color(servers: &[CatalogServer], cx: &App) -> Hsla {
    let mut rank = 0u8;
    for s in servers {
        if !s.installed
            && !matches!(
                s.status,
                ServerDisplayStatus::Running
                    | ServerDisplayStatus::Starting
                    | ServerDisplayStatus::Error(_)
            )
        {
            continue;
        }
        rank = rank.max(match &s.status {
            ServerDisplayStatus::Error(_) => 4,
            ServerDisplayStatus::Starting => 3,
            ServerDisplayStatus::Running => 2,
            ServerDisplayStatus::Idle => 1,
        });
    }
    match rank {
        4 => cx.theme().danger,
        3 => cx.theme().warning,
        2 => cx.theme().success,
        _ => cx.theme().muted_foreground,
    }
}

impl IsengardApp {
    pub(super) fn start_lsp_event_pump(&self, cx: &mut Context<Self>) {
        let rx = self.lsp.events();
        cx.spawn(async move |this, cx| {
            // Async recv: no pool thread parked per wait (and GPUI's test
            // scheduler runs background tasks inline, where a blocking recv hangs).
            while let Ok(event) = rx.recv_async().await {
                let _ = this.update(cx, |this, cx| this.handle_lsp_event(event, cx));
            }
        })
        .detach();
    }

    pub(super) fn handle_lsp_event(&mut self, event: LspEvent, cx: &mut Context<Self>) {
        match event {
            LspEvent::Diagnostics(params) => {
                let Some(path) = crate::lsp::path_for_diagnostics(&params) else {
                    return;
                };
                let Some(state) = self
                    .tabs
                    .iter()
                    .find(|t| t.path == path)
                    .and_then(|t| t.editor_state())
                    .cloned()
                else {
                    return;
                };
                crate::lsp::apply_diagnostics(&state, params, cx);
            }
            LspEvent::ApplyEdit(edit) => {
                let weak = cx.weak_entity();
                cx.spawn(async move |_, cx| {
                    cx.update(|cx| {
                        let Some(window) = cx
                            .active_window()
                            .or_else(|| cx.windows().into_iter().next())
                        else {
                            return;
                        };
                        let _ = window.update(cx, |_root, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.apply_workspace_edit(edit, window, cx);
                            });
                        });
                    });
                })
                .detach();
            }
        }
    }

    /// Go-to-definition / showDocument: open another file and place the cursor.
    pub(super) fn lsp_open_location(
        &mut self,
        path: &Path,
        selection: lsp_types::Range,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file(path, true, window, cx);
        let Some(state) = self
            .tabs
            .iter()
            .find(|t| t.path == path)
            .and_then(|t| t.editor_state())
            .cloned()
        else {
            return;
        };
        state.update(cx, |state, cx| {
            state.set_cursor_position(
                gpui_kit::component::input::Position {
                    line: selection.start.line,
                    character: selection.start.character,
                },
                window,
                cx,
            );
        });
    }

    pub(super) fn lsp_attach_tab(
        &mut self,
        path: &Path,
        language: Language,
        state: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.lsp
            .set_settings(self.effective_config().lsp.clone());
        self.lsp.set_folders(self.workspace.folders().to_vec());
        let text = state.read(cx).value().to_string();
        let app = cx.entity().downgrade();
        match self.lsp.attach_editor(
            path,
            language,
            &text,
            state,
            app,
            |this, path, range, window, cx| {
                this.lsp_open_location(path, range, window, cx);
            },
            window,
            cx,
        ) {
            AttachOutcome::Done => {}
            AttachOutcome::NeedsStart {
                spec,
                init,
                path,
                language,
            } => {
                cx.notify(); // yellow "starting" chip
                let process_key = spec.process_key().to_string();
                cx.spawn_in(window, async move |this, window| {
                    let start = window
                        .background_spawn({
                            let spec = spec.clone();
                            async move { LspClient::start(&spec, init) }
                        })
                        .await;
                    this.update_in(window, |this, window, cx| {
                        match start {
                            Ok((client, incoming)) => {
                                if let Err(err) =
                                    this.lsp.finish_start(&spec, client, incoming)
                                {
                                    this.lsp.mark_start_failed(&process_key, &err);
                                    notify_error(
                                        format!("Language server failed: {err:#}"),
                                        window,
                                        cx,
                                    );
                                    cx.notify();
                                    return;
                                }
                                this.config.lsp = this.lsp.settings().clone();
                                this.config.save();
                                // Wire every open tab that shares this server
                                // (others may have attached while we were Starting).
                                let _ = language;
                                let _ = path;
                                let pending: Vec<_> = this
                                    .tabs
                                    .iter()
                                    .filter_map(|t| {
                                        let lang = t.language()?;
                                        let state = t.editor_state()?.clone();
                                        Some((t.path.clone(), lang, state))
                                    })
                                    .collect();
                                for (tab_path, lang, state) in pending {
                                    let text = state.read(cx).value().to_string();
                                    let app = cx.entity().downgrade();
                                    this.lsp.attach_open_document(
                                        &tab_path,
                                        lang,
                                        &text,
                                        &state,
                                        app,
                                        |this, path, range, window, cx| {
                                            this.lsp_open_location(path, range, window, cx);
                                        },
                                        cx,
                                    );
                                }
                                cx.notify();
                            }
                            Err(err) => {
                                this.lsp.mark_start_failed(&process_key, &err);
                                cx.notify();
                                if LspHost::looks_like_missing(&err) {
                                    if this.lsp.missing_prompt_once(&process_key) {
                                        this.offer_lsp_install(
                                            MissingServerOffer {
                                                recipe: recipe_for_command(&spec.command),
                                                spec,
                                                path,
                                                language,
                                            },
                                            window,
                                            cx,
                                        );
                                    }
                                } else {
                                    notify_error(
                                        format!("Language server failed: {err:#}"),
                                        window,
                                        cx,
                                    );
                                }
                            }
                        }
                    })
                    .ok()
                })
                .detach();
            }
        }
    }

    /// Missing language server: offer one-click install when we know a recipe.
    pub(super) fn offer_lsp_install(
        &mut self,
        offer: MissingServerOffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(recipe) = offer.recipe else {
            notify_error(
                format!(
                    "`{}` not found — install it yourself or set lsp.languages in config.json",
                    offer.spec.command
                ),
                window,
                cx,
            );
            return;
        };

        let app = cx.entity().downgrade();
        let offer_install = offer.clone();
        let recipe_name = recipe.name;
        let recipe_summary = recipe.summary;
        let recipe_hint = recipe.install_hint();
        window.open_dialog(cx, move |dialog, window, _| {
            let app_install = app.clone();
            let app_dismiss = app.clone();
            let offer_install = offer_install.clone();
            center_dialog(
                dialog
                    .title(format!("Install {recipe_name}?"))
                    .child(recipe_summary.to_string())
                    .child(recipe_hint.clone())
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("lsp-install-not-now")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Not now")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let _ = app_dismiss.update(cx, |this, cx| {
                                            // Keep missing_toasted set so we don't spam.
                                            this.restore_focus_after_lsp_ui(window, cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new("lsp-install-run")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Install")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let _ = app_install.update(cx, |this, cx| {
                                            this.run_lsp_install(
                                                offer_install.clone(),
                                                window,
                                                cx,
                                            );
                                            this.restore_focus_after_lsp_ui(window, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    pub(super) fn clear_lsp_providers_for_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for path in paths {
            let Some(state) = self
                .tabs
                .iter()
                .find(|t| t.path == *path)
                .and_then(|t| t.editor_state())
                .cloned()
            else {
                continue;
            };
            state.update(cx, |state, cx| {
                let lsp = state.lsp_mut();
                lsp.completion_provider = None;
                lsp.hover_provider = None;
                lsp.definition_provider = None;
                lsp.code_action_providers.clear();
                lsp.show_document = None;
                if let Some(diags) = state.diagnostics_mut() {
                    diags.clear();
                }
                cx.notify();
            });
        }
    }

    /// Apply an LSP WorkspaceEdit to open tabs and (for closed files) disk.
    pub(super) fn apply_workspace_edit(
        &mut self,
        edit: lsp_types::WorkspaceEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files = match crate::lsp::workspace_edit_file_edits(&edit) {
            Ok(files) => files,
            Err(err) => {
                notify_error(format!("Could not apply edit: {err:#}"), window, cx);
                return;
            }
        };
        if files.is_empty() {
            return;
        }
        let mut touched = false;
        for (path, edits) in files {
            if let Some(state) = self
                .tabs
                .iter()
                .find(|t| t.path == path)
                .and_then(|t| t.editor_state())
                .cloned()
            {
                state.update(cx, |state, cx| {
                    state.apply_lsp_edits(&edits, window, cx);
                });
                // Keep the language server in sync with the buffer.
                let text = state.read(cx).value().to_string();
                self.lsp.did_change(&path, &text);
                touched = true;
                continue;
            }
            // Closed file: patch on disk.
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let next = crate::lsp::apply_text_edits(&text, &edits);
                    if let Err(err) = crate::editor::document::write_text(&path, &next) {
                        notify_error(
                            format!("Could not write {}: {err:#}", path.display()),
                            window,
                            cx,
                        );
                    } else {
                        touched = true;
                    }
                }
                Err(err) => notify_error(
                    format!("Could not read {}: {err}", path.display()),
                    window,
                    cx,
                ),
            }
        }
        if touched {
            self.refresh_git_panel(window, cx);
            let roots: Vec<_> = self.workspace.folders().to_vec();
            self.file_tree.refresh_loaded_dirs(roots.iter().map(|p| p.as_path()), cx);
        }
    }

    /// Confirm + uninstall (or forget) a language server from the status bar.
    pub(super) fn offer_lsp_remove(
        &mut self,
        process_key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = std::path::Path::new(process_key)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(process_key)
            .to_string();
        let app_local = is_app_local(process_key);
        let app = cx.entity().downgrade();
        let key = process_key.to_string();

        window.open_dialog(cx, move |dialog, window, _| {
            let app_ok = app.clone();
            let key_ok = key.clone();
            let dialog = dialog.title(format!("Remove {label}?"));
            let dialog = if app_local {
                dialog.child(
                    "Isengard will stop it and delete the app-local install \
                     (your system PATH binaries are left alone).",
                )
            } else {
                dialog.child(
                    "Isengard will stop it and forget it from the status bar. \
                     System PATH binaries are not removed.",
                )
            };
            center_dialog(
                dialog.footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("lsp-remove-cancel")
                                .cursor_pointer()
                                .outline()
                                .label("Cancel")
                                .on_click({
                                    let app_cancel = app.clone();
                                    move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let _ = app_cancel.update(cx, |this, cx| {
                                            this.restore_focus_after_lsp_ui(window, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("lsp-remove-confirm")
                                .cursor_pointer()
                                .danger()
                                .label("Remove")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = app_ok.update(cx, |this, cx| {
                                        this.run_lsp_remove(&key_ok, window, cx);
                                        this.restore_focus_after_lsp_ui(window, cx);
                                    });
                                }),
                        ),
                ),
                window,
            )
        });
    }

    pub(super) fn run_lsp_remove(
        &mut self,
        process_key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = std::path::Path::new(process_key)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(process_key)
            .to_string();
        let (paths, recipe) = self.lsp.remove_server(process_key);
        self.clear_lsp_providers_for_paths(&paths, cx);
        self.config.lsp = self.lsp.settings().clone();
        self.config.save();

        let Some(recipe) = recipe else {
            notify_success(format!("Removed {label}"), window, cx);
            self.restore_focus_after_lsp_ui(window, cx);
            return;
        };

        let name = recipe.name.to_string();
        notify_success(format!("Uninstalling {name}…"), window, cx);
        self.restore_focus_after_lsp_ui(window, cx);
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_spawn(async move { run_uninstall(&recipe) })
                .await;
            this.update_in(window, |this, window, cx| {
                match result {
                    Ok(()) => notify_success(format!("Removed {name}"), window, cx),
                    Err(err) => notify_error(
                        format!("Forgot {name} in Isengard, but uninstall failed: {err}"),
                        window,
                        cx,
                    ),
                }
                this.lsp.invalidate_ready_cache();
                this.restore_focus_after_lsp_ui(window, cx);
            })
            .ok()
        })
        .detach();
    }

    /// Catalog Install: run recipe for a command key (no open-file context required).
    pub(super) fn run_lsp_install_command(
        &mut self,
        process_key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(recipe) = recipe_for_command(process_key) else {
            notify_error(
                format!("No install recipe for `{process_key}`"),
                window,
                cx,
            );
            return;
        };
        let path = self.tabs.active().map(|t| t.path.clone());
        let language = self.tabs.active().and_then(|t| t.language());
        self.run_lsp_install_recipe(
            recipe,
            process_key.to_string(),
            path,
            language,
            window,
            cx,
        );
    }

    pub(super) fn run_lsp_install(
        &mut self,
        offer: MissingServerOffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(recipe) = offer.recipe else {
            return;
        };
        let process_key = offer.spec.process_key().to_string();
        self.run_lsp_install_recipe(
            recipe,
            process_key,
            Some(offer.path),
            Some(offer.language),
            window,
            cx,
        );
    }

    pub(super) fn run_lsp_install_recipe(
        &mut self,
        recipe: crate::lsp::InstallRecipe,
        process_key: String,
        path: Option<PathBuf>,
        language: Option<Language>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = recipe.name.to_string();
        notify_success(format!("Installing {name}…"), window, cx);
        self.restore_focus_after_lsp_ui(window, cx);

        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_spawn(async move { run_install(&recipe) })
                .await;
            this.update_in(window, |this, window, cx| {
                match result {
                    Ok(()) => {
                        this.lsp.clear_missing_prompt(&process_key);
                        this.lsp.remember_known(&process_key);
                        this.config.lsp = this.lsp.settings().clone();
                        this.config.save();
                        notify_success(
                            format!("{name} installed — enabling IntelliSense"),
                            window,
                            cx,
                        );
                        if let (Some(path), Some(language)) = (path, language) {
                            let state = this
                                .tabs
                                .iter()
                                .find(|t| t.path == path)
                                .and_then(|t| t.editor_state())
                                .cloned();
                            if let Some(state) = state {
                                this.lsp_attach_tab(&path, language, &state, window, cx);
                            }
                        }
                    }
                    Err(err) => {
                        this.lsp.clear_missing_prompt(&process_key);
                        notify_error(format!("Could not install {name}: {err}"), window, cx);
                    }
                }
                this.restore_focus_after_lsp_ui(window, cx);
            })
            .ok()
        })
        .detach();
    }
    /// Format buffer via LSP then write. Format errors fall back to plain save.
    pub(super) fn format_then_save(
        &mut self,
        path: PathBuf,
        after: AfterSave,
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
            let _ = self.save_tab(&path, window, cx);
            after(self, path, window, cx);
            return;
        };
        let text = state.read(cx).value().to_string();
        if !self.lsp.is_attached(&path) {
            match self.save_tab(&path, window, cx) {
                Ok(()) => after(self, path, window, cx),
                Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
            }
            return;
        }
        // Snapshot host methods need &self — clone client path work onto background.
        let format_path = path.clone();
        let edits = {
            match self.lsp.format_document(&format_path, &text) {
                Ok(edits) => edits,
                Err(err) => {
                    notify_warning(format!("Format on save skipped: {err:#}"), window, cx);
                    match self.save_tab(&path, window, cx) {
                        Ok(()) => after(self, path, window, cx),
                        Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
                    }
                    return;
                }
            }
        };
        if !edits.is_empty() {
            state.update(cx, |state, cx| {
                state.apply_lsp_edits(&edits, window, cx);
            });
            let new_text = state.read(cx).value().to_string();
            self.lsp.did_change(&path, &new_text);
        }
        match self.save_tab(&path, window, cx) {
            Ok(()) => after(self, path, window, cx),
            Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
        }
    }

    pub(super) fn on_format_document(
        &mut self,
        _: &FormatDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.format_active_document(window, cx);
    }

    pub(super) fn on_rename_symbol(
        &mut self,
        _: &RenameSymbol,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let path = tab.path.clone();
        if document::is_untitled(&path) {
            return notify_warning("Save the file before renaming a symbol", window, cx);
        }
        let Some(state) = tab.editor_state().cloned() else {
            return;
        };
        if !self.lsp.is_attached(&path) {
            return notify_warning("No language server for this file", window, cx);
        }

        let editor = state.read(cx);
        let text = editor.value().to_string();
        let rope = Rope::from(text.as_str());
        let range = editor.selected_range();
        let selection_name = if range.start < range.end {
            Some(rope.slice(range.start..range.end.min(rope.len())).to_string())
        } else {
            None
        };
        let cursor = editor.cursor_position();
        let position = lsp_types::Position {
            line: cursor.line,
            character: cursor.character,
        };

        let mut default_name = selection_name.filter(|s| !s.is_empty());
        if default_name.is_none()
            && let Ok(placeholder) = self.lsp.prepare_rename(&path, &text, position)
        {
            default_name = placeholder;
        }
        if default_name.is_none() {
            let rope = Rope::from(text.as_str());
            let kit_pos = Position {
                line: position.line,
                character: position.character,
            };
            let offset = rope.position_to_offset(&kit_pos);
            default_name = Some(ident_at(&rope, offset));
        }
        let default_name = default_name.unwrap_or_default();
        if default_name.is_empty() {
            return notify_warning("No symbol under the cursor", window, cx);
        }

        let input = cx.new(|cx| InputState::new(window, cx).default_value(default_name));
        input.update(cx, |input, cx| input.focus(window, cx));
        let app = cx.entity().downgrade();
        let rename_path = path.clone();
        let rename_text = text;
        let rename_pos = position;
        window.open_dialog(cx, move |dialog, window, _| {
            let (app_ok, app_cancel) = (app.clone(), app.clone());
            let (input_ok, input_view) = (input.clone(), input.clone());
            let path = rename_path.clone();
            let doc = rename_text.clone();
            center_dialog(
                dialog
                    .title("Rename Symbol")
                    .child(Input::new(&input_view).w_full().min_w(px(280.)))
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("rename-symbol-cancel")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let _ = app_cancel;
                                    }),
                            )
                            .child(
                                Button::new("rename-symbol-ok")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Rename")
                                    .on_click(move |_, window, cx| {
                                        let new_name = input_ok.read(cx).value().to_string();
                                        window.close_dialog(cx);
                                        let path = path.clone();
                                        let doc = doc.clone();
                                        _ = app_ok.update(cx, |this, cx| {
                                            this.perform_rename_symbol(
                                                &path, &doc, rename_pos, &new_name, window, cx,
                                            );
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    fn perform_rename_symbol(
        &mut self,
        path: &Path,
        text: &str,
        position: lsp_types::Position,
        new_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return;
        }
        match self.lsp.rename_symbol(path, text, position, new_name) {
            Ok(edit) => self.apply_workspace_edit(edit, window, cx),
            Err(err) => notify_error(format!("Rename failed: {err:#}"), window, cx),
        }
    }

    /// Format the active text tab without saving.
    pub(super) fn format_active_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let path = tab.path.clone();
        if document::is_untitled(&path) {
            return notify_warning("Save the file before formatting", window, cx);
        }
        let Some(state) = tab.editor_state().cloned() else {
            return;
        };
        if !self.lsp.is_attached(&path) {
            return notify_warning("No language server for this file", window, cx);
        }
        let text = state.read(cx).value().to_string();
        match self.lsp.format_document(&path, &text) {
            Ok(edits) if edits.is_empty() => {}
            Ok(edits) => {
                state.update(cx, |state, cx| {
                    state.apply_lsp_edits(&edits, window, cx);
                });
                let new_text = state.read(cx).value().to_string();
                self.lsp.did_change(&path, &new_text);
                cx.notify();
            }
            Err(err) => notify_error(format!("Format failed: {err:#}"), window, cx),
        }
    }

    pub(super) fn reload_lsp_settings_if_config(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config_path) = AppConfig::default_path() else {
            return;
        };
        if path != config_path {
            return;
        }
        let previous = self.config.lsp.clone();
        self.config = AppConfig::load_from(&config_path);
        if self.config.lsp == previous {
            self.lsp
            .set_settings(self.effective_config().lsp.clone());
            return;
        }
        self.restart_lsp_servers(window, cx);
    }

    /// Shut down every language server and re-attach open text tabs.
    pub(super) fn restart_lsp_servers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.lsp.attached_paths();
        self.clear_lsp_providers_for_paths(&paths, cx);
        self.lsp.shutdown_all();
        self.lsp
            .set_settings(self.effective_config().lsp.clone());
        self.lsp.set_folders(self.workspace.folders().to_vec());
        let reattach: Vec<(PathBuf, Language, Entity<EditorState>)> = self
            .tabs
            .iter()
            .filter_map(|t| {
                let state = t.editor_state()?.clone();
                let language = Language::from_path(&t.path);
                Some((t.path.clone(), language, state))
            })
            .collect();
        for (path, language, state) in reattach {
            self.lsp_attach_tab(&path, language, &state, window, cx);
        }
        cx.notify();
    }
}

/// Identifier under `offset` (letters, digits, `_`).
fn ident_at(rope: &Rope, offset: usize) -> String {
    let text = rope.to_string();
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return String::new();
    }
    let mut i = offset.min(bytes.len());
    if i > 0 && !is_ident_byte(bytes[i.saturating_sub(1)]) && (i >= bytes.len() || !is_ident_byte(bytes[i])) {
        // caret after a non-ident — nothing to grab
    }
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    let start = i;
    while i < bytes.len() && is_ident_byte(bytes[i]) {
        i += 1;
    }
    text[start..i].to_owned()
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
