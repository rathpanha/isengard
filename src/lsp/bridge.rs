//! Kit `CompletionProvider` / `HoverProvider` / `DefinitionProvider` over an [`LspClient`].

use std::rc::Rc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use gpui_kit::component::input::{
    CompletionProvider, DefinitionProvider, HoverProvider, Rope, RopeExt as _,
    ShowDocumentHandler,
};
use gpui_kit::*;
use lsp_types::notification::{DidChangeTextDocument, Notification as _};
use lsp_types::request::{Completion, GotoDefinition, HoverRequest};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionParams, CompletionResponse,
    CompletionTextEdit, CompletionTriggerKind, DidChangeTextDocumentParams,
    GotoDefinitionParams, GotoDefinitionResponse, HoverParams, Location, LocationLink,
    PartialResultParams, Position as LspPosition, Range as LspRange,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentPositionParams,
    TextEdit, WorkDoneProgressParams,
};

use super::client::LspClient;
use super::uri::path_to_uri;

/// Forwards editor LSP hooks to one running language server for a single document URI.
pub struct DocBridge {
    pub client: Arc<LspClient>,
    pub uri: lsp_types::Uri,
    /// Shared with [`super::host::LspHost`] so didChange versions stay monotonic.
    pub version: Arc<AtomicI32>,
    /// Bumps on every completion request so a slow older reply cannot overwrite
    /// a newer menu (Kit applies whichever Task finishes last).
    pub completion_gen: Arc<AtomicU64>,
}

impl CompletionProvider for DocBridge {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let client = self.client.clone();
        let version = self.version.clone();
        let uri = self.uri.clone();
        let doc = text.to_string();
        let position = text.offset_to_position(offset);
        // Kit latches `a.map` as the "query"/trigger_character. Use the segment
        // after the last `.`/`:` for ranking (VS Code-style), not the raw latch.
        let raw_query = trigger.trigger_character.clone().unwrap_or_default();
        let filter_prefix = completion_filter_prefix(&raw_query);
        // Accept must replace only `map`, not Kit's latch `a.map`.
        let replace_start = replace_start_offset(text, offset, &filter_prefix);
        let replace_range = LspRange {
            start: text.offset_to_position(replace_start),
            end: position,
        };
        let context = normalize_completion_context(trigger);
        let req_id = self.completion_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let completion_gen = self.completion_gen.clone();
        cx.background_spawn(async move {
            // Sync then complete on the same stdio stream (ordered).
            let version = version.fetch_add(1, Ordering::SeqCst) + 1;
            client.notify(
                DidChangeTextDocument::METHOD,
                DidChangeTextDocumentParams {
                    text_document: lsp_types::VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: doc,
                    }],
                },
            )?;
            let params = CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: Some(context),
            };
            match client.request::<Completion>(params) {
                Ok(result) => {
                    // Stale: park forever so Kit never applies this response.
                    if completion_gen.load(Ordering::SeqCst) != req_id {
                        std::future::pending::<()>().await;
                    }
                    let items = match result.unwrap_or(CompletionResponse::Array(vec![])) {
                        CompletionResponse::Array(items) => items,
                        CompletionResponse::List(list) => list.items,
                    };
                    let ranked = rank_completions(items, &filter_prefix);
                    Ok(CompletionResponse::Array(fix_completion_text_edits(
                        ranked,
                        replace_range,
                    )))
                }
                Err(err) => {
                    if completion_gen.load(Ordering::SeqCst) != req_id {
                        std::future::pending::<()>().await;
                    }
                    log::warn!("LSP completion: {err:#}");
                    Err(err)
                }
            }
        })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        new_text
            .chars()
            .any(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':' || c == '(')
    }
}

