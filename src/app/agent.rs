//! Right Agent panel: show/hide, plus the agent's file and login requests.
use super::*;
use super::tab::TabBody;
use crate::acp::{self, BuiltinAgent, launch_for_workspace, slice_lines, with_builtin};
use crate::agent::{Attachment, LineRange, line_at_offset};
use crate::terminal::pty::TerminalCommand;

impl IsengardApp {
    /// `⌘N` / `Ctrl+N` while the Agent panel has key focus → new chat.
    pub(super) fn on_new_agent_chat(
        &mut self,
        _: &NewAgentChat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.agent_visible {
            return;
        }
        let Some(agent) = self.agent.clone() else {
            return;
        };
        agent.update(cx, |agent, cx| agent.new_chat(window, cx));
    }

    /// `⌘L` / `Ctrl+L`: attach editor selection (or whole file), or terminal
    /// selection when the terminal has focus.
    pub(super) fn on_add_selection_to_agent(
        &mut self,
        _: &AddSelectionToAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Require a visible panel — a hidden session can keep focus after
        // toggle-away and would otherwise steal Ctrl/Cmd+L with a dead end.
        let terminal_focused = {
            let term = self.terminal.read(cx);
            term.is_visible()
                && term
                    .focus_handle(cx)
                    .is_some_and(|h| h.is_focused(window))
        };

        let attachment = if terminal_focused {
            let Some(text) = self
                .terminal
                .read(cx)
                .active_selection_text(cx)
                .filter(|t| !t.is_empty())
            else {
                return notify_error("Select text in the terminal first", window, cx);
            };
            Attachment::Snippet {
                label: "Terminal".into(),
                text,
            }
        } else {
            let Some(tab) = self.tabs.active() else {
                return notify_error("Open a file before adding to the agent", window, cx);
            };
            let Some(state) = tab.editor_state().cloned() else {
                return notify_error("Only text editors can be added to the agent", window, cx);
            };
            let path = tab.path.clone();
            if document::is_untitled(&path) {
                return notify_error("Save the file before adding it to the agent", window, cx);
            }

            let (selected, range, full) = state.read_with(cx, |editor, _| {
                (
                    editor.selected_text().to_string(),
                    editor.selected_range(),
                    editor.text().to_string(),
                )
            });
            if range.is_empty() || selected.is_empty() {
                Attachment::File {
                    path,
                    text: Some(full),
                    selection: None,
                }
            } else {
                let start = line_at_offset(&full, range.start);
                let end = line_at_offset(&full, range.end.saturating_sub(1).max(range.start));
                Attachment::File {
                    path,
                    text: Some(selected),
                    selection: Some(LineRange { start, end }),
                }
            }
        };

        let Some(agent) = self.agent.clone() else {
            return notify_error("Open a folder before using the agent", window, cx);
        };
        if !self.agent_visible {
            self.agent_visible = true;
            self.persist_session(cx);
        }
        agent.update(cx, |agent, cx| {
            agent.ensure_started(cx);
            agent.attach_context(attachment, cx);
            agent.input().update(cx, |input, cx| input.focus(window, cx));
        });
        cx.notify();
    }

