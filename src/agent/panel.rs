//! Right-hand Agent panel: transcript + prompt box for one folder/workspace.
//!
//! One panel per folder/workspace (the app keeps them in a map). The panel
//! owns its [`AgentConnection`], transcript and chat index. Switching away
//! suspends it: a running turn finishes in the background, then the agent
//! process stops; the transcript stays so coming back resumes the chat without
//! a replay. Anything that touches editor state (file reads/writes, login in
//! the terminal, hiding, persisting the chat index) is an [`AgentPanelEvent`].

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    list::ListItem,
    message::{Message, MessageAlignment, MessageContent},
    message_scroller::{MessageScroller, MessageScrollerState},
    popover::{Popover, PopoverState},
    scroll::ScrollableElement as _,
    spinner::Spinner,
    text::{TextView, TextViewState},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::acp::schema::{
    ContentBlock, PermissionOption, PermissionOptionId, PermissionOptionKind,
    PlanEntry, PlanEntryStatus, SessionUpdate, StopReason, ToolCall, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolKind,
};
use crate::acp::{
    self, AgentConnection, AgentEvent, AgentInfo, AgentLaunch, CLAUDE_AGENT_NAME, FileRead,
    FileWrite, LoginMethod, PermissionAsk, Resolved, SessionOpen,
};
use crate::config::{AgentChat, AgentSettings, MAX_AGENT_CHATS};
use crate::terminal::pty::TerminalCommand;
use crate::theme;
use crate::ui::components::center_dialog;

/// Side effects the app performs for the panel.
pub enum AgentPanelEvent {
    ReadFile(FileRead),
    WriteFile(FileWrite),
    /// Run the agent's login command in a terminal tab.
    Login { title: String, command: TerminalCommand },
    Hide,
    /// Chat index or active chat changed — persist it for this workspace.
    ChatsChanged,
    /// A tool is waiting for approval (the app toasts if this panel is in
    /// the background).
    PermissionRequested,
}

