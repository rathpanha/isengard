//! Right-hand Agent panel: transcript + prompt box for one ACP agent session.
//!
//! The panel owns the [`AgentConnection`] and the transcript. Anything that
//! touches editor state (file reads/writes, running a login in the terminal,
//! hiding the panel) is emitted as an [`AgentPanelEvent`] for `IsengardApp`.

use std::path::PathBuf;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    message::{Message, MessageAlignment, MessageContent},
    message_scroller::{MessageScroller, MessageScrollerState},
    spinner::Spinner,
    text::{TextView, TextViewState},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::acp::schema::{
    ContentBlock, PermissionOption, PermissionOptionId, PermissionOptionKind, PlanEntry,
    PlanEntryStatus, SessionUpdate, StopReason, ToolCall, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolKind,
};
use crate::acp::{
    self, AgentConnection, AgentEvent, AgentLaunch, CLAUDE_AGENT_NAME, FileRead, FileWrite,
    LoginMethod, PermissionAsk, Resolved,
};
use crate::config::AgentSettings;
use crate::terminal::pty::TerminalCommand;
use crate::theme;

/// Side effects the app performs for the panel.
pub enum AgentPanelEvent {
    ReadFile(FileRead),
    WriteFile(FileWrite),
    /// Run the agent's login command in a terminal tab.
    Login { title: String, command: TerminalCommand },
    Hide,
}

#[derive(Clone)]
enum Status {
    /// No connection yet (or no folder open).
    Idle,
    NotInstalled,
    Installing,
    Starting,
    Ready,
    LoginRequired(Vec<LoginMethod>),
    Failed(SharedString),
}

struct PendingPermission {
    options: Vec<PermissionOption>,
    reply: flume::Sender<Option<PermissionOptionId>>,
}

struct ToolEntry {
    id: ToolCallId,
    title: SharedString,
    kind: ToolKind,
    status: ToolCallStatus,
    location: Option<PathBuf>,
    permission: Option<PendingPermission>,
}

enum Entry {
    User(SharedString),
    Agent(Entity<TextViewState>),
    Thought(Entity<TextViewState>),
    Tool(ToolEntry),
    Plan(Vec<PlanEntry>),
    Notice { text: SharedString, error: bool },
}

