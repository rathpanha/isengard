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
            loop {
                let rx = rx.clone();
                let event = cx.background_spawn(async move { rx.recv() }).await;
                let Ok(event) = event else {
                    break;
                };
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
        self.lsp.set_settings(self.config.lsp.clone());
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
                lsp.show_document = None;
                if let Some(diags) = state.diagnostics_mut() {
                    diags.clear();
                }
                cx.notify();
            });
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
    pub(super) fn reload_lsp_settings_if_config(&mut self, path: &Path) {
        let Some(config_path) = AppConfig::default_path() else {
            return;
        };
        if path != config_path {
            return;
        }
        self.config = AppConfig::load_from(&config_path);
        self.lsp.set_settings(self.config.lsp.clone());
        // Already-running servers keep old settings until workspace switch /
        // reopen; new attaches use the updated map.
    }
}
