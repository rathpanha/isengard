//! Sidebar toggle, About, Open Config, quit.
use super::*;

impl IsengardApp {
    pub(super) fn on_toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.file_tree.is_open() {
            return;
        }
        self.sidebar_visible = !self.sidebar_visible;
        self.persist_session(cx);
        cx.notify();
    }
    pub(super) fn on_about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, |dialog, window, _| {
            center_dialog(
                dialog
                    .title("About Isengard")
                    .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
                    .child("The already configured editor.")
                    .child("Dark mode, one font, no preference archaeology."),
                window,
            )
        });
    }

    pub(super) fn on_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        // A plain Quit is not a pending "restart to update".
        self.relaunch_exe = None;
        if self.confirm_quit(window, cx) {
            self.quit(cx);
        }
    }

    /// The welcome screen replaces the workspace until a folder or file is open.
    pub(super) fn show_welcome(&self) -> bool {
        self.workspace.is_empty() && self.tabs.is_empty()
    }
}