    pub(super) fn on_toggle_agent_panel(
        &mut self,
        _: &ToggleAgentPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_visible {
            self.agent_visible = false;
            self.focus_active_editor(window, cx);
        } else {
            let Some(agent) = self.agent.clone() else {
                return notify_error("Open a folder before using the agent", window, cx);
            };
            self.agent_visible = true;
            agent.update(cx, |agent, cx| agent.ensure_started(cx));
            let input = agent.read(cx).input().clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.sync_git_agent_ready(cx);
        self.persist_session(cx);
        cx.notify();
    }

    /// Claude ↔ Cursor: remember the choice on this workspace session (not
    /// global `agent.command`), remount under that slot, Install / connect.
    fn switch_builtin_agent(
        &mut self,
        agent: BuiltinAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws_key) = self.workspace.session_key() else {
            return;
        };
        // Persist the outgoing slot's chats before remounting.
        self.persist_session(cx);
        self.set_workspace_agent(ws_key.as_str(), agent.slot_id());
        let next = with_builtin(&self.config.agent, agent);
        let key = acp::panel_key(&ws_key, &next);
        let current_key = self.agent.as_ref().and_then(|a| self.agent_key(a));
        if current_key.as_deref() == Some(key.as_str()) {
            if let Some(panel) = self.agent.clone() {
                panel.update(cx, |panel, cx| {
                    panel.set_settings(next, cx);
                    panel.begin_after_pick(cx);
                });
            }
            self.persist_session(cx);
            cx.notify();
            return;
        }
        if let Some(old) = self.agent.take() {
            old.update(cx, |agent, cx| agent.suspend(cx));
        }
        self.switch_agent_panel(window, cx);
        if let Some(panel) = self.agent.clone() {
            // User already chose — skip picker even when the slot has no chats.
            panel.update(cx, |panel, cx| panel.begin_after_pick(cx));
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// Write `sessions.<key>.agent` (Claude / Cursor / custom slot id).
    fn set_workspace_agent(&mut self, ws_key: &str, slot: &str) {
        let mut session = self.config.session(ws_key).cloned().unwrap_or_default();
        session.agent = Some(slot.to_owned());
        self.config.put_session(ws_key.to_owned(), session);
        self.config.save();
    }

    /// After a workspace switch: park the old panel (its agent finishes any
    /// running turn, then stops) and bring up this workspace's panel, reusing
    /// it — transcript and all — if we were here before.
    pub(super) fn switch_agent_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws_key) = self.workspace.session_key() else {
            if let Some(old) = self.agent.take() {
                old.update(cx, |agent, cx| agent.suspend(cx));
            }
            return;
        };
        let remembered = self
            .config
            .session(&ws_key)
            .and_then(|s| s.remembered_agent());
        let (settings, awaiting_pick) =
            launch_for_workspace(&self.config.agent, remembered.as_deref());
        let key = acp::panel_key(&ws_key, &settings);
        let current_key = self.agent.as_ref().and_then(|a| self.agent_key(a));
        if current_key.as_deref() == Some(key.as_str()) {
            return;
        }
        if let Some(old) = self.agent.take() {
            old.update(cx, |agent, cx| agent.suspend(cx));
        }
        if let Some((panel, _)) = self.agents.get(&key) {
            self.agent = Some(panel.clone());
            self.sync_git_agent_ready(cx);
            return;
        }
        let roots = self.workspace.folders().to_vec();
        let slot = acp::agent_slot_id(&settings);
        let saved = self
            .config
            .session(&ws_key)
            .map(|s| s.agent_slot(&slot))
            .unwrap_or_default();
        // A folder with no picks yet starts from the last ones used anywhere.
        let prefs = if saved.prefs.is_empty() {
            self.config.agent_prefs.clone()
        } else {
            saved.prefs
        };
        let panel = cx.new(|cx| {
            AgentPanel::new(
                roots,
                settings,
                saved.chats,
                saved.active,
                prefs,
                awaiting_pick,
                window,
                cx,
            )
        });
        let subscription =
            cx.subscribe_in(&panel, window, |this, panel, event: &AgentPanelEvent, window, cx| {
                this.handle_agent_event(panel, event, window, cx);
            });
        self.agents.insert(key, (panel.clone(), subscription));
        self.agent = Some(panel);
        self.sync_git_agent_ready(cx);
    }

    /// Folders added/removed within the open workspace.
    pub(super) fn sync_agent_roots(&mut self, cx: &mut Context<Self>) {
        let roots = self.workspace.folders().to_vec();
        if let Some(agent) = &self.agent {
            agent.update(cx, |agent, cx| agent.set_roots(roots, cx));
        }
    }

    fn agent_key(&self, panel: &Entity<AgentPanel>) -> Option<String> {
        self.agents
            .iter()
            .find(|(_, (p, _))| p == panel)
            .map(|(key, _)| key.clone())
    }

