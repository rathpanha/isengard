//! Left-sidebar Git view (stacked repos: status + pull/push/commit).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    list::ListItem,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::file_tree::tree_icon;

use super::engine::{
    GitChange, RepoStatus, commit_paths, discard_all, discard_file, load_workspace_status,
    pull_ff_only, push, suggest_commit_message,
};

const MAX_RENDERED_CHANGES: usize = 500;

#[derive(Clone)]
pub enum GitPanelEvent {
    /// Open a read-only side-by-side diff for this change.
    OpenDiff {
        repo: PathBuf,
        path: PathBuf,
        rel_path: String,
        status: char,
        old_rel_path: Option<String>,
    },
    /// Open the worktree file in the editor (same as the file tree).
    OpenFile { path: PathBuf },
    /// Ask the app to confirm discarding one file, then call [`GitPanel::discard_change`].
    ConfirmDiscardFile {
        repo: PathBuf,
        rel_path: String,
        status: char,
        old_rel_path: Option<String>,
    },
    /// Ask the app to confirm discarding every change in a repo.
    ConfirmDiscardAll { repo: PathBuf, count: usize },
    ToastError(String),
    ToastSuccess(String),
}

struct RepoSection {
    status: RepoStatus,
    /// Display label (workspace-relative root name).
    label: String,
    message: Entity<TextareaState>,
    /// Which action is in flight (siblings disable; only this one spins).
    busy: Option<BusyOp>,
    /// Whether the changes list is visible.
    changes_expanded: bool,
    /// `rel_path` values included in the next commit / Suggest.
    selected: HashSet<String>,
}

pub struct GitPanel {
    folders: Vec<PathBuf>,
    sections: Vec<RepoSection>,
    loading: bool,
    /// Bumps so stale background loads are ignored.
    refresh_generation: u64,
    /// Remember expand toggles across refresh (`true` = expanded).
    expanded: HashMap<PathBuf, bool>,
    _message_subs: Vec<Subscription>,
}

