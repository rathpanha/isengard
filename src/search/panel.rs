//! Left-sidebar Search view (find + replace + results).

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::file_tree::tree_icon;

use super::engine::{SearchFileGroup, SearchMatch, SearchOutcome, SearchQuery};

/// Cap painted rows so a 10k-hit outcome cannot freeze the sidebar.
const MAX_RENDERED_MATCHES: usize = 500;

/// Events the panel raises for the app to handle.
#[derive(Clone)]
pub enum SearchPanelEvent {
    QueryChanged(SearchQuery),
    OpenMatch(SearchMatch),
    ReplaceInFile(PathBuf),
    ReplaceAll,
}

pub struct SearchPanel {
    find: Entity<InputState>,
    replace: Entity<InputState>,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
    searching: bool,
    outcome: SearchOutcome,
    /// Workspace-relative labels for result file headers.
    path_labels: HashMap<PathBuf, String>,
    /// `(group_ix, match_ix)` of the selected hit.
    selected: Option<(usize, usize)>,
    _subscriptions: Vec<Subscription>,
}

impl SearchPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let find = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let replace = cx.new(|cx| InputState::new(window, cx).placeholder("Replace"));

        let find_sub = cx.subscribe(&find, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.emit_query(cx);
            }
        });
        let replace_sub = cx.subscribe(&replace, |_this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });

        Self {
            find,
            replace,
            case_sensitive: false,
            whole_word: false,
            use_regex: false,
            searching: false,
            outcome: SearchOutcome::default(),
            path_labels: HashMap::new(),
            selected: None,
            _subscriptions: vec![find_sub, replace_sub],
        }
    }

    pub fn focus_find(&self, window: &mut Window, cx: &mut App) {
        self.find.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn query(&self, cx: &App) -> SearchQuery {
        SearchQuery {
            pattern: self.find.read(cx).value().to_string(),
            replace: self.replace.read(cx).value().to_string(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            use_regex: self.use_regex,
        }
    }

    pub fn set_searching(&mut self, searching: bool, cx: &mut Context<Self>) {
        self.searching = searching;
        cx.notify();
    }

    pub fn set_outcome(
        &mut self,
        outcome: SearchOutcome,
        path_labels: HashMap<PathBuf, String>,
        cx: &mut Context<Self>,
    ) {
        self.searching = false;
        self.outcome = outcome;
        self.path_labels = path_labels;
        self.selected = None;
        cx.notify();
    }

    pub fn clear_results(&mut self, cx: &mut Context<Self>) {
        self.searching = false;
        self.outcome = SearchOutcome::default();
        self.path_labels.clear();
        self.selected = None;
        cx.notify();
    }

    pub fn selected_file(&self) -> Option<PathBuf> {
        let (gi, _) = self.selected?;
        self.outcome.groups.get(gi).map(|g| g.path.clone())
    }

    pub fn groups(&self) -> &[SearchFileGroup] {
        &self.outcome.groups
    }

    pub fn match_count(&self) -> usize {
        self.outcome.match_count
    }

    pub fn file_count(&self) -> usize {
        self.outcome.file_count
    }

    fn emit_query(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        cx.emit(SearchPanelEvent::QueryChanged(query));
        cx.notify();
    }

    fn toggle_case(&mut self, cx: &mut Context<Self>) {
        self.case_sensitive = !self.case_sensitive;
        self.emit_query(cx);
    }

    fn toggle_word(&mut self, cx: &mut Context<Self>) {
        self.whole_word = !self.whole_word;
        self.emit_query(cx);
    }

    fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        self.use_regex = !self.use_regex;
        self.emit_query(cx);
    }
}

