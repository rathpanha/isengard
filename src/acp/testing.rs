//! In-memory fake ACP agent for tests (no process, no network).
//!
//! Chats live in shared state, so they outlive a connection the way the
//! Claude adapter's transcripts outlive its process. Prompt `"shout"` runs a
//! file round trip (read `/w/notes.txt`, ask permission, write it upper-cased);
//! `"hold"` waits until [`FakeAgent::release_hold`] (for queue tests); any
//! other prompt replies `"echo: <text>"`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock, ContentChunk,
    DeleteSessionRequest, DeleteSessionResponse, Implementation, InitializeRequest,
    InitializeResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOptionKind, PromptCapabilities, PromptRequest,
    PromptResponse, ReadTextFileRequest, RequestPermissionOutcome, RequestPermissionRequest,
    ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities, SessionCloseCapabilities,
    SessionAdditionalDirectoriesCapabilities, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOption, SessionDeleteCapabilities, SessionId, SessionMode,
    SessionModeState, SessionNotification, SessionResumeCapabilities, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, StopReason, TextContent, ToolCallUpdate, ToolCallUpdateFields,
    UsageUpdate, WriteTextFileRequest,
};
use agent_client_protocol::{Agent, Channel, Client, ConnectionTo, Error};

use crate::acp::AgentConnection;

thread_local! {
    static INSTALLED: RefCell<Option<FakeAgent>> = const { RefCell::new(None) };
}

/// Make every Agent panel on this (test) thread connect to `agent` instead
/// of launching a process.
pub fn install(agent: &FakeAgent) {
    INSTALLED.with(|slot| *slot.borrow_mut() = Some(agent.clone()));
}

/// A connection to the installed fake agent, if any.
pub fn connection(roots: Vec<PathBuf>) -> Option<AgentConnection> {
    INSTALLED.with(|slot| {
        let agent = slot.borrow().clone()?;
        Some(AgentConnection::spawn_with(agent.connect(), roots))
    })
}

/// One message in a fake chat: `(from_user, text)`.
pub type FakeMessage = (bool, String);

struct State {
    next_id: u32,
    chats: HashMap<String, Vec<FakeMessage>>,
    calls: Vec<String>,
    /// `additionalDirectories` of the last new/load/resume.
    extra_roots: Vec<PathBuf>,
    mode_id: String,
    model_id: String,
    fast_mode: bool,
    /// When set, a `"hold"` prompt blocks until the sender fires.
    hold_rx: Option<flume::Receiver<()>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            next_id: 0,
            chats: HashMap::new(),
            calls: Vec::new(),
            extra_roots: Vec::new(),
            mode_id: "auto".into(),
            model_id: "sonnet-4-5".into(),
            fast_mode: false,
            hold_rx: None,
        }
    }
}

fn session_modes(mode_id: &str) -> SessionModeState {
    SessionModeState::new(
        mode_id.to_owned(),
        vec![
            SessionMode::new("ask", "Ask").description("Ask before changes"),
            SessionMode::new("code", "Code").description("Edit freely"),
            SessionMode::new("auto", "Auto").description("Auto permissions"),
        ],
    )
}