impl GitPanel {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            folders: Vec::new(),
            sections: Vec::new(),
            loading: false,
            refresh_generation: 0,
            expanded: HashMap::new(),
            _message_subs: Vec::new(),
        }
    }

    /// Replace workspace roots and reload status on a background thread.
    pub fn set_folders(
        &mut self,
        folders: Vec<PathBuf>,
        labels: HashMap<PathBuf, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.folders = folders.clone();
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        let refresh_gen = self.refresh_generation;
        self.loading = true;
        cx.notify();

        cx.spawn_in(window, async move |this, window| {
            let statuses = window
                .background_spawn(async move { load_workspace_status(&folders) })
                .await;
            this.update_in(window, |this, window, cx| {
                if this.refresh_generation != refresh_gen {
                    return;
                }
                this.apply_statuses(statuses, labels, window, cx);
            })
            .ok()
        })
        .detach();
    }

    /// Re-run status for current folders (keeps commit message drafts when possible).
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let labels: HashMap<PathBuf, String> = self
            .sections
            .iter()
            .map(|s| (s.status.root.clone(), s.label.clone()))
            .collect();
        let folders = self.folders.clone();
        self.set_folders(folders, labels, window, cx);
    }

    fn apply_statuses(
        &mut self,
        statuses: Vec<RepoStatus>,
        labels: HashMap<PathBuf, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut old_messages: HashMap<PathBuf, String> = HashMap::new();
        let mut old_selection: HashMap<PathBuf, HashSet<String>> = HashMap::new();
        for section in &self.sections {
            self.expanded
                .insert(section.status.root.clone(), section.changes_expanded);
            old_messages.insert(
                section.status.root.clone(),
                section.message.read(cx).value().to_string(),
            );
            old_selection.insert(section.status.root.clone(), section.selected.clone());
        }

        self._message_subs.clear();
        let mut sections = Vec::with_capacity(statuses.len());
        for status in statuses {
            let label = labels.get(&status.root).cloned().unwrap_or_else(|| {
                status
                    .root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| status.root.display().to_string())
            });
            let draft = old_messages.remove(&status.root).unwrap_or_default();
            let changes_expanded = self.expanded.get(&status.root).copied().unwrap_or(true);
            let selected =
                merge_selection(old_selection.remove(&status.root), &status.changes);
            let message = cx.new(|cx| {
                let mut state = TextareaState::new(window, cx).placeholder("Commit message");
                if !draft.is_empty() {
                    state.set_value(draft, window, cx);
                }
                state
            });
            let sub = cx.subscribe(&message, |_this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            self._message_subs.push(sub);
            sections.push(RepoSection {
                status,
                label,
                message,
                busy: None,
                changes_expanded,
                selected,
            });
        }
        self.sections = sections;
        self.loading = false;
        cx.notify();
    }

    fn section_mut(&mut self, root: &Path) -> Option<&mut RepoSection> {
        self.sections
            .iter_mut()
            .find(|s| s.status.root == root)
    }

    fn set_busy(&mut self, root: &Path, busy: Option<BusyOp>, cx: &mut Context<Self>) {
        if let Some(section) = self.section_mut(root) {
            section.busy = busy;
            cx.notify();
        }
    }

    fn toggle_changes(&mut self, root: &Path, cx: &mut Context<Self>) {
        let Some(section) = self.section_mut(root) else {
            return;
        };
        section.changes_expanded = !section.changes_expanded;
        let expanded = section.changes_expanded;
        self.expanded.insert(root.to_path_buf(), expanded);
        cx.notify();
    }

    /// Fill the commit box from **selected** change paths (draft only).
    fn suggest_message(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(section) = self.sections.iter().find(|s| s.status.root == root) else {
            return;
        };
        let selected: Vec<GitChange> = section
            .status
            .changes
            .iter()
            .filter(|c| section.selected.contains(&c.rel_path))
            .cloned()
            .collect();
        let Some(msg) = suggest_commit_message(&selected) else {
            cx.emit(GitPanelEvent::ToastError(
                "No files selected".into(),
            ));
            return;
        };
        let message = section.message.clone();
        message.update(cx, |state, cx| {
            state.set_value(msg, window, cx);
        });
    }

    fn set_change_selected(
        &mut self,
        root: &Path,
        rel_path: &str,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(section) = self.section_mut(root) else {
            return;
        };
        if selected {
            section.selected.insert(rel_path.to_owned());
        } else {
            section.selected.remove(rel_path);
        }
        cx.notify();
    }

    fn set_all_selected(&mut self, root: &Path, selected: bool, cx: &mut Context<Self>) {
        let Some(section) = self.section_mut(root) else {
            return;
        };
        if selected {
            section.selected = section
                .status
                .changes
                .iter()
                .map(|c| c.rel_path.clone())
                .collect();
        } else {
            section.selected.clear();
        }
        cx.notify();
    }

    /// After confirmation: discard one change and refresh.
    pub fn discard_change(
        &mut self,
        root: PathBuf,
        rel_path: String,
        status: char,
        old_rel_path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_mutation(
            root,
            Mutation::DiscardFile {
                rel_path,
                status,
                old_rel_path,
            },
            window,
            cx,
        );
    }

    /// After confirmation: discard every change in `root` and refresh.
    pub fn discard_repo(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.run_mutation(root, Mutation::DiscardAll, window, cx);
    }

    fn run_mutation(
        &mut self,
        root: PathBuf,
        kind: Mutation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .sections
            .iter()
            .any(|s| s.status.root == root && s.busy.is_some())
        {
            return;
        }
        let (message, commit_paths_list) = if matches!(kind, Mutation::Commit) {
            let Some(section) = self.sections.iter().find(|s| s.status.root == root) else {
                return;
            };
            let msg = section.message.read(cx).value().to_string();
            if msg.trim().is_empty() {
                cx.emit(GitPanelEvent::ToastError(
                    "Commit message is empty".into(),
                ));
                return;
            }
            let paths: Vec<String> = section.selected.iter().cloned().collect();
            if paths.is_empty() {
                cx.emit(GitPanelEvent::ToastError("No files selected".into()));
                return;
            }
            (Some(msg), Some(paths))
        } else {
            (None, None)
        };

        let busy_op = BusyOp::from_mutation(&kind);
        self.set_busy(&root, Some(busy_op), cx);
        let folders = self.folders.clone();
        let labels: HashMap<PathBuf, String> = self
            .sections
            .iter()
            .map(|s| (s.status.root.clone(), s.label.clone()))
            .collect();
        let root_bg = root.clone();
        let clear_message = matches!(kind, Mutation::Commit);

        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_spawn(async move {
                    match kind {
                        Mutation::Commit => {
                            let msg = message.unwrap_or_default();
                            let paths = commit_paths_list.unwrap_or_default();
                            commit_paths(&root_bg, &msg, &paths)
                                .map(|_| "Committed".to_owned())
                        }
                        Mutation::Pull => pull_ff_only(&root_bg).map(|out| {
                            if out.is_empty() {
                                "Pulled".into()
                            } else {
                                out
                            }
                        }),
                        Mutation::Push => push(&root_bg).map(|out| {
                            if out.is_empty() {
                                "Everything up-to-date".into()
                            } else {
                                out
                            }
                        }),
                        Mutation::DiscardFile {
                            rel_path,
                            status,
                            old_rel_path,
                        } => discard_file(
                            &root_bg,
                            &rel_path,
                            status,
                            old_rel_path.as_deref(),
                        )
                        .map(|_| format!("Discarded {rel_path}")),
                        Mutation::DiscardAll => {
                            discard_all(&root_bg).map(|_| "Discarded all changes".to_owned())
                        }
                    }
                })
                .await;

            this.update_in(window, |this, window, cx| {
                this.set_busy(&root, None, cx);
                match result {
                    Ok(msg) => {
                        if clear_message
                            && let Some(section) = this.section_mut(&root)
                        {
                            section.message.update(cx, |state, cx| {
                                state.set_value("", window, cx);
                            });
                        }
                        cx.emit(GitPanelEvent::ToastSuccess(short_toast(&msg)));
                        this.set_folders(folders, labels, window, cx);
                    }
                    Err(err) => {
                        cx.emit(GitPanelEvent::ToastError(err));
                    }
                }
            })
            .ok()
        })
        .detach();
    }
}