/// Kit's completion trigger ≠ LSP CompletionContext. Repair before the wire.
fn normalize_completion_context(kit: CompletionContext) -> CompletionContext {
    match kit.trigger_character.as_deref() {
        Some(ch) if ch.chars().count() == 1 => {
            let c = ch.chars().next().unwrap();
            if is_lsp_trigger_character(c) {
                CompletionContext {
                    trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                    trigger_character: Some(ch.to_string()),
                }
            } else {
                CompletionContext {
                    trigger_kind: CompletionTriggerKind::INVOKED,
                    trigger_character: None,
                }
            }
        }
        // Kit stuffs the whole query here (`a.` / `a.map`). If it ends with a
        // real trigger char, advertise that; otherwise Invoked.
        Some(s) => {
            if let Some(c) = s.chars().last().filter(|c| is_lsp_trigger_character(*c)) {
                CompletionContext {
                    trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                    trigger_character: Some(c.to_string()),
                }
            } else {
                CompletionContext {
                    trigger_kind: CompletionTriggerKind::INVOKED,
                    trigger_character: None,
                }
            }
        }
        None => CompletionContext {
            trigger_kind: CompletionTriggerKind::INVOKED,
            trigger_character: None,
        },
    }
}

fn is_lsp_trigger_character(c: char) -> bool {
    matches!(c, '.' | ':' | '(' | '<' | '"' | '\'' | '/' | '@')
}

/// Kit query is often `a.map` — the filter prefix is `map` (after last trigger).
fn completion_filter_prefix(kit_query: &str) -> String {
    let query = kit_query.trim();
    if query.is_empty() {
        return String::new();
    }
    match query.rfind(|c: char| is_lsp_trigger_character(c)) {
        Some(i) => query[i + c_len_at(query, i)..].to_string(),
        None => query.to_string(),
    }
}

fn c_len_at(s: &str, byte_idx: usize) -> usize {
    s[byte_idx..]
        .chars()
        .next()
        .map(|c| c.len_utf8())
        .unwrap_or(1)
}

/// Strip `foo.bar(…)` / snippets down to the identifier the user is typing.
fn normalize_match_token(raw: &str) -> String {
    let mut s = raw.trim();
    // Snippet / insertText: take before `$` or `{`.
    if let Some(i) = s.find(['$', '{']) {
        s = s[..i].trim_end_matches(['(', '.', ' ']);
    }
    let base = s.split('(').next().unwrap_or(s).trim();
    // `Array.prototype.map` / `.map` → `map`
    let after_dot = base.rsplit(['.', ':']).next().unwrap_or(base).trim();
    after_dot.trim_start_matches('.').to_string()
}

fn match_candidates(item: &CompletionItem) -> Vec<String> {
    let mut out = Vec::with_capacity(3);
    for raw in [
        item.filter_text.as_deref(),
        item.insert_text.as_deref(),
        Some(item.label.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if raw.is_empty() {
            continue;
        }
        let token = normalize_match_token(raw);
        if !token.is_empty() && !out.iter().any(|t| t == &token) {
            out.push(token);
        }
    }
    out
}

/// Prefer exact/prefix matches for the typed segment; drop non-matches when we
/// have a non-empty prefix so `flatMap` doesn't bury `map`.
fn rank_completions(items: Vec<CompletionItem>, prefix: &str) -> Vec<CompletionItem> {
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut scored: Vec<(u8, usize, String, CompletionItem)> = items
        .into_iter()
        .filter_map(|mut item| {
            let candidates = match_candidates(&item);
            let best = if prefix_lower.is_empty() {
                candidates.into_iter().next()
            } else {
                // Any candidate that prefixes wins — don't let a weird
                // filterText hide a good label (common with tsserver).
                candidates
                    .into_iter()
                    .filter(|t| t.to_ascii_lowercase().starts_with(&prefix_lower))
                    .min_by_key(|t| {
                        let lower = t.to_ascii_lowercase();
                        let exact = if lower == prefix_lower { 0u8 } else { 1 };
                        (exact, t.len())
                    })
            };
            let text = best?;
            let text_lower = text.to_ascii_lowercase();
            let tier = if !prefix_lower.is_empty() && text_lower == prefix_lower {
                0
            } else {
                1
            };
            // Overwrite sort_text so any consumer that trusts the server order
            // still prefers exact/short matches.
            item.sort_text = Some(format!("{tier:02}_{:04}_{text_lower}", text.len()));
            Some((tier, text.len(), text_lower, item))
        })
        .collect();

    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1)) // shorter: map before mapOr
            .then_with(|| a.2.cmp(&b.2))
    });
    scored.into_iter().map(|(_, _, _, item)| item).collect()
}

