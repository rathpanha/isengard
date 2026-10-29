//! One ACP client connection on its own OS thread.
//!
//! The SDK is runtime-neutral, so the connection future runs under
//! `futures::executor::block_on` (no Tokio). The UI talks to it through two
//! flume channels: [`AgentCommand`]s in, [`AgentEvent`]s out. Requests that
//! need the editor (file reads/writes, permission prompts) carry a reply
//! sender; the SDK handler waits for it in a spawned task so the dispatch loop
//! keeps streaming updates meanwhile. Dropping [`AgentConnection`] ends the
//! loop, which drops the transport and kills the agent's process group.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AuthCapabilities, AuthMethod, CancelNotification, ClientCapabilities, ClientSessionCapabilities,
    SubagentCapabilities,
    CloseSessionRequest, CompactionCapabilities, ContentBlock, DeleteSessionRequest,
    FileSystemCapabilities, Implementation, InitializeRequest, LoadSessionRequest, NewSessionRequest,
    PermissionOption, PermissionOptionId, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    ResumeSessionRequest, SelectedPermissionOutcome, SessionConfigId, SessionConfigOption,
    SessionConfigOptionValue, SessionId, SessionModeId, SessionModeState, SessionNotification,
    SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest, StopReason, ToolCallUpdate,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectTo, ConnectionTo, Error, ErrorCode,
};

use crate::acp::registry::AgentLaunch;

/// Select or boolean config to apply on a throwaway silent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SilentConfigValue {
    Select(String),
    Bool(bool),
}

/// In-flight throwaway silent job (Improve / title / Suggest).
struct SilentJob {
    session_id: SessionId,
    text: String,
    aborted: bool,
}

/// Concurrent silent jobs by their scratch session id.
type SilentSlot = Arc<Mutex<HashMap<SessionId, SilentJob>>>;

/// UI → agent.
enum AgentCommand {
    Open(SessionOpen),
    Prompt(Vec<ContentBlock>),
    /// One-shot on a throwaway session; does not touch the open chat.
    SilentPrompt {
        job: u64,
        blocks: Vec<ContentBlock>,
        mode: Option<String>,
        configs: Vec<(String, SilentConfigValue)>,
    },
    Cancel,
    /// Cancel throwaway silent jobs only — leave the open chat turn alone.
    CancelSilent,
    Delete(String),
    SetMode(String),
    SetConfig { config_id: String, value: String },
    SetConfigBool { config_id: String, value: bool },
}

/// How to open a chat on a connected agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOpen {
    New,
    /// `session/load`: the agent replays the chat as `Update`s first.
    Load(String),
    /// `session/resume`: no replay (the UI still has the transcript).
    Resume(String),
}

/// What the agent said about itself in `initialize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    pub name: String,
    pub can_load: bool,
    pub can_resume: bool,
    pub can_delete: bool,
    pub can_image: bool,
    pub can_embedded_context: bool,
}

/// A terminal login the agent advertised (`authMethods[].type == "terminal"`):
/// run the agent's own command with these args in a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginMethod {
    pub name: String,
    pub description: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// The agent wants approval before running a tool.
pub struct PermissionAsk {
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
    /// `None` = cancelled (e.g. the user stopped the turn).
    pub reply: flume::Sender<Option<PermissionOptionId>>,
}

/// `fs/read_text_file` — the editor answers from the open buffer or disk.
pub struct FileRead {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub limit: Option<u32>,
    pub reply: flume::Sender<Result<String, String>>,
}

/// `fs/write_text_file` — the editor writes disk and refreshes open tabs.
pub struct FileWrite {
    pub path: PathBuf,
    pub content: String,
    pub reply: flume::Sender<Result<(), String>>,
}

/// Agent → UI.
pub enum AgentEvent {
    /// `initialize` done; send [`AgentConnection::open`] next.
    Connected(AgentInfo),
    /// A chat is open (after any replay); prompts go to it from now on.
    SessionOpened {
        id: String,
        modes: Option<SessionModeState>,
        config_options: Option<Vec<SessionConfigOption>>,
    },
    /// Opening a chat failed (e.g. it was deleted outside Isengard).
    SessionOpenFailed { open: SessionOpen, message: String },
    Update { session: String, update: SessionUpdate },
    /// `session/set_config_option` returned a full options snapshot.
    ConfigOptions(Vec<SessionConfigOption>),
    TurnEnded(StopReason),
    TurnFailed(String),
    /// Throwaway silent job finished (`ok` = EndTurn and not aborted).
    SilentTurnEnded { job: u64, ok: bool, text: String },
    /// Not signed in. The connection is finished; restart it after login.
    AuthRequired(Vec<LoginMethod>),
    Permission(PermissionAsk),
    ReadFile(FileRead),
    WriteFile(FileWrite),
    /// Connection ended (`Some` = error such as a spawn failure or crash).
    Closed(Option<String>),
}

