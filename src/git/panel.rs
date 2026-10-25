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

use crate::file_tree::{material_icon, tree_icon};

use crate::config::GitRepoSession;

use super::engine::{
    ConflictSide, GitChange, RepoOp, RepoStatus, accept_conflict_side, commit_paths,
    write_resolved_conflict,
    conflict_banner, discard_all, discard_file, load_workspace_status, pull_ff_only, push,
    read_merge_msg, suggest_commit_message,
};

const MAX_RENDERED_CHANGES: usize = 500;
/// Commit box starts at one row; content height auto-grows up to this many.
const COMMIT_AUTO_MAX_ROWS: usize = 6;
/// Drag can raise the floor through this many rows (past the auto cap).
const COMMIT_MANUAL_MAX_ROWS: usize = 20;
/// Line box used for auto height + drag (Kit textarea chrome is separate pad).
const COMMIT_ROW_H: f32 = 20.;
const COMMIT_PAD: f32 = 10.;

fn commit_min_h() -> Pixels {
    px(COMMIT_ROW_H + COMMIT_PAD)
}

fn commit_auto_max_h() -> Pixels {
    px(COMMIT_AUTO_MAX_ROWS as f32 * COMMIT_ROW_H + COMMIT_PAD)
}

fn commit_manual_max_h() -> Pixels {
    px(COMMIT_MANUAL_MAX_ROWS as f32 * COMMIT_ROW_H + COMMIT_PAD)
}

/// Height from message text alone (1..=auto-max rows).
fn commit_auto_h(message: &str) -> Pixels {
    let rows = message.lines().count().clamp(1, COMMIT_AUTO_MAX_ROWS);
    px(rows as f32 * COMMIT_ROW_H + COMMIT_PAD).min(commit_auto_max_h())
}

/// Visible box: max(auto content height, drag floor), clamped.
fn commit_display_h(floor: Pixels, message: &str) -> Pixels {
    let auto = commit_auto_h(message);
    auto.max(floor).max(commit_min_h()).min(commit_manual_max_h())
}

struct MsgResize {
    section_i: usize,
    last_y: Pixels,
    last_h: Pixels,
}

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
    /// Open Current | Incoming conflict compare for an unmerged path.
    OpenConflict {
        repo: PathBuf,
        path: PathBuf,
        rel_path: String,
    },
    /// Open the worktree file in the editor (same as the file tree).
    OpenFile { path: PathBuf },
    /// Disk changed under an open tab (e.g. Accept Current/Incoming) — reload text.
    ReloadFile { path: PathBuf },
    /// Close the conflict compare tab for this worktree path (after Accept).
    CloseConflictPreview { path: PathBuf },
    /// Ask the app to confirm discarding one file, then call [`GitPanel::discard_change`].
    ConfirmDiscardFile {
        repo: PathBuf,
        rel_path: String,
        status: char,
        old_rel_path: Option<String>,
    },
    /// Ask the app to confirm discarding every change in a repo.
    ConfirmDiscardAll { repo: PathBuf, count: usize },
    /// AI Suggest for selected paths (Ready ACP agent on a throwaway session).
    SuggestCommitAi {
        root: PathBuf,
        paths: Vec<String>,
    },
    /// Worktree paths the user discarded — agent **N Files** cards should drop them.
    DiscardedPaths { paths: Vec<PathBuf> },
    /// Whole-repo discard — drop every **N Files** row under this root.
    DiscardedRepo { root: PathBuf },
    ToastError(String),
    ToastSuccess(String),
    /// Porcelain reload finished — file tree can tint dirty paths.
    StatusUpdated,
    /// Commit draft / selection / box height changed — persist session.
    UiChanged,
}

