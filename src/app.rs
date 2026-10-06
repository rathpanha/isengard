use std::path::Path;
use std::time::{Duration, Instant};

use egui::{Key, KeyboardShortcut, Modifiers};

use crate::config::AppConfig;
use crate::ui::editor_workspace::EditorWorkspace;
use crate::ui::file_tree::FileTreePanel;
use crate::ui::welcome::{self, WelcomeAction};

const CONFIG_KEY: &str = "isengard_config";
const STATUS_TIMEOUT: Duration = Duration::from_secs(4);

const OPEN_FOLDER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const CLOSE_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);

pub struct IsengardApp {
    config: AppConfig,
    file_tree: FileTreePanel,
    workspace: EditorWorkspace,
    status: Option<(String, Instant)>,
    /// Style last applied to the context, to avoid resetting it every frame.
    applied_style: Option<(bool, u32)>,
    show_about: bool,
    show_quit_dialog: bool,
    allow_quit: bool,
}

impl IsengardApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        let config: AppConfig = cc
            .storage
            .and_then(|s| eframe::get_value(s, CONFIG_KEY))
            .unwrap_or_default();

        let mut app = Self {
            config,
            file_tree: FileTreePanel::default(),
            workspace: EditorWorkspace::default(),
            status: None,
            applied_style: None,
            show_about: false,
            show_quit_dialog: false,
            allow_quit: false,
        };

        // `isengard <folder>` opens a folder; `isengard <file>` opens its parent and the file.
        let arg = std::env::args_os().nth(1).map(std::path::PathBuf::from);
        match arg {
            Some(path) if path.is_dir() => app.open_folder(&path),
            Some(path) if path.is_file() => {
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    app.open_folder(parent);
                }
                app.open_file(&path);
            }
            // Otherwise start on the welcome screen.
            _ => {}
        }
        app
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn open_folder_dialog(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.open_folder(&folder);
        }
    }

    fn open_folder(&mut self, folder: &Path) {
        if !folder.is_dir() {
            self.config.remove_recent_folder(folder);
            self.set_status(format!("Folder not found: {}", folder.display()));
            return;
        }
        self.file_tree.open_folder(folder);
        self.config.add_recent_folder(folder);
    }

    fn handle_welcome_action(&mut self, action: WelcomeAction) {
        match action {
            WelcomeAction::OpenFolder => self.open_folder_dialog(),
            WelcomeAction::OpenFile => self.open_file_dialog(),
            WelcomeAction::OpenRecent(folder) => self.open_folder(&folder),
            WelcomeAction::RemoveRecent(folder) => self.config.remove_recent_folder(&folder),
        }
    }

    fn open_file_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(root) = self.file_tree.root_path() {
            dialog = dialog.set_directory(root);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_file(&path);
        }
    }

    fn open_file(&mut self, path: &Path) {
        match self.workspace.open_file(path) {
            Ok(()) => self.file_tree.set_selected(Some(path.to_path_buf())),
            Err(err) => self.set_status(format!("Could not open file: {err:#}")),
        }
    }

    fn save_active(&mut self) {
        match self.workspace.save_active() {
            Ok(Some(path)) => self.set_status(format!("Saved {}", path.display())),
            Ok(None) => {}
            Err(err) => self.set_status(format!("Save failed: {err:#}")),
        }
    }

    fn apply_style(&mut self, ctx: &egui::Context) {
        let wanted = (self.config.dark_mode, self.config.font_size.to_bits());
        if self.applied_style == Some(wanted) {
            return;
        }
        let mut visuals = if self.config.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        square_corners(&mut visuals);
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            if let Some(font) = style.text_styles.get_mut(&egui::TextStyle::Monospace) {
                font.size = self.config.font_size;
            }
        });
        self.applied_style = Some(wanted);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.workspace.is_dialog_open() || self.show_quit_dialog {
            return;
        }
        let (open, save, close, prev, next) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&OPEN_FOLDER),
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&CLOSE_TAB),
                // Shift+Tab variant first: Ctrl+Tab would also match it.
                i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::CTRL, Key::Tab),
            )
        });
        if open {
            self.open_folder_dialog();
        }
        if save {
            self.save_active();
        }
        if close {
            self.workspace.request_close_active();
        }
        if prev {
            self.workspace.cycle_tab(false);
        }
        if next {
            self.workspace.cycle_tab(true);
        }
    }

    /// Intercepts window close while there are unsaved changes.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_quit
            && self.workspace.has_unsaved()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_quit_dialog = true;
        }
    }

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add(egui::Button::new("Open Folder…").shortcut_text(ctx.format_shortcut(&OPEN_FOLDER)))
                        .clicked()
                    {
                        ui.close_menu();
                        self.open_folder_dialog();
                    }
                    if ui.button("Open File…").clicked() {
                        ui.close_menu();
                        self.open_file_dialog();
                    }
                    ui.add_enabled_ui(!self.config.recent_folders.is_empty(), |ui| {
                        ui.menu_button("Open Recent", |ui| {
                            for folder in self.config.recent_folders.clone() {
                                if ui.button(folder.display().to_string()).clicked() {
                                    ui.close_menu();
                                    self.open_folder(&folder);
                                }
                            }
                        });
                    });
                    if ui
                        .add_enabled(self.file_tree.is_open(), egui::Button::new("Close Folder"))
                        .clicked()
                    {
                        ui.close_menu();
                        self.file_tree.close();
                    }
                    ui.separator();
                    if ui
                        .add(egui::Button::new("Save").shortcut_text(ctx.format_shortcut(&SAVE)))
                        .clicked()
                    {
                        ui.close_menu();
                        self.save_active();
                    }
                    if ui.button("Save All").clicked() {
                        ui.close_menu();
                        match self.workspace.save_all() {
                            Ok(()) => self.set_status("Saved all files"),
                            Err(err) => self.set_status(format!("Save failed: {err:#}")),
                        }
                    }
                    if ui
                        .add(egui::Button::new("Close Tab").shortcut_text(ctx.format_shortcut(&CLOSE_TAB)))
                        .clicked()
                    {
                        ui.close_menu();
                        self.workspace.request_close_active();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ui.close_menu();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| {
                    let theme_label = if self.config.dark_mode { "Light Theme" } else { "Dark Theme" };
                    if ui.button(theme_label).clicked() {
                        ui.close_menu();
                        self.config.dark_mode = !self.config.dark_mode;
                    }
                    ui.separator();
                    if ui.button("Increase Font Size").clicked() {
                        self.config.change_font_size(1.0);
                    }
                    if ui.button("Decrease Font Size").clicked() {
                        self.config.change_font_size(-1.0);
                    }
                    if ui.button("Reset Font Size").clicked() {
                        self.config.font_size = AppConfig::default().font_size;
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About Isengard").clicked() {
                        ui.close_menu();
                        self.show_about = true;
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        if self.status.as_ref().is_some_and(|(_, at)| at.elapsed() > STATUS_TIMEOUT) {
            self.status = None;
        }
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(tab) = self.workspace.active_tab() {
                    let path = match self.file_tree.root_path() {
                        Some(root) => tab.buffer.path.strip_prefix(root).unwrap_or(&tab.buffer.path),
                        None => &tab.buffer.path,
                    };
                    ui.label(path.display().to_string());
                    if let Some((line, col)) = tab.cursor {
                        ui.separator();
                        ui.label(format!("Ln {line}, Col {col}"));
                    }
                    ui.separator();
                    ui.label(tab.buffer.language.display_name());
                } else {
                    ui.weak("Ready");
                }
                if let Some((msg, _)) = &self.status {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(msg);
                    });
                    ctx.request_repaint_after(STATUS_TIMEOUT);
                }
            });
        });
    }

    fn quit_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_quit_dialog {
            return;
        }
        egui::Window::new("Quit Isengard?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Some files have unsaved changes.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save All & Quit").clicked() {
                        match self.workspace.save_all() {
                            Ok(()) => self.quit(ctx),
                            Err(err) => {
                                self.show_quit_dialog = false;
                                self.set_status(format!("Save failed: {err:#}"));
                            }
                        }
                    }
                    if ui.button("Quit Without Saving").clicked() {
                        self.quit(ctx);
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.show_quit_dialog = false;
                    }
                });
            });
    }

    /// The welcome screen replaces the whole workspace until a folder or file is open.
    fn show_welcome(&self) -> bool {
        !self.file_tree.is_open() && self.workspace.is_empty()
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.allow_quit = true;
        self.show_quit_dialog = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("About Isengard")
            .open(&mut self.show_about)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Isengard");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A small code editor built with egui and tree-sitter.");
            });
    }
}

