//! In-memory fake ACP agent for tests (no process, no network).
//!
//! Chats live in shared state, so they outlive a connection the way the
//! Claude adapter's transcripts outlive its process. Prompt `"shout"` runs a
//! file round trip (read `/w/notes.txt`, ask permission, write it upper-cased);
//! any other prompt replies `"echo: <text>"`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CloseSessionRequest, CloseSessionResponse, ContentBlock, ContentChunk,
    CreateTerminalRequest, DeleteSessionRequest, DeleteSessionResponse, Implementation,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse,
    NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest,
    PromptResponse, ReadTextFileRequest, ReleaseTerminalRequest, RequestPermissionOutcome,
    RequestPermissionRequest, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities,
    SessionCloseCapabilities, SessionAdditionalDirectoriesCapabilities, SessionDeleteCapabilities,
    SessionId, SessionNotification, SessionResumeCapabilities, SessionUpdate, StopReason,
    TextContent, TerminalOutputRequest, ToolCallUpdate, ToolCallUpdateFields,
    WaitForTerminalExitRequest, WriteTextFileRequest,
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

#[derive(Default)]
struct State {
    next_id: u32,
    chats: HashMap<String, Vec<FakeMessage>>,
    calls: Vec<String>,
    /// `additionalDirectories` of the last new/load/resume.
    extra_roots: Vec<PathBuf>,
    /// Whether the client advertised `clientCapabilities.terminal`.
    client_terminal: bool,
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

    pub fn client_terminal(&self) -> bool {
        self.state.lock().unwrap().client_terminal
    }

    pub fn chat(&self, id: &str) -> Option<Vec<FakeMessage>> {
        self.state.lock().unwrap().chats.get(id).cloned()
    }

    /// Serve one connection on a background thread; returns the client end.
    pub fn connect(&self) -> Channel {
        let (client_end, agent_end) = Channel::duplex();
        let (s_init, s_new, s_load, s_resume, s_close, s_delete, s_prompt) = (
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
                        async move |req: InitializeRequest, responder, _cx| {
                            s_init.lock().unwrap().client_terminal = req.client_capabilities.terminal;
                            responder.respond(
                                InitializeResponse::new(req.protocol_version)
                                    .agent_capabilities(
                                        AgentCapabilities::new().load_session(true).session_capabilities(
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
                            responder.respond(NewSessionResponse::new(id))
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async move |req: LoadSessionRequest, responder, cx: ConnectionTo<Client>| {
                            let id = req.session_id.to_string();
                            let history = {
                                let mut st = s_load.lock().unwrap();
                                st.extra_roots = req.additional_directories.clone();
                                st.calls.push(format!("load {id}"));
                                st.chats.get(&id).cloned()
                            };
                            let Some(history) = history else {
                                return responder.respond_with_error(Error::resource_not_found(None));
                            };
                            for (user, text) in &history {
                                cx.send_notification(chunk(&req.session_id, *user, text))?;
                            }
                            responder.respond(LoadSessionResponse::new())
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
                                responder.respond(ResumeSessionResponse::new())
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
                                let text = match req.prompt.first() {
                                    Some(ContentBlock::Text(t)) => t.text.clone(),
                                    _ => String::new(),
                                };
                                let reply = if text == "shout" {
                                    shout(&cx2, &sid).await?;
                                    "done".to_string()
                                } else if text == "term" {
                                    run_terminal(&cx2, &sid).await?;
                                    "term-ok".to_string()
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
                                responder.respond(PromptResponse::new(StopReason::EndTurn))
                            })
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .connect_to(agent_end),
            );
        });
        client_end
    }
}

async fn run_terminal(cx: &ConnectionTo<Client>, sid: &SessionId) -> Result<(), Error> {
    let created = cx
        .send_request(CreateTerminalRequest::new(sid.clone(), "true"))
        .block_task()
        .await?;
    let tid = created.terminal_id;
    let _ = cx
        .send_request(TerminalOutputRequest::new(sid.clone(), tid.clone()))
        .block_task()
        .await?;
    let _ = cx
        .send_request(WaitForTerminalExitRequest::new(sid.clone(), tid.clone()))
        .block_task()
        .await?;
    cx.send_request(ReleaseTerminalRequest::new(sid.clone(), tid))
        .block_task()
        .await?;
    Ok(())
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
