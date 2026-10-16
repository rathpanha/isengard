//! Workspace-scoped language servers: start, document sync, diagnostics events.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use anyhow::Context as _;
use gpui_kit::base::input::Diagnostic;
use gpui_kit::component::input::EditorState;
use gpui_kit::*;
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::{
    ClientCapabilities, CompletionClientCapabilities, CompletionItemCapability,
    CompletionItemCapabilityResolveSupport, CompletionItemKindCapability,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GeneralClientCapabilities, HoverClientCapabilities, InitializeParams, MarkupKind,
    PositionEncodingKind, PublishDiagnosticsParams, TextDocumentClientCapabilities,
    TextDocumentContentChangeEvent, TextDocumentItem, TextDocumentSyncClientCapabilities,
    WorkspaceClientCapabilities, WorkspaceFolder,
};
use serde_json::{Value, json};

use super::bridge::{DocBridge, show_document_handler, try_path_uri};
use super::client::{Incoming, LspClient};
use super::install::{InstallRecipe, is_server_ready, recipe_for_command};
use super::registry::{self, ResolvedServerSpec};
use super::uri::{path_to_uri, uri_to_path};
use crate::config::LspSettings;
use crate::editor::document;
use crate::editor::language::Language;

/// Events from language servers that the UI must apply on the main thread.
#[derive(Debug)]
pub enum LspEvent {
    Diagnostics(PublishDiagnosticsParams),
}

/// First-time "server missing" signal for the host UI (install dialog / toast).
#[derive(Debug, Clone)]
pub struct MissingServerOffer {
    pub spec: ResolvedServerSpec,
    pub recipe: Option<InstallRecipe>,
    pub path: PathBuf,
    pub language: Language,
}

/// Lifecycle shown in the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerDisplayStatus {
    /// Remembered as installed, not currently started.
    Idle,
    Starting,
    Running,
    Error(String),
}

/// One language server row for the status-bar catalog popover.
#[derive(Debug, Clone)]
pub struct CatalogServer {
    pub process_key: String,
    pub label: String,
    pub status: ServerDisplayStatus,
    /// Binary ready app-local or on PATH (or remembered / running).
    pub installed: bool,
    pub recipe: Option<InstallRecipe>,
}

/// Result of opening a document for LSP.
#[derive(Debug)]
pub enum AttachOutcome {
    Done,
    /// Spawn the server off the UI thread, then [`LspHost::finish_start`].
    NeedsStart {
        spec: ResolvedServerSpec,
        init: InitializeParams,
        path: PathBuf,
        language: Language,
    },
}

struct RunningServer {
    client: Arc<LspClient>,
    /// Shared with [`DocBridge`] so request-path sync and Change notifications
    /// stay on one monotonic version clock.
    versions: HashMap<PathBuf, Arc<AtomicI32>>,
}

/// Owns language servers for the current window workspace.
pub struct LspHost {
    folders: Vec<PathBuf>,
    settings: LspSettings,
    /// Keyed by [`ResolvedServerSpec::process_key`] (usually the command name).
    servers: HashMap<String, RunningServer>,
    /// Starting / error / running overlay (running also implied by `servers`).
    lifecycle: HashMap<String, ServerDisplayStatus>,
    missing_toasted: HashSet<String>,
    events_tx: flume::Sender<LspEvent>,
    events_rx: flume::Receiver<LspEvent>,
}

impl Default for LspHost {
    fn default() -> Self {
        let (events_tx, events_rx) = flume::unbounded();
        Self {
            folders: Vec::new(),
            settings: LspSettings::default(),
            servers: HashMap::new(),
            lifecycle: HashMap::new(),
            missing_toasted: HashSet::new(),
            events_tx,
            events_rx,
        }
    }
}

impl LspHost {
    /// Cloneable receiver for [`LspEvent`] (mpmc).
    pub fn events(&self) -> flume::Receiver<LspEvent> {
        self.events_rx.clone()
    }

    pub fn set_folders(&mut self, folders: Vec<PathBuf>) {
        self.folders = folders;
    }

    pub fn set_settings(&mut self, settings: LspSettings) {
        self.settings = settings;
    }

    pub fn settings(&self) -> &LspSettings {
        &self.settings
    }

    pub fn shutdown_all(&mut self) {
        for (_, server) in self.servers.drain() {
            server.client.shutdown();
        }
        self.lifecycle.clear();
    }

