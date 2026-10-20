//! Multi-root “Open Terminal in…” dialog content.
use super::*;

/// Folder list for the multi-root “Open Terminal in…” dialog.
///
/// Kit dialogs keep a focus trap on their own handle, so `on_key_down` on the
/// list never fires. We intercept keystrokes for the life of this entity instead.
pub(super) struct TerminalCwdPicker {
    pub(super) folders: Vec<PathBuf>,
    pub(super) selected: usize,
    pub(super) hide_on_cancel: bool,
    pub(super) app: WeakEntity<IsengardApp>,
    pub(super) _keys: Subscription,
}

impl TerminalCwdPicker {
    pub(super) fn new(
        folders: Vec<PathBuf>,
        hide_on_cancel: bool,
        app: WeakEntity<IsengardApp>,
        cx: &mut Context<Self>,
    ) -> Self {
        let entity = cx.weak_entity();
        // ponytail: dialog focus trap eats on_key_down; intercept until Kit
        // exposes the dialog focus handle to content.
        let _keys = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(entity) = entity.upgrade() else {
                return;
            };
            entity.update(cx, |this, cx| {
                match event.keystroke.key.as_str() {
                    "up" => {
                        if this.selected > 0 {
                            this.selected -= 1;
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }
                    "down" => {
                        if this.selected + 1 < this.folders.len() {
                            this.selected += 1;
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }
                    "enter" => {
                        this.confirm(window, cx);
                        cx.stop_propagation();
                    }
                    "escape" => {
                        this.cancel(window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            });
        });
        Self {
            folders,
            selected: 0,
            hide_on_cancel,
            app,
            _keys,
        }
    }

    pub(super) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.folders.get(self.selected).cloned() else {
            return;
        };
        window.close_dialog(cx);
        _ = self.app.update(cx, |this, cx| {
            this.add_terminal_in(folder, window, cx);
        });
    }

    pub(super) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        if self.hide_on_cancel {
            _ = self.app.update(cx, |this, cx| {
                this.terminal.update(cx, |term, cx| {
                    term.clear_sessions(cx);
                    term.hide(cx);
                });
                this.persist_session(cx);
                cx.notify();
            });
        }
    }
}

impl Render for TerminalCwdPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;
        v_flex()
            .id("terminal-cwd-picker")
            .w_full()
            .gap_1()
            .children(self.folders.iter().enumerate().map(|(ix, folder)| {
                let name = document::file_name(folder);
                ListItem::new(ix)
                    .w_full()
                    .selected(ix == selected)
                    .cursor_pointer()
                    .accessibility_label(name.clone())
                    .child(div().px_2().py_1().child(name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.selected = ix;
                        this.confirm(window, cx);
                    }))
            }))
    }
}

