//! Bottom integrated terminal: show/hide, create, multi-root cwd.
use super::*;
use super::terminal_cwd::TerminalCwdPicker;

impl IsengardApp {
    pub(super) fn on_toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal.read(cx).is_visible() {
            self.terminal.update(cx, |term, cx| term.hide(cx));
            self.persist_session(cx);
            self.focus_active_editor(window, cx);
            cx.notify();
            return;
        }
        self.open_terminal(window, cx);
    }

    pub(super) fn on_new_terminal(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_terminal(window, cx);
    }

    pub(super) fn on_terminal_tabs_changed(
        &mut self,
        _: &TerminalTabsChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.persist_session(cx);
        if !self.terminal.read(cx).has_sessions() {
            self.focus_active_editor(window, cx);
        }
        cx.notify();
    }

    /// Show the panel: restore existing sessions without asking, or create the first.
    /// No-op on the welcome screen (needs at least one workspace folder).
    pub(super) fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.folders().is_empty() {
            return notify_error("Open a folder before starting a terminal", window, cx);
        }
        if self.terminal.read(cx).has_sessions() {
            self.terminal
                .update(cx, |term, cx| term.show(window, cx));
            self.persist_session(cx);
            cx.notify();
            return;
        }
        self.create_terminal(window, cx);
    }

    /// Always spawn a new terminal tab (asks for root when multi-root).
    pub(super) fn create_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folders = self.workspace.folders().to_vec();
        if folders.is_empty() {
            return notify_error("Open a folder before starting a terminal", window, cx);
        }
        if folders.len() > 1 {
            self.pick_terminal_cwd(folders, false, window, cx);
            return;
        }
        let cwd = crate::terminal::pty::default_cwd(&folders);
        self.add_terminal_in(cwd, window, cx);
    }

    pub(super) fn add_terminal_in(
        &mut self,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .terminal
            .update(cx, |term, cx| term.add_session(cwd, window, cx));
        if let Err(err) = result {
            return notify_error(err, window, cx);
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// Dialog listing workspace roots. `hide_on_cancel` is for workspace switches
    /// where the old session cwd is already invalid.
    pub(super) fn pick_terminal_cwd(
        &self,
        folders: Vec<PathBuf>,
        hide_on_cancel: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        let muted = cx.theme().muted_foreground;
        // Create once — the dialog builder may run every paint.
        let picker = cx.new(|cx| {
            TerminalCwdPicker::new(folders, hide_on_cancel, app, cx)
        });
        let picker_for_dialog = picker.clone();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let picker_cancel = picker_for_dialog.clone();
            let picker_open = picker_for_dialog.clone();
            center_dialog(
                dialog
                    .title("Open Terminal in…")
                    .child(
                        v_flex()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(
                                        "Choose a workspace folder for the shell. ↑↓ to move, Enter to open.",
                                    ),
                            )
                            .child(picker_for_dialog.clone()),
                    )
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("terminal-cwd-cancel")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| {
                                        picker_cancel.update(cx, |picker, cx| {
                                            picker.cancel(window, cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new("terminal-cwd-open")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Open")
                                    .on_click(move |_, window, cx| {
                                        picker_open.update(cx, |picker, cx| {
                                            picker.confirm(window, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }
}