    /// Allow another missing-server prompt after a failed/cancelled install.
    pub fn clear_missing_prompt(&mut self, process_key: &str) {
        self.missing_toasted.remove(process_key);
    }

    /// Returns `true` the first time we should offer install for this server.
    pub fn missing_prompt_once(&mut self, process_key: &str) -> bool {
        self.missing_toasted.insert(process_key.to_string())
    }

    /// After workspace switch: kill servers and remember new roots.
    pub fn reset_workspace(&mut self, folders: Vec<PathBuf>) {
        self.shutdown_all();
        self.folders = folders;
        self.missing_toasted.clear();
    }

    /// Status-bar catalog: installed / remembered / live servers, plus the
    /// default for `focus` (active tab language) so the user can Install it.
    ///
    /// Not the full registry — that dump belongs in a future gallery, not here.
    pub fn catalog_servers(&self, focus: Option<Language>) -> Vec<CatalogServer> {
        let mut keys: BTreeSet<String> = BTreeSet::new();
        keys.extend(self.settings.known.iter().cloned());
        keys.extend(self.lifecycle.keys().cloned());
        keys.extend(self.servers.keys().cloned());
        if let Some(language) = focus
            && let Some(spec) = registry::resolve(language, &self.settings)
        {
            keys.insert(spec.process_key().to_string());
        }

        let mut out: Vec<CatalogServer> = keys
            .into_iter()
            .filter_map(|process_key| {
                let recipe = recipe_for_command(&process_key);
                let status = self.status_for(&process_key);
                let on_path = is_server_ready(&process_key);
                let remembered = self.settings.known.iter().any(|k| k == &process_key);
                let live = matches!(
                    status,
                    ServerDisplayStatus::Running | ServerDisplayStatus::Starting
                );
                let installed = on_path || remembered || live;
                // Skip inert rows: no recipe and nothing installed / focused.
                if !installed && recipe.is_none() {
                    return None;
                }
                Some(CatalogServer {
                    label: command_label(&process_key),
                    process_key,
                    status,
                    installed,
                    recipe,
                })
            })
            .collect();
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
    }

    fn status_for(&self, process_key: &str) -> ServerDisplayStatus {
        if let Some(s) = self.lifecycle.get(process_key) {
            match s {
                ServerDisplayStatus::Starting | ServerDisplayStatus::Error(_) => return s.clone(),
                ServerDisplayStatus::Running | ServerDisplayStatus::Idle => {}
            }
        }
        if let Some(server) = self.servers.get(process_key) {
            if server.client.is_alive() {
                return ServerDisplayStatus::Running;
            }
            return ServerDisplayStatus::Error("language server exited".into());
        }
        ServerDisplayStatus::Idle
    }

    /// Remember a server as installed (persisted via [`Self::settings`]).
    pub fn remember_known(&mut self, process_key: &str) {
        if !self
            .settings
            .known
            .iter()
            .any(|k| k == process_key)
        {
            self.settings.known.push(process_key.to_string());
        }
    }

    fn forget_known(&mut self, process_key: &str) {
        self.settings.known.retain(|k| k != process_key);
    }

    /// Stop process (if any), forget from known, return open paths + uninstall recipe.
    ///
    /// Uninstall deletes app-local files only (never system packages).
    pub fn remove_server(&mut self, process_key: &str) -> (Vec<PathBuf>, Option<InstallRecipe>) {
        let paths = self.stop_server(process_key);
        self.forget_known(process_key);
        self.lifecycle.remove(process_key);
        let recipe = recipe_for_command(process_key)
            .filter(|_| super::install::is_app_local(process_key));
        (paths, recipe)
    }

    /// Shut down one language server. Returns paths that had the document open
    /// so the UI can clear Kit LSP providers / diagnostics.
    pub fn stop_server(&mut self, process_key: &str) -> Vec<PathBuf> {
        self.lifecycle.remove(process_key);
        let Some(server) = self.servers.remove(process_key) else {
            return Vec::new();
        };
        let paths: Vec<PathBuf> = server.versions.keys().cloned().collect();
        for path in &paths {
            if let Some(uri) = try_path_uri(path) {
                let _ = server.client.notify(
                    DidCloseTextDocument::METHOD,
                    DidCloseTextDocumentParams {
                        text_document: lsp_types::TextDocumentIdentifier { uri },
                    },
                );
            }
        }
        server.client.shutdown();
        self.missing_toasted.remove(process_key);
        paths
    }

