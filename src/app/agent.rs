//! Right Agent panel: show/hide, plus the agent's file and login requests.
use super::*;
use super::tab::TabBody;
use crate::acp::slice_lines;
use crate::terminal::pty::TerminalCommand;

impl IsengardApp {
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
        self.persist_session(cx);
        cx.notify();
    }

    /// After a workspace switch: park the old panel (its agent finishes any
    /// running turn, then stops) and bring up this workspace's panel, reusing
    /// it — transcript and all — if we were here before.
    pub(super) fn switch_agent_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.workspace.session_key();
        let current_key = self.agent.as_ref().and_then(|a| self.agent_key(a));
        if current_key.is_some() && current_key == key {
            return;
        }
        if let Some(old) = self.agent.take() {
            old.update(cx, |agent, cx| agent.suspend(cx));
        }
        let Some(key) = key else {
            return;
        };
        if let Some((panel, _)) = self.agents.get(&key) {
            self.agent = Some(panel.clone());
            return;
        }
        let roots = self.workspace.folders().to_vec();
        let settings = self.config.agent.clone();
        let (chats, active) = self
            .config
            .session(&key)
            .map(|s| (s.agent_chats.clone(), s.agent_active.clone()))
            .unwrap_or_default();
        let panel = cx.new(|cx| AgentPanel::new(roots, settings, chats, active, window, cx));
        let subscription =
            cx.subscribe_in(&panel, window, |this, panel, event: &AgentPanelEvent, window, cx| {
                this.handle_agent_event(panel, event, window, cx);
            });
        self.agents.insert(key, (panel.clone(), subscription));
        self.agent = Some(panel);
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
                let result = self.agent_write_file(&write.path, &write.content, window, cx);
                let _ = write.reply.send(result);
            }
            AgentPanelEvent::Login { title, command } if is_current => {
                self.run_agent_login(title.clone(), command, window, cx);
            }
            AgentPanelEvent::Hide if is_current && self.agent_visible => {
                self.on_toggle_agent_panel(&ToggleAgentPanel, window, cx);
            }
            AgentPanelEvent::ChatsChanged if is_current => self.persist_session(cx),
            AgentPanelEvent::ChatsChanged => self.persist_background_chats(panel, cx),
            AgentPanelEvent::PermissionRequested if !is_current => {
                let name = self
                    .agent_key(panel)
                    .map(|key| document::file_name(Path::new(&key)))
                    .unwrap_or_default();
                notify_warning(format!("The agent in {name} is waiting for your approval"), window, cx);
            }
            _ => {}
        }
    }

    /// A background panel's chat index changed: update just those fields of
    /// its saved session (the rest belongs to that workspace's last visit).
    fn persist_background_chats(&mut self, panel: &Entity<AgentPanel>, cx: &App) {
        let Some(key) = self.agent_key(panel) else {
            return;
        };
        let mut session = self.config.session(&key).cloned().unwrap_or_default();
        let agent = panel.read(cx);
        session.agent_chats = agent.chats().to_vec();
        session.agent_active = agent.active_chat().map(str::to_owned);
        self.config.put_session(key, session);
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

    fn run_agent_login(
        &mut self,
        title: String,
        command: &TerminalCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cwd = crate::terminal::pty::default_cwd(self.workspace.folders());
        let result = self.terminal.update(cx, |term, cx| {
            term.add_command_session(cwd, title, command, window, cx)
        });
        if let Err(err) = result {
            return notify_error(err, window, cx);
        }
        self.persist_session(cx);
        cx.notify();
    }
}