pub struct AgentPanel {
    settings: AgentSettings,
    cwd: Option<PathBuf>,
    status: Status,
    agent_name: SharedString,
    launch: Option<AgentLaunch>,
    connection: Option<AgentConnection>,
    /// Bumped per connection so events from a dropped one are ignored.
    generation: u64,
    turn_running: bool,
    entries: Vec<Entry>,
    scroller: Entity<MessageScrollerState>,
    input: Entity<TextareaState>,
    install_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AgentPanelEvent> for AgentPanel {}

impl AgentPanel {
    pub fn new(settings: AgentSettings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Ask the agent…")
                .auto_grow(2, 10)
                .submit_on_enter(true)
        });
        let enter = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = event {
                this.send(window, cx);
            }
        });
        Self {
            settings,
            cwd: None,
            status: Status::Idle,
            agent_name: default_name(&AgentSettings::default()),
            launch: None,
            connection: None,
            generation: 0,
            turn_running: false,
            entries: Vec::new(),
            scroller,
            input,
            install_error: None,
            _subscriptions: vec![enter],
        }
    }

    /// New workspace (or none): drop the agent and transcript. The next
    /// [`Self::ensure_started`] connects in `cwd`.
    pub fn reset(&mut self, cwd: Option<PathBuf>, settings: AgentSettings, cx: &mut Context<Self>) {
        self.cwd = cwd;
        self.agent_name = default_name(&settings);
        self.settings = settings;
        self.disconnect(cx);
        self.status = Status::Idle;
        self.clear_entries(cx);
        cx.notify();
    }

    /// Connect if nothing is running yet (first show for this workspace).
    pub fn ensure_started(&mut self, cx: &mut Context<Self>) {
        if matches!(self.status, Status::Idle) {
            self.start(cx);
        }
    }

    /// Prompt box (the app focuses it when showing the panel).
    pub fn input(&self) -> &Entity<TextareaState> {
        &self.input
    }

    fn focus_input(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.disconnect(cx);
        let Some(cwd) = self.cwd.clone() else {
            self.status = Status::Idle;
            cx.notify();
            return;
        };
        let launch = match acp::resolve(&self.settings) {
            Resolved::Launch(launch) => launch,
            Resolved::ClaudeNotInstalled => {
                self.status = Status::NotInstalled;
                cx.notify();
                return;
            }
        };
        let connection = AgentConnection::spawn(&launch, cwd);
        let events = connection.events.clone();
        self.launch = Some(launch);
        self.connection = Some(connection);
        self.status = Status::Starting;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv_async().await {
                let alive = this
                    .update(cx, |this, cx| {
                        if this.generation == generation {
                            this.handle_event(event, cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Drop the connection (kills the agent) and cancel open prompts.
    fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.connection = None;
        self.generation = self.generation.wrapping_add(1);
        self.turn_running = false;
        self.cancel_pending_permissions(cx);
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.clear_entries(cx);
        self.start(cx);
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        self.status = Status::Installing;
        self.install_error = None;
        cx.notify();
        let task = cx.background_spawn(async move { acp::install_claude() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.start(cx),
                Err(err) => {
                    this.status = Status::NotInstalled;
                    this.install_error = Some(err.into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn login(&mut self, method: &LoginMethod, cx: &mut Context<Self>) {
        let Some(launch) = &self.launch else {
            return;
        };
        let mut env: Vec<(String, String)> = launch
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.extend(method.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        let mut args = launch.args.clone();
        args.extend(method.args.iter().cloned());
        cx.emit(AgentPanelEvent::Login {
            title: method.name.clone(),
            command: TerminalCommand {
                program: launch.command.clone(),
                args,
                env,
            },
        });
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.status, Status::Ready) || self.turn_running {
            return;
        }
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(connection) = &self.connection else {
            return;
        };
        connection.prompt(text.clone());
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.turn_running = true;
        self.push_entry(Entry::User(text.into()), cx);
        // Show the reply even if the user had scrolled up.
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(connection) = &self.connection {
            connection.cancel();
        }
        // ACP: the client answers open permission prompts with Cancelled.
        self.cancel_pending_permissions(cx);
        cx.notify();
    }

    fn cancel_pending_permissions(&mut self, cx: &mut Context<Self>) {
        for ix in 0..self.entries.len() {
            if let Entry::Tool(tool) = &mut self.entries[ix]
                && let Some(pending) = tool.permission.take()
            {
                let _ = pending.reply.send(None);
                self.touched(ix, cx);
            }
        }
    }

    fn answer_permission(
        &mut self,
        tool_id: &ToolCallId,
        option: PermissionOptionId,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.tool_index(tool_id) else {
            return;
        };
        if let Entry::Tool(tool) = &mut self.entries[ix]
            && let Some(pending) = tool.permission.take()
        {
            let _ = pending.reply.send(Some(option));
        }
        self.touched(ix, cx);
        cx.notify();
    }

    fn handle_event(&mut self, event: AgentEvent, cx: &mut Context<Self>) {
        match event {
            AgentEvent::Ready { agent_name } => {
                self.agent_name = agent_name.into();
                self.status = Status::Ready;
            }
            AgentEvent::Update(update) => self.apply_update(update, cx),
            AgentEvent::TurnEnded(reason) => {
                self.turn_running = false;
                let note = match reason {
                    StopReason::EndTurn => None,
                    StopReason::Cancelled => Some("Stopped."),
                    StopReason::MaxTokens => Some("Stopped: the reply hit the token limit."),
                    StopReason::MaxTurnRequests => Some("Stopped: too many steps in one turn."),
                    StopReason::Refusal => Some("The agent declined to continue."),
                    _ => Some("Stopped."),
                };
                if let Some(note) = note {
                    self.push_notice(note, false, cx);
                }
            }
            AgentEvent::TurnFailed(message) => {
                self.turn_running = false;
                self.push_notice(message, true, cx);
            }
            AgentEvent::AuthRequired(methods) => {
                self.disconnect(cx);
                self.status = Status::LoginRequired(methods);
            }
            AgentEvent::Permission(ask) => self.ask_permission(ask, cx),
            AgentEvent::ReadFile(read) => cx.emit(AgentPanelEvent::ReadFile(read)),
            AgentEvent::WriteFile(write) => cx.emit(AgentPanelEvent::WriteFile(write)),
            AgentEvent::Closed(error) => {
                if !matches!(self.status, Status::LoginRequired(_)) {
                    let message = error.unwrap_or_else(|| "The agent exited.".into());
                    self.status = Status::Failed(message.into());
                }
                self.disconnect(cx);
            }
        }
        cx.notify();
    }

    fn apply_update(&mut self, update: SessionUpdate, cx: &mut Context<Self>) {
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let Some(text) = block_text(&chunk.content) {
                    self.append_text(false, &text, cx);
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let Some(text) = block_text(&chunk.content) {
                    self.append_text(true, &text, cx);
                }
            }
            SessionUpdate::ToolCall(call) => self.upsert_tool_call(call, cx),
            SessionUpdate::ToolCallUpdate(update) => self.update_tool_call(update, cx),
            SessionUpdate::Plan(plan) => {
                if let Some(ix) = self.entries.iter().rposition(|e| matches!(e, Entry::Plan(_))) {
                    self.entries[ix] = Entry::Plan(plan.entries);
                    self.touched(ix, cx);
                } else {
                    self.push_entry(Entry::Plan(plan.entries), cx);
                }
            }
            // User chunks only replay on session load (not used); the rest is
            // metadata the v1 panel does not show.
            _ => {}
        }
    }

    fn append_text(&mut self, thought: bool, text: &str, cx: &mut Context<Self>) {
        let last = self.entries.len().checked_sub(1);
        let state = last.and_then(|ix| match (&self.entries[ix], thought) {
            (Entry::Agent(state), false) | (Entry::Thought(state), true) => Some(state.clone()),
            _ => None,
        });
        match (state, last) {
            (Some(state), Some(ix)) => {
                state.update(cx, |state, cx| state.push_str(text, cx));
                self.touched(ix, cx);
            }
            _ => {
                let state = cx.new(|cx| TextViewState::markdown(text, cx).selectable(true));
                self.push_entry(
                    if thought {
                        Entry::Thought(state)
                    } else {
                        Entry::Agent(state)
                    },
                    cx,
                );
            }
        }
    }

    fn tool_index(&self, id: &ToolCallId) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|e| matches!(e, Entry::Tool(tool) if &tool.id == id))
    }

    fn upsert_tool_call(&mut self, call: ToolCall, cx: &mut Context<Self>) {
        let location = call.locations.first().map(|l| l.path.clone());
        if let Some(ix) = self.tool_index(&call.tool_call_id) {
            if let Entry::Tool(tool) = &mut self.entries[ix] {
                tool.title = call.title.into();
                tool.kind = call.kind;
                tool.status = call.status;
                tool.location = location.or(tool.location.take());
            }
            self.touched(ix, cx);
            return;
        }
        self.push_entry(
            Entry::Tool(ToolEntry {
                id: call.tool_call_id,
                title: call.title.into(),
                kind: call.kind,
                status: call.status,
                location,
                permission: None,
            }),
            cx,
        );
    }

    fn update_tool_call(&mut self, update: ToolCallUpdate, cx: &mut Context<Self>) {
        let Some(ix) = self.tool_index(&update.tool_call_id) else {
            // Updates for unseen calls still deserve a row.
            let fields = update.fields;
            self.push_entry(
                Entry::Tool(ToolEntry {
                    id: update.tool_call_id,
                    title: fields.title.unwrap_or_else(|| "Tool call".into()).into(),
                    kind: fields.kind.unwrap_or(ToolKind::Other),
                    status: fields.status.unwrap_or(ToolCallStatus::Pending),
                    location: fields
                        .locations
                        .and_then(|l| l.into_iter().next())
                        .map(|l| l.path),
                    permission: None,
                }),
                cx,
            );
            return;
        };
        if let Entry::Tool(tool) = &mut self.entries[ix] {
            let fields = update.fields;
            if let Some(title) = fields.title {
                tool.title = title.into();
            }
            if let Some(kind) = fields.kind {
                tool.kind = kind;
            }
            if let Some(status) = fields.status {
                tool.status = status;
            }
            if let Some(location) = fields.locations.and_then(|l| l.into_iter().next()) {
                tool.location = Some(location.path);
            }
        }
        self.touched(ix, cx);
    }

    fn ask_permission(&mut self, ask: PermissionAsk, cx: &mut Context<Self>) {
        let PermissionAsk {
            tool_call,
            options,
            reply,
        } = ask;
        let id = tool_call.tool_call_id.clone();
        self.update_tool_call(tool_call, cx);
        if let Some(ix) = self.tool_index(&id)
            && let Entry::Tool(tool) = &mut self.entries[ix]
        {
            if let Some(old) = tool.permission.replace(PendingPermission { options, reply }) {
                let _ = old.reply.send(None);
            }
            self.touched(ix, cx);
        }
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
    }

    fn push_notice(&mut self, text: impl Into<SharedString>, error: bool, cx: &mut Context<Self>) {
        self.push_entry(
            Entry::Notice {
                text: text.into(),
                error,
            },
            cx,
        );
    }

    fn push_entry(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.entries.push(entry);
        self.scroller.update(cx, |s, cx| {
            s.append(1, cx);
        });
    }

    /// Row `ix` changed height (streamed text, tool status, …).
    fn touched(&self, ix: usize, cx: &mut Context<Self>) {
        self.scroller.update(cx, |s, cx| {
            s.remeasure_items(ix..ix + 1, cx);
        });
    }

    fn clear_entries(&mut self, cx: &mut Context<Self>) {
        self.entries.clear();
        self.scroller.update(cx, |s, cx| s.reset(0, cx));
    }
}

fn default_name(settings: &AgentSettings) -> SharedString {
    match settings.command.as_deref() {
        Some(command) if !command.trim().is_empty() => crate::editor::document::file_name(
            std::path::Path::new(command),
        )
        .into(),
        _ => CLAUDE_AGENT_NAME.into(),
    }
}

fn block_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text(text) => Some(text.text.clone()),
        ContentBlock::ResourceLink(link) => Some(format!("[{}]({})", link.name, link.uri)),
        _ => None,
    }
}

fn tool_icon(kind: ToolKind) -> LucideIcon {
    match kind {
        ToolKind::Read => LucideIcon::FileText,
        ToolKind::Edit | ToolKind::Move => LucideIcon::Pencil,
        ToolKind::Delete => LucideIcon::Trash,
        ToolKind::Search => LucideIcon::Search,
        ToolKind::Execute => LucideIcon::SquareTerminal,
        ToolKind::Think => LucideIcon::Brain,
        ToolKind::Fetch => LucideIcon::Globe,
        _ => LucideIcon::Wrench,
    }
}

/// Fixed 7×7 block-style status mark (same as the LSP catalog rows).
fn status_mark(color: Hsla) -> Div {
    div()
        .w(px(7.))
        .h(px(7.))
        .min_w(px(7.))
        .min_h(px(7.))
        .flex_shrink_0()
        .bg(color)
}

fn render_entry(
    panel: &WeakEntity<AgentPanel>,
    entry: &Entry,
    ix: usize,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let body: AnyElement = match entry {
        Entry::User(text) => Message::new()
            .alignment(MessageAlignment::End)
            .content(
                MessageContent::new().bubble(
                    Bubble::new()
                        .with_variant(BubbleVariant::Secondary)
                        .child(div().text_sm().child(text.clone())),
                ),
            )
            .into_any_element(),
        Entry::Agent(state) => div()
            .w_full()
            .min_w_0()
            .text_sm()
            .child(TextView::new(state))
            .into_any_element(),
        Entry::Thought(state) => div()
            .w_full()
            .min_w_0()
            .pl_2()
            .border_l_2()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(TextView::new(state))
            .into_any_element(),
        Entry::Tool(tool) => render_tool(panel, tool, cx),
        Entry::Plan(items) => v_flex()
            .w_full()
            .gap_1()
            .text_sm()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Plan"),
            )
            .children(items.iter().map(|item| {
                let (color, done) = match item.status {
                    PlanEntryStatus::Completed => (theme.success, true),
                    PlanEntryStatus::InProgress => (theme.warning, false),
                    _ => (theme.muted_foreground, false),
                };
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(status_mark(color))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .when(done, |d| d.text_color(theme.muted_foreground).line_through())
                            .child(item.content.clone()),
                    )
            }))
            .into_any_element(),
        Entry::Notice { text, error } => div()
            .text_xs()
            .text_color(if *error {
                theme.danger
            } else {
                theme.muted_foreground
            })
            .child(text.clone())
            .into_any_element(),
    };
    div()
        .id(("agent-entry", ix))
        .w_full()
        .px_3()
        .py_1()
        .child(body)
        .into_any_element()
}

fn render_tool(panel: &WeakEntity<AgentPanel>, tool: &ToolEntry, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let status: AnyElement = match tool.status {
        ToolCallStatus::Completed => Icon::new(LucideIcon::Check)
            .small()
            .text_color(theme.success)
            .into_any_element(),
        ToolCallStatus::Failed => Icon::new(LucideIcon::CircleX)
            .small()
            .text_color(theme.danger)
            .into_any_element(),
        _ => Spinner::new().small().into_any_element(),
    };
    let tool_key = tool.id.to_string();
    v_flex()
        .w_full()
        .gap_2()
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    Icon::new(tool_icon(tool.kind))
                        .small()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().truncate().child(tool.title.clone()))
                        .when_some(tool.location.as_ref(), |col, path| {
                            col.child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(path.display().to_string()),
                            )
                        }),
                )
                .child(status),
        )
        .when_some(tool.permission.as_ref(), |col, pending| {
            col.child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(pending.options.iter().map(|option| {
                        let panel = panel.clone();
                        let tool_id = tool.id.clone();
                        let option_id = option.option_id.clone();
                        let button = Button::new(SharedString::from(format!(
                            "agent-perm-{tool_key}-{}",
                            option.option_id
                        )))
                        .small()
                        .cursor_pointer()
                        .label(option.name.clone());
                        let button = match option.kind {
                            PermissionOptionKind::AllowOnce => button.primary(),
                            PermissionOptionKind::AllowAlways => button.outline(),
                            _ => button.danger().outline(),
                        };
                        button.on_click(move |_, _, cx| {
                            let _ = panel.update(cx, |panel, cx| {
                                panel.answer_permission(&tool_id, option_id.clone(), cx);
                            });
                        })
                    })),
            )
        })
        .into_any_element()
}