    pub(super) fn handle_agent_event(
        &mut self,
        panel: &Entity<AgentPanel>,
        event: &AgentPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_current = self.agent.as_ref() == Some(panel);
        match event {
            AgentPanelEvent::ReadFile(read) => {
                let result = self
                    .agent_read_file(&read.path, cx)
                    .map(|text| slice_lines(&text, read.line, read.limit));
                let _ = read.reply.send(result);
            }
            AgentPanelEvent::WriteFile(write) => {
                let path = write.path.clone();
                let content = write.content.clone();
                let before = if path.exists() || self.open_text_buffer(&path).is_some() {
                    self.agent_read_file(&path, cx).unwrap_or_default()
                } else {
                    String::new()
                };
                let result = self.agent_write_file(&path, &content, window, cx);
                if result.is_ok() {
                    panel.update(cx, |agent, cx| {
                        agent.note_file_write(path, before, content, cx);
                    });
                }
                let _ = write.reply.send(result);
            }
            AgentPanelEvent::OpenPaths(paths) => {
                for path in paths {
                    if path.is_file() {
                        self.open_file(path, true, window, cx);
                    }
                }
            }
            AgentPanelEvent::WorkspaceTouched => {
                self.git.update(cx, |git, cx| git.refresh(window, cx));
            }
            // Attach to the panel that asked — don't require `is_current`.
            AgentPanelEvent::AttachFromDisk => {
                self.agent_pick_from_disk(panel, window, cx);
            }
            AgentPanelEvent::AttachPath(path) => {
                self.agent_attach_path(panel, path.clone(), window, cx);
            }
            AgentPanelEvent::Login { title, command } if is_current => {
                self.run_agent_login(panel, title.clone(), command, window, cx);
            }
            AgentPanelEvent::Hide if is_current && self.agent_visible => {
                self.on_toggle_agent_panel(&ToggleAgentPanel, window, cx);
            }
            AgentPanelEvent::ChatsChanged if is_current => self.persist_session(cx),
            AgentPanelEvent::ChatsChanged => self.persist_background_chats(panel, cx),
            AgentPanelEvent::PrefsChanged => {
                self.config.agent_prefs = panel.read(cx).prefs().clone();
                if is_current {
                    self.persist_session(cx);
                } else {
                    self.persist_background_chats(panel, cx);
                }
            }
            AgentPanelEvent::SettingsChanged => {
                let settings = panel.read(cx).settings();
                // Silent / subagent only — never persist launch `command` from
                // the panel (Claude/Cursor choice lives on the workspace session).
                self.config.agent.silent_model = settings.silent_model.clone();
                self.config.agent.subagent_model = settings.subagent_model.clone();
                self.config.save();
                let silent = settings.silent_model.clone();
                let sub = settings.subagent_model.clone();
                for (other, _) in self.agents.values() {
                    if other != panel {
                        other.update(cx, |a, cx| {
                            a.set_shared_agent_models(silent.clone(), sub.clone(), cx);
                        });
                    }
                }
            }
            AgentPanelEvent::SwitchAgent(agent) if is_current => {
                self.switch_builtin_agent(*agent, window, cx);
            }
            AgentPanelEvent::PermissionRequested if !is_current => {
                let name = self
                    .agent_key(panel)
                    .map(|key| document::file_name(Path::new(&key)))
                    .unwrap_or_default();
                notify_warning(format!("The agent in {name} is waiting for your approval"), window, cx);
            }
            AgentPanelEvent::CommitSuggestReady { root, message } => {
                self.git.update(cx, |git, cx| {
                    git.apply_commit_message(root, message, window, cx);
                });
            }
            AgentPanelEvent::CommitSuggestFailed { root } => {
                self.git.update(cx, |git, cx| {
                    git.set_suggest_busy(root, false, cx);
                });
                notify_warning("Claude could not Suggest a commit message.", window, cx);
            }
            AgentPanelEvent::PromptabilityChanged => {}
            _ => {}
        }
        if is_current {
            self.sync_git_agent_ready(cx);
        }
    }

    /// A background panel's chat index changed: update just those fields of
    /// its saved session (the rest belongs to that workspace's last visit).
    fn persist_background_chats(&mut self, panel: &Entity<AgentPanel>, cx: &App) {
        let Some(panel_key) = self.agent_key(panel) else {
            return;
        };
        let Some((ws_key, slot)) = acp::split_panel_key(&panel_key) else {
            return;
        };
        let mut session = self.config.session(ws_key).cloned().unwrap_or_default();
        let agent = panel.read(cx);
        session.set_agent_slot(
            slot,
            crate::config::AgentSlotSession {
                chats: agent.chats().to_vec(),
                active: agent.active_chat().map(str::to_owned),
                prefs: agent.prefs().clone(),
            },
        );
        self.config.put_session(ws_key.to_owned(), session);
        self.config.save();
    }