impl EventEmitter<SearchPanelEvent> for SearchPanel {}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let query = self.query(cx);
        let has_pattern = !query.pattern.is_empty();
        let has_results = !self.outcome.groups.is_empty();
        let can_replace = has_results && !self.searching;
        let replace_file_disabled = !can_replace || self.selected_file().is_none();

        let selected = self.selected;
        let muted = theme.muted_foreground;
        let mut results: Vec<AnyElement> = Vec::new();
        let dark = theme.is_dark();
        let mut rendered_matches = 0usize;
        let mut render_capped = false;
        let match_bg = theme.warning.opacity(0.35);
        'groups: for (gi, group) in self.outcome.groups.iter().enumerate() {
            let path_label = self
                .path_labels
                .get(&group.path)
                .cloned()
                .unwrap_or_else(|| group.path.display().to_string());
            let icon = tree_icon(&group.path, false, false, false, dark);
            results.push(
                h_flex()
                    .id(ElementId::Name(SharedString::from(format!(
                        "search-file-{gi}"
                    ))))
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(icon)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(muted)
                            .child(path_label),
                    )
                    .into_any_element(),
            );
            for (mi, m) in group.matches.iter().enumerate() {
                if rendered_matches >= MAX_RENDERED_MATCHES {
                    render_capped = true;
                    break 'groups;
                }
                rendered_matches += 1;
                let is_selected = selected == Some((gi, mi));
                let (before, matched, after) =
                    preview_parts(&m.line_text, m.col, m.match_len);
                let line_no = m.line + 1;
                let hit = m.clone();
                results.push(
                    ListItem::new(ElementId::Name(SharedString::from(format!(
                        "search-hit-{gi}-{mi}"
                    ))))
                    .w_full()
                    .selected(is_selected)
                    .cursor_pointer()
                    .py_0()
                    .px_0()
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .px_3()
                            .py_0p5()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .flex_shrink_0()
                                    .child(format!("{line_no}")),
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_sm()
                                    .child(before)
                                    .when(!matched.is_empty(), |row| {
                                        row.child(
                                            div()
                                                .bg(match_bg)
                                                .rounded(px(2.))
                                                .child(matched),
                                        )
                                    })
                                    .child(after),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some((gi, mi));
                        cx.emit(SearchPanelEvent::OpenMatch(hit.clone()));
                        cx.notify();
                    }))
                    .into_any_element(),
                );
            }
        }

        let status = if let Some(err) = &self.outcome.error {
            format!("Invalid pattern: {err}")
        } else if self.searching {
            "Searching…".into()
        } else if !has_pattern {
            String::new()
        } else if self.outcome.match_count == 0 {
            "No results".into()
        } else {
            let mut s = format!(
                "{} results in {} files",
                self.outcome.match_count, self.outcome.file_count
            );
            if self.outcome.truncated || render_capped {
                s.push_str(" (truncated)");
            }
            if render_capped {
                s.push_str(&format!(" · showing first {MAX_RENDERED_MATCHES}"));
            }
            s
        };

        let case_on = self.case_sensitive;
        let word_on = self.whole_word;
        let regex_on = self.use_regex;

        v_flex()
            .id("search-panel")
            .size_full()
            .min_w_0()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .px_2()
                    .pt_2()
                    .pb_2()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(Input::new(&self.find).id("search-find").w_full()),
                            )
                            .child(
                                Button::new("search-case")
                                    .ghost()
                                    .cursor_pointer()
                                    .small()
                                    .icon(LucideIcon::CaseSensitive)
                                    .tooltip("Match Case")
                                    .selected(case_on)
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_case(cx))),
                            )
                            .child(
                                Button::new("search-word")
                                    .ghost()
                                    .cursor_pointer()
                                    .small()
                                    .icon(LucideIcon::WholeWord)
                                    .tooltip("Match Whole Word")
                                    .selected(word_on)
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_word(cx))),
                            )
                            .child(
                                Button::new("search-regex")
                                    .ghost()
                                    .cursor_pointer()
                                    .small()
                                    .icon(LucideIcon::Regex)
                                    .tooltip("Use Regular Expression")
                                    .selected(regex_on)
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_regex(cx))),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .items_center()
                            .child(
                                div().min_w_0().flex_1().child(
                                    Input::new(&self.replace).id("search-replace").w_full(),
                                ),
                            )
                            .child(
                                Button::new("search-replace-file")
                                    .ghost()
                                    .cursor_pointer()
                                    .small()
                                    .icon(LucideIcon::Replace)
                                    .tooltip("Replace in File")
                                    .loading(self.searching)
                                    .disabled(replace_file_disabled)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(path) = this.selected_file() {
                                            cx.emit(SearchPanelEvent::ReplaceInFile(path));
                                        }
                                    })),
                            )
                            .child(
                                Button::new("search-replace-all")
                                    .ghost()
                                    .cursor_pointer()
                                    .small()
                                    .icon(LucideIcon::ReplaceAll)
                                    .tooltip("Replace All")
                                    .loading(self.searching)
                                    .disabled(!can_replace)
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(SearchPanelEvent::ReplaceAll);
                                    })),
                            ),
                    )
                    .when(!status.is_empty(), |this| {
                        this.child(
                            div()
                                .w_full()
                                .px_1()
                                .text_xs()
                                .text_color(if self.outcome.error.is_some() {
                                    theme.danger
                                } else {
                                    theme.muted_foreground
                                })
                                .child(status),
                        )
                    })
                    .border_b_1()
                    .border_color(theme.border),
            )
            .child(
                div()
                    .id("search-results")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .children(results),
            )
    }
}

/// Split a truncated line preview into before / match / after for highlight paint.
fn preview_parts(line_text: &str, col: usize, match_len: usize) -> (String, String, String) {
    let chars: Vec<char> = line_text.chars().collect();
    if match_len == 0 || col >= chars.len() {
        return (line_text.to_string(), String::new(), String::new());
    }
    let end = (col + match_len).min(chars.len());
    (
        chars[..col].iter().collect(),
        chars[col..end].iter().collect(),
        chars[end..].iter().collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::preview_parts;

    #[test]
    fn preview_parts_highlights_span() {
        let (b, m, a) = preview_parts("hello world", 6, 5);
        assert_eq!((b.as_str(), m.as_str(), a.as_str()), ("hello ", "world", ""));
    }

    #[test]
    fn preview_parts_clamps_past_end() {
        let (b, m, a) = preview_parts("abc…", 2, 10);
        assert_eq!((b.as_str(), m.as_str(), a.as_str()), ("ab", "c…", ""));
    }
}
