use std::path::{Path, PathBuf};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    input::{Editor, EditorState, InputEvent, TabSize},
    menu::AppMenuBar,
    notification::Notification,
    resizable::{h_resizable, resizable_panel},
    status_bar::StatusBar,
    tab::{Tab, TabBar},
    tree::TreeEvent,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::config::{AppConfig, DEFAULT_EDITOR_FONT_SIZE};
use crate::editor::document;
use crate::editor::language::Language;
use crate::editor::tabs::TabList;
use crate::theme;
use crate::ui::file_tree::FileTreePanel;
use crate::ui::welcome;

actions!(
    isengard,
    [
        OpenFolder,
        OpenFile,
        CloseFolder,
        Save,
        SaveAll,
        CloseTab,
        NextTab,
        PrevTab,
        ToggleTheme,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        About,
        Quit,
    ]
);

/// Registers global key bindings. `secondary` is Cmd on macOS and Ctrl elsewhere.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-o", OpenFolder, None),
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-alt-s", SaveAll, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("secondary-=", IncreaseFontSize, None),
        KeyBinding::new("secondary--", DecreaseFontSize, None),
        KeyBinding::new("secondary-0", ResetFontSize, None),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
    // Fallback when the app view is not on the focus path (e.g. while a dialog is open).
    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
}

struct EditorTab {
    path: PathBuf,
    language: Language,
    state: Entity<EditorState>,
    is_modified: bool,
    _subscriptions: Vec<Subscription>,
}

impl EditorTab {
    fn label(&self) -> String {
        let name = document::file_name(&self.path);
        if self.is_modified {
            format!("{name} •")
        } else {
            name
        }
    }
}

pub struct IsengardApp {
    config: AppConfig,
    focus_handle: FocusHandle,
    file_tree: FileTreePanel,
    tabs: TabList<EditorTab>,
    app_menu_bar: Entity<AppMenuBar>,
    /// Tab whose close button is under the pointer (its × turns red).
    hovered_close: Option<PathBuf>,
    /// Set once the user chose to quit despite unsaved changes.
    allow_quit: bool,
    _subscriptions: Vec<Subscription>,
}

