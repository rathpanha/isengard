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

    pub(super) fn on_open_config_file(
        &mut self,
        _: &OpenConfigFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = AppConfig::default_path() else {
            return notify_error("Could not resolve config directory", window, cx);
        };
        if !path.exists() {
            self.config.save();
        }
        // Ensure parent exists even if save was a no-op somehow.
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if !path.exists()
            && let Err(err) = self.config.save_to(&path)
        {
            return notify_error(format!("Could not create config: {err:#}"), window, cx);
        }
        self.open_file(&path, true, window, cx);
    }

    pub(super) fn on_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_quit(window, cx) {
            self.quit(cx);
        }
    }

    /// The welcome screen replaces the workspace until a folder or file is open.
    pub(super) fn show_welcome(&self) -> bool {
        self.workspace.is_empty() && self.tabs.is_empty()
    }
}