#[derive(Clone)]
enum Mutation {
    Commit,
    Pull,
    Push,
    DiscardFile {
        rel_path: String,
        status: char,
        old_rel_path: Option<String>,
    },
    DiscardAll,
}

/// Which control is spinning while the repo is locked.
#[derive(Clone, PartialEq, Eq)]
enum BusyOp {
    Commit,
    Pull,
    Push,
    DiscardAll,
    DiscardFile(String),
}

impl BusyOp {
    fn from_mutation(kind: &Mutation) -> Self {
        match kind {
            Mutation::Commit => Self::Commit,
            Mutation::Pull => Self::Pull,
            Mutation::Push => Self::Push,
            Mutation::DiscardAll => Self::DiscardAll,
            Mutation::DiscardFile { rel_path, .. } => Self::DiscardFile(rel_path.clone()),
        }
    }
}

fn short_toast(s: &str) -> String {
    let line = s.lines().next().unwrap_or(s).trim();
    if line.chars().count() > 120 {
        format!("{}…", line.chars().take(119).collect::<String>())
    } else {
        line.to_owned()
    }
}

fn outline_btn(id: SharedString) -> Button {
    Button::new(id).outline().cursor_pointer().small()
}

/// Kit only paints `.loading()` when the button has an `.icon()` — label-only
/// busy buttons need a placeholder (swapped for the spinner).
fn with_loading(btn: Button, loading: bool) -> Button {
    btn.when(loading, |b| b.icon(LucideIcon::Loader).loading(true))
}

impl EventEmitter<GitPanelEvent> for GitPanel {}

