//! Layout paint: title bar, sidebar, editor area, status bar, `Render`.
use super::*;
use super::tab::{DiffKind, TabBody};
use super::lsp::lsp_aggregate_color;
use gpui_kit::component::Icon;

impl IsengardApp {
    pub(super) fn render_sidebar_chrome(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = self.sidebar_view;
        h_flex()
            .id("sidebar-view-toggle")
            .w_full()
            .h(px(theme::PANEL_HEADER_ROW_H))
            .items_center()
            .justify_start()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                Button::new("sidebar-files")
                    .ghost()
                    .cursor_pointer()
                    .small()
                    .icon(LucideIcon::FolderTree)
                    .tooltip("Explorer")
                    .selected(view == SidebarView::Files)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_show_file_tree(&ShowFileTree, window, cx);
                    })),
            )
            .child(
                Button::new("sidebar-git")
                    .ghost()
                    .cursor_pointer()
                    .small()
                    .icon(LucideIcon::GitBranch)
                    .tooltip("Source Control")
                    .selected(view == SidebarView::Git)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_show_git(&ShowGit, window, cx);
                    })),
            )
            .child(
                Button::new("sidebar-search")
                    .ghost()
                    .cursor_pointer()
                    .small()
                    .icon(LucideIcon::Search)
                    .tooltip("Search")
                    .selected(view == SidebarView::Search)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_find_in_files(&FindInFiles, window, cx);
                    })),
            )
            .into_any_element()
    }
    pub(super) fn render_title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let title = self.window_title();
        let bar = if cfg!(target_os = "macos") {
            h_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(title),
                )
        } else {
            h_flex()
                .flex_1()
                .items_center()
                .child(h_flex().flex_1().child(self.app_menu_bar.clone()))
        };
        TitleBar::new()
            // Linux CSD close calls `remove_window()` unless we handle it here
            // (`on_window_should_close` is not invoked for that button).
            .on_close_window(cx.listener(|this, _, window, cx| {
                if this.confirm_quit(window, cx) {
                    this.quit(cx);
                }
            }))
            .child(bar)
            .into_any_element()
    }

    pub(super) fn render_editor_area(&self, cx: &mut Context<Self>) -> AnyElement {
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

        let active_ix = self.tabs.active_index().unwrap_or_default();
        let tab_count = self.tabs.iter().count();
        // TabBar::children requires bare `Tab` (Into<Tab>), so ContextMenu cannot
        // wrap a tab there — compose Kit `Tab`s in divs for the right-click menu.
        // (Tab itself is not an Element, so ContextMenu cannot wrap it either.)
        let mut tab_els: Vec<AnyElement> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let path = tab.path.clone();
                let path_menu = path.clone();
                let path_drop = path.clone();
                let id = SharedString::from(format!("close-tab-{}", tab.path.display()));
                let menu_id = SharedString::from(format!("tab-{}", tab.path.display()));
                let label = tab.label();
                let preview = tab.preview;
                let modified = tab.is_modified;
                let dirty_color = cx.theme().blue;
                let close_others_disabled = tab_count <= 1;
                let selected = ix == active_ix;
                let drag = EditorTabDrag {
                    path: path.clone(),
                    label: SharedString::from(label.clone()),
                };
                // Drag must live on `Tab` (the hit target). An outer wrapper's
                // on_drag never fires — Tab/ContextMenu children eat the mouse
                // (terminal tabs work because drag is on the same h_flex).
                div()
                    .id(ElementId::Name(menu_id))
                    .context_menu(move |menu, _, _| {
                        menu.menu("Close", Box::new(CloseTabAt(path_menu.clone())))
                            .menu_with_disabled(
                                "Close Others",
                                Box::new(CloseOtherTabs(path_menu.clone())),
                                close_others_disabled,
                            )
                            .menu("Close All", Box::new(CloseAllTabs))
                    })
                    .child(
                        Tab::new()
                            // Unique id — Kit defaults every standalone Tab to 0.
                            .element_id(ElementId::Name(SharedString::from(format!(
                                "editor-tab-{}",
                                path.display()
                            ))))
                            // Icon+label as children (not Tab::prefix): Kit's
                            // inner left pad insets the icon; prefix would sit
                            // outside that pad and leave a 12px gap before the name.
                            // `.large()` = 36px — matches `theme::PANEL_HEADER_ROW_H`.
                            .large()
                            .aria_label(label.clone())
                            .child(
                                // Same icon↔label gap as file-tree rows.
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(tree_icon(
                                        &path,
                                        false,
                                        false,
                                        false,
                                        cx.theme().is_dark(),
                                    ))
                                    .child(label),
                            )
                            .selected(selected)
                            .cursor_pointer()
                            .when(preview, |tab| tab.italic())
                            .on_drag(drag, |drag, _, _, cx| {
                                cx.new(|_| TabDragPreview::new(drag.label.clone()))
                            })
                            .drag_over::<EditorTabDrag>(|this, _, _, cx| {
                                // Kit Tab paints L+R borders; color alone would
                                // highlight both. Zero the right like dock tabs.
                                this.border_l_2()
                                    .border_r_0()
                                    .border_color(cx.theme().drag_border)
                            })
                            .on_drop(cx.listener(move |this, drag: &EditorTabDrag, window, cx| {
                                this.drop_editor_tab_before(&drag.path, &path_drop, window, cx);
                            }))
                            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                // Double-click the tab pins a preview (VS Code).
                                if ev.click_count() >= 2 {
                                    this.pin_tab(ix, cx);
                                }
                                this.activate_tab(ix, window, cx);
                            }))
                            .suffix(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    // Same 12px from the tab's right border as the label's left padding.
                                    .mr_2()
                                    .when(modified, |this| {
                                        this.child(
                                            // Block-style dirty mark (docs/design.md: no rounded corners).
                                            div()
                                                .size(px(6.))
                                                .flex_shrink_0()
                                                .bg(dirty_color),
                                        )
                                    })
                                    .child(
                                        Button::new(id)
                                            .ghost()
                                            .cursor_pointer()
                                            .xsmall()
                                            .icon(IconName::Close)
                                            .tooltip("Close")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.request_close(&path, window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        // Empty trailing zone so a tab can be dropped past the last one.
        tab_els.push(
            div()
                .id("editor-tabs-end")
                .h_full()
                .flex_1()
                .min_w_16()
                .drag_over::<EditorTabDrag>(|this, _, _, cx| this.bg(cx.theme().drop_target))
                .on_drop(cx.listener(|this, drag: &EditorTabDrag, window, cx| {
                    this.drop_editor_tab_at_end(&drag.path, window, cx);
                }))
                .into_any_element(),
        );
        let tab_bar = h_flex()
            .id("editor-tabs")
            .w_full()
            .h(px(theme::PANEL_HEADER_ROW_H))
            .flex_shrink_0()
            .items_center()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .children(tab_els);

        let show_preview_toolbar = active.supports_rendered_preview();
        let rendered_preview = active.rendered_preview();
        let conflict_toolbar = match &active.body {
            TabBody::Diff {
                kind: DiffKind::Conflict { hunk_choices, .. },
                old,
                new,
                ..
            } => {
                let old_text = old.read(cx).value();
                let new_text = new.read(cx).value();
                let total = conflict_hunks(old_text.as_ref(), new_text.as_ref()).len();
                let done = hunk_choices.iter().filter(|c| c.is_some()).count();
                let status = if total == 0 {
                    "No line changes — use whole file".to_owned()
                } else {
                    format!("{done}/{total} changes · Accept on each highlight")
                };
                Some(
                    h_flex()
                        .w_full()
                        .h(px(theme::PANEL_HEADER_ROW_H))
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .px_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(status),
                        )
                        .child(
                            Button::new("conflict-accept-current")
                                .outline()
                                .small()
                                .cursor_pointer()
                                .label("Accept Current")
                                .tooltip("Accept whole file (Current)")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.accept_conflict_from_active_tab(
                                        ConflictSide::Current,
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new("conflict-accept-incoming")
                                .outline()
                                .small()
                                .cursor_pointer()
                                .label("Accept Incoming")
                                .tooltip("Accept whole file (Incoming)")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.accept_conflict_from_active_tab(
                                        ConflictSide::Incoming,
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                )
            }
            _ => None,
        };
        let preview_toolbar = show_preview_toolbar.then(|| {
            let preview = rendered_preview;
            h_flex()
                .w_full()
                .h(px(theme::PANEL_HEADER_ROW_H))
                .items_center()
                .justify_end()
                .gap_2()
                .px_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    ButtonGroup::new("rendered-view-mode")
                        .outline()
                        .small()
                        .child(
                            Button::new("rendered-edit")
                                .label("Edit")
                                .cursor_pointer()
                                .selected(!preview),
                        )
                        .child(
                            Button::new("rendered-preview")
                                .label("Preview")
                                .cursor_pointer()
                                .selected(preview),
                        )
                        .on_click(cx.listener(move |this, selected: &Vec<usize>, window, cx| {
                            let want_preview = selected.contains(&1);
                            this.set_rendered_preview(want_preview, window, cx);
                        })),
                )
        });

        let content = if active.is_image() {
            let path = active.path.clone();
            let id = SharedString::from(format!("image-preview-{}", path.display()));
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .bg(cx.theme().sidebar)
                .child(
                    img(path)
                        .id(id)
                        .object_fit(ObjectFit::Contain)
                        .max_w_full()
                        .max_h_full(),
                )
                .into_any_element()
        } else if active.is_diff() {
            let (old, new, kind) = match &active.body {
                TabBody::Diff { old, new, kind, .. } => {
                    (old.clone(), new.clone(), kind.clone())
                }
                _ => unreachable!("is_diff"),
            };
            let old_text = old.read(cx).value();
            let new_text = new.read(cx).value();
            let (old_ranges, new_ranges) = line_highlight_ranges(&old_text, &new_text);
            let old_marks = overview_marks(&old_text, &old_ranges);
            let new_marks = overview_marks(&new_text, &new_ranges);
            let muted = cx.theme().muted_foreground;
            let border = cx.theme().border;
            let del = cx.theme().danger;
            let add = cx.theme().success;
            let mono = cx.theme().mono_font_family.clone();
            let mono_size = cx.theme().mono_font_size;
            let (old_accepts, new_accepts) = match &kind {
                DiffKind::Conflict { hunk_choices, .. } => {
                    let hunks = conflict_hunks(old_text.as_ref(), new_text.as_ref());
                    (
                        Self::conflict_hunk_accept_overlays(
                            &old,
                            &hunks,
                            hunk_choices,
                            ConflictSide::Current,
                            cx,
                        ),
                        Self::conflict_hunk_accept_overlays(
                            &new,
                            &hunks,
                            hunk_choices,
                            ConflictSide::Incoming,
                            cx,
                        ),
                    )
                }
                DiffKind::Change => (Vec::new(), Vec::new()),
            };
            h_flex()
                .size_full()
                .min_w_0()
                .child(Self::render_diff_pane(
                    kind.left_title(),
                    &old,
                    &old_marks,
                    del,
                    muted,
                    border,
                    mono.clone(),
                    mono_size,
                    true,
                    old_accepts,
                ))
                .child(Self::render_diff_pane(
                    kind.right_title(),
                    &new,
                    &new_marks,
                    add,
                    muted,
                    border,
                    mono,
                    mono_size,
                    false,
                    new_accepts,
                ))
                .into_any_element()
        } else if rendered_preview {
            let language = active.language().expect("text tab");
            let text = active
                .editor_state()
                .expect("text tab")
                .read(cx)
                .value();
            match language {
                Language::Svg => {
                    let id = SharedString::from(format!("svg-preview-{}", active.path.display()));
                    let image = Arc::new(Image::from_bytes(
                        ImageFormat::Svg,
                        text.as_bytes().to_vec(),
                    ));
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_4()
                        .bg(cx.theme().sidebar)
                        .child(
                            img(image)
                                .id(id)
                                .object_fit(ObjectFit::Contain)
                                .max_w_full()
                                .max_h_full(),
                        )
                        .into_any_element()
                }
                _ => {
                    // Markdown (and any future TextView-based preview).
                    let id =
                        SharedString::from(format!("md-preview-{}", active.path.display()));
                    div()
                        .size_full()
                        .p_4()
                        .overflow_hidden()
                        .child(
                            TextView::markdown(id, text)
                                .scrollable(true)
                                .selectable(true)
                                .style(crate::theme::text_view_style(cx)),
                        )
                        .into_any_element()
                }
            }
        } else {
            let state = active.editor_state().expect("text tab");
            Editor::new(state)
                .bordered(false)
                .p_0()
                .h_full()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(cx.theme().mono_font_size)
                .context_menu(|menu, _, _| {
                    menu.separator().menu(
                        "Add Selection to Agent",
                        Box::new(AddSelectionToAgent),
                    )
                })
                .into_any_element()
        };

        v_flex()
            .size_full()
            .child(tab_bar)
            .children(conflict_toolbar)
            .children(preview_toolbar)
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_diff_pane(
        title: &'static str,
        state: &Entity<EditorState>,
        marks: &[OverviewMark],
        mark_color: Hsla,
        muted: Hsla,
        border: Hsla,
        mono: SharedString,
        mono_size: Pixels,
        with_right_border: bool,
        hunk_accepts: Vec<AnyElement>,
    ) -> AnyElement {
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .when(with_right_border, |el| {
                el.border_r_1().border_color(border)
            })
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(title),
            )
            .child(
                // Overview + hunk Accept chips are absolute overlays — a flex
                // sibling with `relative()` mark heights blew out pane layout.
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .relative()
                    .overflow_hidden()
                    .child(
                        Editor::new(state)
                            .readonly(true)
                            .bordered(false)
                            .p_0()
                            .h_full()
                            .font_family(mono)
                            .text_size(mono_size),
                    )
                    .child(Self::render_diff_overview(marks, mark_color, border))
                    .children(hunk_accepts),
            )
            .into_any_element()
    }

    /// Accept chips at the text column of each unresolved hunk (viewport + scroll).
    ///
    /// Conflict editors use `line_number(false)` + `folding(false)`. Kit still
    /// reserves `LINE_NUMBER_RIGHT_MARGIN` (6px) for code editors, and the
    /// Editor paints a Medium code-editor left pad of `min(input_px, 6)` = 6px.
    /// Overlay is a sibling of the whole Editor, so text column 0 is at 12px.
    /// `ponytail:` hardcoded Kit constants — upgrade if gutter width goes public.
    pub(super) fn conflict_hunk_accept_overlays(
        state: &Entity<EditorState>,
        hunks: &[ConflictHunk],
        choices: &[Option<ConflictSide>],
        side: ConflictSide,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // Must match gpui-base `LINE_NUMBER_RIGHT_MARGIN` + Kit code-editor pad.
        const TEXT_LEFT: f32 = 6. + 6.;
        let state_read = state.read(cx);
        let line_h = state_read
            .line_height()
            .unwrap_or_else(|| cx.theme().mono_font_size * 1.4);
        let scroll_y = state_read.scroll_offset().y;
        let visible = state_read.visible_row_range();
        let side_label = side.label();
        let id_prefix = match side {
            ConflictSide::Current => "hunk-accept-current",
            ConflictSide::Incoming => "hunk-accept-incoming",
        };

        hunks
            .iter()
            .enumerate()
            .filter(|(i, _)| choices.get(*i).copied().flatten().is_none())
            .filter_map(|(i, hunk)| {
                let line = match side {
                    ConflictSide::Current => hunk.old_lines.start,
                    ConflictSide::Incoming => hunk.new_lines.start,
                };
                if let Some(vis) = &visible
                    && (line + 1 < vis.start || line > vis.end)
                {
                    return None;
                }
                let top = line_h * line as f32 + scroll_y;
                if top + line_h < px(0.) {
                    return None;
                }
                let tooltip = format!("Accept {side_label} for this change");
                Some(
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "{id_prefix}-{i}"
                        ))))
                        .absolute()
                        .top(top)
                        .left(px(TEXT_LEFT))
                        .child(
                            Button::new(SharedString::from(format!("{id_prefix}-btn-{i}")))
                                .outline()
                                .xsmall()
                                .cursor_pointer()
                                .label("Accept")
                                .tooltip(tooltip)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.accept_conflict_hunk(i, side, window, cx);
                                })),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// Overview ticks overlaid on the Kit scrollbar track (no marker API).
    ///
    /// Width = Kit `scrollbar::WIDTH` (16px). Marks pin `top` + `bottom` as
    /// document fractions — `.h(relative(…))` on absolute children stretches
    /// to the pane bottom (full-height bug on both conflict and change diffs).
    /// No track fill: only the ticks, so a whole-file add doesn't paint a
    /// solid full-height bar.
    pub(super) fn render_diff_overview(
        marks: &[OverviewMark],
        color: Hsla,
        _border: Hsla,
    ) -> AnyElement {
        // Same width as Kit's scrollbar track (`scrollbar::WIDTH` = 16px).
        // Marks fill that width edge-to-edge (no inset) so the tick matches
        // the scrollbar bar the user sees.
        const OVERVIEW_W: f32 = 16.;
        div()
            .id("diff-overview")
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(px(OVERVIEW_W))
            .overflow_hidden()
            .children(marks.iter().enumerate().map(|(i, mark)| {
                let bottom = (1.0 - mark.top - mark.height).clamp(0.0, 1.0);
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "diff-mark-{i}"
                    ))))
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(mark.top))
                    .bottom(relative(bottom))
                    .bg(color.opacity(0.85))
            }))
            .into_any_element()
    }

    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let term_visible = self.terminal.read(cx).is_visible();
        let has_folders = self.file_tree.is_open();
        let sidebar_visible = self.sidebar_visible;

        let left = h_flex()
            .gap_2()
            .items_center()
            .when(has_folders, |this| {
                this.child(
                    Button::new("toggle-sidebar")
                        .ghost()
                        .cursor_pointer()
                        .small()
                        .icon(if sidebar_visible {
                            Icon::new(IconName::PanelLeftClose).text_color(cx.theme().info)
                        } else {
                            Icon::new(IconName::PanelLeftOpen)
                        })
                        .tooltip(if sidebar_visible {
                            "Hide Sidebar"
                        } else {
                            "Show Sidebar"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.on_toggle_sidebar(&ToggleSidebar, window, cx)
                        })),
                )
            })
            .when_some(self.tabs.active(), |this, tab| {
                this.child(if document::is_untitled(&tab.path) {
                    tab.label()
                } else {
                    self.workspace.relative_label(&tab.path)
                })
            });

        let focus_lang = self.tabs.active().and_then(|t| t.language());
        let lsp_catalog = self.lsp.catalog_servers(focus_lang);
        // Always offer the icon while a folder is open (empty → short hint).
        let show_lsp = has_folders;

        let right = h_flex()
            .gap_4()
            .items_center()
            .when_some(self.tabs.active(), |this, tab| {
                if let Some(state) = tab.editor_state() {
                    let cursor = state.read(cx).cursor_position();
                    this.child(format!(
                        "Ln {}, Col {}",
                        cursor.line + 1,
                        cursor.character + 1
                    ))
                    .child(tab.language().unwrap().display_name())
                } else {
                    this.child("Image")
                }
            })
            .when(show_lsp, |this| {
                let agg = lsp_aggregate_color(&lsp_catalog, cx);
                let app = cx.entity().downgrade();
                let servers = lsp_catalog;
                this.child(
                    // Kit `Badge::dot` is always a circle — paint a block-style
                    // square ourselves (same language as dirty-tab marks).
                    div()
                        .relative()
                        .child(
                            Popover::new("lsp-catalog")
                                .anchor(Anchor::BottomRight)
                                .open(self.lsp_catalog_open)
                                .on_open_change(cx.listener(|this, open, window, cx| {
                                    this.lsp_catalog_open = *open;
                                    if !*open {
                                        this.focus_active_editor(window, cx);
                                    }
                                    cx.notify();
                                }))
                                .trigger(
                                    Button::new("status-lsp")
                                        .ghost()
                                        .cursor_pointer()
                                        .small()
                                        .icon(LucideIcon::BrainCircuit)
                                        .tooltip("Language Servers"),
                                )
                                .content(move |_state: &mut PopoverState, _, cx| {
                                    let popover = cx.weak_entity();
                                    let empty = servers.is_empty();
                                    v_flex()
                                        .w(px(280.))
                                        .max_h(px(280.))
                                        .gap_1()
                                        .overflow_y_scrollbar()
                                        .when(empty, |col| {
                                            col.child(
                                                div()
                                                    .px_2()
                                                    .py_1()
                                                    .text_sm()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        "Open a supported file to install its \
                                                         language server.",
                                                    ),
                                            )
                                        })
                                        .children(servers.iter().map(|server| {
                                            let app_install = app.clone();
                                            let app_remove = app.clone();
                                            let popover_install = popover.clone();
                                            let popover_remove = popover.clone();
                                            let key = server.process_key.clone();
                                            let key_remove = key.clone();
                                            let label = server.label.clone();
                                            let status_color = match &server.status {
                                                ServerDisplayStatus::Running => {
                                                    cx.theme().success
                                                }
                                                ServerDisplayStatus::Starting => {
                                                    cx.theme().warning
                                                }
                                                ServerDisplayStatus::Error(_) => {
                                                    cx.theme().danger
                                                }
                                                ServerDisplayStatus::Idle => {
                                                    cx.theme().muted_foreground
                                                }
                                            };
                                            // Status mark only. Empty flex kids collapse
                                            // `size_*` into a hairline bar — pin both axes.
                                            // Install when we have a recipe and no app-local
                                            // copy (PATH alone must not hide re-install).
                                            let can_install =
                                                !server.installed && server.recipe.is_some();
                                            let can_remove = server.installed
                                                && is_app_local(&server.process_key);
                                            h_flex()
                                                .w_full()
                                                .items_center()
                                                .gap_2()
                                                .px_1()
                                                .py_1()
                                                .child(
                                                    div()
                                                        .w(px(7.))
                                                        .h(px(7.))
                                                        .min_w(px(7.))
                                                        .min_h(px(7.))
                                                        .flex_shrink_0()
                                                        .bg(status_color),
                                                )
                                                .child(
                                                    div()
                                                        .min_w_0()
                                                        .flex_1()
                                                        .text_sm()
                                                        .truncate()
                                                        .child(label),
                                                )
                                                .when(can_install, |row| {
                                                    row.child(
                                                        Button::new(SharedString::from(
                                                            format!("lsp-install-{key}"),
                                                        ))
                                                        .ghost()
                                                        .cursor_pointer()
                                                        .xsmall()
                                                        .icon(LucideIcon::Download)
                                                        .tooltip("Install")
                                                        .on_click(move |_, window, cx| {
                                                            if let Some(p) =
                                                                popover_install.upgrade()
                                                            {
                                                                p.update(cx, |p, cx| {
                                                                    p.dismiss(window, cx);
                                                                });
                                                            }
                                                            let _ =
                                                                app_install.update(cx, |this, cx| {
                                                                    this.lsp_catalog_open = false;
                                                                    this.run_lsp_install_command(
                                                                        &key, window, cx,
                                                                    );
                                                                    this.focus_active_editor(
                                                                        window, cx,
                                                                    );
                                                                    cx.notify();
                                                                });
                                                        }),
                                                    )
                                                })
                                                .when(can_remove, |row| {
                                                    row.child(
                                                        Button::new(SharedString::from(
                                                            format!("lsp-remove-{key_remove}"),
                                                        ))
                                                        .ghost()
                                                        .cursor_pointer()
                                                        .xsmall()
                                                        .icon(LucideIcon::Trash)
                                                        .tooltip("Remove")
                                                        .on_click(move |_, window, cx| {
                                                            if let Some(p) =
                                                                popover_remove.upgrade()
                                                            {
                                                                p.update(cx, |p, cx| {
                                                                    p.dismiss(window, cx);
                                                                });
                                                            }
                                                            let _ =
                                                                app_remove.update(cx, |this, cx| {
                                                                    this.lsp_catalog_open = false;
                                                                    this.offer_lsp_remove(
                                                                        &key_remove,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                    cx.notify();
                                                                });
                                                        }),
                                                    )
                                                })
                                        }))
                                }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .size(px(6.))
                                .bg(agg),
                        ),
                )
            })
            .when(has_folders, |this| {
                this.child(
                    Button::new("toggle-terminal")
                        .ghost()
                        .cursor_pointer()
                        .small()
                        .icon(if term_visible {
                            Icon::new(IconName::SquareTerminal).text_color(cx.theme().info)
                        } else {
                            Icon::new(IconName::SquareTerminal)
                        })
                        .tooltip(if term_visible {
                            "Minimize Terminal"
                        } else {
                            "Show Terminal"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.on_toggle_terminal(&ToggleTerminal, window, cx)
                        })),
                )
                .child(
                    Button::new("toggle-agent")
                        .ghost()
                        .cursor_pointer()
                        .small()
                        .icon(if self.agent_visible {
                            Icon::new(LucideIcon::Bot).text_color(cx.theme().info)
                        } else {
                            Icon::new(LucideIcon::Bot)
                        })
                        .tooltip(if self.agent_visible {
                            "Hide Agent Panel"
                        } else {
                            "Show Agent Panel"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.on_toggle_agent_panel(&ToggleAgentPanel, window, cx)
                        })),
                )
            });

        StatusBar::new()
            .left(left)
            .right(right)
            .text_xs()
            .into_any_element()
    }

    /// Editor column, optionally stacked with the terminal under it (not under the tree).
    pub(super) fn render_editor_with_terminal(&self, cx: &mut Context<Self>) -> AnyElement {
        let editor = self.render_editor_area(cx);
        self.stack_with_terminal(editor, cx)
    }

    pub(super) fn stack_with_terminal(&self, top: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        if !self.terminal.read(cx).is_visible() {
            return top;
        }
        let height = px(self.terminal_height.max(100.));
        let app = cx.entity().downgrade();
        // Key by workspace so Kit's ResizableState doesn't leak sizes across folders.
        let split_id = self
            .workspace
            .session_key()
            .unwrap_or_else(|| "none".into());
        v_resizable(SharedString::from(format!("editor-term-split-{split_id}")))
            .on_resize(move |state, _, cx| {
                let _ = app.update(cx, |this, cx| {
                    this.remember_terminal_height(state, cx);
                });
            })
            .child(resizable_panel().child(top))
            .child(
                resizable_panel()
                    .size(height)
                    .child(self.terminal.clone()),
            )
            .into_any_element()
    }

    fn show_agent_panel(&self) -> bool {
        self.agent_visible
            && self.agent.is_some()
            && !self.show_welcome()
            && self.file_tree.is_open()
    }

    /// Agent panel as the right child of `agent-split` (full height beside
    /// sidebar + editor + terminal).
    fn with_agent_panel(&self, main: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let width = px(self.agent_width.max(200.));
        let app = cx.entity().downgrade();
        let split_id = self
            .workspace
            .session_key()
            .unwrap_or_else(|| "none".into());
        h_resizable(SharedString::from(format!("agent-split-{split_id}")))
            .on_resize(move |state, _, cx| {
                let _ = app.update(cx, |this, cx| {
                    this.remember_agent_width(state, cx);
                });
            })
            .child(resizable_panel().child(main))
            .child(
                resizable_panel()
                    .size(width)
                    .child(
                        div()
                            .size_full()
                            .min_w_0()
                            .overflow_hidden()
                            .when_some(self.agent.clone(), |d, agent| d.child(agent)),
                    ),
            )
            .into_any_element()
    }
}

