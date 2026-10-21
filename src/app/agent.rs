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
            if self.workspace.folders().is_empty() {
                return notify_error("Open a folder before using the agent", window, cx);
            }
            self.agent_visible = true;
            self.agent.update(cx, |agent, cx| agent.ensure_started(cx));
            let input = self.agent.read(cx).input().clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// Workspace switch: the agent session belongs to the old folders.
    pub(super) fn reset_agent_for_workspace(&mut self, cx: &mut Context<Self>) {
        let cwd = self.workspace.folders().first().cloned();
        let settings = self.config.agent.clone();
        self.agent
            .update(cx, |agent, cx| agent.reset(cwd, settings, cx));
    }

    pub(super) fn handle_agent_event(
        &mut self,
        event: &AgentPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            AgentPanelEvent::Login { title, command } => {
                self.run_agent_login(title.clone(), command, window, cx);
            }
            AgentPanelEvent::Hide => {
                if self.agent_visible {
                    self.on_toggle_agent_panel(&ToggleAgentPanel, window, cx);
                }
            }
        }
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
