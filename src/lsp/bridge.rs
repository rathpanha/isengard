//! Kit `CompletionProvider` / `HoverProvider` / `DefinitionProvider` over an [`LspClient`].

use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use gpui_kit::component::input::{
    CompletionProvider, DefinitionProvider, HoverProvider, Rope, RopeExt as _,
    ShowDocumentHandler,
};
use gpui_kit::*;
use lsp_types::request::{Completion, GotoDefinition, HoverRequest};
use lsp_types::{
    CompletionContext, CompletionParams, CompletionResponse, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, Location, LocationLink, PartialResultParams,
    Position as LspPosition, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};

use super::client::LspClient;
use super::uri::path_to_uri;

/// Forwards editor LSP hooks to one running language server for a single document URI.
pub struct DocBridge {
    pub client: Arc<LspClient>,
    pub uri: lsp_types::Uri,
}

impl DocBridge {
    fn text_document_position(&self, text: &Rope, offset: usize) -> TextDocumentPositionParams {
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri.clone(),
            },
            // Kit RopeExt positions are lsp_types::Position (UTF-16).
            position: text.offset_to_position(offset),
        }
    }
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
        let params = CompletionParams {
            text_document_position: self.text_document_position(text, offset),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(trigger),
        };
        cx.background_spawn(async move {
            let result = client.request::<Completion>(params)?;
            Ok(result.unwrap_or(CompletionResponse::Array(vec![])))
        })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        new_text.chars().any(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':')
    }
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
        let params = HoverParams {
            text_document_position_params: self.text_document_position(text, offset),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        cx.background_spawn(async move { client.request::<HoverRequest>(params) })
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
        let params = GotoDefinitionParams {
            text_document_position_params: self.text_document_position(text, offset),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        cx.background_spawn(async move {
            let response = client.request::<GotoDefinition>(params)?;
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
pub fn show_document_handler<A: 'static>(
    doc_path: std::path::PathBuf,
    app: WeakEntity<A>,
    open: impl Fn(&mut A, &std::path::Path, lsp_types::Range, &mut Window, &mut Context<A>)
    + 'static,
) -> ShowDocumentHandler {
    let open = Rc::new(open);
    Rc::new(move |params, window, cx| {
        let Some(path) = super::uri::uri_to_path(&params.uri) else {
            return false;
        };
        // Same document: let Kit move the cursor locally.
        if path == doc_path {
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
        app.update(cx, |this, cx| {
            open(this, &path, selection, window, cx);
            true
        })
        .unwrap_or(false)
    })
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