impl Render for IsengardApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = if self.show_welcome() {
            welcome::render(self, cx).into_any_element()
        } else if self.file_tree.is_open() && self.sidebar_visible {
            let chrome = self.render_sidebar_chrome(cx);
            let body = match self.sidebar_view {
                SidebarView::Files => {
                    let app = cx.entity().downgrade();
                    let edit = self
                        .tree_edit
                        .as_ref()
                        .map(|edit| (&edit.target, &edit.input));
                    self.file_tree.render(
                        self.workspace.display_name().into(),
                        edit,
                        move |path, permanent, window, cx| {
                            _ = app.update(cx, |this, cx| {
                                this.open_file(&path, permanent, window, cx)
                            });
                        },
                        cx,
                    )
                }
                SidebarView::Search => self.search.clone().into_any_element(),
                SidebarView::Git => self.git.clone().into_any_element(),
            };
            let sidebar = v_flex()
                .size_full()
                .min_w_0()
                .child(chrome)
                .child(div().flex_1().min_h_0().min_w_0().overflow_hidden().child(body));
            let width = px(self.sidebar_width.max(100.));
            let app = cx.entity().downgrade();
            let split_id = self
                .workspace
                .session_key()
                .unwrap_or_else(|| "none".into());
            h_resizable(SharedString::from(format!("main-split-{split_id}")))
                .on_resize(move |state, _, cx| {
                    let _ = app.update(cx, |this, cx| {
                        this.remember_sidebar_width(state, cx);
                    });
                })
                .child(
                    resizable_panel()
                        .size(width)
                        .child(div().size_full().min_w_0().overflow_hidden().child(sidebar)),
                )
                .child(resizable_panel().child(self.render_editor_with_terminal(cx)))
                .into_any_element()
        } else {
            self.render_editor_with_terminal(cx)
        };
        let main = if self.show_agent_panel() {
            self.with_agent_panel(main, cx)
        } else {
            main
        };

        v_flex()
            .id("isengard")
            .key_context("Isengard")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(cx.theme().font_family.clone())
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_new_workspace))
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_open_file))
            .on_action(cx.listener(Self::on_open_workspace))
            .on_action(cx.listener(Self::on_add_folder_to_workspace))
            .on_action(cx.listener(Self::on_save_workspace_as))
            .on_action(cx.listener(Self::on_remove_workspace_folder))
            .on_action(cx.listener(Self::on_rename_path))
            .on_action(cx.listener(Self::on_move_path))
            .on_action(cx.listener(Self::on_reorder_root))
            .on_action(cx.listener(Self::on_toggle_dir))
            .on_action(cx.listener(Self::on_delete_path))
            .on_action(cx.listener(Self::on_new_file_in))
            .on_action(cx.listener(Self::on_new_folder_in))
            .on_action(cx.listener(Self::on_rename_selected_tree_item))
            .on_action(cx.listener(Self::on_delete_selected_tree_item))
            .on_action(cx.listener(Self::on_close_workspace))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_all))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_close_tab_at))
            .on_action(cx.listener(Self::on_close_other_tabs))
            .on_action(cx.listener(Self::on_close_all_tabs))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_increase_font))
            .on_action(cx.listener(Self::on_decrease_font))
            .on_action(cx.listener(Self::on_reset_font))
            .on_action(cx.listener(Self::on_toggle_preview))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_find_in_files))
            .on_action(cx.listener(Self::on_show_file_tree))
            .on_action(cx.listener(Self::on_show_git))
            .on_action(cx.listener(Self::on_toggle_terminal))
            .on_action(cx.listener(Self::on_new_terminal))
            .on_action(cx.listener(Self::on_terminal_tabs_changed))
            .on_action(cx.listener(Self::on_toggle_agent_panel))
            .on_action(cx.listener(Self::on_new_agent_chat))
            .on_action(cx.listener(Self::on_add_selection_to_agent))
            .on_action(cx.listener(Self::on_about))
            .on_action(cx.listener(Self::on_open_config_file))
            .on_action(cx.listener(Self::on_quit))
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h_0().child(main))
            .when(!self.show_welcome(), |this| {
                this.child(self.render_status_bar(cx))
            })
    }
}


