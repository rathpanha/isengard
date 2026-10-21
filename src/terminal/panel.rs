//! Bottom terminal panel: multi-tab shells + Kit chrome.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::editor::document;
use crate::editor::tabs::reorder_for_drop_before;
use crate::terminal::colors::ColorPalette;
use crate::terminal::output_buffer::{
    CapturedExit, DEFAULT_OUTPUT_BYTE_LIMIT, OutputBuffer, SharedExit, TeeReader,
};
use crate::terminal::pty::{PtySession, TerminalCommand};
use crate::terminal::view::{TerminalConfig, TerminalView};
use crate::theme;
use crate::ui::tab_drag::{TabDragPreview, TerminalTabDrag};

struct TerminalTab {
    id: usize,
    title: String,
    cwd: PathBuf,
    /// One-off program tabs (agent login / ACP) are not restored with the session.
    persist: bool,
    /// ACP `terminalId` when this tab was created for the agent.
    agent_id: Option<String>,
    view: Entity<TerminalView>,
    /// Kept so the PTY stays alive for the life of the tab.
    session: PtySession,
}

struct AgentTerm {
    tab_id: usize,
    capture: Arc<Mutex<OutputBuffer>>,
    exit: Arc<SharedExit>,
}

/// Snapshot for ACP `terminal/output`.
pub struct AgentTerminalOutput {
    pub output: String,
    pub truncated: bool,
    pub exit: Option<CapturedExit>,
}

/// Host view for the integrated terminal: tab strip + active [`TerminalView`].
pub struct TerminalPanel {
    visible: bool,
    tabs: Vec<TerminalTab>,
    active: usize,
    next_id: usize,
    next_agent_id: u64,
    agent_terms: HashMap<String, AgentTerm>,
}

impl TerminalPanel {
    pub fn new(_workspace_folders: &[PathBuf]) -> Self {
        Self {
            visible: false,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            next_agent_id: 1,
            agent_terms: HashMap::new(),
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
        self.agent_terms.clear();
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
            if let Ok(tab) = self.spawn_tab(cwd.clone(), None, None, cx) {
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
        let tab = self.spawn_tab(cwd, None, None, cx)?;
        self.push_active(tab, window, cx);
        Ok(())
    }

    /// Run `command` in a new tab titled `title` (not restored with the session).
    pub fn add_command_session(
        &mut self,
        cwd: PathBuf,
        title: String,
        command: &TerminalCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut tab = self.spawn_tab(cwd, Some(command), None, cx)?;
        tab.title = title;
        self.push_active(tab, window, cx);
        Ok(())
    }

    /// ACP `terminal/create`: spawn a command tab, tee output, return `terminalId`.
    pub fn create_agent_terminal(
        &mut self,
        command: &TerminalCommand,
        cwd: PathBuf,
        output_byte_limit: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let limit = output_byte_limit
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_OUTPUT_BYTE_LIMIT);
        let capture = Arc::new(Mutex::new(OutputBuffer::new(limit)));
        let exit = Arc::new(SharedExit::default());
        let acp_id = format!("term_{}", self.next_agent_id);
        self.next_agent_id += 1;

        let title = {
            let mut t = command.program.file_name().map_or_else(
                || command.program.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            if !command.args.is_empty() {
                t.push(' ');
                t.push_str(&command.args.join(" "));
            }
            if t.chars().count() > 40 {
                t = t.chars().take(39).collect::<String>() + "…";
            }
            t
        };

        let mut tab = self.spawn_tab(cwd, Some(command), Some(capture.clone()), cx)?;
        tab.title = title;
        tab.agent_id = Some(acp_id.clone());
        tab.session.watch_exit(exit.clone());
        let tab_id = tab.id;
        self.push_active(tab, window, cx);
        self.agent_terms.insert(
            acp_id.clone(),
            AgentTerm {
                tab_id,
                capture,
                exit,
            },
        );
        Ok(acp_id)
    }

    pub fn agent_terminal_output(&self, id: &str) -> Result<AgentTerminalOutput, String> {
        let term = self
            .agent_terms
            .get(id)
            .ok_or_else(|| format!("Unknown terminal: {id}"))?;
        let (output, truncated) = term.capture.lock().unwrap().snapshot();
        Ok(AgentTerminalOutput {
            output,
            truncated,
            exit: term.exit.get(),
        })
    }

    /// Receiver that completes when the agent terminal's process exits.
    pub fn agent_terminal_exit_rx(
        &self,
        id: &str,
    ) -> Result<flume::Receiver<CapturedExit>, String> {
        let term = self
            .agent_terms
            .get(id)
            .ok_or_else(|| format!("Unknown terminal: {id}"))?;
        Ok(term.exit.subscribe())
    }

    pub fn kill_agent_terminal(&self, id: &str) -> Result<(), String> {
        let term = self
            .agent_terms
            .get(id)
            .ok_or_else(|| format!("Unknown terminal: {id}"))?;
        let tab = self
            .tabs
            .iter()
            .find(|t| t.id == term.tab_id)
            .ok_or_else(|| format!("Terminal tab gone: {id}"))?;
        tab.session.kill().map_err(|err| err.to_string())
    }

    /// ACP `terminal/release`: kill if needed, drop the id, close the tab.
    pub fn release_agent_terminal(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(term) = self.agent_terms.remove(id) else {
            return Err(format!("Unknown terminal: {id}"));
        };
        if let Some(tab) = self.tabs.iter().find(|t| t.id == term.tab_id) {
            let _ = tab.session.kill();
        }
        self.close_tab(term.tab_id, window, cx);
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
        if let Some(agent_id) = self.tabs[ix].agent_id.clone() {
            self.agent_terms.remove(&agent_id);
        }
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
        command: Option<&TerminalCommand>,
        capture: Option<Arc<Mutex<OutputBuffer>>>,
        cx: &mut Context<Self>,
    ) -> Result<TerminalTab, String> {
        let cols = 80u16;
        let rows = 24u16;
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
            let view = if let Some(capture) = capture {
                TerminalView::new(writer, TeeReader::new(reader, capture), config, cx)
            } else {
                TerminalView::new(writer, reader, config, cx)
            };
            view.with_resize_callback(move |c, r| {
                let _ = master.lock().resize(portable_pty::PtySize {
                    cols: c as u16,
                    rows: r as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            })
        });

        let id = self.next_id;
        self.next_id += 1;
        let title = self.title_for(&cwd);
        Ok(TerminalTab {
            id,
            title,
            cwd,
            persist: command.is_none(),
            agent_id: None,
            view,
            session,
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