impl eframe::App for IsengardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_style(ctx);
        self.handle_close_request(ctx);
        self.handle_shortcuts(ctx);

        self.menu_bar(ctx);
        if !self.show_welcome() {
            self.status_bar(ctx);
        }

        let modal_open = self.workspace.is_dialog_open() || self.show_quit_dialog;

        if self.file_tree.is_open() {
            egui::SidePanel::left("file_tree")
                .resizable(true)
                .default_width(240.0)
                .width_range(140.0..=600.0)
                .show(ctx, |ui| {
                    ui.add_enabled_ui(!modal_open, |ui| {
                        if let Some(path) = self.file_tree.show(ui) {
                            self.open_file(&path);
                        }
                    });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_welcome() {
                let shortcut = ctx.format_shortcut(&OPEN_FOLDER);
                if let Some(action) = welcome::show(ui, &self.config.recent_folders, &shortcut) {
                    self.handle_welcome_action(action);
                }
                return;
            }
            ui.add_enabled_ui(!modal_open, |ui| {
                self.workspace.show(ui, self.config.font_size);
            });
        });

        if let Some(err) = self.workspace.show_close_dialog(ctx) {
            self.set_status(format!("Save failed: {err}"));
        }
        self.quit_dialog(ctx);
        self.about_window(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, CONFIG_KEY, &self.config);
    }
}

/// Block style: no rounded corners on any widget, window, or menu.
fn square_corners(visuals: &mut egui::Visuals) {
    visuals.window_rounding = egui::Rounding::ZERO;
    visuals.menu_rounding = egui::Rounding::ZERO;
    let widgets = &mut visuals.widgets;
    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.rounding = egui::Rounding::ZERO;
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "jetbrains_mono".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")),
    );
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "jetbrains_mono".to_owned());
    ctx.set_fonts(fonts);
}