impl AgentPanel {
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let color = match &self.status {
            Status::Ready if self.turn_running => theme.warning,
            Status::Ready => theme.success,
            Status::Starting | Status::Installing => theme.warning,
            Status::Failed(_) => theme.danger,
            _ => theme.muted_foreground,
        };
        let can_new_chat = matches!(self.status, Status::Ready | Status::Failed(_));
        h_flex()
            .w_full()
            .h(px(theme::PANEL_HEADER_ROW_H))
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(theme.border)
            .child(status_mark(color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(self.agent_name.clone()),
            )
            .child(
                Button::new("agent-new-chat")
                    .ghost()
                    .small()
                    .icon(LucideIcon::SquarePen)
                    .tooltip("New Chat")
                    .disabled(!can_new_chat)
                    .when(can_new_chat, |b| b.cursor_pointer())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.restart(cx);
                        this.focus_input(window, cx);
                    })),
            )
            .child(
                Button::new("agent-hide")
                    .ghost()
                    .small()
                    .cursor_pointer()
                    .icon(LucideIcon::PanelRightClose)
                    .tooltip("Hide Agent Panel")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(AgentPanelEvent::Hide))),
            )
            .into_any_element()
    }

    /// Centered message for every state without a live session.
    fn render_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let name = self.agent_name.clone();
        let col = v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_4()
            .text_center();
        let title = |text: SharedString| div().text_sm().child(text);
        let hint = |text: SharedString| div().text_xs().text_color(muted).child(text);
        match &self.status {
            Status::Idle if self.cwd.is_none() => col
                .child(hint("Open a folder to use the agent.".into()))
                .into_any_element(),
            Status::Idle | Status::Starting | Status::Ready => col
                .child(Spinner::new())
                .child(hint(format!("Starting {name}…").into()))
                .into_any_element(),
            Status::NotInstalled | Status::Installing => {
                let installing = matches!(self.status, Status::Installing);
                col.child(title(format!("{name} is not installed").into()))
                    .child(hint(
                        "Install downloads the Claude Agent adapter into Isengard's data \
                         folder (needs Node.js). It signs in with your Claude Code login."
                            .into(),
                    ))
                    .child(
                        Button::new("agent-install")
                            .primary()
                            .cursor_pointer()
                            .icon(LucideIcon::Download)
                            .label("Install")
                            .loading(installing)
                            .on_click(cx.listener(|this, _, _, cx| this.install(cx))),
                    )
                    .when_some(self.install_error.clone(), |col, err| {
                        col.child(div().text_xs().text_color(theme.danger).child(err))
                    })
                    .into_any_element()
            }
            Status::LoginRequired(methods) => col
                .child(title(format!("Sign in to {name}").into()))
                .child(hint(if methods.is_empty() {
                    "Sign in with the agent's own CLI, then press Retry.".into()
                } else {
                    "Login runs in the terminal panel. Press Retry when it finishes.".into()
                }))
                .child(
                    h_flex().flex_wrap().justify_center().gap_2().children(
                        methods.iter().enumerate().map(|(i, method)| {
                            let method = method.clone();
                            let button = Button::new(("agent-login", i))
                                .cursor_pointer()
                                .icon(LucideIcon::LogIn)
                                .label(method.name.clone())
                                .when_some(method.description.clone(), |b, d| b.tooltip(d));
                            if i == 0 {
                                button.primary()
                            } else {
                                button.outline()
                            }
                            .on_click(cx.listener(move |this, _, _, cx| this.login(&method, cx)))
                        }),
                    ),
                )
                .child(
                    Button::new("agent-retry")
                        .outline()
                        .cursor_pointer()
                        .icon(LucideIcon::RotateCw)
                        .label("Retry")
                        .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                )
                .into_any_element(),
            Status::Failed(message) => col
                .child(title(format!("{name} stopped").into()))
                .child(
                    div()
                        .max_w_full()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(message.clone()),
                )
                .child(
                    Button::new("agent-retry")
                        .primary()
                        .cursor_pointer()
                        .icon(LucideIcon::RotateCw)
                        .label("Retry")
                        .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                )
                .into_any_element(),
        }
    }

    fn render_transcript(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.entries.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .p_4()
                .text_center()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("Ask {} about this workspace.", self.agent_name))
                .into_any_element();
        }
        let panel = cx.entity().downgrade();
        MessageScroller::new("agent-transcript", self.scroller.clone(), move |ix, _, cx| {
            let Some(this) = panel.upgrade() else {
                return div().into_any_element();
            };
            let this = this.read(cx);
            match this.entries.get(ix) {
                Some(entry) => render_entry(&panel, entry, ix, cx),
                None => div().into_any_element(),
            }
        })
        .size_full()
        .into_any_element()
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let action = if self.turn_running {
            Button::new("agent-stop")
                .outline()
                .small()
                .cursor_pointer()
                .icon(LucideIcon::Square)
                .label("Stop")
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        } else {
            Button::new("agent-send")
                .primary()
                .small()
                .cursor_pointer()
                .icon(LucideIcon::ArrowUp)
                .label("Send")
                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
        };
        v_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(theme.border)
            .child(Textarea::new(&self.input))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Enter to send · Shift+Enter for a new line"),
                    )
                    .child(action),
            )
            .into_any_element()
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = matches!(self.status, Status::Ready);
        v_flex()
            .id("agent-panel")
            .size_full()
            .min_w_0()
            .bg(cx.theme().sidebar)
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(if ready {
                        self.render_transcript(cx)
                    } else {
                        self.render_state(cx)
                    }),
            )
            .when(ready, |col| col.child(self.render_composer(cx)))
    }
}
