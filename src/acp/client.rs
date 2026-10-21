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

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AuthCapabilities, AuthMethod, CancelNotification, ClientCapabilities, ContentBlock,
    FileSystemCapabilities, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOption, PermissionOptionId, PromptRequest, ReadTextFileRequest,
    ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    StopReason, TextContent, ToolCallUpdate, WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectTo, ConnectionTo, Error, ErrorCode,
};

use crate::acp::registry::AgentLaunch;

/// UI → agent.
enum AgentCommand {
    Prompt(String),
    Cancel,
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
    /// Session created; prompts are accepted from now on.
    Ready { agent_name: String },
    Update(SessionUpdate),
    TurnEnded(StopReason),
    TurnFailed(String),
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
    /// Spawn the agent process for `launch` with session cwd `cwd`.
    pub fn spawn(launch: &AgentLaunch, cwd: PathBuf) -> Self {
        let config = AcpAgentConfig::new(launch.command.clone())
            .args(launch.args.iter().cloned())
            .envs(launch.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        Self::spawn_with(AcpAgent::new(config), cwd)
    }

    /// Run against any transport (tests use an in-memory fake agent).
    pub fn spawn_with(transport: impl ConnectTo<Client> + 'static, cwd: PathBuf) -> Self {
        let (commands, command_rx) = flume::unbounded();
        let (event_tx, events) = flume::unbounded();
        let spawned = std::thread::Builder::new()
            .name("acp-agent".into())
            .spawn({
                let event_tx = event_tx.clone();
                move || {
                    let result =
                        futures::executor::block_on(run(transport, cwd, command_rx, event_tx.clone()));
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

    pub fn prompt(&self, text: String) {
        let _ = self.commands.send(AgentCommand::Prompt(text));
    }

    pub fn cancel(&self) {
        let _ = self.commands.send(AgentCommand::Cancel);
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
    cwd: PathBuf,
    commands: flume::Receiver<AgentCommand>,
    events: flume::Sender<AgentEvent>,
) -> Result<(), Error> {
    let on_update = events.clone();
    let on_permission = events.clone();
    let on_read = events.clone();
    let on_write = events.clone();
    Client
        .builder()
        .name("isengard")
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                let _ = on_update.send(AgentEvent::Update(n.update));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, cx: ConnectionTo<Agent>| {
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
            session(cx, cwd, commands, events).await
        })
        .await
}

async fn session(
    cx: ConnectionTo<Agent>,
    cwd: PathBuf,
    commands: flume::Receiver<AgentCommand>,
    events: flume::Sender<AgentEvent>,
) -> Result<(), Error> {
    let init = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(
                    ClientCapabilities::new()
                        .fs(FileSystemCapabilities::new()
                            .read_text_file(true)
                            .write_text_file(true))
                        .auth(AuthCapabilities::new().terminal(true)),
                )
                .client_info(
                    Implementation::new("isengard", env!("CARGO_PKG_VERSION")).title("Isengard"),
                ),
        )
        .block_task()
        .await?;
    let logins = login_methods(&init.auth_methods);
    let agent_name = init
        .agent_info
        .map(|info| info.title.unwrap_or(info.name))
        .unwrap_or_else(|| "Agent".into());

    let session = match cx.send_request(NewSessionRequest::new(cwd)).block_task().await {
        Ok(session) => session,
        Err(err) if err.code == ErrorCode::AuthRequired => {
            let _ = events.send(AgentEvent::AuthRequired(logins));
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    let session_id = session.session_id;
    let _ = events.send(AgentEvent::Ready { agent_name });

    while let Ok(command) = commands.recv_async().await {
        match command {
            AgentCommand::Prompt(text) => {
                let request = PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(text))],
                );
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
            AgentCommand::Cancel => {
                cx.send_notification(CancelNotification::new(session_id.clone()))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, ContentChunk, InitializeResponse, NewSessionResponse,
        PermissionOptionKind, PromptResponse, ToolCallUpdateFields,
    };
    use agent_client_protocol::Channel;
    use std::time::Duration;

    /// Minimal agent: on prompt it reads `notes.txt`, asks permission, writes
    /// it back upper-cased (if allowed), streams one chunk, and ends the turn.
    fn fake_agent(channel: Channel) {
        std::thread::spawn(move || {
            let _ = futures::executor::block_on(
                Agent
                    .builder()
                    .on_receive_request(
                        async |req: InitializeRequest, responder, _cx| {
                            responder.respond(
                                InitializeResponse::new(req.protocol_version)
                                    .agent_capabilities(AgentCapabilities::new())
                                    .agent_info(
                                        Implementation::new("fake", "0").title("Fake Agent"),
                                    ),
                            )
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async |_req: NewSessionRequest, responder, _cx| {
                            responder.respond(NewSessionResponse::new("s1"))
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .on_receive_request(
                        async |req: PromptRequest, responder, cx: ConnectionTo<Client>| {
                            let cx2 = cx.clone();
                            cx.spawn(async move {
                                let sid = req.session_id;
                                let text = cx2
                                    .send_request(ReadTextFileRequest::new(
                                        sid.clone(),
                                        "/w/notes.txt",
                                    ))
                                    .block_task()
                                    .await?
                                    .content;
                                let ask = cx2
                                    .send_request(RequestPermissionRequest::new(
                                        sid.clone(),
                                        ToolCallUpdate::new(
                                            "t1",
                                            ToolCallUpdateFields::new().title("Edit notes.txt"),
                                        ),
                                        vec![
                                            PermissionOption::new(
                                                "allow",
                                                "Allow",
                                                PermissionOptionKind::AllowOnce,
                                            ),
                                            PermissionOption::new(
                                                "reject",
                                                "Reject",
                                                PermissionOptionKind::RejectOnce,
                                            ),
                                        ],
                                    ))
                                    .block_task()
                                    .await?;
                                if matches!(ask.outcome, RequestPermissionOutcome::Selected(_)) {
                                    cx2.send_request(WriteTextFileRequest::new(
                                        sid.clone(),
                                        "/w/notes.txt",
                                        text.to_uppercase(),
                                    ))
                                    .block_task()
                                    .await?;
                                }
                                cx2.send_notification(SessionNotification::new(
                                    sid,
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::Text(TextContent::new("done")),
                                    )),
                                ))?;
                                responder.respond(PromptResponse::new(StopReason::EndTurn))
                            })
                        },
                        agent_client_protocol::on_receive_request!(),
                    )
                    .connect_to(channel),
            );
        });
    }

    fn next(conn: &AgentConnection) -> AgentEvent {
        conn.events
            .recv_timeout(Duration::from_secs(10))
            .expect("agent event")
    }

    #[test]
    fn prompt_roundtrip_with_file_io_and_permission() {
        let (client_end, agent_end) = Channel::duplex();
        fake_agent(agent_end);
        let conn = AgentConnection::spawn_with(client_end, PathBuf::from("/w"));

        match next(&conn) {
            AgentEvent::Ready { agent_name } => assert_eq!(agent_name, "Fake Agent"),
            _ => panic!("expected Ready"),
        }
        conn.prompt("shout".into());

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

        let AgentEvent::Update(SessionUpdate::AgentMessageChunk(chunk)) = next(&conn) else {
            panic!("expected a message chunk");
        };
        let ContentBlock::Text(text) = chunk.content else {
            panic!("expected text");
        };
        assert_eq!(text.text, "done");
        assert!(matches!(next(&conn), AgentEvent::TurnEnded(StopReason::EndTurn)));
    }

    #[test]
    fn missing_executable_reports_closed_with_error() {
        let launch = AgentLaunch {
            command: PathBuf::from("/definitely/not/an/agent"),
            args: Vec::new(),
            env: Default::default(),
        };
        let conn = AgentConnection::spawn(&launch, std::env::temp_dir());
        match next(&conn) {
            AgentEvent::Closed(Some(_)) => {}
            _ => panic!("expected Closed(Some(error))"),
        }
    }
}