fn replace_start_offset(text: &Rope, cursor: usize, prefix: &str) -> usize {
    if prefix.is_empty() || cursor == 0 {
        return cursor;
    }
    let start = cursor.saturating_sub(prefix.len());
    if text.is_char_boundary(start)
        && text.is_char_boundary(cursor)
        && text.slice(start..cursor).to_string() == prefix
    {
        return start;
    }
    // Fallback: walk back over identifier chars.
    let before = text.slice(0..cursor).to_string();
    let ident_start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .map(|(i, _)| i)
        .last()
        .unwrap_or(cursor);
    ident_start
}

fn position_before(a: LspPosition, b: LspPosition) -> bool {
    a.line < b.line || (a.line == b.line && a.character < b.character)
}

fn completion_insert_text(item: &CompletionItem) -> String {
    if let Some(t) = item.insert_text.as_ref().filter(|s| !s.is_empty()) {
        return t.clone();
    }
    normalize_match_token(&item.label)
}

/// Kit's fallback replace range is the whole latch (`a.map`). Force a textEdit
/// that only covers the typed segment so Accept yields `a.map`, not `map`.
fn fix_completion_text_edits(
    items: Vec<CompletionItem>,
    replace: LspRange,
) -> Vec<CompletionItem> {
    items
        .into_iter()
        .map(|mut item| {
            match item.text_edit.take() {
                None => {
                    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                        range: replace,
                        new_text: completion_insert_text(&item),
                    }));
                }
                Some(CompletionTextEdit::Edit(mut edit)) => {
                    if position_before(edit.range.start, replace.start) {
                        edit.range.start = replace.start;
                    }
                    if position_before(replace.end, edit.range.end) {
                        edit.range.end = replace.end;
                    }
                    item.text_edit = Some(CompletionTextEdit::Edit(edit));
                }
                Some(CompletionTextEdit::InsertAndReplace(edit)) => {
                    let mut start = edit.replace.start;
                    let mut end = edit.replace.end;
                    if position_before(start, replace.start) {
                        start = replace.start;
                    }
                    if position_before(replace.end, end) {
                        end = replace.end;
                    }
                    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                        range: LspRange { start, end },
                        new_text: edit.new_text,
                    }));
                }
            }
            item
        })
        .collect()
}

/// Tab → IndentInline in Kit; when the completion menu is open, accept instead.
pub fn install_tab_accepts_completion<T: 'static>(
    state: &Entity<gpui_kit::component::input::EditorState>,
    cx: &mut Context<T>,
) -> Subscription {
    state.update(cx, |_, cx| {
        let editor = cx.weak_entity();
        cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key.as_str() != "tab" {
                return;
            }
            let mods = &event.keystroke.modifiers;
            if mods.shift || mods.control || mods.alt || mods.platform {
                return;
            }
            let Some(editor) = editor.upgrade() else {
                return;
            };
            let handled = editor.update(cx, |state, cx| {
                if !state.completion_menu_state().open {
                    return false;
                }
                state.route_overlay_action(
                    Box::new(gpui_kit::component::input::Enter {
                        secondary: false,
                        shift: false,
                    }),
                    window,
                    cx,
                )
            });
            if handled {
                cx.stop_propagation();
            }
        })
    })
}

impl HoverProvider for DocBridge {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<lsp_types::Hover>>> {
        let client = self.client.clone();
        let version = self.version.clone();
        let uri = self.uri.clone();
        let doc = text.to_string();
        let position = text.offset_to_position(offset);
        cx.background_spawn(async move {
            let version = version.fetch_add(1, Ordering::SeqCst) + 1;
            client.notify(
                DidChangeTextDocument::METHOD,
                DidChangeTextDocumentParams {
                    text_document: lsp_types::VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: doc,
                    }],
                },
            )?;
            client.request::<HoverRequest>(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
        })
    }
}

