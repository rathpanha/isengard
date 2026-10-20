//! Source Control: open diff/conflict tabs, accept hunks, discard confirms.
use super::*;
use super::tab::{DiffKind, TabBody};
use super::tab::conflict_side_to_hunk;

impl IsengardApp {
    pub(super) fn on_show_git(
        &mut self,
        _: &ShowGit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() {
            return;
        }
        self.sidebar_view = SidebarView::Git;
        self.sidebar_visible = true;
        self.persist_session(cx);
        self.refresh_git_panel(window, cx);
        // Don't leave focus on the Explorer/Git chrome button — that handle
        // goes stale after paint and menu Close then misses the app.
        self.focus_handle.focus(window, cx);
        cx.notify();
    }
    pub(super) fn confirm_git_discard_file(
        &mut self,
        repo: PathBuf,
        rel_path: String,
        status: char,
        old_rel_path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = Path::new(&rel_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(rel_path.as_str())
            .to_owned();
        self.open_choice_dialog(
            "Discard Changes?",
            format!("Discard changes to {name}? This cannot be undone."),
            "Discard",
            "Keep",
            {
                let repo = repo.clone();
                let rel_path = rel_path.clone();
                let old_rel_path = old_rel_path.clone();
                move |this, window, cx| {
                    this.git.update(cx, |panel, cx| {
                        panel.discard_change(
                            repo.clone(),
                            rel_path.clone(),
                            status,
                            old_rel_path.clone(),
                            window,
                            cx,
                        );
                    });
                }
            },
            |_, _, _| {},
            window,
            cx,
        );
    }

    pub(super) fn confirm_git_discard_all(
        &mut self,
        repo: PathBuf,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self.workspace.relative_label(&repo);
        self.open_choice_dialog(
            "Discard All Changes?",
            format!(
                "Discard all {count} change{} in {label}? This cannot be undone.",
                if count == 1 { "" } else { "s" }
            ),
            "Discard All",
            "Keep",
            move |this, window, cx| {
                this.git.update(cx, |panel, cx| {
                    panel.discard_repo(repo.clone(), window, cx);
                });
            },
            |_, _, _| {},
            window,
            cx,
        );
    }

    /// Read-only side-by-side diff preview tab for a Source Control change.
    pub(super) fn open_git_diff(
        &mut self,
        repo: PathBuf,
        file: PathBuf,
        rel_path: String,
        status: char,
        old_rel_path: Option<String>,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (old_text, new_text) =
            match file_sides(&repo, &rel_path, status, old_rel_path.as_deref()) {
                Ok(sides) => sides,
                Err(err) => {
                    return notify_error(format!("Diff failed: {err}"), window, cx);
                }
            };
        self.open_side_by_side_diff(
            document::diff_preview_path(&file),
            Language::from_path(&file),
            DiffKind::Change,
            old_text,
            new_text,
            preview,
            window,
            cx,
        );
    }

    /// Current | Incoming conflict compare with Accept toolbar.
    pub(super) fn open_git_conflict(
        &mut self,
        repo: PathBuf,
        file: PathBuf,
        rel_path: String,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (ours, theirs) = match conflict_file_sides(&repo, &rel_path) {
            Ok(sides) => sides,
            Err(err) => {
                return notify_error(format!("Conflict compare failed: {err}"), window, cx);
            }
        };
        let hunk_choices = vec![None; conflict_hunks(&ours, &theirs).len()];
        self.open_side_by_side_diff(
            document::conflict_preview_path(&file),
            Language::from_path(&file),
            DiffKind::Conflict {
                repo,
                rel_path,
                hunk_choices,
            },
            ours,
            theirs,
            preview,
            window,
            cx,
        );
    }

    pub(super) fn open_side_by_side_diff(
        &mut self,
        virtual_path: PathBuf,
        language: Language,
        kind: DiffKind,
        old_text: String,
        new_text: String,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.tabs.position(|t| t.path == virtual_path) {
            let sides = self.tabs.get(ix).and_then(|t| match &t.body {
                TabBody::Diff { old, new, .. } => Some((old.clone(), new.clone())),
                _ => None,
            });
            if let Some((old, new)) = sides {
                old.update(cx, |state, cx| {
                    state.set_value(old_text.clone(), window, cx);
                    state.set_readonly(true, cx);
                });
                new.update(cx, |state, cx| {
                    state.set_value(new_text.clone(), window, cx);
                    state.set_readonly(true, cx);
                });
                let highlights = Self::apply_diff_pane_highlights(
                    &old,
                    &new,
                    &old_text,
                    &new_text,
                    &kind,
                    cx,
                );
                if let Some(tab) = self.tabs.get_mut(ix)
                    && let TabBody::Diff {
                        kind: tab_kind,
                        _highlights,
                        ..
                    } = &mut tab.body
                {
                    *tab_kind = kind;
                    _highlights.0.dispose(cx);
                    _highlights.1.dispose(cx);
                    *_highlights = highlights;
                }
                Self::scroll_diff_to_first_change(&old, &new, &old_text, &new_text, window, cx);
            }
            self.tabs.activate(ix);
            self.sync_git_active_from_tab(cx);
            self.focus_active_editor(window, cx);
            cx.notify();
            return;
        }

        // Replace active preview tab when present (VS Code–style).
        if preview {
            let preview_ix = self
                .tabs
                .active_index()
                .filter(|&ix| self.tabs.get(ix).is_some_and(|t| t.preview));
            if let Some(ix) = preview_ix {
                self.tabs.remove(ix);
            }
        }

        // Conflict: no line numbers / folding — less gutter so Accept can sit
        // on the text column (Kit does not expose `line_number_width`).
        let is_conflict = matches!(kind, DiffKind::Conflict { .. });
        let old =
            Self::new_diff_editor(language, old_text.clone(), is_conflict, window, cx);
        let new =
            Self::new_diff_editor(language, new_text.clone(), is_conflict, window, cx);
        let highlights = Self::apply_diff_pane_highlights(
            &old,
            &new,
            &old_text,
            &new_text,
            &kind,
            cx,
        );
        let subscriptions = Self::diff_scroll_subscriptions(&old, &new, window, cx);
        Self::scroll_diff_to_first_change(&old, &new, &old_text, &new_text, window, cx);
        self.tabs.push(EditorTab {
            path: virtual_path,
            body: TabBody::Diff {
                language,
                kind,
                old,
                new,
                _highlights: highlights,
                _subscriptions: subscriptions,
            },
            is_modified: false,
            preview,
            untitled_save_dir: None,
        });
        self.sync_git_active_from_tab(cx);
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    /// Jump both panes to the first change hunk (long files open mid-diff).
    pub(super) fn scroll_diff_to_first_change(
        old: &Entity<EditorState>,
        new: &Entity<EditorState>,
        old_text: &str,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(hunk) = conflict_hunks(old_text, new_text).into_iter().next() else {
            return;
        };
        old.update(cx, |state, cx| {
            state.set_cursor_position(
                Position {
                    line: hunk.old_lines.start as u32,
                    character: 0,
                },
                window,
                cx,
            );
        });
        new.update(cx, |state, cx| {
            state.set_cursor_position(
                Position {
                    line: hunk.new_lines.start as u32,
                    character: 0,
                },
                window,
                cx,
            );
        });
    }

    /// Keep Diff/Conflict panes locked to the same scroll offset.
    ///
    /// `set_scroll_offset` is deferred until layout, so we ignore notifies from
    /// the peer for the rest of the frame (clamp/paint must not shove back).
    pub(super) fn diff_scroll_subscriptions(
        old: &Entity<EditorState>,
        new: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let ignoring = Rc::new(Cell::new(None::<EntityId>));
        let subscribe = |source: &Entity<EditorState>,
                         target: &Entity<EditorState>,
                         ignoring: Rc<Cell<Option<EntityId>>>,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
            let target = target.clone();
            cx.observe_in(source, window, move |_, source, window, cx| {
                if ignoring.get() == Some(source.entity_id()) {
                    cx.notify();
                    return;
                }
                let offset = source.read(cx).scroll_offset();
                if target.read(cx).scroll_offset() == offset {
                    cx.notify();
                    return;
                }
                let target_id = target.entity_id();
                ignoring.set(Some(target_id));
                target.update(cx, |state, cx| {
                    state.set_scroll_offset(offset, cx);
                });
                let ignoring = ignoring.clone();
                cx.on_next_frame(window, move |_, _, _| {
                    if ignoring.get() == Some(target_id) {
                        ignoring.set(None);
                    }
                });
                cx.notify();
            })
        };
        vec![
            subscribe(old, new, ignoring.clone(), window, cx),
            subscribe(new, old, ignoring, window, cx),
        ]
    }

    pub(super) fn close_conflict_preview(&mut self, worktree_path: &Path, cx: &mut Context<Self>) {
        let virtual_path = document::conflict_preview_path(worktree_path);
        if let Some(ix) = self.tabs.position(|t| t.path == virtual_path) {
            self.tabs.remove(ix);
            self.sync_git_active_from_tab(cx);
            cx.notify();
        }
    }

    pub(super) fn accept_conflict_from_active_tab(
        &mut self,
        side: ConflictSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Accept buttons (and their tooltips) leave the tree when the conflict resolves.
        window.dismiss_tooltip(cx);
        let Some(ix) = self.tabs.active_index() else {
            return;
        };
        let Some((repo, rel_path)) = self.tabs.get(ix).and_then(|t| match &t.body {
            TabBody::Diff {
                kind: DiffKind::Conflict { repo, rel_path, .. },
                ..
            } => Some((repo.clone(), rel_path.clone())),
            _ => None,
        }) else {
            return;
        };
        self.git.update(cx, |panel, cx| {
            panel.accept_conflict(repo, rel_path, side, window, cx);
        });
    }

    /// Accept one conflict hunk by index (inline Accept on that pane).
    pub(super) fn accept_conflict_hunk(
        &mut self,
        hunk_i: usize,
        side: ConflictSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.dismiss_tooltip(cx);
        let Some(ix) = self.tabs.active_index() else {
            return;
        };
        let Some((old, new, repo, rel_path, mut choices)) =
            self.tabs.get(ix).and_then(|t| match &t.body {
                TabBody::Diff {
                    kind:
                        DiffKind::Conflict {
                            repo,
                            rel_path,
                            hunk_choices,
                        },
                    old,
                    new,
                    ..
                } => {
                    let old_text = old.read(cx).value();
                    let new_text = new.read(cx).value();
                    let hunks = conflict_hunks(old_text.as_ref(), new_text.as_ref());
                    if hunk_i >= hunks.len() {
                        return None;
                    }
                    let mut choices = hunk_choices.clone();
                    if choices.len() != hunks.len() {
                        choices.resize(hunks.len(), None);
                    }
                    Some((
                        old.clone(),
                        new.clone(),
                        repo.clone(),
                        rel_path.clone(),
                        choices,
                    ))
                }
                _ => None,
            })
        else {
            return;
        };

        choices[hunk_i] = Some(side);

        let old_text = old.read(cx).value().to_string();
        let new_text = new.read(cx).value().to_string();
        let all_done = choices.iter().all(|c| c.is_some());

        if all_done {
            let sides: Vec<HunkSide> = choices
                .iter()
                .map(|c| conflict_side_to_hunk(c.expect("all_done")))
                .collect();
            match apply_hunk_choices(&old_text, &new_text, &sides) {
                Ok(content) => {
                    self.git.update(cx, |panel, cx| {
                        panel.resolve_conflict_content(repo, rel_path, content, window, cx);
                    });
                }
                Err(err) => notify_error(format!("Resolve failed: {err}"), window, cx),
            }
            return;
        }

        let kind = DiffKind::Conflict {
            repo,
            rel_path,
            hunk_choices: choices,
        };
        let highlights =
            Self::apply_diff_pane_highlights(&old, &new, &old_text, &new_text, &kind, cx);
        if let Some(tab) = self.tabs.get_mut(ix)
            && let TabBody::Diff {
                kind: tab_kind,
                _highlights,
                ..
            } = &mut tab.body
        {
            *tab_kind = kind;
            _highlights.0.dispose(cx);
            _highlights.1.dispose(cx);
            *_highlights = highlights;
        }
        cx.notify();
    }

    pub(super) fn new_diff_editor(
        language: Language,
        text: String,
        conflict: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language(language.highlighter_name())
                // Change diffs keep line numbers + folding. Conflict panes drop
                // both so the Accept overlay can share the text column origin.
                .line_number(!conflict)
                .folding(!conflict)
                .indent_guides(true)
                .soft_wrap(false)
                .tab_size(TabSize {
                    tab_size: 4,
                    hard_tabs: false,
                })
                .default_value(text);
            state.set_readonly(true, cx);
            state
        })
    }

    pub(super) fn apply_diff_pane_highlights(
        old: &Entity<EditorState>,
        new: &Entity<EditorState>,
        old_text: &str,
        new_text: &str,
        kind: &DiffKind,
        cx: &mut Context<Self>,
    ) -> (RangeDecorationCollection, RangeDecorationCollection) {
        match kind {
            DiffKind::Conflict { hunk_choices, .. } => {
                Self::apply_conflict_highlights(old, new, old_text, new_text, hunk_choices, cx)
            }
            DiffKind::Change => {
                let (old_ranges, new_ranges) = line_highlight_ranges(old_text, new_text);
                let del = cx.theme().danger.opacity(0.28);
                let add = cx.theme().success.opacity(0.28);
                (
                    Self::diff_range_collection(old, &old_ranges, del, cx),
                    Self::diff_range_collection(new, &new_ranges, add, cx),
                )
            }
        }
    }

    /// Unresolved hunks: danger/success fills. Resolved: muted (still visible).
    pub(super) fn apply_conflict_highlights(
        old: &Entity<EditorState>,
        new: &Entity<EditorState>,
        old_text: &str,
        new_text: &str,
        choices: &[Option<ConflictSide>],
        cx: &mut Context<Self>,
    ) -> (RangeDecorationCollection, RangeDecorationCollection) {
        let hunks = conflict_hunks(old_text, new_text);
        let del = cx.theme().danger.opacity(0.28);
        let add = cx.theme().success.opacity(0.28);
        let muted = cx.theme().muted_foreground.opacity(0.18);
        let mut old_decs = Vec::new();
        let mut new_decs = Vec::new();
        for (i, hunk) in hunks.iter().enumerate() {
            let resolved = choices.get(i).and_then(|c| *c).is_some();
            let old_color = if resolved { muted } else { del };
            let new_color = if resolved { muted } else { add };
            if !hunk.old_bytes.is_empty() {
                old_decs.push(
                    RangeDecoration::new(hunk.old_bytes.clone())
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(old_color),
                );
            }
            if !hunk.new_bytes.is_empty() {
                new_decs.push(
                    RangeDecoration::new(hunk.new_bytes.clone())
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(new_color),
                );
            }
        }
        let old_col = old.update(cx, |state, cx| {
            state.create_range_decorations_collection(old_decs, cx)
        });
        let new_col = new.update(cx, |state, cx| {
            state.create_range_decorations_collection(new_decs, cx)
        });
        (old_col, new_col)
    }

    pub(super) fn diff_range_collection(
        state: &Entity<EditorState>,
        ranges: &[(usize, usize)],
        color: Hsla,
        cx: &mut Context<Self>,
    ) -> RangeDecorationCollection {
        state.update(cx, |state, cx| {
            let decorations: Vec<RangeDecoration> = ranges
                .iter()
                .map(|&(start, end)| {
                    RangeDecoration::new(start..end)
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(color)
                })
                .collect();
            state.create_range_decorations_collection(decorations, cx)
        })
    }
}