impl IsengardApp {
    pub fn new(
        config: AppConfig,
        initial_path: Option<PathBuf>,
        app_menu_bar: Entity<AppMenuBar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let file_tree = FileTreePanel::new(cx);
        let tree_subscription =
            cx.subscribe(file_tree.state(), |this, _, event: &TreeEvent, cx| {
                this.file_tree.handle_event(event, cx);
            });

        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.confirm_quit(window, cx))
                .unwrap_or(true)
        });

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let mut app = Self {
            config,
            focus_handle,
            file_tree,
            tabs: TabList::default(),
            app_menu_bar,
            hovered_close: None,
            allow_quit: false,
            _subscriptions: vec![tree_subscription],
        };

        // Otherwise start on the welcome screen.
        match initial_path {
            Some(path) if path.is_dir() => app.open_folder(&path, window, cx),
            Some(path) if path.is_file() => {
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    app.open_folder(parent, window, cx);
                }
                app.open_file(&path, window, cx);
            }
            _ => {}
        }
        app
    }

    // ---- Folders and files -------------------------------------------------

    pub fn open_folder(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if !folder.is_dir() {
            self.remove_recent_folder(folder, cx);
            notify_error(
                format!("Folder not found: {}", folder.display()),
                window,
                cx,
            );
            return;
        }
        self.file_tree.open_folder(folder, cx);
        self.config.add_recent_folder(folder);
        self.config.save();
        window.set_window_title(&format!("{} — Isengard", document::file_name(folder)));
        cx.notify();
    }

    pub fn remove_recent_folder(&mut self, folder: &Path, cx: &mut Context<Self>) {
        self.config.remove_recent_folder(folder);
        self.config.save();
        cx.notify();
    }

    fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.position(|t| t.path == path) {
            self.activate_tab(ix, window, cx);
            return;
        }
        let text = match document::read_text(path) {
            Ok(text) => text,
            Err(err) => return notify_error(format!("Could not open file: {err:#}"), window, cx),
        };

        let language = Language::from_path(path);
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .line_number(true)
                .indent_guides(true)
                .soft_wrap(false)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                .default_value(text)
        });
        let subscriptions = vec![
            cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let id = state.entity_id();
                    if let Some(tab) = this.tabs.find_mut(|t| t.state.entity_id() == id) {
                        tab.is_modified = true;
                    }
                    cx.notify();
                }
            }),
            // Re-render on cursor moves so the status bar's Ln/Col stays current.
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];

        self.tabs.push(EditorTab {
            path: path.to_path_buf(),
            language,
            state,
            is_modified: false,
            _subscriptions: subscriptions,
        });
        self.file_tree.select_path(path, cx);
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(ix) {
            if let Some(path) = self.tabs.active().map(|t| t.path.clone()) {
                self.file_tree.select_path(&path, cx);
            }
            self.focus_active_editor(window, cx);
            cx.notify();
        }
    }

    fn focus_active_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.active() {
            Some(tab) => tab.state.update(cx, |state, cx| state.focus(window, cx)),
            None => self.focus_handle.focus(window, cx),
        }
    }

    fn save_tab(&mut self, path: &Path, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let Some(tab) = self.tabs.find_mut(|t| t.path == path) else {
            return Ok(());
        };
        let text = tab.state.read(cx).value();
        document::write_text(&tab.path, &text)?;
        tab.is_modified = false;
        cx.notify();
        Ok(())
    }

    fn save_all(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let modified: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.is_modified)
            .map(|t| t.path.clone())
            .collect();
        for path in modified {
            self.save_tab(&path, cx)?;
        }
        Ok(())
    }

    fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.is_modified)
    }

    /// Closes the tab for `path`, asking first if it has unsaved changes.
    fn request_close(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.path == path) else {
            return;
        };
        if tab.is_modified {
            self.confirm_close_tab(path.to_path_buf(), window, cx);
        } else {
            self.close_tab(path, window, cx);
        }
    }

    fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.position(|t| t.path == path) {
            self.tabs.remove(ix);
            // The removed button never reports hover-out.
            self.hovered_close = None;
            self.focus_active_editor(window, cx);
            cx.notify();
        }
    }

    // ---- Dialogs -----------------------------------------------------------

    fn confirm_close_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let name = document::file_name(&path);
        window.open_dialog(cx, move |dialog, _, _| {
            let (app_save, app_discard) = (app.clone(), app.clone());
            let (path_save, path_discard) = (path.clone(), path.clone());
            dialog
                .title("Unsaved changes")
                .child(format!("Save changes to {name} before closing?"))
                .footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("close-cancel")
                                .outline()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("close-discard")
                                .outline()
                                .label("Don't Save")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = app_discard.update(cx, |this, cx| {
                                        this.close_tab(&path_discard, window, cx)
                                    });
                                }),
                        )
                        .child(Button::new("close-save").primary().label("Save").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                _ = app_save.update(cx, |this, cx| {
                                    match this.save_tab(&path_save, cx) {
                                        Ok(()) => this.close_tab(&path_save, window, cx),
                                        Err(err) => notify_error(
                                            format!("Save failed: {err:#}"),
                                            window,
                                            cx,
                                        ),
                                    }
                                });
                            },
                        )),
                )
        });
    }

    /// Returns `true` if the app may close now; otherwise asks the user first.
    fn confirm_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.allow_quit || !self.has_unsaved() {
            return true;
        }
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (app_save, app_discard) = (app.clone(), app.clone());
            dialog
                .title("Quit Isengard?")
                .child("Some files have unsaved changes.")
                .footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("quit-cancel")
                                .outline()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("quit-discard")
                                .outline()
                                .label("Quit Without Saving")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = app_discard.update(cx, |this, cx| this.quit(cx));
                                }),
                        )
                        .child(
                            Button::new("quit-save")
                                .primary()
                                .label("Save All & Quit")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = app_save.update(cx, |this, cx| match this.save_all(cx) {
                                        Ok(()) => this.quit(cx),
                                        Err(err) => notify_error(
                                            format!("Save failed: {err:#}"),
                                            window,
                                            cx,
                                        ),
                                    });
                                }),
                        ),
                )
        });
        false
    }

    fn quit(&mut self, cx: &mut Context<Self>) {
        self.allow_quit = true;
        cx.quit();
    }

    // ---- Actions -----------------------------------------------------------

    fn on_open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(true, window, cx);
    }

    fn on_open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(false, window, cx);
    }

    fn prompt_and_open(&mut self, directories: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: Some(
                if directories {
                    "Open Folder"
                } else {
                    "Open File"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, window| {
            let path = paths.await.ok()?.ok()??.into_iter().next()?;
            this.update_in(window, |this, window, cx| {
                if directories {
                    this.open_folder(&path, window, cx);
                } else {
                    this.open_file(&path, window, cx);
                }
            })
            .ok()
        })
        .detach();
    }

    fn on_close_folder(&mut self, _: &CloseFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree.close(cx);
        window.set_window_title("Isengard");
        cx.notify();
    }

    fn on_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(|t| t.path.clone()) else {
            return;
        };
        match self.save_tab(&path, cx) {
            Ok(()) => window.push_notification(format!("Saved {}", document::file_name(&path)), cx),
            Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
        }
    }

    fn on_save_all(&mut self, _: &SaveAll, window: &mut Window, cx: &mut Context<Self>) {
        match self.save_all(cx) {
            Ok(()) => window.push_notification("Saved all files", cx),
            Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
        }
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.tabs.active().map(|t| t.path.clone()) {
            self.request_close(&path, window, cx);
        }
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(true);
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    fn on_prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(false);
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    fn on_toggle_theme(&mut self, _: &ToggleTheme, window: &mut Window, cx: &mut Context<Self>) {
        self.config.dark_mode = !self.config.dark_mode;
        self.apply_config(window, cx);
    }

    fn on_increase_font(
        &mut self,
        _: &IncreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.change_font_size(1.0);
        self.apply_config(window, cx);
    }

    fn on_decrease_font(
        &mut self,
        _: &DecreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.change_font_size(-1.0);
        self.apply_config(window, cx);
    }

    fn on_reset_font(&mut self, _: &ResetFontSize, window: &mut Window, cx: &mut Context<Self>) {
        self.config.font_size = DEFAULT_EDITOR_FONT_SIZE;
        self.apply_config(window, cx);
    }

    fn apply_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        theme::apply(&self.config, Some(window), cx);
        self.config.save();
        cx.notify();
    }

    fn on_about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, |dialog, _, _| {
            dialog
                .title("About Isengard")
                .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
                .child("A code editor built with GPUI Kit.")
        });
    }

    fn on_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_quit(window, cx) {
            self.quit(cx);
        }
    }

    // ---- Rendering ---------------------------------------------------------

    /// The welcome screen replaces the workspace until a folder or file is open.
    fn show_welcome(&self) -> bool {
        !self.file_tree.is_open() && self.tabs.is_empty()
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let title = match self.file_tree.root_path() {
            Some(root) => format!("{} — Isengard", document::file_name(root)),
            None => "Isengard".to_owned(),
        };
        TitleBar::new()
            .child(
                h_flex()
                    .flex_1()
                    .when(cfg!(target_os = "macos"), |this| {
                        // macOS shows the menus in the system menu bar.
                        this.justify_center()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(title)
                    })
                    .when(!cfg!(target_os = "macos"), |this| {
                        this.child(self.app_menu_bar.clone())
                    }),
            )
            .into_any_element()
    }

    fn render_editor_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(active) = self.tabs.active() else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Select a file to start editing")
                .into_any_element();
        };

        // Muted × that turns destructive (danger red icon + faint red tint) on hover.
        let danger = cx.theme().danger;
        let muted = cx.theme().muted_foreground;
        let close_variant = ButtonCustomVariant::new(cx)
            .foreground(muted)
            .hover(danger.opacity(0.15))
            .active(danger.opacity(0.25));

        let tab_bar = TabBar::new("editor-tabs")
            .selected_index(self.tabs.active_index().unwrap_or_default())
            .on_click(
                cx.listener(|this, ix: &usize, window, cx| this.activate_tab(*ix, window, cx)),
            )
            .children(self.tabs.iter().map(|tab| {
                let path = tab.path.clone();
                // Button's hover style can only change the background, and an Icon
                // resolves its color at render time, so the red icon is driven by
                // hover state tracked on a wrapper.
                let is_hovered = self.hovered_close.as_deref() == Some(tab.path.as_path());
                let hover_path = tab.path.clone();
                Tab::new().label(tab.label()).suffix(
                    div()
                        .id(SharedString::from(format!(
                            "close-tab-hover-{}",
                            tab.path.display()
                        )))
                        // The tab's label padding is 12px; mr_2 plus the button's 4px
                        // inset puts the × the same 12px from the tab's right border.
                        .mr_2()
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            let path = hover_path.clone();
                            this.hovered_close = hovered.then_some(path);
                            cx.notify();
                        }))
                        .child(
                            Button::new(SharedString::from(format!(
                                "close-tab-{}",
                                tab.path.display()
                            )))
                            .custom(close_variant)
                            .xsmall()
                            .icon(Icon::new(IconName::Close).text_color(if is_hovered {
                                danger
                            } else {
                                muted
                            }))
                            .tooltip("Close")
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.request_close(&path, window, cx);
                                },
                            )),
                        ),
                )
            }));

        v_flex()
            .size_full()
            .child(tab_bar)
            .child(
                div().flex_1().min_h_0().child(
                    Editor::new(&active.state)
                        .bordered(false)
                        .p_0()
                        .h_full()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(cx.theme().mono_font_size),
                ),
            )
            .into_any_element()
    }

    /// The status bar describes the active file, so it only exists while one is open.
    fn render_status_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tab = self.tabs.active()?;
        let path = match self.file_tree.root_path() {
            Some(root) => tab.path.strip_prefix(root).unwrap_or(&tab.path),
            None => &tab.path,
        };
        let cursor = tab.state.read(cx).cursor_position();
        let bar = StatusBar::new()
            .left(div().child(path.display().to_string()))
            .right(
                h_flex()
                    .gap_4()
                    .child(format!(
                        "Ln {}, Col {}",
                        cursor.line + 1,
                        cursor.character + 1
                    ))
                    .child(tab.language.display_name()),
            )
            .text_xs();
        Some(bar.into_any_element())
    }
}

impl Render for IsengardApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.show_welcome() {
            welcome::render(&self.config.recent_folders, cx).into_any_element()
        } else if self.file_tree.is_open() {
            let app = cx.entity().downgrade();
            let tree = self.file_tree.render(
                move |path, window, cx| {
                    _ = app.update(cx, |this, cx| this.open_file(&path, window, cx));
                },
                cx,
            );
            h_resizable("main-split")
                .child(resizable_panel().size(px(260.)).child(tree))
                .child(resizable_panel().child(self.render_editor_area(cx)))
                .into_any_element()
        } else {
            self.render_editor_area(cx).into_any_element()
        };

        v_flex()
            .id("isengard")
            .key_context("Isengard")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(cx.theme().font_family.clone())
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_open_file))
            .on_action(cx.listener(Self::on_close_folder))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_all))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_increase_font))
            .on_action(cx.listener(Self::on_decrease_font))
            .on_action(cx.listener(Self::on_reset_font))
            .on_action(cx.listener(Self::on_about))
            .on_action(cx.listener(Self::on_quit))
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h_0().child(body))
            .children(self.render_status_bar(cx))
    }
}

fn notify_error(message: String, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::error(message), cx);
}