#[derive(Clone)]
enum Status {
    /// Not connected yet (or no folder open).
    Idle,
    NotInstalled,
    Installing,
    Starting,
    /// Connected; creating / loading / resuming a chat.
    Opening,
    Ready,
    /// Workspace in the background and idle: agent stopped, transcript kept.
    Suspended,
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
    /// Workspace roots: the agent runs in the first, sees all of them.
    roots: Vec<PathBuf>,
    /// Roots changed during a turn: reconnect once it ends.
    reconnect_after_turn: bool,
    status: Status,
    agent_name: SharedString,
    info: Option<AgentInfo>,
    launch: Option<AgentLaunch>,
    connection: Option<AgentConnection>,
    /// Bumped per connection so events from a dropped one are ignored.
    generation: u64,
    turn_running: bool,
    /// In the background with a turn running: stop the agent when it ends.
    suspend_after_turn: bool,
    /// Chats started here, newest first (persisted by the app).
    chats: Vec<AgentChat>,
    /// Chat shown in the panel (`None` = a new chat not yet prompted).
    active: Option<String>,
    /// Session the open connection is on; updates for others are dropped.
    session: Option<String>,
    /// What is being opened while `Status::Opening`.
    opening: Option<SessionOpen>,
    /// Opens sent but not answered. The agent answers in order, so only the
    /// last answer counts (the user may click another chat mid-load).
    opens_in_flight: usize,
    /// Which chat `entries` belongs to (lets a reconnect resume, not replay).
    transcript_of: Option<String>,
    entries: Vec<Entry>,
    scroller: Entity<MessageScrollerState>,
    input: Entity<TextareaState>,
    install_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AgentPanelEvent> for AgentPanel {}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// First line of the first prompt, shortened — until the agent sends a title.
fn chat_title(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or("").trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    if title.is_empty() {
        "Untitled chat".into()
    } else {
        title
    }
}

/// `just now` / `5m ago` / `3h ago` / `2d ago`.
fn relative_time(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

impl AgentPanel {
    pub fn new(
        roots: Vec<PathBuf>,
        settings: AgentSettings,
        chats: Vec<AgentChat>,
        active: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            agent_name: default_name(&settings),
            settings,
            roots,
            reconnect_after_turn: false,
            status: Status::Idle,
            info: None,
            launch: None,
            connection: None,
            generation: 0,
            turn_running: false,
            suspend_after_turn: false,
            chats,
            active,
            session: None,
            opening: None,
            opens_in_flight: 0,
            transcript_of: None,
            entries: Vec::new(),
            scroller,
            input,
            install_error: None,
            _subscriptions: vec![enter],
        }
    }

    pub fn chats(&self) -> &[AgentChat] {
        &self.chats
    }

    /// Active chat, only once it is in the index (a never-prompted new chat
    /// is not worth reopening).
    pub fn active_chat(&self) -> Option<&str> {
        self.active
            .as_deref()
            .filter(|id| self.chats.iter().any(|c| c.id == *id))
    }

    /// Connect (or reconnect after a suspend) if nothing is running.
    pub fn ensure_started(&mut self, cx: &mut Context<Self>) {
        self.suspend_after_turn = false;
        if matches!(self.status, Status::Idle | Status::Suspended) {
            self.start(cx);
        }
    }

    /// Folders were added to / removed from this workspace. A live agent
    /// reconnects (resuming the chat) so it sees the new set of roots.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        if roots == self.roots {
            return;
        }
        self.roots = roots;
        if self.turn_running {
            self.reconnect_after_turn = true;
        } else if self.connection.is_some() {
            self.start(cx);
        }
    }

    /// The workspace went to the background. A running turn keeps going; the
    /// agent stops once idle. The transcript stays for a later resume.
    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        if self.turn_running {
            self.suspend_after_turn = true;
        } else if self.connection.is_some() {
            self.disconnect(cx);
            self.status = Status::Suspended;
            cx.notify();
        }
    }

    /// `"you: …"` / `"agent: …"` lines of the transcript (tests).
    #[cfg(test)]
    pub fn transcript(&self, cx: &App) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::User(text) => Some(format!("you: {text}")),
                Entry::Agent(state) => Some(format!("agent: {}", state.read(cx).rendered_text().source())),
                _ => None,
            })
            .collect()
    }

    /// Background-idle with its agent stopped (tests).
    #[cfg(test)]
    pub fn is_suspended(&self) -> bool {
        matches!(self.status, Status::Suspended) && self.connection.is_none()
    }

    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready) && !self.turn_running
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
        if self.roots.is_empty() {
            self.status = Status::Idle;
            cx.notify();
            return;
        }
        let roots = self.roots.clone();
        #[cfg(test)]
        if let Some(connection) = acp::testing::connection(roots.clone()) {
            self.attach(connection, cx);
            return;
        }
        let launch = match acp::resolve(&self.settings) {
            Resolved::Launch(launch) => launch,
            Resolved::ClaudeNotInstalled => {
                self.status = Status::NotInstalled;
                cx.notify();
                return;
            }
        };
        let connection = AgentConnection::spawn(&launch, roots);
        self.launch = Some(launch);
        self.attach(connection, cx);
    }

    /// Pump `connection`'s events into this panel (tests attach fakes here).
    pub fn attach(&mut self, connection: AgentConnection, cx: &mut Context<Self>) {
        self.disconnect(cx);
        let events = connection.events.clone();
        self.connection = Some(connection);
        self.status = Status::Starting;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            while let Some(event) = next_event(&events, cx).await {
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
        self.session = None;
        self.opening = None;
        self.opens_in_flight = 0;
        self.generation = self.generation.wrapping_add(1);
        self.turn_running = false;
        self.cancel_pending_permissions(cx);
    }

    /// Ask the connected agent to open the active chat (or a new one).
    fn open_active(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.info.clone() else {
            return;
        };
        if self.connection.is_none() {
            return;
        }
        let open = match self.active_chat().map(str::to_owned) {
            // Same transcript still on screen: no replay needed.
            Some(id) if info.can_resume && self.transcript_of.as_deref() == Some(id.as_str()) => {
                SessionOpen::Resume(id)
            }
            Some(id) if info.can_load => SessionOpen::Load(id),
            _ => SessionOpen::New,
        };
        if !matches!(open, SessionOpen::Resume(_)) {
            self.clear_entries(cx);
            self.transcript_of = None;
        }
        // Replayed updates carry the chat's id before the open completes.
        self.session = match &open {
            SessionOpen::Load(id) | SessionOpen::Resume(id) => Some(id.clone()),
            SessionOpen::New => None,
        };
        if let Some(connection) = &self.connection {
            connection.open(open.clone());
            self.opens_in_flight += 1;
        }
        self.opening = Some(open);
        self.status = Status::Opening;
        cx.notify();
    }

    /// Retry after a failure / login: reconnect, keeping the active chat.
    fn retry(&mut self, cx: &mut Context<Self>) {
        self.start(cx);
    }

    fn new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.turn_running {
            return;
        }
        self.active = None;
        cx.emit(AgentPanelEvent::ChatsChanged);
        if matches!(self.status, Status::Ready | Status::Opening) {
            self.open_active(cx);
        } else {
            self.clear_entries(cx);
            self.transcript_of = None;
            self.start(cx);
        }
        self.focus_input(window, cx);
    }

    fn switch_chat(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.turn_running || self.active.as_deref() == Some(id.as_str()) {
            return;
        }
        self.active = Some(id);
        cx.emit(AgentPanelEvent::ChatsChanged);
        if matches!(self.status, Status::Ready | Status::Opening) {
            self.open_active(cx);
        } else {
            self.start(cx);
        }
        self.focus_input(window, cx);
    }

    fn confirm_delete_chat(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .chats
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.title.clone())
            .unwrap_or_default();
        let panel = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            let panel = panel.clone();
            let id = id.clone();
            center_dialog(
                dialog
                    .title("Delete chat?")
                    .child(format!("“{title}” will be deleted from the agent's history."))
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("dialog-cancel")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                Button::new("dialog-primary")
                                    .cursor_pointer()
                                    .danger()
                                    .label("Delete")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let id = id.clone();
                                        _ = panel.update(cx, |this, cx| {
                                            this.delete_chat(id, window, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    fn delete_chat(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.turn_running && self.active.as_deref() == Some(id.as_str()) {
            return;
        }
        if let Some(connection) = &self.connection
            && self.info.as_ref().is_some_and(|i| i.can_delete)
        {
            connection.delete(id.clone());
        }
        self.chats.retain(|c| c.id != id);
        if self.active.as_deref() == Some(id.as_str()) {
            self.new_chat(window, cx);
        }
        cx.emit(AgentPanelEvent::ChatsChanged);
        cx.notify();
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
        let (Some(connection), Some(session)) = (&self.connection, self.session.clone()) else {
            return;
        };
        connection.prompt(text.clone());
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.turn_running = true;
        self.touch_chat(&session, Some(&text));
        cx.emit(AgentPanelEvent::ChatsChanged);
        self.push_entry(Entry::User(text.into()), cx);
        // Show the reply even if the user had scrolled up.
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        cx.notify();
    }

    /// Move `id` to the front of the index (adding it on its first prompt).
    fn touch_chat(&mut self, id: &str, first_prompt: Option<&str>) {
        let mut chat = match self.chats.iter().position(|c| c.id == id) {
            Some(ix) => self.chats.remove(ix),
            None => AgentChat {
                id: id.to_owned(),
                title: chat_title(first_prompt.unwrap_or("")),
                updated: 0,
            },
        };
        chat.updated = now_secs();
        self.chats.insert(0, chat);
        self.chats.truncate(MAX_AGENT_CHATS);
        self.active = Some(id.to_owned());
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

    fn turn_finished(&mut self, cx: &mut Context<Self>) {
        self.turn_running = false;
        if std::mem::take(&mut self.reconnect_after_turn) && !self.suspend_after_turn {
            self.start(cx);
        } else if self.suspend_after_turn {
            self.suspend_after_turn = false;
            self.disconnect(cx);
            self.status = Status::Suspended;
        }
    }

    /// Count an open's answer; `true` if a newer open is still pending.
    fn take_stale_open(&mut self) -> bool {
        self.opens_in_flight = self.opens_in_flight.saturating_sub(1);
        self.opens_in_flight > 0
    }

    fn handle_event(&mut self, event: AgentEvent, cx: &mut Context<Self>) {
        match event {
            AgentEvent::Connected(info) => {
                self.agent_name = info.name.clone().into();
                self.info = Some(info);
                self.open_active(cx);
            }
            AgentEvent::SessionOpened { .. } | AgentEvent::SessionOpenFailed { .. }
                if self.take_stale_open() => {}
            AgentEvent::SessionOpened { id } => {
                self.session = Some(id.clone());
                self.transcript_of = Some(id.clone());
                // A brand-new chat is active but stays out of the index (and
                // so is not reopened) until its first prompt.
                self.active = Some(id);
                self.opening = None;
                self.status = Status::Ready;
            }
            AgentEvent::SessionOpenFailed { open, message } => {
                self.opening = None;
                match open {
                    SessionOpen::Load(id) | SessionOpen::Resume(id) => {
                        // Deleted outside Isengard (or never stored): forget it.
                        log::warn!("agent: could not reopen chat {id}: {message}");
                        self.chats.retain(|c| c.id != id);
                        self.active = None;
                        self.transcript_of = None;
                        cx.emit(AgentPanelEvent::ChatsChanged);
                        self.open_active(cx);
                        self.push_notice("That chat is no longer available.", false, cx);
                    }
                    SessionOpen::New => self.status = Status::Failed(message.into()),
                }
            }
            AgentEvent::Update { session, update } => {
                if self.session.as_deref() == Some(session.as_str()) {
                    self.apply_update(update, cx);
                }
            }
            AgentEvent::TurnEnded(reason) => {
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
                self.turn_finished(cx);
            }
            AgentEvent::TurnFailed(message) => {
                self.push_notice(message, true, cx);
                self.turn_finished(cx);
            }
            AgentEvent::AuthRequired(methods) => {
                self.disconnect(cx);
                self.status = Status::LoginRequired(methods);
            }
            AgentEvent::Permission(ask) => {
                self.ask_permission(ask, cx);
                cx.emit(AgentPanelEvent::PermissionRequested);
            }
            AgentEvent::ReadFile(read) => cx.emit(AgentPanelEvent::ReadFile(read)),
            AgentEvent::WriteFile(write) => cx.emit(AgentPanelEvent::WriteFile(write)),
            AgentEvent::Closed(error) => {
                if !matches!(self.status, Status::LoginRequired(_) | Status::Suspended) {
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
            // Only arrives while `session/load` replays history.
            SessionUpdate::UserMessageChunk(chunk) => {
                if let Some(text) = block_text(&chunk.content) {
                    self.append_user(&text, cx);
                }
            }
            SessionUpdate::SessionInfoUpdate(info) => {
                if let (Some(title), Some(id)) = (info.title.take(), self.session.clone())
                    && let Some(chat) = self.chats.iter_mut().find(|c| c.id == id)
                    && chat.title != title
                {
                    chat.title = title;
                    cx.emit(AgentPanelEvent::ChatsChanged);
                }
            }
            // Metadata the panel does not show.
            _ => {}
        }
    }

    /// Replayed user chunks merge into one bubble.
    fn append_user(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.entries.len().checked_sub(1)
            && let Entry::User(existing) = &self.entries[ix]
        {
            self.entries[ix] = Entry::User(format!("{existing}{text}").into());
            self.touched(ix, cx);
        } else {
            self.push_entry(Entry::User(text.to_owned().into()), cx);
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

/// Next agent event, or `None` once the connection is gone.
#[cfg(not(test))]
async fn next_event(events: &flume::Receiver<AgentEvent>, _: &mut AsyncApp) -> Option<AgentEvent> {
    events.recv_async().await.ok()
}

/// GPUI's test scheduler rejects wakeups from other threads (the agent
/// thread), so tests poll on the scheduler's own clock instead.
#[cfg(test)]
async fn next_event(events: &flume::Receiver<AgentEvent>, cx: &mut AsyncApp) -> Option<AgentEvent> {
    loop {
        match events.try_recv() {
            Ok(event) => return Some(event),
            Err(flume::TryRecvError::Disconnected) => return None,
            Err(flume::TryRecvError::Empty) => {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(5))
                    .await
            }
        }
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
    /// Live status line: what the agent is doing right now (real tool title when possible).
    fn activity_label(&self) -> Option<SharedString> {
        match &self.status {
            Status::Installing => Some("Installing…".into()),
            Status::Starting => Some("Starting…".into()),
            Status::Opening => Some(
                match &self.opening {
                    Some(SessionOpen::Load(_)) => "Loading chat…",
                    Some(SessionOpen::Resume(_)) => "Resuming…",
                    _ => "Opening chat…",
                }
                .into(),
            ),
            Status::Ready if self.turn_running => {
                if let Some(title) = self.entries.iter().rev().find_map(|e| match e {
                    Entry::Tool(tool)
                        if matches!(
                            tool.status,
                            ToolCallStatus::Pending | ToolCallStatus::InProgress
                        ) =>
                    {
                        Some(tool.title.clone())
                    }
                    _ => None,
                }) {
                    return Some(title);
                }
                match self.entries.last() {
                    Some(Entry::Thought(_)) => Some("Thinking…".into()),
                    Some(Entry::Agent(_)) => Some("Writing…".into()),
                    Some(Entry::Plan(_)) => Some("Planning…".into()),
                    _ => Some("Working…".into()),
                }
            }
            _ => None,
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let color = match &self.status {
            Status::Ready if self.turn_running => theme.warning,
            Status::Ready => theme.success,
            Status::Starting | Status::Installing | Status::Opening => theme.warning,
            Status::Failed(_) => theme.danger,
            _ => theme.muted_foreground,
        };
        let can_new_chat =
            !self.turn_running && matches!(self.status, Status::Ready | Status::Failed(_));
        let activity = self.activity_label();
        let title: SharedString = match &activity {
            Some(what) => format!("{} · {what}", self.agent_name).into(),
            None => self.agent_name.clone(),
        };
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
            .when(activity.is_some(), |row| {
                row.child(Spinner::new().small())
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .when(activity.is_some(), |t| t.text_color(theme.muted_foreground))
                    .child(title),
            )
            .child(
                Button::new("agent-new-chat")
                    .ghost()
                    .small()
                    .icon(LucideIcon::SquarePen)
                    .tooltip("New Chat")
                    .disabled(!can_new_chat)
                    .when(can_new_chat, |b| b.cursor_pointer())
                    .on_click(cx.listener(|this, _, window, cx| this.new_chat(window, cx))),
            )
            .when(self.info.as_ref().is_none_or(|i| i.can_load), |row| {
                row.child(self.render_history(cx))
            })
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

    /// Past chats of this folder/workspace (newest first).
    fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel = cx.entity().downgrade();
        let chats = self.chats.clone();
        let active = self.active.clone();
        let busy = self.turn_running;
        Popover::new("agent-history")
            .anchor(Anchor::TopRight)
            .trigger(
                Button::new("agent-history-button")
                    .ghost()
                    .small()
                    .cursor_pointer()
                    .icon(LucideIcon::MessagesSquare)
                    .tooltip("Chat History"),
            )
            .content(move |_state: &mut PopoverState, _, cx| {
                let popover = cx.weak_entity();
                let theme = cx.theme();
                let now = now_secs();
                v_flex()
                    .w(px(300.))
                    .max_h(px(360.))
                    .overflow_y_scrollbar()
                    .when(chats.is_empty(), |col| {
                        col.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("No chats in this workspace yet."),
                        )
                    })
                    .children(chats.iter().map(|chat| {
                        let selected = active.as_deref() == Some(chat.id.as_str());
                        let (open_panel, delete_panel) = (panel.clone(), panel.clone());
                        let (open_popover, delete_popover) = (popover.clone(), popover.clone());
                        let (open_id, delete_id) = (chat.id.clone(), chat.id.clone());
                        ListItem::new(SharedString::from(format!("agent-chat-{}", chat.id)))
                            .selected(selected)
                            .disabled(busy)
                            .when(!busy, |row| row.cursor_pointer())
                            .on_click(move |_, window, cx| {
                                if let Some(p) = open_popover.upgrade() {
                                    p.update(cx, |p, cx| p.dismiss(window, cx));
                                }
                                let id = open_id.clone();
                                _ = open_panel.update(cx, |this, cx| this.switch_chat(id, window, cx));
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .child(div().text_sm().truncate().child(chat.title.clone()))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(relative_time(chat.updated, now)),
                                            ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "agent-chat-delete-{}",
                                            chat.id
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(LucideIcon::Trash)
                                        .tooltip("Delete Chat")
                                        .disabled(busy && selected)
                                        .when(!(busy && selected), |b| b.cursor_pointer())
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            if let Some(p) = delete_popover.upgrade() {
                                                p.update(cx, |p, cx| p.dismiss(window, cx));
                                            }
                                            let id = delete_id.clone();
                                            _ = delete_panel.update(cx, |this, cx| {
                                                this.confirm_delete_chat(id, window, cx);
                                            });
                                        }),
                                    ),
                            )
                    }))
            })
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
            Status::Idle if self.roots.is_empty() => col
                .child(hint("Open a folder to use the agent.".into()))
                .into_any_element(),
            Status::Opening if matches!(self.opening, Some(SessionOpen::Load(_))) => col
                .child(Spinner::new())
                .child(hint("Loading chat…".into()))
                .into_any_element(),
            Status::Idle
            | Status::Starting
            | Status::Opening
            | Status::Suspended
            | Status::Ready => col
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
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
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
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
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
        // Keep the transcript on screen while reconnecting / replaying into it.
        let show_transcript = ready
            || (!self.entries.is_empty()
                && matches!(
                    self.status,
                    Status::Starting | Status::Opening | Status::Suspended
                ));
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
                    .child(if show_transcript {
                        self.render_transcript(cx)
                    } else {
                        self.render_state(cx)
                    }),
            )
            .when(ready, |col| col.child(self.render_composer(cx)))
    }
}