impl DefinitionProvider for DocBridge {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<LocationLink>>> {
        let client = self.client.clone();
        let version = self.version.clone();
        let uri = self.uri.clone();
        let doc = text.to_string();
        let position = text.offset_to_position(offset);
        cx.background_spawn(async move {
            let version = version.fetch_add(1, Ordering::SeqCst) + 1;
            client.notify(
                DidChangeTextDocument::METHOD,
                DidChangeTextDocumentParams {
                    text_document: lsp_types::VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: doc,
                    }],
                },
            )?;
            let response = client.request::<GotoDefinition>(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })?;
            Ok(goto_definition_to_links(response.unwrap_or(
                GotoDefinitionResponse::Array(vec![]),
            )))
        })
    }
}

fn goto_definition_to_links(response: GotoDefinitionResponse) -> Vec<LocationLink> {
    match response {
        GotoDefinitionResponse::Scalar(loc) => vec![location_to_link(loc)],
        GotoDefinitionResponse::Array(locs) => locs.into_iter().map(location_to_link).collect(),
        GotoDefinitionResponse::Link(links) => links,
    }
}

fn location_to_link(loc: Location) -> LocationLink {
    LocationLink {
        origin_selection_range: None,
        target_uri: loc.uri,
        target_range: loc.range,
        target_selection_range: loc.range,
    }
}

/// Build Kit `show_document` hook: open another file in the host, or fall through for same-buffer.
///
/// **Must not** call `Entity::update` on the current editor (or anything that
/// re-enters it) synchronously — Kit invokes this from inside
/// `EditorState::go_to_definition`, and GPUI panics on nested leases
/// (`cannot update … while it is already being updated`). Cross-file opens
/// are deferred onto the window.
pub fn show_document_handler<A: 'static>(
    doc_path: std::path::PathBuf,
    app: WeakEntity<A>,
    open: impl Fn(&mut A, &std::path::Path, lsp_types::Range, &mut Window, &mut Context<A>)
    + 'static,
) -> ShowDocumentHandler {
    let open = Rc::new(open);
    Rc::new(move |params, window, cx| {
        let Some(path) = super::uri::uri_to_path(&params.uri) else {
            // Virtual / non-file URIs: swallow rather than let Kit jump in the
            // *current* buffer using another file's line range.
            log::warn!("LSP showDocument: unsupported URI {}", params.uri.as_str());
            return true;
        };
        // Same document: let Kit move the cursor locally (no nested update).
        if same_document_path(&path, &doc_path) {
            return false;
        }
        let selection = params.selection.unwrap_or(lsp_types::Range {
            start: LspPosition {
                line: 0,
                character: 0,
            },
            end: LspPosition {
                line: 0,
                character: 0,
            },
        });
        let open = open.clone();
        let app = app.clone();
        window.defer(cx, move |window, cx| {
            let _ = app.update(cx, |this, cx| {
                open(this, &path, selection, window, cx);
            });
        });
        true
    })
}