    /// Open buffer text wins over disk (the agent sees unsaved edits).
    fn agent_read_file(&self, path: &Path, cx: &App) -> Result<String, String> {
        if !path.is_absolute() {
            return Err(format!("Path must be absolute: {}", path.display()));
        }
        if let Some(state) = self.open_text_buffer(path) {
            return Ok(state.read(cx).value().to_string());
        }
        document::read_text(path).map_err(|err| format!("{err:#}"))
    }

    /// Write to disk, then reload an open tab so it shows the agent's text
    /// as saved. New files appear in the tree when their folder is expanded.
    fn agent_write_file(
        &mut self,
        path: &Path,
        content: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !path.is_absolute() {
            return Err(format!("Path must be absolute: {}", path.display()));
        }
        let parent = path.parent().map(Path::to_path_buf);
        if let Some(parent) = &parent {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let existed = path.exists();
        document::write_text(path, content).map_err(|err| format!("{err:#}"))?;
        if self.open_text_buffer(path).is_some() {
            self.reload_open_file_from_disk(path, window, cx);
        }
        if !existed
            && let Some(parent) = parent
            && (self.workspace.folders().contains(&parent)
                || self.file_tree.expanded_paths().contains(&parent))
        {
            self.file_tree.refresh_dir(&parent, cx);
        }
        // Same as manual Save / Refresh — agent writes must show in Source Control.
        self.git.update(cx, |git, cx| git.refresh(window, cx));
        cx.notify();
        Ok(())
    }

    fn open_text_buffer(&self, path: &Path) -> Option<&Entity<EditorState>> {
        self.tabs
            .iter()
            .find(|tab| tab.path == path)
            .and_then(|tab| match &tab.body {
                TabBody::Text { state, .. } => Some(state),
                _ => None,
            })
    }

    /// Run the agent's login in a terminal tab; when it exits, reconnect the
    /// panel that asked (no manual Retry).
    fn run_agent_login(
        &mut self,
        panel: &Entity<AgentPanel>,
        title: String,
        command: &TerminalCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cwd = crate::terminal::pty::default_cwd(self.workspace.folders());
        let panel = panel.downgrade();
        let result = self.terminal.update(cx, |term, cx| {
            term.add_command_session(
                cwd,
                title,
                command,
                move |_, cx| {
                    let _ = panel.update(cx, |panel, cx| panel.retry_after_login(cx));
                },
                window,
                cx,
            )
        });
        if let Err(err) = result {
            return notify_error(err, window, cx);
        }
        self.persist_session(cx);
        cx.notify();
    }

    fn agent_attach_path(
        &mut self,
        panel: &Entity<AgentPanel>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if document::is_untitled(&path) {
            return notify_error("Save the file before attaching it", window, cx);
        }
        // Raster images → ACP image blocks + thumbnail chips (not file/folder).
        if let Some(image) = crate::agent::image_from_path(&path) {
            panel.update(cx, |panel, cx| panel.attach_context(image, cx));
            return;
        }
        let text = if path.is_file() {
            self.agent_read_file(&path, cx).ok()
        } else if path.is_dir() {
            // Folders go as a resource link (no embed); agent can browse from the URI.
            None
        } else {
            return notify_error("That path is not a file or folder", window, cx);
        };
        panel.update(cx, |panel, cx| {
            panel.attach_context(
                Attachment::File {
                    path,
                    text,
                    selection: None,
                },
                cx,
            );
        });
    }

    /// Native OS picker (multi file + folder) → attach each path as a chip.
    fn agent_pick_from_disk(
        &mut self,
        panel: &Entity<AgentPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Files only: with `directories: true`, Linux file choosers often treat
        // Open as "use this folder" (e.g. ~/Pictures) and break double-click on
        // files. Attach folders via `@` in the composer instead.
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach to agent".into()),
        });
        let panel = panel.clone();
        cx.spawn_in(window, async move |this, window| {
            let chosen = match paths.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) => return,
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Open dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Open dialog was interrupted", window, cx);
                    })
                    .ok();
                    return;
                }
            };
            this.update_in(window, |this, window, cx| {
                for path in chosen {
                    this.agent_attach_path(&panel, path, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}