impl Render for GitPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let dark = theme.is_dark();

        let mut body: Vec<AnyElement> = Vec::new();
        if self.loading && self.sections.is_empty() {
            body.push(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(muted)
                    .child("Loading…")
                    .into_any_element(),
            );
        } else if self.sections.is_empty() {
            body.push(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(muted)
                    .child("No Git repositories in this workspace.")
                    .into_any_element(),
            );
        } else {
            let mut painted = 0usize;
            let section_count = self.sections.len();
            'sections: for (si, section) in self.sections.iter().enumerate() {
                let root = section.status.root.clone();
                let busy_op = section.busy.clone();
                let busy = busy_op.is_some();
                let branch = section.status.branch.clone();
                let label = section.label.clone();
                let expanded = section.changes_expanded;
                let change_count = section.status.changes.len();
                let chevron = if expanded { "▾" } else { "▸" };
                // Separator only between repos — never under a lone / last section.
                let show_sep = si + 1 < section_count;
                let message_empty = section.message.read(cx).value().trim().is_empty();
                let selected_count = section.selected.len();
                let all_selected = change_count > 0 && selected_count == change_count;
                let nothing_selected = selected_count == 0;

                // Fixed commit-box height — `min_h` + `h_full` let the first
                // section steal leftover flex and look uneven vs the next.
                const COMMIT_BOX_H: f32 = 72.;
                let mut section_el = v_flex()
                    .id(ElementId::Name(SharedString::from(format!("git-repo-{si}"))))
                    .w_full()
                    .flex_shrink_0()
                    .gap_2()
                    .when(show_sep, |el| el.border_b_1().border_color(theme.border))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .pt_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(branch),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .px_2()
                            .child(
                                with_loading(
                                    outline_btn(format!("git-pull-{si}").into()).label(
                                        if section.status.behind > 0 {
                                            format!("Pull ({})", section.status.behind)
                                        } else {
                                            "Pull".into()
                                        },
                                    ),
                                    busy_op == Some(BusyOp::Pull),
                                )
                                .disabled(busy)
                                .on_click({
                                    let root = root.clone();
                                    cx.listener(move |this, _, window, cx| {
                                        this.run_mutation(
                                            root.clone(),
                                            Mutation::Pull,
                                            window,
                                            cx,
                                        );
                                    })
                                }),
                            )
                            .child(
                                with_loading(
                                    outline_btn(format!("git-push-{si}").into()).label(
                                        if section.status.ahead > 0 {
                                            format!("Push ({})", section.status.ahead)
                                        } else {
                                            "Push".into()
                                        },
                                    ),
                                    busy_op == Some(BusyOp::Push),
                                )
                                .disabled(busy)
                                .on_click({
                                    let root = root.clone();
                                    cx.listener(move |this, _, window, cx| {
                                        this.run_mutation(
                                            root.clone(),
                                            Mutation::Push,
                                            window,
                                            cx,
                                        );
                                    })
                                }),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .px_2()
                            .child(
                                div()
                                    .id(ElementId::Name(SharedString::from(format!(
                                        "git-msg-{si}"
                                    ))))
                                    .w_full()
                                    .h(px(COMMIT_BOX_H))
                                    .overflow_hidden()
                                    .child(Textarea::new(&section.message).size_full()),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_end()
                                    .gap_1()
                                    .child(
                                        outline_btn(format!("git-suggest-{si}").into())
                                            .label("Suggest")
                                            .disabled(busy || nothing_selected)
                                            .on_click({
                                                let root = root.clone();
                                                cx.listener(move |this, _, window, cx| {
                                                    this.suggest_message(&root, window, cx);
                                                })
                                            }),
                                    )
                                    .child(
                                        with_loading(
                                            outline_btn(format!("git-commit-{si}").into())
                                                .label("Commit"),
                                            busy_op == Some(BusyOp::Commit),
                                        )
                                        .disabled(busy || message_empty || nothing_selected)
                                        .on_click({
                                            let root = root.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                this.run_mutation(
                                                    root.clone(),
                                                    Mutation::Commit,
                                                    window,
                                                    cx,
                                                );
                                            })
                                        }),
                                    ),
                            ),
                    )
                    .when_some(section.status.error.clone(), |el, err| {
                        el.child(
                            div()
                                .px_2()
                                .text_xs()
                                .text_color(theme.danger)
                                .child(err),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .pb_2()
                            .when(change_count > 0, |row| {
                                row.child(
                                    Checkbox::new(format!("git-select-all-{si}"))
                                        .small()
                                        .checked(all_selected)
                                        .tooltip(if all_selected {
                                            "Deselect All"
                                        } else {
                                            "Select All"
                                        })
                                        .disabled(busy)
                                        .when(!busy, |el| el.cursor_pointer())
                                        .on_click({
                                            let root = root.clone();
                                            cx.listener(move |this, checked: &bool, _, cx| {
                                                this.set_all_selected(&root, *checked, cx);
                                            })
                                        }),
                                )
                            })
                            .child(
                                h_flex()
                                    .id(ElementId::Name(SharedString::from(format!(
                                        "git-toggle-{si}"
                                    ))))
                                    .min_w_0()
                                    .flex_1()
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.accent.opacity(0.15)))
                                    .on_click({
                                        let root = root.clone();
                                        cx.listener(move |this, _, _, cx| {
                                            this.toggle_changes(&root, cx);
                                        })
                                    })
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .w_3()
                                            .child(chevron),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(if change_count == 0 {
                                                "No changes".into()
                                            } else {
                                                format!(
                                                    "{selected_count}/{change_count} change{}",
                                                    if change_count == 1 { "" } else { "s" }
                                                )
                                            }),
                                    ),
                            )
                            .when(change_count > 0, |row| {
                                row.child(
                                    Button::new(format!("git-discard-all-{si}"))
                                        .ghost()
                                        .cursor_pointer()
                                        .small()
                                        .icon(LucideIcon::Undo2)
                                        .tooltip("Discard All Changes")
                                        .loading(busy_op == Some(BusyOp::DiscardAll))
                                        .disabled(busy)
                                        .on_click({
                                            let root = root.clone();
                                            cx.listener(move |this, _, _, cx| {
                                                let count = this
                                                    .sections
                                                    .iter()
                                                    .find(|s| s.status.root == root)
                                                    .map(|s| s.status.changes.len())
                                                    .unwrap_or(0);
                                                if count == 0 {
                                                    return;
                                                }
                                                cx.emit(GitPanelEvent::ConfirmDiscardAll {
                                                    repo: root.clone(),
                                                    count,
                                                });
                                            })
                                        }),
                                )
                            }),
                    );

                if expanded && !section.status.changes.is_empty() {
                    let mut rows: Vec<AnyElement> = Vec::new();
                    for (ci, change) in section.status.changes.iter().enumerate() {
                        if painted >= MAX_RENDERED_CHANGES {
                            rows.push(
                                div()
                                    .px_2()
                                    .pb_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!(
                                        "…and more (showing first {MAX_RENDERED_CHANGES})"
                                    ))
                                    .into_any_element(),
                            );
                            section_el = section_el.child(v_flex().w_full().children(rows));
                            body.push(section_el.into_any_element());
                            break 'sections;
                        }
                        painted += 1;
                        let badge = match change.status {
                            'A' | '?' => theme.success,
                            'D' | 'U' => theme.danger,
                            'R' | 'C' => theme.warning,
                            _ => theme.warning,
                        };
                        let is_selected = section.selected.contains(&change.rel_path);
                        rows.push(render_change_row(
                            si,
                            ci,
                            &root,
                            change,
                            is_selected,
                            badge,
                            muted,
                            dark,
                            busy,
                            busy_op.as_ref(),
                            cx,
                        ));
                    }
                    section_el = section_el.child(v_flex().w_full().children(rows));
                }

                body.push(section_el.into_any_element());
            }
        }

        v_flex()
            .id("git-panel")
            .size_full()
            .min_w_0()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Source Control"),
                    )
                    .child(
                        Button::new("git-refresh")
                            .ghost()
                            .cursor_pointer()
                            .small()
                            .icon(LucideIcon::RefreshCw)
                            .tooltip("Refresh")
                            .loading(self.loading)
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.refresh(window, cx);
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("git-sections")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .children(body),
            )
    }
}