/// Handle to a running connection. Drop to shut the agent down.
pub struct AgentConnection {
    commands: flume::Sender<AgentCommand>,
    pub events: flume::Receiver<AgentEvent>,
}

impl AgentConnection {
    /// Spawn the agent process for `launch`. Sessions run in `roots[0]`; the
    /// other workspace roots are sent as additional directories.
    pub fn spawn(launch: &AgentLaunch, roots: Vec<PathBuf>) -> Self {
        let config = AcpAgentConfig::new(launch.command.clone())
            .args(launch.args.iter().cloned())
            .envs(launch.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        Self::spawn_with(AcpAgent::new(config), roots, launch.claude)
    }

    /// Run against any transport (tests use an in-memory fake agent).
    /// `claude` marks the built-in Claude adapter (see [`claude_session_meta`]).
    pub fn spawn_with(
        transport: impl ConnectTo<Client> + 'static,
        roots: Vec<PathBuf>,
        claude: bool,
    ) -> Self {
        let (commands, command_rx) = flume::unbounded();
        let (event_tx, events) = flume::unbounded();
        let spawned = std::thread::Builder::new()
            .name("acp-agent".into())
            .spawn({
                let event_tx = event_tx.clone();
                move || {
                    let result =
                        futures::executor::block_on(run(transport, roots, claude, command_rx, event_tx.clone()));
                    let _ = event_tx.send(AgentEvent::Closed(result.err().map(describe_error)));
                }
            });
        if let Err(err) = spawned {
            let _ = event_tx.send(AgentEvent::Closed(Some(format!(
                "Could not start the agent thread: {err}"
            ))));
        }
        Self { commands, events }
    }

    pub fn open(&self, open: SessionOpen) {
        let _ = self.commands.send(AgentCommand::Open(open));
    }

    /// Delete a stored chat (only when [`AgentInfo::can_delete`]).
    pub fn delete(&self, id: String) {
        let _ = self.commands.send(AgentCommand::Delete(id));
    }

    pub fn prompt(&self, blocks: Vec<ContentBlock>) {
        let _ = self.commands.send(AgentCommand::Prompt(blocks));
    }

    /// Prompt on a fresh throwaway session (Improve / title / Suggest).
    ///
    /// The open chat's `session_id` is not used; the scratch session is
    /// deleted (or closed) when the turn ends.
    pub fn silent_prompt(
        &self,
        job: u64,
        blocks: Vec<ContentBlock>,
        mode: Option<String>,
        configs: Vec<(String, SilentConfigValue)>,
    ) {
        let _ = self.commands.send(AgentCommand::SilentPrompt {
            job,
            blocks,
            mode,
            configs,
        });
    }

    pub fn cancel(&self) {
        let _ = self.commands.send(AgentCommand::Cancel);
    }

    /// Abort in-flight Improve / title / Suggest sessions without stopping the
    /// open chat turn.
    pub fn cancel_silent(&self) {
        let _ = self.commands.send(AgentCommand::CancelSilent);
    }

    /// `session/set_mode` — no-op if no chat is open.
    pub fn set_mode(&self, mode_id: impl Into<String>) {
        let _ = self.commands.send(AgentCommand::SetMode(mode_id.into()));
    }

    /// `session/set_config_option` with a select value id (e.g. model).
    pub fn set_config(&self, config_id: impl Into<String>, value: impl Into<String>) {
        let _ = self.commands.send(AgentCommand::SetConfig {
            config_id: config_id.into(),
            value: value.into(),
        });
    }

    /// `session/set_config_option` with a boolean value (e.g. Fast mode).
    pub fn set_config_bool(&self, config_id: impl Into<String>, value: bool) {
        let _ = self.commands.send(AgentCommand::SetConfigBool {
            config_id: config_id.into(),
            value,
        });
    }
}

fn describe_error(err: Error) -> String {
    let data = err
        .data
        .as_ref()
        .and_then(|d| d.as_str().map(str::to_owned).or_else(|| Some(d.to_string())))
        .filter(|d| !d.is_empty() && d != "null");
    match data {
        Some(data) => format!("{}: {data}", err.message),
        None => err.message,
    }
}

fn login_methods(methods: &[AuthMethod]) -> Vec<LoginMethod> {
    methods
        .iter()
        .filter_map(|m| match m {
            AuthMethod::Terminal(t) => Some(LoginMethod {
                name: t.name.clone(),
                description: t.description.clone(),
                args: t.args.clone(),
                env: t.env.clone(),
            }),
            _ => None,
        })
        .collect()
}

async fn run(
    transport: impl ConnectTo<Client> + 'static,
    roots: Vec<PathBuf>,
    claude: bool,
    commands: flume::Receiver<AgentCommand>,
    events: flume::Sender<AgentEvent>,
) -> Result<(), Error> {
    let silent: SilentSlot = Arc::new(Mutex::new(HashMap::new()));
    let on_update = events.clone();
    let on_update_silent = silent.clone();
    let on_permission = events.clone();
    let on_permission_silent = silent.clone();
    let on_read = events.clone();
    let on_write = events.clone();
    Client
        .builder()
        .name("isengard")
        .on_receive_notification(
            async move |n: SessionNotification, cx: ConnectionTo<Agent>| {
                // Throwaway silent session: collect text, abort on tools, never
                // forward into the open-chat UI stream.
                {
                    let mut slot = on_update_silent.lock().unwrap();
                    if let Some(job) = slot.get_mut(&n.session_id)
                    {
                        match &n.update {
                            SessionUpdate::AgentMessageChunk(chunk) => {
                                if let ContentBlock::Text(t) = &chunk.content {
                                    job.text.push_str(&t.text);
                                }
                            }
                            SessionUpdate::ToolCall(_) | SessionUpdate::ToolCallUpdate(_) => {
                                job.aborted = true;
                                let _ = cx.send_notification(CancelNotification::new(
                                    job.session_id.clone(),
                                ));
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                }
                let _ = on_update.send(AgentEvent::Update {
                    session: n.session_id.to_string(),
                    update: n.update,
                });
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, cx: ConnectionTo<Agent>| {
                // Silent jobs must not open tools — cancel immediately.
                {
                    let mut slot = on_permission_silent.lock().unwrap();
                    if let Some(job) = slot.get_mut(&req.session_id)
                    {
                        job.aborted = true;
                        let _ = cx
                            .send_notification(CancelNotification::new(job.session_id.clone()));
                        return responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ));
                    }
                }
                let (reply, answer) = flume::bounded(1);
                let _ = on_permission.send(AgentEvent::Permission(PermissionAsk {
                    tool_call: req.tool_call,
                    options: req.options,
                    reply,
                }));
                // Wait off the dispatch loop so updates keep streaming.
                cx.spawn(async move {
                    let outcome = match answer.recv_async().await {
                        Ok(Some(id)) => {
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id))
                        }
                        _ => RequestPermissionOutcome::Cancelled,
                    };
                    responder.respond(RequestPermissionResponse::new(outcome))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: ReadTextFileRequest, responder, cx: ConnectionTo<Agent>| {
                let (reply, answer) = flume::bounded(1);
                let _ = on_read.send(AgentEvent::ReadFile(FileRead {
                    path: req.path,
                    line: req.line,
                    limit: req.limit,
                    reply,
                }));
                cx.spawn(async move {
                    match answer.recv_async().await {
                        Ok(Ok(content)) => responder.respond(ReadTextFileResponse::new(content)),
                        Ok(Err(msg)) => responder
                            .respond_with_error(Error::resource_not_found(None).data(msg)),
                        Err(_) => responder.respond_with_error(Error::internal_error()),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: WriteTextFileRequest, responder, cx: ConnectionTo<Agent>| {
                let (reply, answer) = flume::bounded(1);
                let _ = on_write.send(AgentEvent::WriteFile(FileWrite {
                    path: req.path,
                    content: req.content,
                    reply,
                }));
                cx.spawn(async move {
                    match answer.recv_async().await {
                        Ok(Ok(())) => responder.respond(WriteTextFileResponse::new()),
                        Ok(Err(msg)) => {
                            responder.respond_with_error(Error::internal_error().data(msg))
                        }
                        Err(_) => responder.respond_with_error(Error::internal_error()),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx: ConnectionTo<Agent>| {
            session(cx, roots, claude, commands, events, silent).await
        })
        .await
}

/// Session `_meta` for the built-in Claude adapter. Claude Code lists the
/// `opusplan` Model alias only when its configured model already is
/// `opusplan` (what terminal `/model opusplan` writes to settings.json), so the
/// OpusPlan Switch would stay hidden. This flag-tier setting makes it listed
/// without touching the user's settings: the adapter still reports and selects
/// the model from their files, so nothing switches until the Switch is turned
/// on.
fn claude_session_meta() -> serde_json::Map<String, serde_json::Value> {
    let serde_json::Value::Object(meta) =
        serde_json::json!({ "claudeCode": { "options": { "settings": { "model": "opusplan" } } } })
    else {
        unreachable!("object literal")
    };
    meta
}

async fn session(
    cx: ConnectionTo<Agent>,
    roots: Vec<PathBuf>,
    claude: bool,
    commands: flume::Receiver<AgentCommand>,
    events: flume::Sender<AgentEvent>,
    silent: SilentSlot,
) -> Result<(), Error> {
    let mut roots = roots.into_iter();
    let cwd = roots.next().unwrap_or_else(std::env::temp_dir);
    let extra_roots: Vec<PathBuf> = roots.collect();
    let init = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(
                    ClientCapabilities::new()
                        .fs(FileSystemCapabilities::new()
                            .read_text_file(true)
                            .write_text_file(true))
                        .auth(AuthCapabilities::new().terminal(true))
                        // So agents can send CompactionUpdate / SummaryChunk when
                        // they auto-compact near the context limit.
                        .session(
                            ClientSessionCapabilities::new()
                                .compaction(CompactionCapabilities::new()),
                        )
                        // Nested Agent/Explore progress as `subagent_update` +
                        // child-session tool traffic (Claude Agent ACP).
                        .subagents(SubagentCapabilities::new()),
                )
                .client_info(
                    Implementation::new("isengard", env!("CARGO_PKG_VERSION")).title("Isengard"),
                ),
        )
        .block_task()
        .await?;
    let logins = login_methods(&init.auth_methods);
    let caps = &init.agent_capabilities;
    let prompt = &caps.prompt_capabilities;
    let info = AgentInfo {
        name: init
            .agent_info
            .clone()
            .map(|info| info.title.unwrap_or(info.name))
            .unwrap_or_else(|| "Agent".into()),
        can_load: caps.load_session,
        can_resume: caps.session_capabilities.resume.is_some(),
        can_delete: caps.session_capabilities.delete.is_some(),
        can_image: prompt.image,
        can_embedded_context: prompt.embedded_context,
    };
    let can_close = caps.session_capabilities.close.is_some();
    // Multi-root workspaces: without the capability the agent only sees `cwd`.
    let extra_roots = if caps.session_capabilities.additional_directories.is_some() {
        extra_roots
    } else {
        Vec::new()
    };
    let _ = events.send(AgentEvent::Connected(info.clone()));

    let session_meta = claude.then(claude_session_meta);
    let mut current: Option<SessionId> = None;
    while let Ok(command) = commands.recv_async().await {
        match command {
            AgentCommand::Open(open) => {
                // One open chat per connection; let the agent free the old one.
                if let Some(old) = current.take()
                    && can_close
                {
                    let _ = cx.send_request(CloseSessionRequest::new(old)).block_task().await;
                }
                let opened = match &open {
                    SessionOpen::New => cx
                        .send_request(
                            NewSessionRequest::new(cwd.clone())
                                .additional_directories(extra_roots.clone())
                                .meta(session_meta.clone()),
                        )
                        .block_task()
                        .await
                        .map(|r| (r.session_id, r.modes, r.config_options)),
                    SessionOpen::Load(id) => cx
                        .send_request(
                            LoadSessionRequest::new(id.clone(), cwd.clone())
                                .additional_directories(extra_roots.clone())
                                .meta(session_meta.clone()),
                        )
                        .block_task()
                        .await
                        .map(|r| (SessionId::new(id.clone()), r.modes, r.config_options)),
                    SessionOpen::Resume(id) => cx
                        .send_request(
                            ResumeSessionRequest::new(id.clone(), cwd.clone())
                                .additional_directories(extra_roots.clone())
                                .meta(session_meta.clone()),
                        )
                        .block_task()
                        .await
                        .map(|r| (SessionId::new(id.clone()), r.modes, r.config_options)),
                };
                match opened {
                    Ok((id, modes, config_options)) => {
                        let _ = events.send(AgentEvent::SessionOpened {
                            id: id.to_string(),
                            modes,
                            config_options,
                        });
                        current = Some(id);
                    }
                    Err(err) if err.code == ErrorCode::AuthRequired => {
                        let _ = events.send(AgentEvent::AuthRequired(logins));
                        return Ok(());
                    }
                    Err(err) => {
                        let _ = events.send(AgentEvent::SessionOpenFailed {
                            open,
                            message: describe_error(err),
                        });
                    }
                }
            }
            AgentCommand::Prompt(blocks) => {
                let Some(session_id) = current.clone() else {
                    let _ = events.send(AgentEvent::TurnFailed("No chat is open.".into()));
                    continue;
                };
                let request = PromptRequest::new(session_id, blocks);
                let turn = cx.send_request(request);
                let events = events.clone();
                let logins = logins.clone();
                // Prompt replies arrive after all of the turn's updates; wait in a
                // task so Cancel can still be sent while it runs.
                cx.spawn(async move {
                    let event = match turn.block_task().await {
                        Ok(reply) => AgentEvent::TurnEnded(reply.stop_reason),
                        Err(err) if err.code == ErrorCode::AuthRequired => {
                            AgentEvent::AuthRequired(logins)
                        }
                        Err(err) => AgentEvent::TurnFailed(describe_error(err)),
                    };
                    let _ = events.send(event);
                    Ok(())
                })?;
            }
            AgentCommand::SilentPrompt {
                job: job_id,
                blocks,
                mode,
                configs,
            } => {
                // Home `current` stays put — scratch session never becomes the UI chat.
                let opened = cx
                    .send_request(
                        NewSessionRequest::new(cwd.clone())
                            .additional_directories(extra_roots.clone())
                            .meta(session_meta.clone()),
                    )
                    .block_task()
                    .await;
                let ephemeral = match opened {
                    Ok(r) => r.session_id,
                    Err(err) if err.code == ErrorCode::AuthRequired => {
                        let _ = events.send(AgentEvent::AuthRequired(logins.clone()));
                        return Ok(());
                    }
                    Err(err) => {
                        let _ = events.send(AgentEvent::SilentTurnEnded {
                            job: job_id,
                            ok: false,
                            text: String::new(),
                        });
                        log::warn!(
                            "agent: silent session/new failed: {}",
                            describe_error(err)
                        );
                        continue;
                    }
                };
                if let Some(mode_id) = mode {
                    let _ = cx
                        .send_request(SetSessionModeRequest::new(
                            ephemeral.clone(),
                            SessionModeId::new(mode_id),
                        ))
                        .block_task()
                        .await;
                }
                for (config_id, value) in configs {
                    let opt_value = match value {
                        SilentConfigValue::Select(v) => SessionConfigOptionValue::value_id(v),
                        SilentConfigValue::Bool(on) => SessionConfigOptionValue::boolean(on),
                    };
                    // Do not emit ConfigOptions — that snapshot is for the home chat.
                    let _ = cx
                        .send_request(SetSessionConfigOptionRequest::new(
                            ephemeral.clone(),
                            SessionConfigId::new(config_id),
                            opt_value,
                        ))
                        .block_task()
                        .await;
                }
                silent.lock().unwrap().insert(
                    ephemeral.clone(),
                    SilentJob {
                    session_id: ephemeral.clone(),
                    text: String::new(),
                    aborted: false,
                    },
                );
                let turn = cx.send_request(PromptRequest::new(ephemeral.clone(), blocks));
                let events = events.clone();
                let logins = logins.clone();
                let silent = silent.clone();
                let can_delete = info.can_delete;
                let cx2 = cx.clone();
                cx.spawn(async move {
                    let result = turn.block_task().await;
                    let job = silent.lock().unwrap().remove(&ephemeral);
                    let text = job.as_ref().map(|j| j.text.clone()).unwrap_or_default();
                    let aborted = job.as_ref().is_some_and(|j| j.aborted);
                    let event = match result {
                        Err(err) if err.code == ErrorCode::AuthRequired => {
                            AgentEvent::AuthRequired(logins)
                        }
                        Ok(reply) => AgentEvent::SilentTurnEnded {
                            job: job_id,
                            // Callers scrub empty text themselves (Improve / title).
                            ok: !aborted && matches!(reply.stop_reason, StopReason::EndTurn),
                            text,
                        },
                        Err(err) => {
                            log::warn!("agent: silent prompt failed: {}", describe_error(err));
                            AgentEvent::SilentTurnEnded {
                                job: job_id,
                                ok: false,
                                text,
                            }
                        }
                    };
                    // Drop the scratch session so it never pollutes chat lists.
                    if can_delete {
                        let _ = cx2
                            .send_request(DeleteSessionRequest::new(ephemeral.to_string()))
                            .block_task()
                            .await;
                    } else if can_close {
                        let _ = cx2
                            .send_request(CloseSessionRequest::new(ephemeral))
                            .block_task()
                            .await;
                    }
                    let _ = events.send(event);
                    Ok(())
                })?;
            }
            AgentCommand::Cancel => {
                // Home chat and throwaway silent can run at once — cancel both.
                let silent_ids: Vec<SessionId> =
                    silent.lock().unwrap().keys().cloned().collect();
                for session_id in silent_ids {
                    cx.send_notification(CancelNotification::new(session_id))?;
                }
                if let Some(session_id) = current.clone() {
                    cx.send_notification(CancelNotification::new(session_id))?;
                }
            }
            AgentCommand::CancelSilent => {
                let silent_ids: Vec<SessionId> =
                    silent.lock().unwrap().keys().cloned().collect();
                for session_id in silent_ids {
                    cx.send_notification(CancelNotification::new(session_id))?;
                }
            }
            AgentCommand::Delete(id) => {
                if info.can_delete {
                    if current.as_ref().is_some_and(|c| c.to_string() == id) {
                        current = None;
                    }
                    if let Err(err) =
                        cx.send_request(DeleteSessionRequest::new(id)).block_task().await
                    {
                        log::warn!("agent: delete session failed: {}", describe_error(err));
                    }
                }
            }
            AgentCommand::SetMode(mode_id) => {
                let Some(session_id) = current.clone() else {
                    continue;
                };
                if let Err(err) = cx
                    .send_request(SetSessionModeRequest::new(
                        session_id,
                        SessionModeId::new(mode_id),
                    ))
                    .block_task()
                    .await
                {
                    log::warn!("agent: set_mode failed: {}", describe_error(err));
                }
            }
            AgentCommand::SetConfig { config_id, value } => {
                let Some(session_id) = current.clone() else {
                    continue;
                };
                match cx
                    .send_request(SetSessionConfigOptionRequest::new(
                        session_id,
                        SessionConfigId::new(config_id),
                        SessionConfigOptionValue::value_id(value),
                    ))
                    .block_task()
                    .await
                {
                    Ok(reply) => {
                        let _ = events.send(AgentEvent::ConfigOptions(reply.config_options));
                    }
                    Err(err) => {
                        log::warn!("agent: set_config_option failed: {}", describe_error(err));
                    }
                }
            }
            AgentCommand::SetConfigBool { config_id, value } => {
                let Some(session_id) = current.clone() else {
                    continue;
                };
                match cx
                    .send_request(SetSessionConfigOptionRequest::new(
                        session_id,
                        SessionConfigId::new(config_id),
                        SessionConfigOptionValue::boolean(value),
                    ))
                    .block_task()
                    .await
                {
                    Ok(reply) => {
                        let _ = events.send(AgentEvent::ConfigOptions(reply.config_options));
                    }
                    Err(err) => {
                        log::warn!("agent: set_config_option failed: {}", describe_error(err));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::testing::FakeAgent;
    use std::time::Duration;

    fn next(conn: &AgentConnection) -> AgentEvent {
        conn.events
            .recv_timeout(Duration::from_secs(10))
            .expect("agent event")
    }

    fn connected(agent: &FakeAgent) -> AgentConnection {
        let conn = AgentConnection::spawn_with(agent.connect(), vec![PathBuf::from("/w")], false);
        match next(&conn) {
            AgentEvent::Connected(info) => {
                assert_eq!(info.name, "Fake Agent");
                assert!(info.can_load && info.can_resume && info.can_delete);
                assert!(info.can_image && info.can_embedded_context);
            }
            _ => panic!("expected Connected"),
        }
        conn
    }

    fn opened(conn: &AgentConnection) -> String {
        match next(conn) {
            AgentEvent::SessionOpened { id, .. } => id,
            _ => panic!("expected SessionOpened"),
        }
    }

    /// `(from_user, text)` of the next message chunk.
    fn chunk_text(conn: &AgentConnection) -> (bool, String) {
        let AgentEvent::Update { update, .. } = next(conn) else {
            panic!("expected an update");
        };
        let (user, chunk) = match update {
            SessionUpdate::UserMessageChunk(c) => (true, c),
            SessionUpdate::AgentMessageChunk(c) => (false, c),
            _ => panic!("expected a message chunk"),
        };
        let ContentBlock::Text(text) = chunk.content else {
            panic!("expected text");
        };
        (user, text.text)
    }

    /// FakeAgent sends `usage_update` before ending the turn.
    fn turn_ended(conn: &AgentConnection) -> StopReason {
        loop {
            match next(conn) {
                AgentEvent::Update {
                    update: SessionUpdate::UsageUpdate(_),
                    ..
                } => continue,
                AgentEvent::TurnEnded(reason) => return reason,
                _ => panic!("expected TurnEnded"),
            }
        }
    }

    #[test]
    fn prompt_roundtrip_with_file_io_and_permission() {
        let agent = FakeAgent::new();
        let conn = connected(&agent);
        conn.open(SessionOpen::New);
        assert_eq!(opened(&conn), "s1");
        conn.prompt(vec![ContentBlock::Text(
            agent_client_protocol::schema::v1::TextContent::new("shout"),
        )]);

        let AgentEvent::ReadFile(read) = next(&conn) else {
            panic!("expected ReadFile");
        };
        assert_eq!(read.path, PathBuf::from("/w/notes.txt"));
        read.reply.send(Ok("hello".into())).unwrap();

        let AgentEvent::Permission(ask) = next(&conn) else {
            panic!("expected Permission");
        };
        assert_eq!(ask.options.len(), 2);
        ask.reply.send(Some(ask.options[0].option_id.clone())).unwrap();

        let AgentEvent::WriteFile(write) = next(&conn) else {
            panic!("expected WriteFile");
        };
        assert_eq!(write.content, "HELLO");
        write.reply.send(Ok(())).unwrap();

        assert_eq!(chunk_text(&conn), (false, "done".into()));
        assert_eq!(turn_ended(&conn), StopReason::EndTurn);
    }

    #[test]
    fn chats_survive_the_connection_and_replay_on_load() {
        let agent = FakeAgent::new();
        let conn = connected(&agent);
        conn.open(SessionOpen::New);
        let id = opened(&conn);
        conn.prompt(vec![ContentBlock::Text(
            agent_client_protocol::schema::v1::TextContent::new("hi"),
        )]);
        assert_eq!(chunk_text(&conn), (false, "echo: hi".into()));
        let _ = turn_ended(&conn);
        drop(conn);

        // A new process loads the stored chat: history replays before it opens.
        let conn = connected(&agent);
        conn.open(SessionOpen::Load(id.clone()));
        assert_eq!(chunk_text(&conn), (true, "hi".into()));
        assert_eq!(chunk_text(&conn), (false, "echo: hi".into()));
        assert_eq!(opened(&conn), id);

        // Switching chats closes the old one; resume does not replay.
        conn.open(SessionOpen::New);
        let other = opened(&conn);
        conn.open(SessionOpen::Resume(id.clone()));
        assert_eq!(opened(&conn), id);
        assert_eq!(
            agent.calls(),
            [
                format!("new {id}"),
                format!("load {id}"),
                format!("close {id}"),
                format!("new {other}"),
                format!("close {other}"),
                format!("resume {id}"),
            ]
        );
    }

    #[test]
    fn loading_a_missing_chat_fails_without_closing_the_connection() {
        let agent = FakeAgent::new();
        let conn = connected(&agent);
        conn.open(SessionOpen::Load("gone".into()));
        match next(&conn) {
            AgentEvent::SessionOpenFailed { open, .. } => {
                assert_eq!(open, SessionOpen::Load("gone".into()))
            }
            _ => panic!("expected SessionOpenFailed"),
        }
        conn.open(SessionOpen::New);
        let id = opened(&conn);
        conn.delete(id.clone());
        conn.open(SessionOpen::New);
        opened(&conn);
        assert!(agent.chat(&id).is_none(), "deleted on the agent side");
    }

    #[test]
    fn other_workspace_roots_go_to_the_agent_as_additional_directories() {
        let agent = FakeAgent::new();
        let roots = vec![PathBuf::from("/w/graph"), PathBuf::from("/w/system")];
        let conn = AgentConnection::spawn_with(agent.connect(), roots, false);
        assert!(matches!(next(&conn), AgentEvent::Connected(_)));
        conn.open(SessionOpen::New);
        let id = opened(&conn);
        assert_eq!(agent.extra_roots(), [PathBuf::from("/w/system")]);
        conn.open(SessionOpen::Load(id));
        opened(&conn);
        assert_eq!(agent.extra_roots(), [PathBuf::from("/w/system")], "load passes them too");
    }

    #[test]
    fn claude_sessions_carry_the_opusplan_setting_and_others_do_not() {
        let want = serde_json::json!({
            "claudeCode": { "options": { "settings": { "model": "opusplan" } } }
        });
        for (claude, expected) in [(true, Some(want)), (false, None)] {
            let agent = FakeAgent::new();
            let conn = AgentConnection::spawn_with(agent.connect(), vec![PathBuf::from("/w")], claude);
            assert!(matches!(next(&conn), AgentEvent::Connected(_)));
            conn.open(SessionOpen::New);
            let id = opened(&conn);
            conn.open(SessionOpen::Load(id.clone()));
            opened(&conn);
            conn.open(SessionOpen::Resume(id));
            opened(&conn);
            assert_eq!(agent.session_metas(), vec![expected; 3], "new, load and resume (claude={claude})");
        }
    }

    #[test]
    fn missing_executable_reports_closed_with_error() {
        let launch = AgentLaunch {
            command: PathBuf::from("/definitely/not/an/agent"),
            args: Vec::new(),
            env: Default::default(),
            claude: false,
        };
        let conn = AgentConnection::spawn(&launch, vec![std::env::temp_dir()]);
        match next(&conn) {
            AgentEvent::Closed(Some(_)) => {}
            _ => panic!("expected Closed(Some(error))"),
        }
    }

    #[test]
    fn silent_prompt_uses_throwaway_session_and_leaves_home_chat_clean() {
        let agent = FakeAgent::new();
        let conn = connected(&agent);
        conn.open(SessionOpen::New);
        let home = opened(&conn);
        conn.prompt(vec![ContentBlock::Text(
            agent_client_protocol::schema::v1::TextContent::new("keep me"),
        )]);
        assert_eq!(chunk_text(&conn), (false, "echo: keep me".into()));
        let _ = turn_ended(&conn);

        conn.silent_prompt(
            7,
            vec![ContentBlock::Text(
                agent_client_protocol::schema::v1::TextContent::new("silent secret"),
            )],
            None,
            vec![],
        );
        match next(&conn) {
            AgentEvent::SilentTurnEnded { job, ok, text } => {
                assert_eq!(job, 7);
                assert!(ok);
                assert_eq!(text, "echo: silent secret");
            }
            _ => panic!("expected SilentTurnEnded"),
        }
        let home_chat = agent.chat(&home).expect("home chat");
        assert_eq!(
            home_chat,
            vec![
                (true, "keep me".into()),
                (false, "echo: keep me".into()),
            ]
        );
        // Scratch session was deleted (FakeAgent removes it).
        assert!(
            agent.calls().iter().any(|c| c.starts_with("delete ")),
            "silent scratch should be deleted: {:?}",
            agent.calls()
        );
        assert_eq!(
            agent.calls().iter().filter(|c| c.starts_with("new ")).count(),
            2,
            "home + throwaway: {:?}",
            agent.calls()
        );
        // Follow-up on home still works and stays isolated.
        conn.prompt(vec![ContentBlock::Text(
            agent_client_protocol::schema::v1::TextContent::new("again"),
        )]);
        assert_eq!(chunk_text(&conn), (false, "echo: again".into()));
        let _ = turn_ended(&conn);
        let home_chat = agent.chat(&home).expect("home chat");
        assert!(!home_chat.iter().any(|(_, t)| t.contains("silent secret")));
    }

    #[test]
    fn overlapping_silent_prompts_report_their_own_job() {
        let agent = FakeAgent::new();
        let conn = connected(&agent);
        conn.open(SessionOpen::New);
        let _ = opened(&conn);
        for (job, text) in [(1, "alpha"), (2, "beta")] {
            conn.silent_prompt(
                job,
                vec![ContentBlock::Text(
                    agent_client_protocol::schema::v1::TextContent::new(text),
                )],
                None,
                vec![],
            );
        }
        let mut got = Vec::new();
        for _ in 0..2 {
            match next(&conn) {
                AgentEvent::SilentTurnEnded { job, ok, text } => {
                    assert!(ok);
                    got.push((job, text));
                }
                _ => panic!("expected SilentTurnEnded"),
            }
        }
        got.sort();
        assert_eq!(
            got,
            vec![(1, "echo: alpha".to_string()), (2, "echo: beta".to_string())]
        );
    }
}