fn session_config(mode_id: &str, model_id: &str, fast_mode: bool) -> Vec<SessionConfigOption> {
    vec![
        SessionConfigOption::select(
            "mode",
            "Mode",
            mode_id.to_owned(),
            vec![
                SessionConfigSelectOption::new("ask", "Ask"),
                SessionConfigSelectOption::new("code", "Code"),
                SessionConfigSelectOption::new("auto", "Auto"),
                SessionConfigSelectOption::new("plan", "Plan"),
            ],
        )
        .category(SessionConfigOptionCategory::Mode),
        // Family + version + context (1M) so the UI can split Sonnet → 4.5/4.6
        // and Context → 200K/1M when both exist for the selected version.
        // Haiku 4.5: Claude Auto mode unsupported (<4.6) — UI filters it out.
        SessionConfigOption::select(
            "model",
            "Model",
            model_id.to_owned(),
            vec![
                SessionConfigSelectOption::new("sonnet-4-5", "Sonnet 4.5"),
                SessionConfigSelectOption::new("sonnet-4-6", "Sonnet 4.6"),
                SessionConfigSelectOption::new("sonnet-4-6[1m]", "Sonnet 4.6 1M"),
                SessionConfigSelectOption::new("haiku-4-5", "Haiku 4.5"),
                SessionConfigSelectOption::new("opus-5", "Opus 5"),
                SessionConfigSelectOption::new("opus-5[1m]", "Opus 5 1M"),
                // Claude alias: Opus in Plan, Sonnet otherwise — composer OpusPlan Switch.
                SessionConfigSelectOption::new("opusplan", "Opus Plan Mode"),
                SessionConfigSelectOption::new("opusplan[1m]", "Opus Plan Mode 1M"),
            ],
        )
        .category(SessionConfigOptionCategory::Model),
        // Fast mode — UI renders as a Switch labeled "Fast", not On/Off text.
        SessionConfigOption::select(
            "fast_mode",
            "Fast mode",
            if fast_mode { "on" } else { "off" },
            vec![
                SessionConfigSelectOption::new("on", "On"),
                SessionConfigSelectOption::new("off", "Off"),
            ],
        )
        .category(SessionConfigOptionCategory::ModelConfig),
    ]
}

#[derive(Clone, Default)]
pub struct FakeAgent {
    state: Arc<Mutex<State>>,
}

fn chunk(session: &SessionId, user: bool, text: &str) -> SessionNotification {
    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
    SessionNotification::new(
        session.clone(),
        if user {
            SessionUpdate::UserMessageChunk(chunk)
        } else {
            SessionUpdate::AgentMessageChunk(chunk)
        },
    )
}

impl FakeAgent {
    pub fn new() -> Self {
        Self::default()
    }

