//! Project search/replace wiring and selection soft-highlight.
use super::*;

impl IsengardApp {
    pub(super) fn on_find_in_files(
        &mut self,
        _: &FindInFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() {
            return;
        }
        self.sidebar_view = SidebarView::Search;
        self.sidebar_visible = true;
        self.persist_session(cx);
        self.search.update(cx, |panel, cx| panel.focus_find(window, cx));
        cx.notify();
    }

    pub(super) fn on_show_file_tree(
        &mut self,
        _: &ShowFileTree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() {
            return;
        }
        self.sidebar_view = SidebarView::Files;
        self.sidebar_visible = true;
        self.persist_session(cx);
        self.focus_file_tree(window, cx);
        cx.notify();
    }
    pub(super) fn open_buffer_texts(&self, cx: &App) -> HashMap<PathBuf, String> {
        let mut map = HashMap::new();
        for tab in self.tabs.iter() {
            if document::is_untitled(&tab.path) {
                continue;
            }
            if let Some(state) = tab.editor_state() {
                map.insert(tab.path.clone(), state.read(cx).value().to_string());
            }
        }
        map
    }

    pub(super) fn schedule_search(&mut self, query: SearchQuery, cx: &mut Context<Self>) {
        // Abort any walk still chewing through the previous pattern.
        self.search_cancel.store(true, Ordering::Relaxed);
        self.search_generation = self.search_generation.wrapping_add(1);
        let search_gen = self.search_generation;
        if query.pattern.is_empty() {
            self.search
                .update(cx, |panel, cx| panel.clear_results(cx));
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = cancel.clone();
        self.search
            .update(cx, |panel, cx| panel.set_searching(true, cx));
        let roots = self.workspace.folders().to_vec();
        let buffers = self.open_buffer_texts(cx);
        cx.spawn(async move |this, cx| {
            // Debounce typing; generation check drops superseded timers.
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let still = this
                .update(cx, |this, _| this.search_generation == search_gen)
                .ok()?;
            if !still {
                return None;
            }
            let outcome = cx
                .background_spawn(async move {
                    run_search(&roots, &query, &buffers, &cancel)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.search_generation != search_gen {
                    return;
                }
                let labels: HashMap<PathBuf, String> = outcome
                    .groups
                    .iter()
                    .map(|g| {
                        (
                            g.path.clone(),
                            this.workspace.relative_label(&g.path),
                        )
                    })
                    .collect();
                this.search
                    .update(cx, |panel, cx| panel.set_outcome(outcome, labels, cx));
            })
            .ok()
        })
        .detach();
    }
    pub(super) fn open_search_match(
        &mut self,
        hit: &SearchMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file(&hit.path, false, window, cx);
        let siblings: Vec<SearchMatch> = self
            .search
            .read(cx)
            .groups()
            .iter()
            .find(|g| g.path == hit.path)
            .map(|g| g.matches.clone())
            .unwrap_or_else(|| vec![hit.clone()]);
        let Some(state) = self
            .tabs
            .iter()
            .find(|t| t.path == hit.path)
            .and_then(|t| t.editor_state())
            .cloned()
        else {
            return;
        };
        if let Some(prev) = self.search_hit_decorations.take() {
            prev.dispose(cx);
        }
        let fill = cx.theme().warning.opacity(0.35); // search hits (distinct from selection matches)
        let hit_line = hit.line as u32;
        let hit_col = hit.col as u32;
        let hit_end = (hit.col + hit.match_len) as u32;
        let collection = state.update(cx, |state, cx| {
            let decorations: Vec<RangeDecoration> = siblings
                .iter()
                .map(|m| {
                    let start = state.text().position_to_offset(&Position {
                        line: m.line as u32,
                        character: m.col as u32,
                    });
                    let end = state.text().position_to_offset(&Position {
                        line: m.line as u32,
                        character: (m.col + m.match_len) as u32,
                    });
                    RangeDecoration::new(start..end)
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(fill)
                })
                .collect();
            let collection = state.create_range_decorations_collection(decorations, cx);
            let start = state.text().position_to_offset(&Position {
                line: hit_line,
                character: hit_col,
            });
            let end = state.text().position_to_offset(&Position {
                line: hit_line,
                character: hit_end,
            });
            // Selects the clicked hit and scrolls it into view (`move_to` inside).
            state.set_selected_range(start..end, cx);
            state.focus(window, cx);
            collection
        });
        self.search_hit_decorations = Some(collection);
    }

    /// Empty decoration bucket so later refreshes only `set` (no create-under-observe).
    pub(super) fn ensure_selection_match_collection(
        &mut self,
        state: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        let id = state.entity_id();
        if self.selection_match_decorations.contains_key(&id) {
            return;
        }
        let collection = state.update(cx, |state, cx| {
            state.create_range_decorations_collection(Vec::new(), cx)
        });
        self.selection_match_decorations.insert(id, collection);
    }

    /// Editor observe fires on scroll/blink too — only refresh matches / status
    /// bar when the selection or cursor actually moved.
    pub(super) fn on_editor_state_observed(
        &mut self,
        state: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        self.schedule_selection_matches(state, cx);
        let cursor = state.read(cx).cursor_position();
        let key = (cursor.line, cursor.character);
        if self.status_bar_cursor != Some(key) {
            self.status_bar_cursor = Some(key);
            cx.notify();
        }
    }

    /// Queue a selection-match refresh (debounced).
    ///
    /// Hot path: `cx.observe` fires on scroll and blink as well as selection
    /// changes. Skip when the selected range is unchanged. Never call Kit's
    /// `value()` until the selection is worth scanning. Clears stay immediate;
    /// scans wait ~50ms so drag-select doesn't thrash.
    pub(super) fn schedule_selection_matches(
        &mut self,
        state: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        let id = state.entity_id();
        // Cheap peek: selection only (no full-buffer copy).
        let (range, needle, haystack_len) = {
            let editor = state.read(cx);
            (
                editor.selected_range(),
                editor.selected_text().to_string(),
                editor.text().len(),
            )
        };
        if self.selection_match_last_range.get(&id) == Some(&range) {
            return;
        }
        self.selection_match_last_range.insert(id, range.clone());

        self.selection_match_gen = self.selection_match_gen.wrapping_add(1);
        let ticket = self.selection_match_gen;

        let worth_scan = !range.is_empty()
            && selection_matches::should_highlight(&needle)
            && haystack_len <= selection_matches::MAX_HAYSTACK_BYTES;

        let state = state.downgrade();
        if !worth_scan {
            // Clear stale fills right away (still deferred to avoid nested notify).
            let app = cx.weak_entity();
            cx.defer(move |cx| {
                let Some(app_entity) = app.upgrade() else {
                    return;
                };
                if app_entity.read(cx).selection_match_gen != ticket {
                    return;
                }
                let Some(state) = state.upgrade() else {
                    return;
                };
                Self::apply_selection_match_decorations(
                    &app_entity,
                    &state,
                    Vec::new(),
                    cx,
                );
            });
            return;
        }

        // Debounce the expensive rope → String + scan (drag-select).
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            this.update(cx, |this, cx| {
                if this.selection_match_gen != ticket {
                    return;
                }
                let Some(state) = state.upgrade() else {
                    return;
                };
                let id = state.entity_id();
                let fill = Hsla {
                    s: 0.1,
                    ..cx.theme().selection
                };
                // Re-read: selection may have changed during the debounce wait.
                let (range, needle, haystack) = {
                    let editor = state.read(cx);
                    let range = editor.selected_range();
                    let needle = editor.selected_text().to_string();
                    if range.is_empty()
                        || !selection_matches::should_highlight(&needle)
                        || editor.text().len() > selection_matches::MAX_HAYSTACK_BYTES
                    {
                        Self::apply_selection_match_decorations_for(
                            this, id, &state, Vec::new(), cx,
                        );
                        return;
                    }
                    // `text().to_string()` once — avoid Kit `value()` (same copy +
                    // SharedString wrap) on this hot path.
                    (range, needle, editor.text().to_string())
                };
                let decorations = selection_matches::find_selection_matches(
                    &haystack, &needle, range,
                )
                .into_iter()
                .map(|r| {
                    RangeDecoration::new(r)
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(fill)
                })
                .collect();
                Self::apply_selection_match_decorations_for(
                    this, id, &state, decorations, cx,
                );
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn apply_selection_match_decorations(
        app: &Entity<Self>,
        state: &Entity<EditorState>,
        decorations: Vec<RangeDecoration>,
        cx: &mut App,
    ) {
        let id = state.entity_id();
        app.update(cx, |this, cx| {
            Self::apply_selection_match_decorations_for(this, id, state, decorations, cx);
        });
    }

    pub(super) fn apply_selection_match_decorations_for(
        this: &mut Self,
        id: EntityId,
        state: &Entity<EditorState>,
        decorations: Vec<RangeDecoration>,
        cx: &mut App,
    ) {
        let collection = match this.selection_match_decorations.get(&id).cloned() {
            Some(c) => c,
            None => {
                let c = state.update(cx, |state, cx| {
                    state.create_range_decorations_collection(Vec::new(), cx)
                });
                this.selection_match_decorations.insert(id, c.clone());
                c
            }
        };
        // `RangeDecorationCollection::set` always notifies, even on identical
        // content. Notifying re-enters our observe → schedule → set loop.
        let next: Vec<_> = decorations.iter().map(|d| d.range().clone()).collect();
        if collection.get_ranges(cx) == next {
            return;
        }
        collection.set(decorations, cx);
    }

    pub(super) fn clear_selection_matches(&mut self, id: EntityId, cx: &mut App) {
        self.selection_match_last_range.remove(&id);
        if let Some(prev) = self.selection_match_decorations.remove(&id) {
            prev.dispose(cx);
        }
    }

    pub(super) fn replace_in_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = self.search.read(cx).query(cx);
        if query.pattern.is_empty() {
            return;
        }
        if let Err(err) = self.apply_replace_to_path(&path, &query, window, cx) {
            return notify_error(format!("Replace failed: {err:#}"), window, cx);
        }
        self.rescan_search(cx);
    }

    pub(super) fn confirm_replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.search.read(cx);
        let files = panel.file_count();
        let matches = panel.match_count();
        if files == 0 || matches == 0 {
            return;
        }
        self.open_choice_dialog(
            "Replace All?",
            format!("Replace {matches} occurrences across {files} files?"),
            "Don't Replace",
            "Replace All",
            |_, _, _| {},
            |this, window, cx| this.replace_all(window, cx),
            window,
            cx,
        );
    }

    pub(super) fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).query(cx);
        let paths: Vec<PathBuf> = self
            .search
            .read(cx)
            .groups()
            .iter()
            .map(|g| g.path.clone())
            .collect();
        for path in paths {
            if let Err(err) = self.apply_replace_to_path(&path, &query, window, cx) {
                return notify_error(format!("Replace failed: {err:#}"), window, cx);
            }
        }
        self.rescan_search(cx);
    }

    pub(super) fn apply_replace_to_path(
        &mut self,
        path: &Path,
        query: &SearchQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let open_state = self
            .tabs
            .iter()
            .find(|t| t.path == *path)
            .and_then(|t| t.editor_state().cloned());
        if let Some(state) = open_state {
            let text = state.read(cx).value().to_string();
            let (new_text, count) =
                apply_replace(&text, query).map_err(anyhow::Error::msg)?;
            if count == 0 {
                return Ok(());
            }
            state.update(cx, |state, cx| {
                state.set_value(new_text, window, cx);
            });
            if let Some(tab) = self.tabs.find_mut(|t| t.path == *path) {
                tab.is_modified = true;
                tab.preview = false;
            }
            cx.notify();
            return Ok(());
        }
        let text = document::read_text(path)?;
        let (new_text, count) = apply_replace(&text, query).map_err(anyhow::Error::msg)?;
        if count > 0 {
            document::write_text(path, &new_text)?;
        }
        Ok(())
    }

    pub(super) fn rescan_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).query(cx);
        self.schedule_search(query, cx);
    }

}
