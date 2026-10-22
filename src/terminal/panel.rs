//! Bottom terminal panel: multi-tab shells + Kit chrome.

use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::editor::document;
use crate::editor::tabs::reorder_for_drop_before;
use crate::terminal::colors::ColorPalette;
use crate::terminal::pty::{PtySession, TerminalCommand};
use crate::terminal::view::{TerminalConfig, TerminalView};
use crate::theme;
use crate::ui::tab_drag::{TabDragPreview, TerminalTabDrag};

/// Runs when a one-off command tab's program exits.
type ExitHook = Box<dyn Fn(&mut Window, &mut App)>;

struct TerminalTab {
    id: usize,
    title: String,
    cwd: PathBuf,
    /// One-off program tabs (agent login) are not restored with the session.
    persist: bool,
    view: Entity<TerminalView>,
    /// Kept so the PTY stays alive for the life of the tab.
    _session: PtySession,
}

/// Host view for the integrated terminal: tab strip + active [`TerminalView`].
pub struct TerminalPanel {
    visible: bool,
    tabs: Vec<TerminalTab>,
    active: usize,
    next_id: usize,
}

impl TerminalPanel {
    pub fn new(_workspace_folders: &[PathBuf]) -> Self {
        Self {
            visible: false,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// True when at least one shell session exists (even if the panel is minimized).
    pub fn has_sessions(&self) -> bool {
        !self.tabs.is_empty()
    }

    pub fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        self.tabs
            .get(self.active)
            .map(|tab| tab.view.read(cx).focus_handle().clone())
    }

    /// Selected text in the active terminal tab, if any.
    pub fn active_selection_text(&self, cx: &App) -> Option<String> {
        self.tabs
            .get(self.active)
            .and_then(|tab| tab.view.read(cx).selection_text())
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = true;
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.visible = false;
        cx.notify();
    }

    /// Drop every session (e.g. workspace switch / welcome).
    pub fn clear_sessions(&mut self, cx: &mut Context<Self>) {
        self.tabs.clear();
        self.active = 0;
        cx.notify();
    }

    /// Cwds of the shell tabs to restore (one-off program tabs are skipped).
    pub fn session_cwds(&self) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter(|t| t.persist)
            .map(|t| t.cwd.clone())
            .collect()
    }

    /// Index of the active tab within [`Self::session_cwds`].
    pub fn active_index(&self) -> usize {
        self.tabs
            .iter()
            .take(self.active)
            .filter(|t| t.persist)
            .count()
    }