    /// Method log, e.g. `["new s1", "close s1", "load s1"]`.
    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }

    pub fn extra_roots(&self) -> Vec<PathBuf> {
        self.state.lock().unwrap().extra_roots.clone()
    }

    pub fn chat(&self, id: &str) -> Option<Vec<FakeMessage>> {
        self.state.lock().unwrap().chats.get(id).cloned()
    }

    /// Next `"hold"` prompt waits until [`Self::release_hold`].
    pub fn arm_hold(&self) -> flume::Sender<()> {
        let (tx, rx) = flume::bounded(1);
        self.state.lock().unwrap().hold_rx = Some(rx);
        tx
    }

    /// Unblock a waiting `"hold"` prompt (no-op if none armed / already released).
    pub fn release_hold(&self, tx: flume::Sender<()>) {
        let _ = tx.send(());
    }

    /// Serve one connection on a background thread; returns the client end.
    pub fn connect(&self) -> Channel {
        let (client_end, agent_end) = Channel::duplex();
        // Seed default mode/model once (empty strings mean "not yet").
        {
            let mut st = self.state.lock().unwrap();
            if st.mode_id.is_empty() {
                st.mode_id = "auto".into();
            }
            if st.model_id.is_empty() {
                st.model_id = "sonnet-4-5".into();
            }
        }
        let s_cancel = self.state.clone();
        let (s_new, s_load, s_resume, s_close, s_delete, s_prompt, s_mode, s_cfg) = (
            self.state.clone(),
            self.state.clone(),
            self.state.clone(),
            self.state.clone(),
            self.state.clone(),
            self.state.clone(),
            self.state.clone(),
            self.state.clone(),
        );
        std::thread::spawn(move || {
            let _ = futures::executor::block_on(
                Agent
                    .builder()
                    .on_receive_request(
                        async |req: InitializeRequest, responder, _cx| {
                            responder.respond(
                                InitializeResponse::new(req.protocol_version)
                                    .agent_capabilities(
                                        AgentCapabilities::new()
                                            .load_session(true)
                                            .prompt_capabilities(
                                                PromptCapabilities::new()
                                                    .image(true)
                                                    .embedded_context(true),
                                            )
                                            .session_capabilities(
                                            SessionCapabilities::new()
                                                .resume(SessionResumeCapabilities::new())
                                                .close(SessionCloseCapabilities::new())
                                                .delete(SessionDeleteCapabilities::new())
                                                .additional_directories(
                                                    SessionAdditionalDirectoriesCapabilities::new(),
                                                ),
                                        ),
                                    )
                                    .agent_info(Implementation::new("fake", "0").title("Fake Agent")),
                            )
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: NewSessionRequest, responder, _cx| {
                            let mut st = s_new.lock().unwrap();
                            st.extra_roots = req.additional_directories;
                            st.next_id += 1;
                            let id = format!("s{}", st.next_id);
                            st.chats.insert(id.clone(), Vec::new());
                            st.calls.push(format!("new {id}"));
                            let modes = session_modes(&st.mode_id);
                            let config =
                                session_config(&st.mode_id, &st.model_id, st.fast_mode);
                            responder.respond(
                                NewSessionResponse::new(id)
                                    .modes(modes)
                                    .config_options(config),
                            )
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: LoadSessionRequest, responder, cx: ConnectionTo<Client>| {
                            let id = req.session_id.to_string();
                            let (history, modes, config) = {
                                let mut st = s_load.lock().unwrap();
                                st.extra_roots = req.additional_directories.clone();
                                st.calls.push(format!("load {id}"));
                                (
                                    st.chats.get(&id).cloned(),
                                    session_modes(&st.mode_id),
                                    session_config(&st.mode_id, &st.model_id, st.fast_mode),
                                )
                            };
                            let Some(history) = history else {
                                return responder.respond_with_error(Error::resource_not_found(None));
                            };
                            for (user, text) in &history {
                                cx.send_notification(chunk(&req.session_id, *user, text))?;
                            }
                            responder.respond(
                                LoadSessionResponse::new()
                                    .modes(modes)
                                    .config_options(config),
                            )
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: ResumeSessionRequest, responder, _cx| {
                            let id = req.session_id.to_string();
                            let mut st = s_resume.lock().unwrap();
                            st.extra_roots = req.additional_directories;
                            st.calls.push(format!("resume {id}"));
                            if st.chats.contains_key(&id) {
                                let modes = session_modes(&st.mode_id);
                                let config =
                                    session_config(&st.mode_id, &st.model_id, st.fast_mode);
                                responder.respond(
                                    ResumeSessionResponse::new()
                                        .modes(modes)
                                        .config_options(config),
                                )
                            } else {
                                responder.respond_with_error(Error::resource_not_found(None))
                            }
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: CloseSessionRequest, responder, _cx| {
                            s_close.lock().unwrap().calls.push(format!("close {}", req.session_id));
                            responder.respond(CloseSessionResponse::new())
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: DeleteSessionRequest, responder, _cx| {
                            let id = req.session_id.to_string();
                            let mut st = s_delete.lock().unwrap();
                            st.calls.push(format!("delete {id}"));
                            st.chats.remove(&id);
                            responder.respond(DeleteSessionResponse::new())
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: PromptRequest, responder, cx: ConnectionTo<Client>| {
                            let state = s_prompt.clone();
                            let cx2 = cx.clone();
                            cx.spawn(async move {
                                let sid = req.session_id;
                                let text = req
                                    .prompt
                                    .iter()
                                    .rev()
                                    .find_map(|b| match b {
                                        ContentBlock::Text(t) => Some(t.text.clone()),
                                        _ => None,
                                    })
                                    .unwrap_or_default();
                                let reply = if text == "shout" {
                                    shout(&cx2, &sid).await?;
                                    "done".to_string()
                                } else if text == "hold" {
                                    // Block until the test releases — keeps the turn open
                                    // so a follow-up can be queued.
                                    let gate = state.lock().unwrap().hold_rx.take();
                                    if let Some(rx) = gate {
                                        let _ = rx.recv();
                                    }
                                    format!("echo: {text}")
                                } else if text.starts_with("Rewrite this user message") {
                                    // Improve prompt: optional hold for cancel-mid-flight tests.
                                    state.lock().unwrap().calls.push(format!("improve {sid}"));
                                    let gate = state.lock().unwrap().hold_rx.take();
                                    if let Some(rx) = gate {
                                        let _ = rx.recv();
                                    }
                                    "Clearer coding-agent prompt.".to_string()
                                } else if text.starts_with("Write a short chat title") {
                                    "Generated Title".to_string()
                                } else {
                                    format!("echo: {text}")
                                };
                                if let Some(chat) =
                                    state.lock().unwrap().chats.get_mut(&sid.to_string())
                                {
                                    chat.push((true, text));
                                    chat.push((false, reply.clone()));
                                }
                                cx2.send_notification(chunk(&sid, false, &reply))?;
                                // Fake context growth so UI tests can assert usage_update.
                                let used = {
                                    let n = state
                                        .lock()
                                        .unwrap()
                                        .chats
                                        .get(&sid.to_string())
                                        .map(|c| c.len())
                                        .unwrap_or(0)
                                        as u64;
                                    10_000 + n.saturating_mul(5_000)
                                };
                                cx2.send_notification(SessionNotification::new(
                                    sid.clone(),
                                    SessionUpdate::UsageUpdate(UsageUpdate::new(used, 200_000)),
                                ))?;
                                responder.respond(PromptResponse::new(StopReason::EndTurn))
                            })
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_notification(
                        async move |n: CancelNotification, _cx: ConnectionTo<Client>| {
                            s_cancel
                                .lock()
                                .unwrap()
                                .calls
                                .push(format!("cancel {}", n.session_id));
                            Ok(())
                        },
                        agent_client_protocol::on_receive_notification!(),
                    )
                    .on_receive_request(
                        async move |req: SetSessionModeRequest, responder, _cx| {
                            let id = req.mode_id.to_string();
                            let mut st = s_mode.lock().unwrap();
                            st.mode_id = id.clone();
                            st.calls.push(format!("set_mode {id}"));
                            responder.respond(SetSessionModeResponse::new())
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                            let config_id = req.config_id.to_string();
                            let mut st = s_cfg.lock().unwrap();
                            if let Some(on) = req.value.as_bool() {
                                st.calls.push(format!("set_config {config_id}={on}"));
                                if config_id == "fast_mode" {
                                    st.fast_mode = on;
                                }
                            } else {
                                let value = req
                                    .value
                                    .as_value_id()
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                                st.calls.push(format!("set_config {config_id}={value}"));
                                match config_id.as_str() {
                                    "mode" => st.mode_id = value,
                                    "model" => st.model_id = value,
                                    "fast_mode" => st.fast_mode = value == "on",
                                    _ => {}
                                }
                            }
                            let options =
                                session_config(&st.mode_id, &st.model_id, st.fast_mode);
                            responder.respond(SetSessionConfigOptionResponse::new(options))
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .connect_to(agent_end),
            );
        });
        client_end
    }
}

async fn shout(cx: &ConnectionTo<Client>, sid: &SessionId) -> Result<(), Error> {
    let text = cx
        .send_request(ReadTextFileRequest::new(sid.clone(), "/w/notes.txt"))
        .block_task()
        .await?
        .content;
    let ask = cx
        .send_request(RequestPermissionRequest::new(
            sid.clone(),
            ToolCallUpdate::new("t1", ToolCallUpdateFields::new().title("Edit notes.txt")),
            vec![
                PermissionOption::new("allow", "Allow", PermissionOptionKind::AllowOnce),
                PermissionOption::new("reject", "Reject", PermissionOptionKind::RejectOnce),
            ],
        ))
        .block_task()
        .await?;
    if matches!(ask.outcome, RequestPermissionOutcome::Selected(_)) {
        cx.send_request(WriteTextFileRequest::new(
            sid.clone(),
            "/w/notes.txt",
            text.to_uppercase(),
        ))
        .block_task()
        .await?;
    }
    Ok(())
}