struct RepoSection {
    status: RepoStatus,
    /// Display label (workspace-relative root name).
    label: String,
    message: Entity<TextareaState>,
    /// Drag-resize floor in px (`0` = pure auto height from content).
    message_floor_h: Pixels,
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
    /// Absolute worktree path of the open diff/conflict tab (row highlight).
    active_path: Option<PathBuf>,
    /// Agent panel can run a silent Suggest turn (`can_silent_prompt`).
    agent_ready: bool,
    /// Commit-box bottom-edge drag (pixel floor past the auto cap).
    msg_resize: Option<MsgResize>,
    /// Applied on the next [`Self::apply_statuses`] (async load after restore).
    pending_session_ui: Option<Vec<GitRepoSession>>,
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
            active_path: None,
            agent_ready: false,
            msg_resize: None,
            pending_session_ui: None,
            _message_subs: Vec::new(),
        }
    }

    /// Snapshot drafts / selections / commit-box floors for session persist.
    pub fn snapshot_session_ui(&self, cx: &App) -> Vec<GitRepoSession> {
        self.sections
            .iter()
            .filter_map(|section| {
                let message = section.message.read(cx).value().to_string();
                let selected: Vec<String> = section.selected.iter().cloned().collect();
                let floor: f32 = section.message_floor_h.into();
                if message.trim().is_empty() && selected.is_empty() && floor == 0.0 {
                    return None;
                }
                Some(GitRepoSession {
                    root: section.status.root.clone(),
                    message,
                    selected,
                    message_floor_h: floor,
                })
            })
            .collect()
    }

    /// Queue or apply restored Source Control UI (async status may still be loading).
    pub fn restore_session_ui(
        &mut self,
        repos: Vec<GitRepoSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if repos.is_empty() {
            self.pending_session_ui = None;
            return;
        }
        if self.sections.is_empty() {
            self.pending_session_ui = Some(repos);
            return;
        }
        self.apply_session_ui(&repos, window, cx);
        self.pending_session_ui = None;
    }

    fn apply_session_ui(
        &mut self,
        repos: &[GitRepoSession],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for repo in repos {
            let Some(section) = self
                .sections
                .iter_mut()
                .find(|s| s.status.root == repo.root)
            else {
                continue;
            };
            section.message_floor_h = px(repo.message_floor_h);
            section.selected = merge_selection(
                Some(repo.selected.iter().cloned().collect()),
                &section.status.changes,
            );
            let draft = repo.message.clone();
            section.message.update(cx, |state, cx| {
                state.set_value(draft, window, cx);
            });
        }
        cx.notify();
    }

    fn set_message_floor_h(&mut self, section_i: usize, floor: Pixels, cx: &mut Context<Self>) {
        let floor = floor.max(px(0.)).min(commit_manual_max_h());
        let Some(section) = self.sections.get_mut(section_i) else {
            return;
        };
        if section.message_floor_h == floor {
            return;
        }
        section.message_floor_h = floor;
        cx.emit(GitPanelEvent::UiChanged);
        cx.notify();
    }

    fn end_msg_resize(&mut self, cx: &mut Context<Self>) {
        if self.msg_resize.take().is_some() {
            cx.notify();
        }
    }

    /// Enable/disable the Claude Suggest icon (synced from the Agent panel).
    pub fn set_agent_ready(&mut self, ready: bool, cx: &mut Context<Self>) {
        if self.agent_ready == ready {
            return;
        }
        self.agent_ready = ready;
        cx.notify();
    }

    /// Highlight the change row for the active diff/conflict tab (or clear).
    pub fn set_active_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.active_path == path {
            return;
        }
        self.active_path = path;
        cx.notify();
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
        let mut old_floor_h: HashMap<PathBuf, Pixels> = HashMap::new();
        for section in &self.sections {
            self.expanded
                .insert(section.status.root.clone(), section.changes_expanded);
            old_messages.insert(
                section.status.root.clone(),
                section.message.read(cx).value().to_string(),
            );
            old_selection.insert(section.status.root.clone(), section.selected.clone());
            old_floor_h.insert(section.status.root.clone(), section.message_floor_h);
        }
        let pending = self.pending_session_ui.take().unwrap_or_default();
        let pending_by_root: HashMap<PathBuf, &GitRepoSession> =
            pending.iter().map(|r| (r.root.clone(), r)).collect();

        let first_paint = self.sections.is_empty();
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
            let pending_ui = pending_by_root.get(&status.root).copied();
            let mut draft = old_messages.remove(&status.root).unwrap_or_default();
            if draft.trim().is_empty()
                && let Some(ui) = pending_ui
            {
                draft = ui.message.clone();
            }
            if draft.trim().is_empty()
                && matches!(status.op, Some(RepoOp::Merge) | Some(RepoOp::CherryPick))
                && let Some(msg) = read_merge_msg(&status.root)
            {
                draft = msg;
            }
            let changes_expanded = self.expanded.get(&status.root).copied().unwrap_or(true);
            let selected = if let Some(ui) = pending_ui {
                merge_selection(
                    Some(ui.selected.iter().cloned().collect()),
                    &status.changes,
                )
            } else {
                merge_selection(old_selection.remove(&status.root), &status.changes)
            };
            let message_floor_h = pending_ui
                .map(|ui| px(ui.message_floor_h))
                .or_else(|| old_floor_h.remove(&status.root))
                .unwrap_or(px(0.));
            let message = cx.new(|cx| {
                let mut state = TextareaState::new(window, cx).placeholder("Commit message");
                if !draft.is_empty() {
                    state.set_value(draft, window, cx);
                }
                state
            });
            let sub = cx.subscribe(&message, |_this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.emit(GitPanelEvent::UiChanged);
                    cx.notify();
                }
            });
            self._message_subs.push(sub);
            sections.push(RepoSection {
                status,
                label,
                message,
                message_floor_h,
                busy: None,
                changes_expanded,
                selected,
            });
        }
        self.sections = sections;
        self.loading = false;
        // ponytail: QA/demo — open first conflict compare (`ISENGARD_OPEN_CONFLICT=1`).
        if first_paint
            && std::env::var_os("ISENGARD_OPEN_CONFLICT").is_some()
            && let Some((repo, path, rel_path)) = self.sections.iter().find_map(|section| {
                section.status.changes.iter().find(|c| c.status == 'U').map(|c| {
                    (
                        section.status.root.clone(),
                        c.path.clone(),
                        c.rel_path.clone(),
                    )
                })
            })
        {
            cx.emit(GitPanelEvent::OpenConflict {
                repo,
                path,
                rel_path,
            });
        }
        cx.emit(GitPanelEvent::StatusUpdated);
        cx.notify();
    }

    /// Absolute worktree paths + status letter for explorer tinting.
    pub fn dirty_paths(&self) -> Vec<(PathBuf, char)> {
        self.sections
            .iter()
            .flat_map(|s| {
                s.status
                    .changes
                    .iter()
                    .map(|c| (c.path.clone(), c.status))
            })
            .collect()
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

    fn selected_paths(&self, root: &Path) -> Option<Vec<String>> {
        let section = self.sections.iter().find(|s| s.status.root == root)?;
        let paths: Vec<String> = section
            .status
            .changes
            .iter()
            .filter(|c| section.selected.contains(&c.rel_path))
            .map(|c| c.rel_path.clone())
            .collect();
        (!paths.is_empty()).then_some(paths)
    }

    /// Path-heuristic draft into the commit box (no agent).
    fn suggest_heuristic(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .sections
            .iter()
            .find(|s| s.status.root == root)
            .is_some_and(|s| s.busy.is_some())
        {
            return;
        }
        self.apply_heuristic_suggest(root, window, cx);
    }

    /// Ask the app to AI-Suggest (Ready agent only — no heuristic fallback).
    fn suggest_ai(&mut self, root: &Path, cx: &mut Context<Self>) {
        let Some(section) = self.sections.iter().find(|s| s.status.root == root) else {
            return;
        };
        if section.busy.is_some() || !self.agent_ready {
            return;
        }
        let Some(paths) = self.selected_paths(root) else {
            cx.emit(GitPanelEvent::ToastError("No files selected".into()));
            return;
        };
        cx.emit(GitPanelEvent::SuggestCommitAi {
            root: root.to_path_buf(),
            paths,
        });
    }

    /// Path-heuristic draft.
    pub fn apply_heuristic_suggest(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            cx.emit(GitPanelEvent::ToastError("No files selected".into()));
            return;
        };
        self.apply_commit_message(root, &msg, window, cx);
    }

    /// Put `msg` in the repo's commit box and clear Suggest busy.
    pub fn apply_commit_message(
        &mut self,
        root: &Path,
        msg: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(section) = self.section_mut(root) else {
            return;
        };
        section.busy = None;
        let message = section.message.clone();
        message.update(cx, |state, cx| {
            state.set_value(msg.to_owned(), window, cx);
        });
        cx.notify();
    }

    /// Spin Suggest while an agent turn runs (or clear on failure before fallback).
    pub fn set_suggest_busy(&mut self, root: &Path, busy: bool, cx: &mut Context<Self>) {
        self.set_busy(
            root,
            busy.then_some(BusyOp::Suggest),
            cx,
        );
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
        cx.emit(GitPanelEvent::UiChanged);
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
        cx.emit(GitPanelEvent::UiChanged);
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

    /// Whole-file Accept Current / Incoming for a conflicted path.
    pub fn accept_conflict(
        &mut self,
        root: PathBuf,
        rel_path: String,
        side: ConflictSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_mutation(
            root,
            Mutation::AcceptConflict { rel_path, side },
            window,
            cx,
        );
    }

    /// Write merged conflict content (hunk resolve) and stage it.
    pub fn resolve_conflict_content(
        &mut self,
        root: PathBuf,
        rel_path: String,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_mutation(
            root,
            Mutation::ResolveConflictContent { rel_path, content },
            window,
            cx,
        );
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
        let reload_path = match &kind {
            Mutation::AcceptConflict { rel_path, .. }
            | Mutation::ResolveConflictContent { rel_path, .. } => Some(root.join(rel_path)),
            _ => None,
        };
        let discard_notify = match &kind {
            Mutation::DiscardFile {
                rel_path,
                old_rel_path,
                ..
            } => {
                let mut paths = vec![root.join(rel_path)];
                if let Some(old) = old_rel_path {
                    paths.push(root.join(old));
                }
                Some(GitPanelEvent::DiscardedPaths { paths })
            }
            Mutation::DiscardAll => Some(GitPanelEvent::DiscardedRepo {
                root: root.clone(),
            }),
            _ => None,
        };

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
                        Mutation::AcceptConflict { rel_path, side } => {
                            accept_conflict_side(&root_bg, &rel_path, side).map(|_| {
                                format!("Accepted {} for {rel_path}", side.label())
                            })
                        }
                        Mutation::ResolveConflictContent { rel_path, content } => {
                            write_resolved_conflict(&root_bg, &rel_path, &content).map(|_| {
                                format!("Resolved {rel_path}")
                            })
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
                        if let Some(path) = reload_path {
                            cx.emit(GitPanelEvent::CloseConflictPreview {
                                path: path.clone(),
                            });
                            cx.emit(GitPanelEvent::ReloadFile { path });
                        }
                        if let Some(ev) = discard_notify {
                            cx.emit(ev);
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
    AcceptConflict {
        rel_path: String,
        side: ConflictSide,
    },
    ResolveConflictContent {
        rel_path: String,
        content: String,
    },
}

/// Which control is spinning while the repo is locked.
#[derive(Clone, PartialEq, Eq)]
enum BusyOp {
    Commit,
    Suggest,
    Pull,
    Push,
    DiscardAll,
    DiscardFile(String),
    AcceptConflict(String),
}

impl BusyOp {
    fn from_mutation(kind: &Mutation) -> Self {
        match kind {
            Mutation::Commit => Self::Commit,
            Mutation::Pull => Self::Pull,
            Mutation::Push => Self::Push,
            Mutation::DiscardAll => Self::DiscardAll,
            Mutation::DiscardFile { rel_path, .. } => Self::DiscardFile(rel_path.clone()),
            Mutation::AcceptConflict { rel_path, .. }
            | Mutation::ResolveConflictContent { rel_path, .. } => {
                Self::AcceptConflict(rel_path.clone())
            }
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
            let agent_ready = self.agent_ready;
            'sections: for (si, section) in self.sections.iter().enumerate() {
                let root = section.status.root.clone();
                let busy_op = section.busy.clone();
                let busy = busy_op.is_some();
                let branch = section.status.branch.clone();
                let label = section.label.clone();
                let expanded = section.changes_expanded;
                let change_count = section.status.changes.len();
                let conflict_count = section.status.conflict_count();
                let lock_sync = section.status.blocks_sync_and_discard();
                let lock_suggest = section.status.blocks_suggest();
                let lock_commit = section.status.blocks_commit();
                let op_banner = conflict_banner(section.status.op, conflict_count);
                let chevron = if expanded { "▾" } else { "▸" };
                // Separator only between repos — never under a lone / last section.
                let show_sep = si + 1 < section_count;
                let message_empty = section.message.read(cx).value().trim().is_empty();
                let selected_count = section.selected.len();
                let all_selected = change_count > 0 && selected_count == change_count;
                let nothing_selected = selected_count == 0;

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
                                .disabled(busy || lock_sync)
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
                                .disabled(busy || lock_sync)
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
                    .when_some(op_banner, |el, text| {
                        // Red while conflicts remain; yellow when the op is
                        // still open but every `U` is cleared.
                        let color = if conflict_count == 0 {
                            theme.warning
                        } else {
                            theme.danger
                        };
                        el.child(
                            div()
                                .px_2()
                                .text_xs()
                                .text_color(color)
                                .child(text),
                        )
                    })
                    .child({
                        let msg_text = section.message.read(cx).value().to_string();
                        let display_h =
                            commit_display_h(section.message_floor_h, &msg_text);
                        v_flex()
                            .w_full()
                            .gap_1()
                            .px_2()
                            .child(
                                v_flex()
                                    .id(ElementId::Name(SharedString::from(format!(
                                        "git-msg-{si}"
                                    ))))
                                    .w_full()
                                    .child(
                                        // Pixel height (auto from content up to 6
                                        // rows, or the drag floor). No Kit
                                        // `auto_grow` — rebuilding that mode
                                        // mid-drag was snapping the box back.
                                        div()
                                            .w_full()
                                            .h(display_h)
                                            .overflow_hidden()
                                            .child(Textarea::new(&section.message).size_full()),
                                    )
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!(
                                                "git-msg-resize-{si}"
                                            ))))
                                            .w_full()
                                            .h(px(6.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor(CursorStyle::ResizeUpDown)
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                                    let start_h = this
                                                        .sections
                                                        .get(si)
                                                        .map(|s| {
                                                            commit_display_h(
                                                                s.message_floor_h,
                                                                &s.message.read(cx).value(),
                                                            )
                                                        })
                                                        .unwrap_or_else(commit_min_h);
                                                    this.set_message_floor_h(si, start_h, cx);
                                                    this.msg_resize = Some(MsgResize {
                                                        section_i: si,
                                                        last_y: e.position.y,
                                                        last_h: start_h,
                                                    });
                                                    cx.notify();
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .w(px(28.))
                                                    .h(px(2.))
                                                    .rounded_full()
                                                    .bg(theme.border),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_end()
                                    .gap_1()
                                    .child({
                                        let heur_disabled =
                                            busy || nothing_selected || lock_suggest;
                                        let mut btn = Button::new(format!("git-suggest-{si}"))
                                            .outline()
                                            .small()
                                            .icon(LucideIcon::WandSparkles)
                                            .tooltip(if nothing_selected || lock_suggest {
                                                "Select files to Suggest a commit message"
                                            } else if busy {
                                                "Wait for the current Git action to finish"
                                            } else {
                                                "Suggest commit message"
                                            })
                                            .disabled(heur_disabled);
                                        if !heur_disabled {
                                            let root = root.clone();
                                            btn = btn.cursor_pointer().on_click(cx.listener(
                                                move |this, _, window, cx| {
                                                    this.suggest_heuristic(&root, window, cx);
                                                },
                                            ));
                                        }
                                        btn
                                    })
                                    .child({
                                        let suggesting = busy_op == Some(BusyOp::Suggest);
                                        let ai_disabled = busy
                                            || nothing_selected
                                            || lock_suggest
                                            || !agent_ready;
                                        // Don't use outline_btn — it always sets
                                        // cursor_pointer even when disabled.
                                        let mut btn = Button::new(format!("git-suggest-ai-{si}"))
                                            .outline()
                                            .small()
                                            .tooltip(if agent_ready {
                                                if nothing_selected || lock_suggest {
                                                    "Select files to Suggest with Claude"
                                                } else if busy {
                                                    "Wait for the current Git action to finish"
                                                } else {
                                                    "Suggest with Claude"
                                                }
                                            } else {
                                                "Open the Agent panel and wait until Ready"
                                            })
                                            .disabled(ai_disabled)
                                            .when(suggesting, |b| {
                                                b.icon(LucideIcon::Loader).loading(true)
                                            })
                                            .when(!suggesting, |b| {
                                                // Material SVG ignores Kit disabled `fg`;
                                                // match outline disabled (~muted @ 0.5).
                                                b.child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .when(ai_disabled, |d| d.opacity(0.5))
                                                        .child(material_icon("claude", 14.)),
                                                )
                                            });
                                        if !ai_disabled {
                                            let root = root.clone();
                                            btn = btn.cursor_pointer().on_click(cx.listener(
                                                move |this, _, _, cx| {
                                                    this.suggest_ai(&root, cx);
                                                },
                                            ));
                                        }
                                        btn
                                    })
                                    .child(
                                        with_loading(
                                            outline_btn(format!("git-commit-{si}").into())
                                                .label("Commit"),
                                            busy_op == Some(BusyOp::Commit),
                                        )
                                        .disabled(
                                            busy
                                                || message_empty
                                                || nothing_selected
                                                || lock_commit,
                                        )
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
                            )
                    })
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
                                        .disabled(busy || lock_sync)
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
                        let is_active = self.active_path.as_ref() == Some(&change.path);
                        rows.push(render_change_row(
                            si,
                            ci,
                            &root,
                            change,
                            is_selected,
                            is_active,
                            badge,
                            muted,
                            dark,
                            busy,
                            lock_sync,
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
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                let Some((si, new_h)) = this.msg_resize.as_mut().and_then(|drag| {
                    let dy: f32 = (e.position.y - drag.last_y).into();
                    if dy.abs() < 0.5 {
                        return None;
                    }
                    let new_h = (drag.last_h + px(dy))
                        .max(commit_min_h())
                        .min(commit_manual_max_h());
                    // Resync to the pointer every step. Growing the box moves
                    // the grip; anchoring last_y to the mouse avoids a reverse
                    // delta on the next event (the old row/`auto_grow` path).
                    drag.last_y = e.position.y;
                    if new_h == drag.last_h {
                        return None;
                    }
                    drag.last_h = new_h;
                    Some((drag.section_i, new_h))
                }) else {
                    return;
                };
                this.set_message_floor_h(si, new_h, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_msg_resize(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_msg_resize(cx)),
            )
            .child(
                h_flex()
                    .w_full()
                    .h(px(crate::theme::PANEL_HEADER_ROW_H))
                    .items_center()
                    .justify_between()
                    .px_2()
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

#[allow(clippy::too_many_arguments)]
fn render_change_row(
    si: usize,
    ci: usize,
    repo: &Path,
    change: &GitChange,
    is_selected: bool,
    is_active: bool,
    badge: Hsla,
    muted: Hsla,
    dark: bool,
    busy: bool,
    lock_discard: bool,
    busy_op: Option<&BusyOp>,
    cx: &mut Context<GitPanel>,
) -> AnyElement {
    let repo = repo.to_path_buf();
    let path = change.path.clone();
    let rel = change.rel_path.clone();
    let old_rel = change.old_rel_path.clone();
    let status = change.status;
    let is_conflict = status == 'U';
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
        .when(is_active, |el| el.bg(cx.theme().accent.opacity(0.35)))
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
            .selected(is_active)
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
                    if is_conflict {
                        cx.emit(GitPanelEvent::OpenConflict {
                            repo: repo.clone(),
                            path: path.clone(),
                            rel_path: rel.clone(),
                        });
                    } else {
                        cx.emit(GitPanelEvent::OpenDiff {
                            repo: repo.clone(),
                            path: path.clone(),
                            rel_path: rel.clone(),
                            status,
                            old_rel_path: old_rel.clone(),
                        });
                    }
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
                .tooltip(if is_conflict {
                    "Compare Current / Incoming"
                } else {
                    "Open File"
                })
                .disabled(status == 'D')
                .on_click({
                    let repo = repo.clone();
                    let path = path.clone();
                    let rel = rel.clone();
                    cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        if is_conflict {
                            cx.emit(GitPanelEvent::OpenConflict {
                                repo: repo.clone(),
                                path: path.clone(),
                                rel_path: rel.clone(),
                            });
                        } else {
                            cx.emit(GitPanelEvent::OpenFile {
                                path: path.clone(),
                            });
                        }
                    })
                }),
        )
        .when(!is_conflict, |row| {
            row.child(
                Button::new(format!("git-discard-{si}-{ci}"))
                    .ghost()
                    .cursor_pointer()
                    .small()
                    .icon(LucideIcon::Undo2)
                    .tooltip("Discard Changes")
                    .loading(discard_loading)
                    .disabled(busy || lock_discard)
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
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{
        commit_auto_h, commit_auto_max_h, commit_display_h, commit_manual_max_h, commit_min_h,
    };
    use gpui_kit::px;

    #[test]
    fn commit_display_h_auto_then_floor() {
        assert_eq!(commit_auto_h(""), commit_min_h());
        assert_eq!(
            commit_auto_h("a\nb\nc\nd\ne\nf\ng"),
            commit_auto_max_h()
        );
        assert_eq!(commit_display_h(px(0.), "a"), commit_min_h());
        let tall = px(200.);
        assert_eq!(commit_display_h(tall, "a"), tall);
        assert_eq!(
            commit_display_h(px(9999.), "a"),
            commit_manual_max_h()
        );
    }
}