    /// Open a disk-backed document for LSP (no-op for untitled / unmapped languages).
    #[allow(clippy::too_many_arguments)] // path/lang/text/state + showDocument host hooks
    pub fn attach_editor<A: 'static>(
        &mut self,
        path: &Path,
        language: Language,
        text: &str,
        state: &Entity<EditorState>,
        app: WeakEntity<A>,
        open_location: impl Fn(&mut A, &Path, lsp_types::Range, &mut Window, &mut Context<A>)
        + 'static,
        _window: &mut Window,
        cx: &mut Context<A>,
    ) -> AttachOutcome {
        if document::is_untitled(path) {
            return AttachOutcome::Done;
        }
        let Some(spec) = registry::resolve(language, &self.settings) else {
            return AttachOutcome::Done;
        };
        let Some(uri) = try_path_uri(path) else {
            return AttachOutcome::Done;
        };

        let process_key = spec.process_key().to_string();
        let language_id = spec.language_id.clone();

        if matches!(
            self.lifecycle.get(&process_key),
            Some(ServerDisplayStatus::Starting)
        ) {
            // Spawn already in flight; document wires when [`finish_start`] runs.
            return AttachOutcome::Done;
        }

        if let Some(server) = self.servers.get(&process_key)
            && server.client.is_alive()
        {
            let client = server.client.clone();
            self.wire_document(
                &process_key,
                path,
                &language_id,
                text,
                uri,
                &client,
                state,
                app,
                open_location,
                cx,
            );
            return AttachOutcome::Done;
        }

        let init = match self.build_init(&spec.command) {
            Ok(init) => init,
            Err(err) => {
                log::warn!("LSP: {err:#}");
                self.lifecycle.insert(
                    process_key.clone(),
                    ServerDisplayStatus::Error(err.to_string()),
                );
                return AttachOutcome::Done;
            }
        };

        self.lifecycle
            .insert(process_key, ServerDisplayStatus::Starting);
        AttachOutcome::NeedsStart {
            spec,
            init,
            path: path.to_path_buf(),
            language,
        }
    }

    /// Register a client started off-thread; returns the shared handle.
    pub fn finish_start(
        &mut self,
        spec: &ResolvedServerSpec,
        client: LspClient,
        incoming: flume::Receiver<Incoming>,
    ) -> anyhow::Result<Arc<LspClient>> {
        let process_key = spec.process_key().to_string();
        let client = Arc::new(client);
        self.spawn_event_thread(client.clone(), incoming)?;
        self.servers.insert(
            process_key.clone(),
            RunningServer {
                client: client.clone(),
                versions: HashMap::new(),
            },
        );
        self.lifecycle
            .insert(process_key.clone(), ServerDisplayStatus::Running);
        self.remember_known(&process_key);
        self.missing_toasted.remove(&process_key);
        Ok(client)
    }

    pub fn mark_start_failed(&mut self, process_key: &str, err: &anyhow::Error) {
        self.lifecycle.insert(
            process_key.to_string(),
            ServerDisplayStatus::Error(format!("{err:#}")),
        );
    }

    /// Whether a start failure should offer the Install dialog.
    pub fn looks_like_missing(err: &anyhow::Error) -> bool {
        let msg = format!("{err:#}").to_lowercase();
        msg.contains("not found")
            || msg.contains("not installed")
            || msg.contains("no such file")
            || msg.contains("is not an executable")
    }

    /// Wire Kit providers + didOpen after the client is running.
    #[allow(clippy::too_many_arguments)]
    pub fn attach_open_document<A: 'static>(
        &mut self,
        path: &Path,
        language: Language,
        text: &str,
        state: &Entity<EditorState>,
        app: WeakEntity<A>,
        open_location: impl Fn(&mut A, &Path, lsp_types::Range, &mut Window, &mut Context<A>)
        + 'static,
        cx: &mut Context<A>,
    ) {
        let Some(spec) = registry::resolve(language, &self.settings) else {
            return;
        };
        let Some(uri) = try_path_uri(path) else {
            return;
        };
        let process_key = spec.process_key().to_string();
        let Some(server) = self.servers.get(&process_key) else {
            return;
        };
        if !server.client.is_alive() {
            return;
        }
        let client = server.client.clone();
        let language_id = spec.language_id.clone();
        self.wire_document(
            &process_key,
            path,
            &language_id,
            text,
            uri,
            &client,
            state,
            app,
            open_location,
            cx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn wire_document<A: 'static>(
        &mut self,
        process_key: &str,
        path: &Path,
        language_id: &str,
        text: &str,
        uri: lsp_types::Uri,
        client: &Arc<LspClient>,
        state: &Entity<EditorState>,
        app: WeakEntity<A>,
        open_location: impl Fn(&mut A, &Path, lsp_types::Range, &mut Window, &mut Context<A>)
        + 'static,
        cx: &mut Context<A>,
    ) {
        let server = match self.servers.get_mut(process_key) {
            Some(s) => s,
            None => return,
        };

        let version = if let Some(v) = server.versions.get(path) {
            v.clone()
        } else {
            let v = Arc::new(AtomicI32::new(0));
            if let Err(err) = client.notify(
                DidOpenTextDocument::METHOD,
                DidOpenTextDocumentParams {
                    text_document: TextDocumentItem {
                        uri: uri.clone(),
                        language_id: language_id.to_string(),
                        version: 0,
                        text: text.to_string(),
                    },
                },
            ) {
                log::warn!("LSP didOpen: {err:#}");
                return;
            }
            server.versions.insert(path.to_path_buf(), v.clone());
            v
        };

        let bridge = Rc::new(DocBridge {
            client: client.clone(),
            uri,
            version,
            completion_gen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        });
        let show = show_document_handler(path.to_path_buf(), app, open_location);

        state.update(cx, |state, cx| {
            let lsp = state.lsp_mut();
            lsp.completion_provider = Some(bridge.clone());
            lsp.hover_provider = Some(bridge.clone());
            lsp.definition_provider = Some(bridge);
            lsp.show_document = Some(show);
            // List can grow for "Auto import from '…'" labels; docs capped in Kit.
            lsp.completion_menu.max_width = gpui_kit::px(400.);
            cx.notify();
        });
    }

    pub fn did_change(&mut self, path: &Path, text: &str) {
        if document::is_untitled(path) {
            return;
        }
        let Some(uri) = try_path_uri(path) else {
            return;
        };
        for server in self.servers.values_mut() {
            let Some(version) = server.versions.get(path) else {
                continue;
            };
            let version = version.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = server.client.notify(
                DidChangeTextDocument::METHOD,
                DidChangeTextDocumentParams {
                    text_document: lsp_types::VersionedTextDocumentIdentifier { uri, version },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: text.to_string(),
                    }],
                },
            );
            return;
        }
    }

    pub fn did_close(&mut self, path: &Path) {
        if document::is_untitled(path) {
            return;
        }
        let Some(uri) = try_path_uri(path) else {
            return;
        };
        for server in self.servers.values_mut() {
            if server.versions.remove(path).is_none() {
                continue;
            }
            let _ = server.client.notify(
                DidCloseTextDocument::METHOD,
                DidCloseTextDocumentParams {
                    text_document: lsp_types::TextDocumentIdentifier { uri },
                },
            );
            return;
        }
    }

    fn build_init(&self, command: &str) -> anyhow::Result<InitializeParams> {
        let folders = self.folders.clone();
        if folders.is_empty() {
            anyhow::bail!("no workspace folder for language server");
        }

        let workspace_folders: Vec<WorkspaceFolder> = folders
            .iter()
            .filter_map(|folder| {
                let uri = path_to_uri(folder).ok()?;
                let name = folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| folder.display().to_string());
                Some(WorkspaceFolder { uri, name })
            })
            .collect();

        let root_uri = workspace_folders
            .first()
            .map(|f| f.uri.clone())
            .context("workspace folder URI")?;

        #[allow(deprecated)]
        Ok(InitializeParams {
            process_id: Some(std::process::id()),
            root_path: None,
            root_uri: Some(root_uri),
            initialization_options: initialization_options(command),
            capabilities: client_capabilities(),
            trace: None,
            workspace_folders: Some(workspace_folders),
            client_info: Some(lsp_types::ClientInfo {
                name: "Isengard".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            locale: None,
            work_done_progress_params: Default::default(),
        })
    }

    fn spawn_event_thread(
        &self,
        client: Arc<LspClient>,
        incoming: flume::Receiver<Incoming>,
    ) -> anyhow::Result<()> {
        let events_tx = self.events_tx.clone();
        let folders_for_config: Vec<WorkspaceFolder> = self
            .folders
            .iter()
            .filter_map(|folder| {
                let uri = path_to_uri(folder).ok()?;
                let name = folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| folder.display().to_string());
                Some(WorkspaceFolder { uri, name })
            })
            .collect();
        let client_for_events = client;

        std::thread::Builder::new()
            .name("isengard-lsp-events".into())
            .spawn(move || {
                while let Ok(msg) = incoming.recv() {
                    match msg {
                        Incoming::Notification { method, params } => {
                            if method == PublishDiagnostics::METHOD
                                && let Ok(p) =
                                    serde_json::from_value::<PublishDiagnosticsParams>(params)
                            {
                                let _ = events_tx.send(LspEvent::Diagnostics(p));
                            }
                        }
                        Incoming::ServerRequest { id, method, params } => {
                            let result = match method.as_str() {
                                "workspace/configuration" => {
                                    let count = params
                                        .get("items")
                                        .and_then(|i| i.as_array())
                                        .map(|a| a.len())
                                        .unwrap_or(1);
                                    json!(vec![json!({}); count])
                                }
                                "workspace/workspaceFolders" => json!(folders_for_config),
                                "client/registerCapability" | "client/unregisterCapability" => {
                                    Value::Null
                                }
                                "window/workDoneProgress/create" => Value::Null,
                                _ => {
                                    let _ = client_for_events.respond_error(
                                        id,
                                        &format!("Method Not Found: {method}"),
                                    );
                                    continue;
                                }
                            };
                            let _ = client_for_events.respond(id, result);
                        }
                    }
                }
            })
            .context("spawn LSP event thread")?;
        Ok(())
    }
}