fn merge_selection(
    prev_selected: Option<HashSet<String>>,
    changes: &[GitChange],
) -> HashSet<String> {
    // Default: nothing selected. Across refresh, keep only still-present picks.
    let Some(prev_sel) = prev_selected else {
        return HashSet::new();
    };
    changes
        .iter()
        .map(|c| c.rel_path.clone())
        .filter(|rel| prev_sel.contains(rel))
        .collect()
}

fn render_change_row(
    si: usize,
    ci: usize,
    repo: &Path,
    change: &GitChange,
    is_selected: bool,
    badge: Hsla,
    muted: Hsla,
    dark: bool,
    busy: bool,
    busy_op: Option<&BusyOp>,
    cx: &mut Context<GitPanel>,
) -> AnyElement {
    let repo = repo.to_path_buf();
    let path = change.path.clone();
    let rel = change.rel_path.clone();
    let old_rel = change.old_rel_path.clone();
    let status = change.status;
    let discard_loading = matches!(busy_op, Some(BusyOp::DiscardFile(p)) if p == &rel);
    let icon = tree_icon(&change.path, false, false, false, dark);

    h_flex()
        .id(ElementId::Name(SharedString::from(format!(
            "git-change-{si}-{ci}"
        ))))
        .w_full()
        .min_w_0()
        .gap_1()
        .px_2()
        .py_0p5()
        .items_center()
        .child(
            Checkbox::new(format!("git-sel-{si}-{ci}"))
                .small()
                .checked(is_selected)
                .disabled(busy)
                .when(!busy, |el| el.cursor_pointer())
                .on_click({
                    let root = repo.clone();
                    let rel = rel.clone();
                    cx.listener(move |this, checked: &bool, _, cx| {
                        this.set_change_selected(&root, &rel, *checked, cx);
                    })
                }),
        )
        .child(
            ListItem::new(ElementId::Name(SharedString::from(format!(
                "git-change-open-{si}-{ci}"
            ))))
            .min_w_0()
            .flex_1()
            .cursor_pointer()
            .py_0()
            .px_0()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(icon)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_sm()
                            .text_color(muted)
                            .child(rel.clone()),
                    ),
            )
            .on_click({
                let repo = repo.clone();
                let path = path.clone();
                let rel = rel.clone();
                let old_rel = old_rel.clone();
                cx.listener(move |_, _, _, cx| {
                    cx.emit(GitPanelEvent::OpenDiff {
                        repo: repo.clone(),
                        path: path.clone(),
                        rel_path: rel.clone(),
                        status,
                        old_rel_path: old_rel.clone(),
                    });
                })
            }),
        )
        .child(
            div()
                .w_4()
                .flex_shrink_0()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(badge)
                .child(status.to_string()),
        )
        .child(
            Button::new(format!("git-open-{si}-{ci}"))
                .ghost()
                .cursor_pointer()
                .small()
                .icon(LucideIcon::ExternalLink)
                .tooltip("Open File")
                .disabled(status == 'D')
                .on_click({
                    let path = path.clone();
                    cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(GitPanelEvent::OpenFile {
                            path: path.clone(),
                        });
                    })
                }),
        )
        .child(
            Button::new(format!("git-discard-{si}-{ci}"))
                .ghost()
                .cursor_pointer()
                .small()
                .icon(LucideIcon::Undo2)
                .tooltip("Discard Changes")
                .loading(discard_loading)
                .disabled(busy)
                .on_click({
                    let repo = repo.clone();
                    let rel = rel.clone();
                    cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(GitPanelEvent::ConfirmDiscardFile {
                            repo: repo.clone(),
                            rel_path: rel.clone(),
                            status,
                            old_rel_path: old_rel.clone(),
                        });
                    })
                }),
        )
        .into_any_element()
}