    /// Respawn tabs from saved cwds (skips missing directories).
    pub fn restore_sessions(
        &mut self,
        cwds: &[PathBuf],
        active: usize,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tabs.clear();
        self.active = 0;
        for cwd in cwds.iter().filter(|p| p.is_dir()) {
            if let Ok(tab) = self.spawn_tab(cwd.clone(), None, cx) {
                self.tabs.push(tab);
            }
        }
        if self.tabs.is_empty() {
            self.visible = false;
            cx.notify();
            return;
        }
        self.active = active.min(self.tabs.len() - 1);
        self.visible = visible;
        if visible {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Spawn a new tab in `cwd`, select it, and show the panel.
    pub fn add_session(
        &mut self,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let tab = self.spawn_tab(cwd, None, cx)?;
        self.push_active(tab, window, cx);
        Ok(())
    }

    /// Run `command` in a new tab titled `title` (not restored with the session).
    /// `on_exit` runs when the program exits (the tab stays open).
    pub fn add_command_session(
        &mut self,
        cwd: PathBuf,
        title: String,
        command: &TerminalCommand,
        on_exit: impl Fn(&mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut tab = self.spawn_tab(cwd, Some((command, Box::new(on_exit))), cx)?;
        tab.title = title;
        self.push_active(tab, window, cx);
        Ok(())
    }

    fn push_active(&mut self, tab: TerminalTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.visible = true;
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn select_tab(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| t.id == id) {
            self.active = ix;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    /// Move tab `dragged_id` before the tab `before_id` (left-border drop).
    pub fn drop_tab_before(
        &mut self,
        dragged_id: usize,
        before_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.tabs.iter().position(|t| t.id == dragged_id) else {
            return;
        };
        let Some(before_ix) = self.tabs.iter().position(|t| t.id == before_id) else {
            return;
        };
        let Some((from, to)) = reorder_for_drop_before(from, before_ix) else {
            return;
        };
        self.reorder_tab(from, to, window, cx);
    }

    /// Move tab `dragged_id` to the end of the strip.
    pub fn drop_tab_at_end(
        &mut self,
        dragged_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.tabs.iter().position(|t| t.id == dragged_id) else {
            return;
        };
        let to = self.tabs.len().saturating_sub(1);
        if from == to {
            return;
        }
        self.reorder_tab(from, to, window, cx);
    }

    fn reorder_tab(&mut self, from: usize, to: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        // Activate the dragged tab at its new index.
        self.active = to;
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        // Close × tooltip can stick if the tab unmounts before Kit's hide settles.
        window.dismiss_tooltip(cx);
        self.tabs.remove(ix);
        if self.tabs.is_empty() {
            self.active = 0;
            self.visible = false;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
            self.focus_active(window, cx);
        } else if ix < self.active {
            self.active -= 1;
            self.focus_active(window, cx);
        } else {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            let handle = tab.view.read(cx).focus_handle().clone();
            handle.focus(window, cx);
        }
    }

    fn spawn_tab(
        &mut self,
        cwd: PathBuf,
        command: Option<(&TerminalCommand, ExitHook)>,
        cx: &mut Context<Self>,
    ) -> Result<TerminalTab, String> {
        let cols = 80u16;
        let rows = 24u16;
        let persist = command.is_none();
        let (command, on_exit) = match command {
            Some((command, hook)) => (Some(command), Some(hook)),
            None => (None, None),
        };
        let (session, writer, reader) = match PtySession::spawn(&cwd, command, cols, rows) {
            Ok(parts) => parts,
            Err(err) => {
                log::error!("terminal: failed to spawn PTY: {err:#}");
                return Err(format!("Failed to start terminal: {err:#}"));
            }
        };

        let master = session.master_handle();
        let config = TerminalConfig {
            font_family: theme::FONT_FAMILY.into(),
            font_size: cx.theme().mono_font_size,
            cols: cols as usize,
            rows: rows as usize,
            scrollback: 10_000,
            line_height_multiplier: 1.0,
            padding: Edges::all(px(8.)),
            colors: palette_from_theme(cx),
        };

        let view = cx.new(|cx| {
            let view =
                TerminalView::new(writer, reader, config, cx).with_resize_callback(move |c, r| {
                    let _ = master.lock().resize(portable_pty::PtySize {
                        cols: c as u16,
                        rows: r as u16,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                });
            match on_exit {
                Some(hook) => view.with_exit_callback(move |window, cx| hook(window, cx)),
                None => view,
            }
        });

        let id = self.next_id;
        self.next_id += 1;
        let title = self.title_for(&cwd);
        Ok(TerminalTab {
            id,
            title,
            cwd,
            persist,
            view,
            _session: session,
        })
    }

    fn title_for(&self, cwd: &PathBuf) -> String {
        let base = document::file_name(cwd);
        let same = self.tabs.iter().filter(|t| t.cwd == *cwd).count();
        if same == 0 {
            base
        } else {
            format!("{base} ({})", same + 1)
        }
    }
}

fn palette_from_theme(cx: &App) -> ColorPalette {
    let bg = cx.theme().sidebar;
    let fg = cx.theme().foreground;
    let to_rgb = |c: Hsla| {
        let rgba = c.to_rgb();
        (
            (rgba.r * 255.) as u8,
            (rgba.g * 255.) as u8,
            (rgba.b * 255.) as u8,
        )
    };
    let (br, bg_g, bb) = to_rgb(bg);
    let (fr, fg_g, fb) = to_rgb(fg);
    ColorPalette::builder()
        .background(br, bg_g, bb)
        .foreground(fr, fg_g, fb)
        .cursor(fr, fg_g, fb)
        .build()
}

impl Render for TerminalPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }

        let active_id = self.tabs.get(self.active).map(|t| t.id);
        let active_view = self.tabs.get(self.active).map(|t| t.view.clone());

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child({
                        let mut tab_els: Vec<AnyElement> = self
                            .tabs
                            .iter()
                            .map(|tab| {
                                let id = tab.id;
                                let before_id = id;
                                let selected = Some(id) == active_id;
                                let title = tab.title.clone();
                                let drag = TerminalTabDrag {
                                    id,
                                    title: SharedString::from(title.clone()),
                                };
                                h_flex()
                                    .id(ElementId::Name(
                                        SharedString::from(format!("term-tab-{id}")),
                                    ))
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .cursor_pointer()
                                    .rounded(cx.theme().radius)
                                    .when(selected, |this| this.bg(cx.theme().muted))
                                    .on_drag(drag, |drag, _, _, cx| {
                                        cx.new(|_| TabDragPreview::new(drag.title.clone()))
                                    })
                                    .drag_over::<TerminalTabDrag>(|this, _, _, cx| {
                                        this.border_l_2().border_color(cx.theme().drag_border)
                                    })
                                    .on_drop(cx.listener(
                                        move |this, drag: &TerminalTabDrag, window, cx| {
                                            this.drop_tab_before(drag.id, before_id, window, cx);
                                            window.dispatch_action(
                                                Box::new(crate::app::TerminalTabsChanged),
                                                cx,
                                            );
                                        },
                                    ))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_tab(id, window, cx);
                                        window.dispatch_action(
                                            Box::new(crate::app::TerminalTabsChanged),
                                            cx,
                                        );
                                    }))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1()
                                            .text_sm()
                                            .text_color(if selected {
                                                cx.theme().foreground
                                            } else {
                                                cx.theme().muted_foreground
                                            })
                                            .child(IconName::SquareTerminal)
                                            .child(title),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "term-tab-close-{id}"
                                        )))
                                        .ghost()
                                        .cursor_pointer()
                                        .xsmall()
                                        .icon(IconName::Close)
                                        .tooltip("Close Terminal")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.close_tab(id, window, cx);
                                            window.dispatch_action(
                                                Box::new(crate::app::TerminalTabsChanged),
                                                cx,
                                            );
                                        })),
                                    )
                                    .into_any_element()
                            })
                            .collect();
                        tab_els.push(
                            div()
                                .id("terminal-tabs-end")
                                .h_full()
                                .flex_1()
                                .min_w_16()
                                .drag_over::<TerminalTabDrag>(|this, _, _, cx| {
                                    this.bg(cx.theme().drop_target)
                                })
                                .on_drop(cx.listener(
                                    move |this, drag: &TerminalTabDrag, window, cx| {
                                        this.drop_tab_at_end(drag.id, window, cx);
                                        window.dispatch_action(
                                            Box::new(crate::app::TerminalTabsChanged),
                                            cx,
                                        );
                                    },
                                ))
                                .into_any_element(),
                        );
                        h_flex()
                            .id("terminal-tabs")
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .overflow_x_scroll()
                            .children(tab_els)
                    })
                    .child(
                        Button::new("terminal-new")
                            .ghost()
                            .cursor_pointer()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New Terminal")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::app::NewTerminal), cx);
                            }),
                    )
                    .child(
                        Button::new("terminal-minimize")
                            .ghost()
                            .cursor_pointer()
                            .xsmall()
                            .icon(IconName::Minus)
                            .tooltip("Minimize Terminal")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(
                                    Box::new(crate::app::ToggleTerminal),
                                    cx,
                                );
                            }),
                    ),
            )
            .child(
                div().flex_1().min_h_0().w_full().when_some(
                    active_view,
                    |this, view| this.child(view),
                ),
            )
            .into_any_element()
    }
}