/// Path equality that survives symlink / non-canonical LSP `file://` targets.
fn same_document_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Convenience: URI for an open path (logs and skips LSP on failure).
pub fn try_path_uri(path: &std::path::Path) -> Option<lsp_types::Uri> {
    match path_to_uri(path) {
        Ok(uri) => Some(uri),
        Err(err) => {
            log::warn!("LSP URI for {}: {err:#}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_completion_context, same_document_path};
    use lsp_types::{CompletionContext, CompletionTriggerKind};
    use std::path::Path;

    #[test]
    fn same_path_compares_equal() {
        let p = Path::new("/tmp/isengard-lsp-same.rs");
        assert!(same_document_path(p, p));
    }

    #[test]
    fn kit_query_as_trigger_becomes_invoked() {
        let fixed = normalize_completion_context(CompletionContext {
            trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
            trigger_character: Some("println".into()),
        });
        assert_eq!(fixed.trigger_kind, CompletionTriggerKind::INVOKED);
        assert!(fixed.trigger_character.is_none());
    }

    #[test]
    fn single_dot_stays_trigger_character() {
        let fixed = normalize_completion_context(CompletionContext {
            trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
            trigger_character: Some(".".into()),
        });
        assert_eq!(fixed.trigger_kind, CompletionTriggerKind::TRIGGER_CHARACTER);
        assert_eq!(fixed.trigger_character.as_deref(), Some("."));
    }

    #[test]
    fn query_ending_in_dot_becomes_trigger_dot() {
        let fixed = normalize_completion_context(CompletionContext {
            trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
            trigger_character: Some("a.".into()),
        });
        assert_eq!(fixed.trigger_kind, CompletionTriggerKind::TRIGGER_CHARACTER);
        assert_eq!(fixed.trigger_character.as_deref(), Some("."));
    }

    #[test]
    fn filter_prefix_after_dot() {
        assert_eq!(super::completion_filter_prefix("a.map"), "map");
        assert_eq!(super::completion_filter_prefix("a."), "");
        assert_eq!(super::completion_filter_prefix("foo"), "foo");
    }

    #[test]
    fn rank_puts_map_before_flatmap_and_drops_non_prefix() {
        use lsp_types::CompletionItem;
        let items = vec![
            CompletionItem {
                label: "flatMap".into(),
                sort_text: Some("01".into()),
                ..Default::default()
            },
            CompletionItem {
                label: "map".into(),
                sort_text: Some("99".into()),
                ..Default::default()
            },
            CompletionItem {
                label: "filter".into(),
                ..Default::default()
            },
        ];
        let ranked = super::rank_completions(items, "map");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].label, "map");
    }

    #[test]
    fn rank_uses_label_when_filter_text_is_wrong() {
        use lsp_types::CompletionItem;
        let items = vec![
            CompletionItem {
                label: "map".into(),
                // tsserver occasionally ships a non-prefix filterText; label still wins.
                filter_text: Some("Array.prototype.map".into()),
                ..Default::default()
            },
            CompletionItem {
                label: "flatMap".into(),
                filter_text: Some("flatMap".into()),
                ..Default::default()
            },
        ];
        let ranked = super::rank_completions(items, "map");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].label, "map");
    }

    #[test]
    fn filter_prefix_from_dot_query() {
        assert_eq!(super::completion_filter_prefix(".map"), "map");
    }

    #[test]
    fn text_edit_only_replaces_typed_segment() {
        use lsp_types::{CompletionItem, Position, Range};
        let replace = Range {
            start: Position::new(0, 2), // after "a."
            end: Position::new(0, 5),   // end of "map"
        };
        let items = vec![CompletionItem {
            label: "map".into(),
            ..Default::default()
        }];
        let fixed = super::fix_completion_text_edits(items, replace);
        let edit = match fixed[0].text_edit.as_ref().unwrap() {
            lsp_types::CompletionTextEdit::Edit(e) => e,
            _ => panic!("expected Edit"),
        };
        assert_eq!(edit.range, replace);
        assert_eq!(edit.new_text, "map");
    }

    #[test]
    fn text_edit_clamps_server_range_that_wipes_receiver() {
        use lsp_types::{CompletionItem, CompletionTextEdit, Position, Range, TextEdit};
        let replace = Range {
            start: Position::new(0, 2),
            end: Position::new(0, 5),
        };
        let items = vec![CompletionItem {
            label: "map".into(),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                // Wrong: covers "a.map"
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(0, 5),
                },
                new_text: "map".into(),
            })),
            ..Default::default()
        }];
        let fixed = super::fix_completion_text_edits(items, replace);
        let edit = match fixed[0].text_edit.as_ref().unwrap() {
            CompletionTextEdit::Edit(e) => e,
            _ => panic!("expected Edit"),
        };
        assert_eq!(edit.range.start, replace.start);
        assert_eq!(edit.new_text, "map");
    }
}