fn command_label(command: &str) -> String {
    std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command)
        .to_string()
}

fn initialization_options(command: &str) -> Option<Value> {
    // typescript-language-server hides module-export / auto-import completions
    // unless these preferences are on (VS Code sets them by default).
    if command.contains("typescript-language-server") {
        return Some(json!({
            "preferences": {
                "includeCompletionsForModuleExports": true,
                "includeCompletionsWithInsertText": true,
                "includeCompletionsWithSnippetText": true,
            }
        }));
    }
    None
}

fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        workspace: Some(WorkspaceClientCapabilities {
            workspace_folders: Some(true),
            configuration: Some(true),
            ..Default::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            synchronization: Some(TextDocumentSyncClientCapabilities {
                dynamic_registration: Some(false),
                will_save: Some(false),
                will_save_wait_until: Some(false),
                did_save: Some(true),
            }),
            completion: Some(CompletionClientCapabilities {
                dynamic_registration: Some(false),
                context_support: Some(true),
                completion_item: Some(CompletionItemCapability {
                    snippet_support: Some(true),
                    commit_characters_support: Some(true),
                    documentation_format: Some(vec![
                        MarkupKind::Markdown,
                        MarkupKind::PlainText,
                    ]),
                    deprecated_support: Some(true),
                    preselect_support: Some(true),
                    insert_replace_support: Some(true),
                    // Without this, rust-analyzer / tsserver omit auto-import
                    // items (additionalTextEdits arrive via completionItem/resolve).
                    resolve_support: Some(CompletionItemCapabilityResolveSupport {
                        properties: vec![
                            "documentation".into(),
                            "detail".into(),
                            "additionalTextEdits".into(),
                            "insertText".into(),
                            "textEdit".into(),
                            "command".into(),
                        ],
                    }),
                    label_details_support: Some(true),
                    ..Default::default()
                }),
                completion_item_kind: Some(CompletionItemKindCapability {
                    value_set: None,
                }),
                ..Default::default()
            }),
            hover: Some(HoverClientCapabilities {
                dynamic_registration: Some(false),
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
            }),
            definition: Some(Default::default()),
            publish_diagnostics: Some(Default::default()),
            ..Default::default()
        }),
        general: Some(GeneralClientCapabilities {
            position_encodings: Some(vec![PositionEncodingKind::UTF16]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Apply `textDocument/publishDiagnostics` to an open editor, if any.
pub fn apply_diagnostics(
    state: &Entity<EditorState>,
    params: PublishDiagnosticsParams,
    cx: &mut App,
) {
    state.update(cx, |state, cx| {
        let rope = gpui_kit::component::input::Rope::from(state.value().as_str());
        if let Some(set) = state.diagnostics_mut() {
            set.reset(&rope);
            set.extend(params.diagnostics.into_iter().map(Diagnostic::from));
        }
        cx.notify();
    });
}

pub fn path_for_diagnostics(params: &PublishDiagnosticsParams) -> Option<PathBuf> {
    uri_to_path(&params.uri)
}
