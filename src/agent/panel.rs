//! Right-hand Agent panel: transcript + prompt box for one folder/workspace.
//!
//! One panel per folder/workspace (the app keeps them in a map). The panel
//! owns its [`AgentConnection`], transcript and chat index. Switching away
//! suspends it: a running turn finishes in the background, then the agent
//! process stops; the transcript stays so coming back resumes the chat without
//! a replay. Anything that touches editor state (file reads/writes, login in
//! the terminal, hiding, persisting the chat index) is an [`AgentPanelEvent`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    input::{
        InlineToken, InlineTokenContext, InlineTokenSpan, Input, InputContent, InputEvent,
        InputState, Textarea, TextareaState,
    },
    list::ListItem,
    message::{Message, MessageAlignment, MessageContent},
    message_scroller::{MessageScroller, MessageScrollerState},
    popover::{Popover, PopoverState},
    progress::Progress,
    scroll::ScrollableElement as _,
    spinner::Spinner,
    switch::Switch,
    text::{TextView, TextViewState},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::acp::schema::{
    CompactionStatus, ContentBlock, PermissionOption, PermissionOptionId, PermissionOptionKind,
    PlanEntry, PlanEntryStatus, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOptions, SessionModeState, SessionUpdate,
    StopReason, TextContent, ToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolKind,
};
#[cfg(test)]
use crate::acp::schema::ToolCallUpdateFields;
use crate::acp::{
    self, AgentConnection, AgentEvent, AgentInfo, AgentLaunch, CLAUDE_AGENT_NAME, FileRead,
    FileWrite, LoginMethod, PermissionAsk, Resolved, SessionOpen, SilentConfigValue,
};
use crate::agent::mention::{self, MentionHit, MentionQuery, MAX_MENTION_HITS};
use crate::agent::prompt::{
    self, Attachment, PromptCaps, MAX_ATTACHMENTS, display_user_message, image_from_bytes,
    image_from_path,
};
use crate::agent::limits;
use crate::editor::document;
use crate::file_tree::tree_icon;
use crate::config::{
    AgentChat, AgentFileChange, AgentFileChangesCard, AgentPrefValue, AgentPrefs, AgentSettings,
    MAX_AGENT_CHATS,
};
use crate::git::{line_change_counts, snapshot_dirty_contents, worktree_deltas_since_turn};
use crate::terminal::pty::TerminalCommand;
use crate::theme;
use crate::ui::components::center_dialog;
use crate::ui::file_tree::TreeEntryDrag;
use crate::ui::tab_drag::EditorTabDrag;

/// Side effects the app performs for the panel.
pub enum AgentPanelEvent {
    ReadFile(FileRead),
    WriteFile(FileWrite),
    /// Run the agent's login command in a terminal tab.
    Login { title: String, command: TerminalCommand },
    /// Open the OS file picker and attach the chosen paths.
    AttachFromDisk,
    /// Attach a path as a chip (paperclip, paste, or drag-drop from tree /
    /// editor tab / OS explorer). `@` mentions stay inline in the prompt.
    AttachPath(PathBuf),
    /// Open these paths in editor tabs (file-change summary row click).
    OpenPaths(Vec<PathBuf>),
    /// Agent changed the worktree (write / edit / delete / shell) — refresh Git.
    WorkspaceTouched,
    Hide,
    /// Chat index or active chat changed — persist it for this workspace.
    ChatsChanged,
    /// User picked a Mode / Model / … — persist [`AgentPanel::prefs`].
    PrefsChanged,
    /// User changed [`AgentSettings`] (e.g. utility / silent model) — persist
    /// global `config.json` `agent`.
    SettingsChanged,
    /// A tool is waiting for approval (the app toasts if this panel is in
    /// the background).
    PermissionRequested,
    /// Silent commit-message turn finished with a draft for Source Control.
    CommitSuggestReady { root: PathBuf, message: String },
    /// Silent commit-message turn failed / was cancelled.
    CommitSuggestFailed { root: PathBuf },
    /// [`AgentPanel::can_silent_prompt`] may have flipped (Source Control Claude Suggest).
    PromptabilityChanged,
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

/// One file touched this turn (writes + deletes), shown in the summary card.
struct PendingFileChange {
    path: PathBuf,
    /// Content before the first touch this turn.
    baseline: String,
    latest: String,
    deleted: bool,
}

/// Row in the end-of-turn file-change summary.
#[derive(Clone)]
struct FileChangeRow {
    path: PathBuf,
    added: u32,
    removed: u32,
    deleted: bool,
}

impl FileChangeRow {
    fn stored(&self) -> AgentFileChange {
        AgentFileChange {
            path: self.path.clone(),
            added: self.added,
            removed: self.removed,
            deleted: self.deleted,
        }
    }

    fn from_stored(row: AgentFileChange) -> Self {
        Self {
            path: row.path,
            added: row.added,
            removed: row.removed,
            deleted: row.deleted,
        }
    }
}

impl PendingFileChange {
    fn row(&self) -> FileChangeRow {
        let (added, removed) = if self.deleted {
            (0, line_change_counts(&self.baseline, "").1)
        } else {
            line_change_counts(&self.baseline, &self.latest)
        };
        FileChangeRow {
            path: self.path.clone(),
            added,
            removed,
            deleted: self.deleted,
        }
    }
}

/// Thumbnail source for an attached image (path preferred; paste is in-memory).
#[derive(Clone)]
enum ImageThumb {
    Path(PathBuf),
    Memory(std::sync::Arc<Image>),
}

/// File/folder/image chip shown inside a user bubble (not the `@name` text dump).
#[derive(Clone)]
struct UserRef {
    /// Basename (folders keep a trailing `/`).
    name: SharedString,
    /// Full path for Material icons (`tree_icon`); empty when paste-only image.
    path: PathBuf,
    is_dir: bool,
    /// Terminal (or other) snippet — Lucide terminal icon instead of Material path.
    snippet: bool,
    /// When set, render a small preview instead of a file/folder icon.
    thumb: Option<ImageThumb>,
}

enum Entry {
    User {
        text: SharedString,
        refs: Vec<UserRef>,
    },
    Agent(Entity<TextViewState>),
    Thought(Entity<TextViewState>),
    Tool(ToolEntry),
    Plan(Vec<PlanEntry>),
    /// Collapsible end-of-turn summary of files the agent wrote/deleted.
    FileChanges {
        files: Vec<FileChangeRow>,
        expanded: bool,
    },
    Notice { text: SharedString, error: bool },
}

fn user_refs_from(
    attachments: &[Attachment],
    thumbs: &[Option<ImageThumb>],
) -> Vec<UserRef> {
    attachments
        .iter()
        .enumerate()
        .map(|(ix, a)| {
            let thumb = thumbs.get(ix).and_then(|t| t.clone());
            match a {
                Attachment::File { path, .. } => {
                    let is_dir = path.is_dir();
                    let mut name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    if is_dir && !name.ends_with('/') {
                        name.push('/');
                    }
                    UserRef {
                        name: name.into(),
                        path: path.clone(),
                        is_dir,
                        snippet: false,
                        thumb,
                    }
                }
                Attachment::Snippet { label, .. } => UserRef {
                    name: label.clone().into(),
                    path: PathBuf::new(),
                    is_dir: false,
                    snippet: true,
                    thumb,
                },
                Attachment::Image { label, path, .. } => UserRef {
                    name: label.clone().into(),
                    path: path.clone().unwrap_or_default(),
                    is_dir: false,
                    snippet: false,
                    thumb,
                },
            }
        })
        .collect()
}

fn user_refs_from_mention_tokens(tokens: &[InlineTokenSpan]) -> Vec<UserRef> {
    tokens
        .iter()
        .map(|span| {
            let path = PathBuf::from(span.token().id().as_ref());
            let is_dir = path.is_dir();
            let thumb = document::is_image(&path).then(|| ImageThumb::Path(path.clone()));
            UserRef {
                name: span.token().label().clone(),
                path,
                is_dir,
                snippet: false,
                thumb,
            }
        })
        .collect()
}

/// Prompt text with inline-token backing strings removed (attachments carry them).
fn text_without_tokens(text: &str, tokens: &[InlineTokenSpan]) -> String {
    let mut out = text.to_owned();
    for span in tokens.iter().rev() {
        let range = span.range();
        if range.end <= out.len() {
            out.replace_range(range, "");
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Inline `@` chip inside the composer Textarea (Material icon or image thumb).
fn render_mention_token(ctx: &InlineTokenContext, roots: &[PathBuf], cx: &App) -> AnyElement {
    let theme = cx.theme();
    let path = PathBuf::from(ctx.token().id().as_ref());
    let h = ctx.line_height();
    if document::is_image(&path) {
        return div()
            .id(SharedString::from(format!("agent-inline-img-{}", path.display())))
            .h(h)
            .w(h)
            .min_w(h)
            .min_h(h)
            .flex_shrink_0()
            .overflow_hidden()
            .rounded(theme.radius)
            .border_1()
            .border_color(if ctx.is_selected() {
                theme.selection.alpha(1.)
            } else {
                theme.border
            })
            .child(
                img(path)
                    .id("agent-inline-img-src")
                    .object_fit(ObjectFit::Cover)
                    .size_full(),
            )
            .into_any_element();
    }
    let is_dir = path.is_dir();
    let is_root = roots.iter().any(|r| r == &path);
    // Material icons (same as the tree) — Kit InputToken only takes Lucide Icon.
    div()
        .id(SharedString::from(format!("agent-inline-tok-{}", path.display())))
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .h(h)
        .max_w(ctx.available_width())
        .rounded(theme.radius)
        .border_1()
        .map(|this| {
            if ctx.is_selected() {
                this.bg(theme.selection)
                    .border_color(theme.selection.alpha(1.))
            } else {
                this.bg(theme.muted).border_color(theme.border)
            }
        })
        .text_color(theme.foreground)
        .when(ctx.is_disabled(), |this| this.opacity(0.5))
        .child(material_path_icon(&path, is_dir, is_root, cx))
        .child(
            div()
                .min_w_0()
                .text_ellipsis()
                .child(ctx.token().label().clone()),
        )
        .into_any_element()
}

/// Material file/folder icon (same as the explorer tree).
fn material_path_icon(path: &Path, is_dir: bool, is_root: bool, cx: &App) -> AnyElement {
    tree_icon(path, is_dir, false, is_root, cx.theme().is_dark())
}

fn thumb_for_attachment(attachment: &Attachment) -> Option<ImageThumb> {
    match attachment {
        Attachment::Image {
            path: Some(path), ..
        } => Some(ImageThumb::Path(path.clone())),
        Attachment::Image {
            mime, data_b64, ..
        } => {
            let format = ImageFormat::from_mime_type(mime)?;
            let bytes = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                data_b64,
            )
            .ok()?;
            Some(ImageThumb::Memory(std::sync::Arc::new(Image::from_bytes(
                format, bytes,
            ))))
        }
        Attachment::File { path, .. } if document::is_image(path) => {
            Some(ImageThumb::Path(path.clone()))
        }
        _ => None,
    }
}

/// Small square preview for an attached image (composer chips + user bubble).
fn render_image_thumb(thumb: &ImageThumb, id: SharedString, border: Hsla) -> AnyElement {
    let img_el = match thumb {
        ImageThumb::Path(path) => img(path.clone()),
        ImageThumb::Memory(image) => img(image.clone()),
    };
    div()
        .w(px(40.))
        .h(px(40.))
        .min_w(px(40.))
        .min_h(px(40.))
        .flex_shrink_0()
        .overflow_hidden()
        .border_1()
        .border_color(border)
        .child(
            img_el
                .id(id)
                .object_fit(ObjectFit::Cover)
                .size_full(),
        )
        .into_any_element()
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
    /// Prompts typed while a turn (or silent job) is running — drained FIFO
    /// when the current turn ends. User bubbles are already in `entries`.
    outbound: VecDeque<QueuedPrompt>,
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
    /// Plain-text transcripts for AI titles (survives switching chats this session).
    title_transcripts: HashMap<String, String>,
    /// After loading a non-active chat, run Generate once entries land.
    pending_title: Option<String>,
    /// Improved composer draft waiting for `render` (needs `Window` for set_value).
    pending_improve: Option<String>,
    /// New chats get a first-prompt placeholder title; after the first real
    /// turn we run a silent title turn once (cleared on rename / success).
    needs_auto_title: HashSet<String>,
    /// Re-splice **N Files** cards as ACP history arrives (SessionOpened can
    /// race ahead of replay `Update`s on cold start).
    pending_file_change_restore: bool,
    entries: Vec<Entry>,
    /// Context chips for the next prompt (paperclip / paste / drag-drop).
    attachments: Vec<Attachment>,
    /// Parallel to `attachments`: image thumbnails (path or decoded paste).
    attachment_thumbs: Vec<Option<ImageThumb>>,
    /// Open `@` file/folder picker above the composer, if any.
    mention: Option<MentionMenu>,
    scroller: Entity<MessageScrollerState>,
    input: Entity<TextareaState>,
    install_error: Option<SharedString>,
    /// Silent one-shot turn (commit / title / improve) — not shown in chat.
    silent: Option<SilentTurn>,
    /// ACP session modes from the last open (Ask/Code/…).
    modes: Option<SessionModeState>,
    /// ACP session config options (model selector, …).
    config_options: Option<Vec<SessionConfigOption>>,
    /// Last composer choices, re-applied whenever a chat opens.
    prefs: AgentPrefs,
    /// Live context-window usage for the open session (`usage_update`).
    usage: Option<ContextUsage>,
    /// Last known usage per chat id (this process only — not persisted).
    usage_by_chat: HashMap<String, ContextUsage>,
    /// Claude plan limits (5-hour session + week); `None` until fetched.
    plan_limits: Option<limits::PlanLimits>,
    /// Usage strip expanded in place (details capped so transcript keeps height).
    plan_usage_open: bool,
    /// Agent is mid-compaction (`CompactionUpdate` InProgress).
    compacting: bool,
    /// Files written/deleted during the current turn (flushed to a summary card).
    turn_file_changes: HashMap<PathBuf, PendingFileChange>,
    /// Dirty/untracked contents at turn start — catches shell create/edit/delete.
    turn_git_baseline: Option<HashMap<PathBuf, Option<String>>>,
    _subscriptions: Vec<Subscription>,
}

/// Context fill for the open session.
///
/// `used` is `None` until the agent sends `usage_update` (Claude only emits
/// that mid-turn). We still seed `size` on open so the header is not blank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContextUsage {
    used: Option<u64>,
    size: u64,
}

/// One-shot prompt that accumulates assistant text without chat bubbles.
enum SilentTurn {
    Commit { root: PathBuf, text: String },
    /// `notify`: show toasts when Generate was user-requested (not auto).
    Title {
        chat_id: String,
        text: String,
        notify: bool,
    },
    /// Rewrite composer draft; applied through `pending_improve` on next render.
    Improve { text: String },
}

/// A user prompt waiting for the current turn to finish.
struct QueuedPrompt {
    /// First-line title / touch_chat seed (same as an immediate send).
    title: String,
    text: String,
    refs: Vec<UserRef>,
    blocks: Vec<ContentBlock>,
}

impl SilentTurn {
    fn text_mut(&mut self) -> &mut String {
        match self {
            Self::Commit { text, .. } | Self::Title { text, .. } | Self::Improve { text } => text,
        }
    }
}

struct MentionMenu {
    query: MentionQuery,
    /// Cursor byte offset when the menu was last refreshed.
    cursor: usize,
    selected: usize,
    hits: Vec<MentionHit>,
    /// Keeps ↑↓ selection inside the scroll viewport.
    scroll: ScrollHandle,
}

impl EventEmitter<AgentPanelEvent> for AgentPanel {}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Compact token count for the header / history (`53K`, `1.2M`).
fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        let m = n as f64 / 1_000_000.0;
        if m >= 10.0 {
            format!("{m:.0}M")
        } else {
            format!("{m:.1}M")
        }
    } else if n >= 10_000 {
        format!("{}K", (n + 500) / 1_000)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_context_usage(u: ContextUsage) -> String {
    let used = match u.used {
        Some(n) => format_tokens(n),
        None => "—".into(),
    };
    format!("{} / {}", used, format_tokens(u.size))
}

/// Best-effort window size from the model / Context picker (until ACP reports).
fn estimate_context_window_size(options: &Option<Vec<SessionConfigOption>>) -> u64 {
    if let Some(label) = model_family_split(options).and_then(|s| s.context_label)
        && let Some(n) = tokens_from_context_label(&label)
    {
        return n;
    }
    if let Some(ctx) = find_context_select(options) {
        let label = ctx
            .choices
            .iter()
            .find(|(id, _)| id == &ctx.current)
            .map(|(_, n)| n.as_str())
            .unwrap_or(ctx.current.as_str());
        if let Some(n) = tokens_from_context_label(label) {
            return n;
        }
    }
    if let Some(id) = find_select(options, Some(SessionConfigOptionCategory::Model), "model")
        .map(|m| m.current)
        && let Some(n) = context_from_model_id(&id).and_then(|l| tokens_from_context_label(&l))
    {
        return n;
    }
    // Claude ACP's own default until the first authoritative usage_update.
    200_000
}

fn tokens_from_context_label(label: &str) -> Option<u64> {
    let l = label.trim().to_ascii_uppercase();
    if l == "1M" || l == "1.0M" {
        return Some(1_000_000);
    }
    if let Some(rest) = l.strip_suffix('M')
        && let Ok(m) = rest.parse::<f64>()
    {
        return Some((m * 1_000_000.0).round() as u64);
    }
    if let Some(rest) = l.strip_suffix('K')
        && let Ok(k) = rest.parse::<u64>()
    {
        return Some(k.saturating_mul(1_000));
    }
    None
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
        prefs: AgentPrefs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Plan limits come from Claude's own login; skip for custom agents
        // and in tests (no network).
        if !cfg!(test) && settings.command.is_none() {
            cx.spawn(async move |this, cx| {
                // Undocumented OAuth usage endpoint 429s if polled hard (or
                // after a burst of other clients). On failure with no bars
                // yet, back off 5→10→20→40→60s instead of waiting a full
                // 120s so a restart mid-rate-limit does not hide the strip.
                let mut retry_secs = 5u64;
                loop {
                    let fetched = cx.background_spawn(async { limits::fetch() }).await;
                    let (alive, have_bars) = match this.update(cx, |this, cx| {
                        // Err → keep last. Empty/partial Ok → merge (API
                        // often returns null windows without failing).
                        match fetched {
                            Ok(l) => {
                                let next = limits::merge(this.plan_limits.clone(), l);
                                if this.plan_limits != next {
                                    this.plan_limits = next;
                                    cx.notify();
                                }
                            }
                            Err(err) => {
                                log::debug!("agent plan limits: {err}");
                            }
                        }
                        this.plan_limits
                            .as_ref()
                            .is_some_and(|l| l.session.is_some() || l.week.is_some())
                    }) {
                        Ok(have) => (true, have),
                        Err(_) => (false, false),
                    };
                    if !alive {
                        break;
                    }
                    let wait = if have_bars {
                        retry_secs = 5;
                        120
                    } else {
                        let w = retry_secs;
                        retry_secs = (retry_secs.saturating_mul(2)).min(60);
                        w
                    };
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(wait))
                        .await;
                }
            })
            .detach();
        }
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Ask the agent… · @ to mention files")
                .auto_grow(2, 10)
                .submit_on_enter(true)
        });
        let enter = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { shift: false, .. } => {
                    if this.mention.is_some() {
                        this.confirm_mention(window, cx);
                    } else {
                        this.send(window, cx);
                    }
                }
                InputEvent::Change => this.refresh_mention(cx),
                _ => {}
            }
        });
        // ponytail: Textarea eats ↑↓/Esc; intercept while @ menu is open.
        // Also ⌘N/Ctrl+N — Kit focus may not put parent `Agent` key context on
        // the stack, so catch New Chat here when the composer is focused.
        let panel = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(panel) = panel.upgrade() else {
                return;
            };
            panel.update(cx, |this, cx| {
                let ks = &event.keystroke;
                if ks.key.as_str() == "n"
                    && ks.modifiers.secondary()
                    && !ks.modifiers.alt
                    && !ks.modifiers.shift
                {
                    let composer_focused = this
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window);
                    if composer_focused {
                        this.new_chat(window, cx);
                        cx.stop_propagation();
                        return;
                    }
                }
                if this.mention.is_none() {
                    return;
                }
                match ks.key.as_str() {
                    "up" => {
                        this.mention_move(-1);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "down" => {
                        this.mention_move(1);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    // Dive into folder without attaching (Enter attaches).
                    "right" | "tab" => {
                        this.dive_mention(window, cx);
                        cx.stop_propagation();
                    }
                    "escape" => {
                        this.close_mention(cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            });
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
            outbound: VecDeque::new(),
            suspend_after_turn: false,
            chats,
            active,
            session: None,
            opening: None,
            opens_in_flight: 0,
            transcript_of: None,
            title_transcripts: HashMap::new(),
            pending_title: None,
            pending_improve: None,
            needs_auto_title: HashSet::new(),
            pending_file_change_restore: false,
            entries: Vec::new(),
            attachments: Vec::new(),
            attachment_thumbs: Vec::new(),
            mention: None,
            scroller,
            input,
            install_error: None,
            silent: None,
            modes: None,
            config_options: None,
            prefs,
            usage: None,
            usage_by_chat: HashMap::new(),
            plan_limits: None,
            plan_usage_open: false,
            compacting: false,
            turn_file_changes: HashMap::new(),
            turn_git_baseline: None,
            _subscriptions: vec![enter, keys],
        }
    }

    fn prompt_caps(&self) -> PromptCaps {
        PromptCaps {
            image: self.info.as_ref().is_some_and(|i| i.can_image),
            embedded_context: self.info.as_ref().is_some_and(|i| i.can_embedded_context),
        }
    }

    /// Add a file / selection / image chip (dedupes identical attachments).
    pub fn attach_context(&mut self, attachment: Attachment, cx: &mut Context<Self>) {
        if self.attachments.len() >= MAX_ATTACHMENTS {
            self.push_notice(
                format!("At most {MAX_ATTACHMENTS} attachments per message."),
                true,
                cx,
            );
            return;
        }
        let dup = self.attachments.iter().any(|a| a == &attachment);
        if dup {
            return;
        }
        if matches!(attachment, Attachment::Image { .. }) && !self.prompt_caps().image {
            self.push_notice("This agent does not accept images.", true, cx);
            return;
        }
        let thumb = thumb_for_attachment(&attachment);
        self.attachments.push(attachment);
        self.attachment_thumbs.push(thumb);
        cx.notify();
    }

    /// Labels of chips queued for the next prompt (tests).
    #[cfg(test)]
    pub fn attachment_labels(&self) -> Vec<String> {
        self.attachments.iter().map(Attachment::label).collect()
    }

    /// Active chat title from the index (tests).
    #[cfg(test)]
    pub fn active_chat_title(&self) -> Option<String> {
        let id = self.active.as_deref()?;
        self.chats.iter().find(|c| c.id == id).map(|c| c.title.clone())
    }

    /// Current ACP session mode id (tests).
    #[cfg(test)]
    pub fn current_mode_id(&self) -> Option<String> {
        self.modes
            .as_ref()
            .map(|m| m.current_mode_id.to_string())
    }

    /// Active chat id (tests).
    #[cfg(test)]
    pub fn active_chat_id(&self) -> Option<String> {
        self.active.clone()
    }

    /// Title for a chat id (tests).
    #[cfg(test)]
    pub fn chat_title(&self, id: &str) -> Option<String> {
        self.chats.iter().find(|c| c.id == id).map(|c| c.title.clone())
    }

    /// Start a new chat from tests (same as the header Plus).
    #[cfg(test)]
    pub fn new_chat_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_chat(window, cx);
    }

    /// Current model config value id (tests).
    #[cfg(test)]
    pub fn current_model_id(&self) -> Option<String> {
        find_select(&self.config_options, Some(SessionConfigOptionCategory::Model), "model")
            .map(|s| s.current)
    }

    /// Parsed model family label from the Model option (tests), e.g. `Sonnet`.
    #[cfg(test)]
    pub fn current_model_family(&self) -> Option<String> {
        model_family_split(&self.config_options).map(|s| s.family)
    }

    /// Parsed version label for the current family (tests), e.g. `4.5`.
    #[cfg(test)]
    pub fn current_model_version_label(&self) -> Option<String> {
        model_family_split(&self.config_options).and_then(|s| s.version_label)
    }

    /// Context window label for the current model (tests), e.g. `1M` / `200K`.
    #[cfg(test)]
    pub fn current_model_context_label(&self) -> Option<String> {
        model_family_split(&self.config_options).and_then(|s| s.context_label)
    }

    /// Whether Fast mode is on (tests).
    #[cfg(test)]
    pub fn fast_mode_on(&self) -> Option<bool> {
        find_fast_option(&self.config_options).map(fast_is_on)
    }

    /// Live context usage label for the open session (tests), e.g. `20K / 200K`.
    #[cfg(test)]
    pub fn context_usage_label(&self) -> Option<String> {
        self.usage.map(format_context_usage)
    }

    /// Whether Mode/Model/… row is populated (tests).
    #[cfg(test)]
    pub fn has_composer_config_for_test(&self) -> bool {
        self.has_composer_config_controls()
    }

    /// Mode choice ids shown in the composer picker (tests) — after model filtering.
    #[cfg(test)]
    pub fn mode_choice_ids(&self) -> Vec<String> {
        let model_id = find_select(
            &self.config_options,
            Some(SessionConfigOptionCategory::Model),
            "model",
        )
        .map(|m| m.current);
        if let Some(opt) = find_select(
            &self.config_options,
            Some(SessionConfigOptionCategory::Mode),
            "mode",
        ) {
            return filter_mode_choices(&opt.choices, model_id.as_deref())
                .into_iter()
                .map(|(id, _)| id)
                .collect();
        }
        let Some(modes) = &self.modes else {
            return Vec::new();
        };
        let raw: Vec<(String, String)> = modes
            .available_modes
            .iter()
            .map(|m| (m.id.to_string(), m.name.clone()))
            .collect();
        filter_mode_choices(&raw, model_id.as_deref())
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    /// Set session mode from tests (same path as the composer picker).
    #[cfg(test)]
    pub fn set_mode_for_test(&mut self, mode_id: &str, cx: &mut Context<Self>) {
        self.set_mode(mode_id.to_owned(), cx);
    }

    /// Pick a model family (tests) — keeps context when possible.
    #[cfg(test)]
    pub fn set_model_family_for_test(&mut self, family: &str, cx: &mut Context<Self>) {
        let Some(split) = model_family_split(&self.config_options) else {
            return;
        };
        let prefer_ctx = split.context_label.clone();
        let Some(id) = split
            .choices
            .iter()
            .filter(|c| c.family == family)
            .find(|c| display_context_label(&split.choices, c).as_ref() == prefer_ctx.as_ref())
            .or_else(|| split.choices.iter().find(|c| c.family == family))
            .map(|c| c.id.clone())
        else {
            return;
        };
        self.set_config_value(split.config_id, id, cx);
    }

    /// Pick a version within the current family (tests).
    #[cfg(test)]
    pub fn set_model_version_for_test(&mut self, version: &str, cx: &mut Context<Self>) {
        let Some(split) = model_family_split(&self.config_options) else {
            return;
        };
        let prefer_ctx = split.context_label.clone();
        let Some(id) = split
            .choices
            .iter()
            .filter(|c| c.family == split.family && c.version.as_deref() == Some(version))
            .find(|c| c.context == prefer_ctx)
            .or_else(|| {
                split.choices.iter().find(|c| {
                    c.family == split.family && c.version.as_deref() == Some(version)
                })
            })
            .map(|c| c.id.clone())
        else {
            return;
        };
        self.set_config_value(split.config_id, id, cx);
    }

    /// Pick a context window within the current family+version (tests).
    #[cfg(test)]
    pub fn set_model_context_for_test(&mut self, context: &str, cx: &mut Context<Self>) {
        let Some(split) = model_family_split(&self.config_options) else {
            return;
        };
        let Some(id) = context_choice_id(&split, context) else {
            return;
        };
        self.set_config_value(split.config_id, id, cx);
    }

    /// Toggle Fast mode from tests.
    #[cfg(test)]
    pub fn set_fast_mode_for_test(&mut self, on: bool, cx: &mut Context<Self>) {
        self.set_fast_mode(on, cx);
    }

    /// Labels currently shown in the `@` mention menu (tests).
    #[cfg(test)]
    pub fn mention_labels(&self) -> Vec<String> {
        self.mention
            .as_ref()
            .map(|m| m.hits.iter().map(|h| h.label.clone()).collect())
            .unwrap_or_default()
    }

    /// Set composer text and refresh `@` mentions (tests — `set_value` alone
    /// does not emit `InputEvent::Change`).
    #[cfg(test)]
    pub fn set_composer_text_for_test(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let caret = text.len();
        self.input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.set_selected_range(caret..caret, cx);
        });
        self.refresh_mention(cx);
    }

    /// Confirm the selected `@` mention into the composer (tests).
    #[cfg(test)]
    pub fn confirm_mention_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_mention(window, cx);
    }

    /// Workspace roots the agent was given; the first is its cwd (tests).
    #[cfg(test)]
    pub fn roots_for_test(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Composer text (tests).
    #[cfg(test)]
    pub fn composer_text_for_test(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Labels of inline `@` tokens in the composer (tests).
    #[cfg(test)]
    pub fn prompt_mention_names_for_test(&self, cx: &App) -> Vec<String> {
        self.input
            .read(cx)
            .tokens()
            .iter()
            .map(|s| s.token().label().to_string())
            .collect()
    }

    pub fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            if index < self.attachment_thumbs.len() {
                self.attachment_thumbs.remove(index);
            }
            cx.notify();
        }
    }

    fn refresh_mention(&mut self, cx: &mut Context<Self>) {
        let (text, cursor, token_ranges) = self.input.read_with(cx, |input, _| {
            (
                input.value().to_string(),
                input.selected_range().end,
                input
                    .tokens()
                    .iter()
                    .map(|s| s.range())
                    .collect::<Vec<_>>(),
            )
        });
        let Some(query) = mention::mention_at_cursor(&text, cursor) else {
            if self.mention.take().is_some() {
                cx.notify();
            }
            return;
        };
        // Ignore `@` that is only the backing text of an existing inline token.
        if token_ranges
            .iter()
            .any(|r| r.start <= query.at && query.at < r.end)
        {
            if self.mention.take().is_some() {
                cx.notify();
            }
            return;
        }
        let hits = mention::list_mentions(&self.roots, &query.query, MAX_MENTION_HITS);
        let selected = self
            .mention
            .as_ref()
            .filter(|m| m.query.query == query.query)
            .map(|m| m.selected.min(hits.len().saturating_sub(1)))
            .unwrap_or(0);
        let scroll = self
            .mention
            .as_ref()
            .filter(|m| m.query.query == query.query)
            .map(|m| m.scroll.clone())
            .unwrap_or_default();
        self.mention = Some(MentionMenu {
            query,
            cursor,
            selected,
            hits,
            scroll: scroll.clone(),
        });
        scroll.scroll_to_item(selected);
        cx.notify();
    }

    fn close_mention(&mut self, cx: &mut Context<Self>) {
        if self.mention.take().is_some() {
            cx.notify();
        }
    }

    fn mention_move(&mut self, delta: isize) {
        let Some(menu) = self.mention.as_mut() else {
            return;
        };
        if menu.hits.is_empty() {
            return;
        }
        let n = menu.hits.len() as isize;
        let next = (menu.selected as isize + delta).rem_euclid(n) as usize;
        menu.selected = next;
        menu.scroll.scroll_to_item(next);
    }

    fn confirm_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.mention.as_ref() else {
            return;
        };
        let Some(hit) = menu.hits.get(menu.selected).cloned() else {
            self.close_mention(cx);
            return;
        };
        let at = menu.query.at;
        let cursor = menu.cursor;
        let token_count = self.input.read(cx).tokens().len();
        if token_count >= MAX_ATTACHMENTS {
            self.push_notice(
                format!("At most {MAX_ATTACHMENTS} @ mentions per message."),
                true,
                cx,
            );
            self.close_mention(cx);
            return;
        }
        let mut name = hit
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| hit.path.display().to_string());
        if hit.is_dir && !name.ends_with('/') {
            name.push('/');
        }
        // Backing text is the basename (no leading `@` — that would re-trigger the
        // mention menu). Rendered as an inline chip inside the prompt Textarea.
        let token = InlineToken::new(hit.path.to_string_lossy().to_string(), name.clone())
            .with_label(name);
        self.mention = None;
        let result = self.input.update(cx, |input, cx| {
            input.replace_range_with_token(at..cursor, token, window, cx)
        });
        if let Err(err) = result {
            log::warn!("agent: insert @ token failed: {err}");
            self.push_notice("Could not insert that mention.", true, cx);
        }
        cx.notify();
    }

    /// Browse into the selected folder (`→` / Tab); does not attach.
    fn dive_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.mention.as_ref() else {
            return;
        };
        let Some(hit) = menu.hits.get(menu.selected).filter(|h| h.is_dir).cloned() else {
            return;
        };
        let at = menu.query.at;
        let cursor = menu.cursor;
        let text = self.input.read_with(cx, |input, _| input.value().to_string());
        let rel = mention::mention_rel_label(&self.roots, &hit.path);
        let insert = format!("@{}/", rel.trim_end_matches('/'));
        let (out, caret) = mention::replace_mention(&text, at, cursor, &insert);
        self.input.update(cx, |input, cx| {
            input.set_value(&out, window, cx);
            input.set_selected_range(caret..caret, cx);
        });
        self.refresh_mention(cx);
    }

    fn pick_mention(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.mention.as_mut() {
            menu.selected = index;
        }
        // Same as Enter: insert `@path` into the prompt. Dive with →/Tab only.
        self.confirm_mention(window, cx);
    }

    pub fn chats(&self) -> &[AgentChat] {
        &self.chats
    }

    pub fn prefs(&self) -> &AgentPrefs {
        &self.prefs
    }

    pub fn settings(&self) -> &AgentSettings {
        &self.settings
    }

    /// Replace launch / silent-model settings (e.g. after another panel saved).
    pub fn set_settings(&mut self, settings: AgentSettings, cx: &mut Context<Self>) {
        if self.settings == settings {
            return;
        }
        self.settings = settings;
        cx.notify();
    }

    /// Global utility model for silent jobs (`None` = inherit chat model).
    pub fn set_silent_model(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let next = id.filter(|s| !s.is_empty());
        if self.settings.silent_model == next {
            return;
        }
        self.settings.silent_model = next;
        cx.emit(AgentPanelEvent::SettingsChanged);
        cx.notify();
    }

    /// Pick a utility-model family (keeps version/context when possible).
    fn set_silent_model_family(&mut self, family: String, cx: &mut Context<Self>) {
        let Some(split) = model_family_split(&self.config_options) else {
            return;
        };
        let current = self
            .settings
            .silent_model
            .as_ref()
            .and_then(|id| split.choices.iter().find(|c| &c.id == id));
        let prefer_ver = current.and_then(|c| c.version.clone());
        let prefer_ctx = current.and_then(|c| display_context_label(&split.choices, c));
        let id = pick_silent_model_id(
            &split.choices,
            &family,
            prefer_ver.as_deref(),
            prefer_ctx.as_deref(),
        )
        .or_else(|| pick_silent_model_id(&split.choices, &family, None, None));
        self.set_silent_model(id, cx);
    }

    /// Pick a utility-model version within the current silent family.
    fn set_silent_model_version(&mut self, version: String, cx: &mut Context<Self>) {
        let Some(split) = model_family_split(&self.config_options) else {
            return;
        };
        let current = self
            .settings
            .silent_model
            .as_ref()
            .and_then(|id| split.choices.iter().find(|c| &c.id == id));
        let family = current
            .map(|c| c.family.clone())
            .unwrap_or_else(|| split.family.clone());
        let prefer_ctx = current.and_then(|c| display_context_label(&split.choices, c));
        let id = pick_silent_model_id(
            &split.choices,
            &family,
            Some(&version),
            prefer_ctx.as_deref(),
        );
        self.set_silent_model(id, cx);
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
        if self.turn_running || self.silent.is_some() {
            self.reconnect_after_turn = true;
        } else if self.connection.is_some() {
            self.start(cx);
        }
    }

    /// The workspace went to the background. A running turn keeps going; the
    /// agent stops once idle. The transcript stays for a later resume.
    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        if self.turn_running || self.silent.is_some() {
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
                Entry::User { text, refs } => {
                    let mut line = text.to_string();
                    for r in refs {
                        if !line.is_empty() {
                            line.push(' ');
                        }
                        line.push_str(&r.name);
                    }
                    Some(format!("you: {line}"))
                }
                Entry::Agent(state) => Some(format!("agent: {}", state.read(cx).rendered_text().source())),
                _ => None,
            })
            .collect()
    }

    /// Flush pending write/delete rows into a summary card (tests).
    #[cfg(test)]
    pub fn flush_file_changes_for_test(&mut self, cx: &mut Context<Self>) {
        self.flush_turn_file_changes(cx);
    }

    /// Snapshot dirty worktree like a real user turn start (tests).
    #[cfg(test)]
    pub fn begin_turn_file_tracking_for_test(&mut self) {
        self.begin_turn_file_tracking();
    }

    /// Simulate Edit tool pending → disk change → completed (tests).
    #[cfg(test)]
    pub fn note_edit_tool_for_test(
        &mut self,
        path: &Path,
        status: ToolCallStatus,
        cx: &mut Context<Self>,
    ) {
        self.track_tool_file_change(ToolKind::Edit, status, Some(path), cx);
    }

    /// Last file-change summary rows: `(basename, added, removed, deleted)`.
    #[cfg(test)]
    pub fn file_change_summaries_for_test(&self) -> Vec<(String, u32, u32, bool)> {
        self.entries.iter().rev().find_map(|entry| match entry {
            Entry::FileChanges { files, .. } => Some(
                files
                    .iter()
                    .map(|f| {
                        let name = f
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| f.path.display().to_string());
                        (name, f.added, f.removed, f.deleted)
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
    }

    /// How many **N Files** cards are in the transcript (tests).
    #[cfg(test)]
    pub fn file_change_card_count_for_test(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e, Entry::FileChanges { .. }))
            .count()
    }

    /// Re-run restore after a simulated `session/load` (tests).
    #[cfg(test)]
    pub fn restore_file_change_cards_for_test(&mut self, cx: &mut Context<Self>) {
        self.restore_file_change_cards(cx);
    }

    /// Drop transcript UI only (tests) — stored chat cards stay in `chats`.
    #[cfg(test)]
    pub fn clear_entries_for_test(&mut self, cx: &mut Context<Self>) {
        self.clear_entries(cx);
    }

    /// Bind a chat id so flush can persist **N Files** cards (tests).
    #[cfg(test)]
    pub fn seed_chat_session_for_test(&mut self, id: &str) {
        self.session = Some(id.to_owned());
        self.transcript_of = Some(id.to_owned());
        self.active = Some(id.to_owned());
        if !self.chats.iter().any(|c| c.id == id) {
            self.chats.push(AgentChat {
                id: id.to_owned(),
                title: "Test".into(),
                ..Default::default()
            });
        }
    }

    /// Push a bare user bubble so restore can place cards by turn (tests).
    #[cfg(test)]
    pub fn push_user_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        self.push_entry(
            Entry::User {
                text: text.into(),
                refs: Vec::new(),
            },
            cx,
        );
    }

    /// Background-idle with its agent stopped (tests).
    #[cfg(test)]
    pub fn is_suspended(&self) -> bool {
        matches!(self.status, Status::Suspended) && self.connection.is_none()
    }

    /// Connected chat idle — user Send / queue (independent of silent jobs).
    pub fn can_prompt(&self) -> bool {
        matches!(self.status, Status::Ready)
            && !self.turn_running
            && self.connection.is_some()
            && self.session.is_some()
    }

    /// Agent can run a throwaway silent job (Improve / title / Git Suggest).
    ///
    /// Silent turns open their own ACP session (open chat `session_id`
    /// untouched). Requires Ready + no in-flight silent; home-chat
    /// `turn_running` still blocks so we do not stack a Suggest on top of an
    /// active user turn (Cancel would be ambiguous).
    pub fn can_silent_prompt(&self) -> bool {
        matches!(self.status, Status::Ready)
            && !self.turn_running
            && self.connection.is_some()
            && self.silent.is_none()
    }

    fn emit_promptability_changed(&self, cx: &mut Context<Self>) {
        cx.emit(AgentPanelEvent::PromptabilityChanged);
    }

    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        self.can_prompt()
    }

    /// Whether a prompt turn (or silent job) is in flight.
    #[cfg(test)]
    pub fn is_turn_running(&self) -> bool {
        self.turn_running
    }

    /// Seed a tool row with Allow/Reject pending permission (tests).
    #[cfg(test)]
    pub fn inject_pending_permission_for_test(
        &mut self,
        cx: &mut Context<Self>,
    ) -> flume::Receiver<Option<PermissionOptionId>> {
        let (tx, rx) = flume::bounded(1);
        self.ask_permission(
            PermissionAsk {
                tool_call: ToolCallUpdate::new(
                    "t-perm",
                    ToolCallUpdateFields::new().title("Edit notes.txt"),
                ),
                options: vec![
                    PermissionOption::new("allow", "Allow", PermissionOptionKind::AllowOnce),
                    PermissionOption::new("reject", "Reject", PermissionOptionKind::RejectOnce),
                ],
                reply: tx,
            },
            cx,
        );
        rx
    }

    /// Answer the pending permission on tool `t-perm` (tests).
    #[cfg(test)]
    pub fn answer_permission_for_test(&mut self, option_id: &str, cx: &mut Context<Self>) {
        self.answer_permission(
            &ToolCallId::new("t-perm"),
            PermissionOptionId::new(option_id.to_owned()),
            cx,
        );
    }

    /// True when tool `t-perm` still has a pending permission (tests).
    #[cfg(test)]
    pub fn has_pending_permission_for_test(&self) -> bool {
        let id = ToolCallId::new("t-perm");
        self.tool_index(&id).is_some_and(|ix| {
            matches!(&self.entries[ix], Entry::Tool(t) if t.permission.is_some())
        })
    }

    /// Delete a chat from tests (same path as the confirm dialog).
    #[cfg(test)]
    pub fn delete_chat_for_test(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_chat(id.to_owned(), window, cx);
    }

    /// Force the Install empty-state (tests; does not run npm).
    #[cfg(test)]
    pub fn force_not_installed_for_test(&mut self, cx: &mut Context<Self>) {
        self.status = Status::NotInstalled;
        self.install_error = None;
        cx.notify();
    }

    /// Force the Login empty-state (tests; does not open a terminal).
    #[cfg(test)]
    pub fn force_login_required_for_test(&mut self, cx: &mut Context<Self>) {
        self.status = Status::LoginRequired(vec![LoginMethod {
            name: "Claude.ai".into(),
            description: Some("Sign in with Claude.ai".into()),
            args: vec!["--cli".into(), "auth".into(), "login".into()],
            env: std::collections::HashMap::new(),
        }]);
        cx.notify();
    }

    /// Current Mode + config picks for a throwaway silent session.
    fn silent_session_prefs(&self) -> (Option<String>, Vec<(String, SilentConfigValue)>) {
        let mode = self
            .modes
            .as_ref()
            .map(|m| m.current_mode_id.0.to_string())
            .or_else(|| self.prefs.mode.clone());
        let mut configs = Vec::new();
        if let Some(options) = &self.config_options {
            for opt in options {
                let id = opt.id.to_string();
                match &opt.kind {
                    SessionConfigKind::Boolean(b) => {
                        configs.push((id, SilentConfigValue::Bool(b.current_value)));
                    }
                    SessionConfigKind::Select(select) => {
                        configs.push((
                            id,
                            SilentConfigValue::Select(select.current_value.to_string()),
                        ));
                    }
                    _ => {}
                }
            }
        }
        let model = find_select(
            &self.config_options,
            Some(SessionConfigOptionCategory::Model),
            "model",
        );
        apply_silent_model_override(
            &mut configs,
            model.as_ref(),
            self.settings.silent_model.as_deref(),
        );
        (mode, configs)
    }

    fn start_silent(&mut self, prompt: String, kind: SilentTurn, cx: &mut Context<Self>) -> bool {
        if !self.can_silent_prompt() {
            return false;
        }
        let Some(connection) = &self.connection else {
            return false;
        };
        let (mode, configs) = self.silent_session_prefs();
        connection.silent_prompt(
            vec![ContentBlock::Text(TextContent::new(prompt))],
            mode,
            configs,
        );
        // Do not set `turn_running` — that is the open chat's turn. Silent
        // uses a throwaway session; the open transcript must stay idle.
        self.turn_file_changes.clear();
        self.turn_git_baseline = None;
        self.silent = Some(kind);
        self.emit_promptability_changed(cx);
        cx.notify();
        true
    }

    /// One-shot commit-message turn (any ACP agent). Does not add chat bubbles.
    ///
    /// Returns `false` if a silent job cannot start ([`Self::can_silent_prompt`]).
    pub fn request_commit_message(
        &mut self,
        root: PathBuf,
        prompt: String,
        cx: &mut Context<Self>,
    ) -> bool {
        self.start_silent(
            prompt,
            SilentTurn::Commit {
                root,
                text: String::new(),
            },
            cx,
        )
    }

    /// Whether Rename → Generate can run for `chat_id` (live, cached, or loadable).
    fn can_generate_title(&self, chat_id: &str, cx: &App) -> bool {
        if self.turn_running || self.silent.is_some() {
            return false;
        }
        if !matches!(self.status, Status::Ready) || self.connection.is_none() {
            return false;
        }
        if self.title_source(chat_id, cx).is_some() {
            return true;
        }
        // Not opened this session — Generate will load it first.
        self.info.as_ref().is_some_and(|i| i.can_load)
            && self.chats.iter().any(|c| c.id == chat_id)
    }

    /// Live transcript if this chat is on screen, else a cached copy.
    fn title_source(&self, chat_id: &str, cx: &App) -> Option<String> {
        if self.active.as_deref() == Some(chat_id) {
            let live = self.transcript_for_title(cx);
            if !live.is_empty() {
                return Some(live);
            }
        }
        self.title_transcripts
            .get(chat_id)
            .filter(|t| !t.is_empty())
            .cloned()
    }

    /// One-shot chat-title turn. Uses the open transcript or a cached one.
    /// Returns `false` if busy / no text (caller may load then retry via
    /// [`Self::begin_chat_title`]).
    pub fn request_chat_title(&mut self, chat_id: String, cx: &mut Context<Self>) -> bool {
        self.request_chat_title_inner(chat_id, true, cx)
    }

    fn request_chat_title_inner(
        &mut self,
        chat_id: String,
        notify: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(transcript) = self.title_source(&chat_id, cx) else {
            return false;
        };
        self.title_transcripts
            .insert(chat_id.clone(), transcript.clone());
        self.needs_auto_title.remove(&chat_id);
        self.start_silent(
            build_chat_title_prompt(&transcript),
            SilentTurn::Title {
                chat_id,
                text: String::new(),
                notify,
            },
            cx,
        )
    }

    /// After the first real turn on a new chat, replace the first-line
    /// placeholder with an AI title (Claude sends no `session_info_update`).
    fn maybe_auto_title_after_turn(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.active.clone() else {
            return;
        };
        if !self.needs_auto_title.contains(&id) {
            return;
        }
        let _ = self.request_chat_title_inner(id, false, cx);
    }

    /// Generate a title for any chat: silent turn if we have text, else load
    /// that chat and generate when the replay lands.
    fn begin_chat_title(
        &mut self,
        chat_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.request_chat_title(chat_id.clone(), cx) {
            return true;
        }
        if self.turn_running || self.silent.is_some() {
            return false;
        }
        if !matches!(self.status, Status::Ready) || self.connection.is_none() {
            return false;
        }
        if !self.info.as_ref().is_some_and(|i| i.can_load) {
            return false;
        }
        if self.chats.iter().all(|c| c.id != chat_id) {
            return false;
        }
        self.pending_title = Some(chat_id.clone());
        if self.active.as_deref() == Some(chat_id.as_str()) {
            // Already active but empty — nothing to title.
            self.pending_title = None;
            return false;
        }
        self.switch_chat(chat_id, window, cx);
        true
    }

    fn cache_active_transcript(&mut self, cx: &App) {
        let Some(id) = self
            .transcript_of
            .clone()
            .or_else(|| self.active.clone())
        else {
            return;
        };
        let text = self.transcript_for_title(cx);
        if !text.is_empty() {
            self.title_transcripts.insert(id, text);
        }
    }

    fn maybe_start_pending_title(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.pending_title.clone() else {
            return;
        };
        if self.active.as_deref() != Some(id.as_str()) {
            return;
        }
        if !self.can_silent_prompt() {
            return;
        }
        if self.transcript_for_title(cx).is_empty() {
            return;
        }
        self.pending_title = None;
        let _ = self.request_chat_title(id, cx);
    }

    fn transcript_for_title(&self, cx: &App) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let line = match entry {
                Entry::User { text, .. } => format!("User: {text}"),
                Entry::Agent(state) => {
                    format!("Assistant: {}", state.read(cx).rendered_text().source())
                }
                _ => continue,
            };
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&line);
            if out.chars().count() > 6_000 {
                break;
            }
        }
        out
    }

    fn finish_silent(&mut self, ok: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.silent.take() else {
            return;
        };
        match pending {
            SilentTurn::Commit { root, text } => {
                if ok {
                    let message = crate::git::scrub_commit_message(&text);
                    if message.is_empty() {
                        cx.emit(AgentPanelEvent::CommitSuggestFailed { root });
                    } else {
                        cx.emit(AgentPanelEvent::CommitSuggestReady { root, message });
                    }
                } else {
                    cx.emit(AgentPanelEvent::CommitSuggestFailed { root });
                }
            }
            SilentTurn::Title {
                chat_id,
                text,
                notify,
            } => {
                if ok {
                    let title = scrub_chat_title(&text);
                    if !title.is_empty() {
                        self.rename_chat(&chat_id, &title, cx);
                    } else if notify {
                        self.push_notice("Could not generate a title.", true, cx);
                    }
                } else if notify {
                    self.push_notice("Title generation stopped.", false, cx);
                }
            }
            SilentTurn::Improve { text } => {
                if ok {
                    let improved = scrub_improved_prompt(&text);
                    if improved.is_empty() {
                        self.push_notice("Could not improve the prompt.", true, cx);
                    } else {
                        self.pending_improve = Some(improved);
                    }
                } else {
                    self.push_notice("Prompt improve stopped.", false, cx);
                }
            }
        }
    }

    /// Apply a finished Improve rewrite (needs `Window` for Textarea `set_value`).
    ///
    /// Paperclip chips live outside the Textarea and are untouched. Inline `@`
    /// tokens are snapshotted before `set_value` and spliced back in front of
    /// the rewritten prose (Improve only rewrites plain text).
    fn flush_pending_improve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.pending_improve.take() else {
            return;
        };
        self.mention = None;
        let tokens: Vec<InlineToken> = self
            .input
            .read(cx)
            .tokens()
            .iter()
            .map(|s| s.token().clone())
            .collect();
        let content = improved_composer_content(&text, &tokens);
        let caret = content.text().len();
        self.input.update(cx, |input, cx| {
            input.set_value(content, window, cx);
            input.set_selected_range(caret..caret, cx);
        });
        self.focus_input(window, cx);
        cx.notify();
    }

    /// Silent rewrite of the composer draft into plain text (`@` chips +
    /// paperclip attachments kept).
    fn request_improve_prompt(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.can_silent_prompt() {
            return false;
        }
        let content = self.input.read(cx).content();
        let tokens = content.tokens();
        let draft = text_without_tokens(content.text(), tokens);
        if draft.is_empty() {
            return false;
        }
        self.start_silent(
            build_improve_prompt(&draft),
            SilentTurn::Improve {
                text: String::new(),
            },
            cx,
        )
    }

    /// Abort an in-flight silent turn (tools / permissions / disconnect).
    fn abort_silent(&mut self, cx: &mut Context<Self>) {
        if self.silent.is_none() {
            return;
        }
        if let Some(connection) = &self.connection {
            connection.cancel();
        }
        self.finish_silent(false, cx);
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
        if self.silent.is_some() {
            self.finish_silent(false, cx);
        }
        self.connection = None;
        self.session = None;
        self.opening = None;
        self.opens_in_flight = 0;
        self.generation = self.generation.wrapping_add(1);
        self.turn_running = false;
        self.outbound.clear();
        self.pending_title = None;
        // Keep modes / config_options so the composer row does not vanish
        // (layout shift) while reconnecting; SessionOpened replaces them.
        self.compacting = false;
        // Keep usage_by_chat; live `usage` restored on reopen from that map.
        self.usage = None;
        self.emit_promptability_changed(cx);
        self.cancel_pending_permissions(cx);
    }

    /// Mode / Model / … stay visible while Opening, but must not change.
    fn config_controls_locked(&self) -> bool {
        self.turn_running || !matches!(self.status, Status::Ready)
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
            self.cache_active_transcript(cx);
            self.clear_entries(cx);
            self.transcript_of = None;
        }
        // Replayed updates carry the chat's id before the open completes.
        self.session = match &open {
            SessionOpen::Load(id) | SessionOpen::Resume(id) => Some(id.clone()),
            SessionOpen::New => None,
        };
        self.compacting = false;
        self.usage = self
            .session
            .as_ref()
            .and_then(|id| self.usage_by_chat.get(id).copied());
        if self.usage.is_none() {
            self.ensure_usage_placeholder();
        }
        if let Some(connection) = &self.connection {
            connection.open(open.clone());
            self.opens_in_flight += 1;
        }
        self.opening = Some(open);
        self.status = Status::Opening;
        cx.notify();
    }

    /// Show at least `— / {size}` before the agent has sent `usage_update`.
    fn ensure_usage_placeholder(&mut self) {
        let size = estimate_context_window_size(&self.config_options);
        match &mut self.usage {
            Some(u) if u.used.is_some() => {}
            Some(u) => u.size = size,
            None => {
                self.usage = Some(ContextUsage { used: None, size });
            }
        }
    }

    /// Retry after a failure / login: reconnect, keeping the active chat.
    fn retry(&mut self, cx: &mut Context<Self>) {
        self.start(cx);
    }

    /// The login terminal exited: reconnect if we are still waiting on it.
    pub(crate) fn retry_after_login(&mut self, cx: &mut Context<Self>) {
        if matches!(self.status, Status::LoginRequired(_)) {
            self.retry(cx);
        }
    }

    /// Header Plus / `⌘N`·`Ctrl+N` in the Agent key context.
    pub(crate) fn new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.turn_running {
            return;
        }
        self.pending_title = None;
        self.cache_active_transcript(cx);
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
        if self.pending_title.as_deref() != Some(id.as_str()) {
            self.pending_title = None;
        }
        self.cache_active_transcript(cx);
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

    fn confirm_rename_chat(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .chats
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.title.clone())
            .unwrap_or_default();
        let can_generate = self.can_generate_title(&id, cx);
        let panel = cx.entity().downgrade();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        input.update(cx, |input, cx| input.focus(window, cx));
        window.open_dialog(cx, move |dialog, window, _| {
            let (panel_ok, panel_gen, panel_cancel) =
                (panel.clone(), panel.clone(), panel.clone());
            let (input_ok, input_view) = (input.clone(), input.clone());
            let (id_ok, id_gen) = (id.clone(), id.clone());
            center_dialog(
                dialog
                    .title("Rename chat")
                    .child(Input::new(&input_view).w_full().min_w(px(280.)))
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("dialog-cancel")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let _ = panel_cancel;
                                    }),
                            )
                            .child(
                                Button::new("dialog-generate")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Generate")
                                    .tooltip("Ask the agent for a title from this chat")
                                    .disabled(!can_generate)
                                    .when(can_generate, |b| {
                                        b.on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            let id = id_gen.clone();
                                            _ = panel_gen.update(cx, |this, cx| {
                                                if !this.begin_chat_title(id, window, cx) {
                                                    this.push_notice(
                                                        "Could not generate a title right now.",
                                                        true,
                                                        cx,
                                                    );
                                                }
                                            });
                                        })
                                    }),
                            )
                            .child(
                                Button::new("dialog-primary")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Rename")
                                    .on_click(move |_, window, cx| {
                                        let title = input_ok.read(cx).value().to_string();
                                        window.close_dialog(cx);
                                        let id = id_ok.clone();
                                        _ = panel_ok.update(cx, |this, cx| {
                                            this.rename_chat(&id, &title, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    /// Local title only (ACP has no rename). Empty → keep current.
    pub fn rename_chat(&mut self, id: &str, title: &str, cx: &mut Context<Self>) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        let Some(chat) = self.chats.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if chat.title == title {
            return;
        }
        chat.title = title.to_owned();
        chat.updated = now_secs();
        self.needs_auto_title.remove(id);
        cx.emit(AgentPanelEvent::ChatsChanged);
        cx.notify();
    }

    fn set_mode(&mut self, mode_id: String, cx: &mut Context<Self>) {
        let Some(modes) = &mut self.modes else {
            return;
        };
        if modes.current_mode_id.0.as_ref() == mode_id {
            return;
        }
        modes.current_mode_id = mode_id.clone().into();
        if let Some(connection) = &self.connection {
            connection.set_mode(mode_id.clone());
        }
        self.remember_pref(None, Some(mode_id), cx);
        cx.notify();
    }

    /// Record a user pick (config option or legacy mode) for future chats.
    fn remember_pref(
        &mut self,
        config: Option<(String, AgentPrefValue)>,
        mode: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let before = self.prefs.clone();
        if let Some((id, value)) = config {
            self.prefs.config.insert(id, value);
        }
        if mode.is_some() {
            self.prefs.mode = mode;
        }
        if self.prefs != before {
            cx.emit(AgentPanelEvent::PrefsChanged);
        }
    }

    /// Push remembered choices into the chat that just opened, model first
    /// (it can change which efforts / modes exist). Skips values the agent
    /// no longer offers; the agent's reply stays the source of truth.
    fn apply_prefs(&mut self, cx: &mut Context<Self>) {
        let Some(connection) = &self.connection else {
            return;
        };
        if let Some(options) = &mut self.config_options {
            let mut order: Vec<usize> = (0..options.len()).collect();
            order.sort_by_key(|&i| options[i].category != Some(SessionConfigOptionCategory::Model));
            for i in order {
                let opt = &mut options[i];
                let id = opt.id.to_string();
                let Some(want) = self.prefs.config.get(&id) else {
                    continue;
                };
                match (&mut opt.kind, want) {
                    (SessionConfigKind::Boolean(b), AgentPrefValue::Bool(on)) => {
                        if b.current_value != *on {
                            b.current_value = *on;
                            connection.set_config_bool(id, *on);
                        }
                    }
                    (SessionConfigKind::Select(_), AgentPrefValue::Value(value)) => {
                        let valid = select_from_option(opt).is_some_and(|s| {
                            s.current != *value && s.choices.iter().any(|(v, _)| v == value)
                        });
                        let is_mode = opt.category == Some(SessionConfigOptionCategory::Mode);
                        if valid && let SessionConfigKind::Select(select) = &mut opt.kind {
                            select.current_value = value.clone().into();
                            connection.set_config(id, value.clone());
                            // Keep legacy mode state in sync, as the picker does.
                            if is_mode && let Some(modes) = &mut self.modes {
                                modes.current_mode_id = value.clone().into();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if let (Some(modes), Some(want)) = (&mut self.modes, &self.prefs.mode)
            && modes.current_mode_id.0.as_ref() != want
            && modes
                .available_modes
                .iter()
                .any(|m| m.id.0.as_ref() == want)
        {
            modes.current_mode_id = want.clone().into();
            connection.set_mode(want.clone());
        }
        cx.notify();
    }

    fn set_config_value(&mut self, config_id: String, value: String, cx: &mut Context<Self>) {
        // Optimistic label update; agent reply / ConfigOptionUpdate is source of truth.
        if let Some(options) = &mut self.config_options
            && let Some(opt) = options.iter_mut().find(|o| o.id.0.as_ref() == config_id)
            && let SessionConfigKind::Select(select) = &mut opt.kind
        {
            select.current_value = value.clone().into();
        }
        if let Some(connection) = &self.connection {
            connection.set_config(config_id.clone(), value.clone());
        }
        self.remember_pref(Some((config_id, AgentPrefValue::Value(value))), None, cx);
        cx.notify();
    }

    fn set_config_bool(&mut self, config_id: String, value: bool, cx: &mut Context<Self>) {
        if let Some(options) = &mut self.config_options
            && let Some(opt) = options.iter_mut().find(|o| o.id.0.as_ref() == config_id)
            && let SessionConfigKind::Boolean(b) = &mut opt.kind
        {
            b.current_value = value;
        }
        if let Some(connection) = &self.connection {
            connection.set_config_bool(config_id.clone(), value);
        }
        self.remember_pref(Some((config_id, AgentPrefValue::Bool(value))), None, cx);
        cx.notify();
    }

    fn set_fast_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(opt) = find_fast_option(&self.config_options) else {
            return;
        };
        let config_id = opt.id.to_string();
        match &opt.kind {
            SessionConfigKind::Boolean(_) => self.set_config_bool(config_id, on, cx),
            SessionConfigKind::Select(_) => {
                let Some(value) = fast_select_value(opt, on) else {
                    return;
                };
                self.set_config_value(config_id, value, cx);
            }
            _ => {}
        }
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
        if !matches!(self.status, Status::Ready) {
            return;
        }
        let content = self.input.read(cx).content();
        let tokens = content.tokens().to_vec();
        let text = text_without_tokens(content.text(), &tokens);
        if text.is_empty() && self.attachments.is_empty() && tokens.is_empty() {
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        if self.connection.is_none() {
            return;
        }
        let attachments = std::mem::take(&mut self.attachments);
        let thumbs = std::mem::take(&mut self.attachment_thumbs);
        self.mention = None;
        let caps = self.prompt_caps();
        // Inline `@` tokens in the prompt → ACP blocks. Paperclip chips stay
        // on the attach strip. Also honor any leftover typed `@rel`.
        let mut mentioned: Vec<_> = tokens
            .iter()
            .filter_map(|s| prompt::attachment_from_path(Path::new(s.token().id().as_ref())))
            .collect();
        for path in mention::paths_in_text(&self.roots, content.text()) {
            if let Some(a) = prompt::attachment_from_path(&path) {
                mentioned.push(a);
            }
        }
        let for_wire = prompt::merge_attachments(attachments.clone(), mentioned);
        let blocks = prompt::build_blocks(&text, &for_wire, caps);
        if blocks.is_empty() {
            self.attachments = attachments;
            self.attachment_thumbs = thumbs;
            return;
        }
        let title = if text.is_empty() {
            display_user_message(&text, &for_wire)
        } else {
            text.clone()
        };
        let mut refs = user_refs_from_mention_tokens(&tokens);
        refs.extend(user_refs_from(&attachments, &thumbs));
        self.input.update(cx, |input, cx| input.set_value("", window, cx));

        // Busy user turn: queue without a transcript bubble yet so the current
        // reply stays contiguous; drain pushes User + prompts. Silent jobs use
        // a throwaway session — home Send is allowed concurrently.
        if self.turn_running {
            self.outbound.push_back(QueuedPrompt {
                title,
                text,
                refs,
                blocks,
            });
            cx.notify();
            return;
        }

        let Some(connection) = &self.connection else {
            return;
        };
        connection.prompt(blocks);
        self.turn_running = true;
        self.begin_turn_file_tracking();
        self.touch_chat(&session, Some(&title));
        cx.emit(AgentPanelEvent::ChatsChanged);
        self.push_entry(
            Entry::User {
                text: text.into(),
                refs,
            },
            cx,
        );
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        cx.notify();
    }

    /// How many prompts are waiting for the current turn (tests).
    #[cfg(test)]
    pub fn outbound_queue_len(&self) -> usize {
        self.outbound.len()
    }

    /// Preview lines for the outbound queue strip (tests).
    #[cfg(test)]
    pub fn outbound_previews(&self) -> Vec<String> {
        self.outbound.iter().map(queued_preview).collect()
    }

    /// Drop a queued prompt by index (composer queue strip).
    fn remove_outbound(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.outbound.len() {
            self.outbound.remove(index);
            cx.notify();
        }
    }

    /// Remove a queued prompt from tests (same path as the strip `CircleX`).
    #[cfg(test)]
    pub fn remove_outbound_for_test(&mut self, index: usize, cx: &mut Context<Self>) {
        self.remove_outbound(index, cx);
    }

    /// Send the next queued prompt, if any and idle.
    fn drain_outbound_queue(&mut self, cx: &mut Context<Self>) {
        if !self.can_prompt() {
            return;
        }
        let Some(item) = self.outbound.pop_front() else {
            return;
        };
        let Some(connection) = &self.connection else {
            self.outbound.push_front(item);
            return;
        };
        let Some(session) = self.session.clone() else {
            self.outbound.push_front(item);
            return;
        };
        connection.prompt(item.blocks);
        self.turn_running = true;
        self.begin_turn_file_tracking();
        self.touch_chat(&session, Some(&item.title));
        cx.emit(AgentPanelEvent::ChatsChanged);
        self.push_entry(
            Entry::User {
                text: item.text.into(),
                refs: item.refs,
            },
            cx,
        );
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        cx.notify();
    }

    /// Clipboard paste into the composer: images and dropped file paths become chips.
    fn handle_composer_paste(
        &mut self,
        item: &gpui_kit::ClipboardItem,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut consumed = false;
        for entry in item.entries() {
            match entry {
                gpui_kit::ClipboardEntry::Image(image) => {
                    let mime = image.format.mime_type();
                    let label = format!("paste.{}", image.format.extension());
                    self.attach_context(image_from_bytes(mime, &image.bytes, label), cx);
                    consumed = true;
                }
                gpui_kit::ClipboardEntry::ExternalPaths(paths) => {
                    for path in paths.paths() {
                        if let Some(image) = image_from_path(path) {
                            self.attach_context(image, cx);
                            consumed = true;
                        } else if path.is_file() {
                            let text = std::fs::read_to_string(path).ok();
                            self.attach_context(
                                Attachment::File {
                                    path: path.to_path_buf(),
                                    text,
                                    selection: None,
                                },
                                cx,
                            );
                            consumed = true;
                        } else if path.is_dir() {
                            self.attach_context(
                                Attachment::File {
                                    path: path.to_path_buf(),
                                    text: None,
                                    selection: None,
                                },
                                cx,
                            );
                            consumed = true;
                        }
                    }
                }
                _ => {}
            }
        }
        consumed
    }

    /// Move `id` to the front of the index (adding it on its first prompt).
    fn touch_chat(&mut self, id: &str, first_prompt: Option<&str>) {
        let mut chat = match self.chats.iter().position(|c| c.id == id) {
            Some(ix) => self.chats.remove(ix),
            None => {
                // Placeholder until the first turn finishes → auto title.
                self.needs_auto_title.insert(id.to_owned());
                AgentChat {
                    id: id.to_owned(),
                    title: chat_title(first_prompt.unwrap_or("")),
                    ..Default::default()
                }
            }
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

    /// Open-chat turn ended (not a throwaway silent job).
    fn home_turn_finished(&mut self, ok: bool, cx: &mut Context<Self>) {
        self.turn_running = false;
        self.flush_turn_file_changes(cx);
        if self.reconnect_after_turn && !self.suspend_after_turn && self.silent.is_none() {
            // Reconnect drops the connection (and the outbound queue).
            self.reconnect_after_turn = false;
            self.start(cx);
        } else if self.suspend_after_turn && self.silent.is_none() {
            self.suspend_after_turn = false;
            self.disconnect(cx);
            self.status = Status::Suspended;
        } else {
            self.cache_active_transcript(cx);
            self.drain_outbound_queue(cx);
            if ok {
                self.maybe_auto_title_after_turn(cx);
            }
        }
        self.emit_promptability_changed(cx);
    }

    /// Throwaway silent job ended (Improve / title / Suggest).
    fn silent_job_finished(&mut self, ok: bool, cx: &mut Context<Self>) {
        // Resolve before reconnect/suspend so Git still gets a Suggest result.
        self.finish_silent(ok, cx);
        if self.reconnect_after_turn && !self.suspend_after_turn && !self.turn_running {
            self.reconnect_after_turn = false;
            self.start(cx);
        } else if self.suspend_after_turn && !self.turn_running {
            self.suspend_after_turn = false;
            self.disconnect(cx);
            self.status = Status::Suspended;
        } else if !self.turn_running {
            self.drain_outbound_queue(cx);
        }
        self.emit_promptability_changed(cx);
    }

    /// Clear prior turn file state; snapshot git dirties for shell-edit detection.
    fn begin_turn_file_tracking(&mut self) {
        self.pending_file_change_restore = false;
        self.turn_file_changes.clear();
        self.turn_git_baseline = Some(snapshot_dirty_contents(&self.roots));
    }

    /// Record a successful `fs/write_text_file` for the turn summary card.
    pub fn note_file_write(
        &mut self,
        path: PathBuf,
        before: String,
        after: String,
        cx: &mut Context<Self>,
    ) {
        if !self.tracking_file_changes() {
            return;
        }
        match self.turn_file_changes.get_mut(&path) {
            Some(pending) => {
                pending.latest = after;
                pending.deleted = false;
            }
            None => {
                self.turn_file_changes.insert(
                    path.clone(),
                    PendingFileChange {
                        path,
                        baseline: before,
                        latest: after,
                        deleted: false,
                    },
                );
            }
        }
        cx.notify();
    }

    /// Snapshot baseline early so a completed delete still has line counts.
    fn ensure_turn_file_baseline(&mut self, path: &Path) {
        if !self.tracking_file_changes() || self.turn_file_changes.contains_key(path) {
            return;
        }
        let baseline = std::fs::read_to_string(path).unwrap_or_default();
        self.turn_file_changes.insert(
            path.to_path_buf(),
            PendingFileChange {
                path: path.to_path_buf(),
                baseline: baseline.clone(),
                latest: baseline,
                deleted: false,
            },
        );
    }

    fn note_file_delete(&mut self, path: PathBuf) {
        if !self.tracking_file_changes() {
            return;
        }
        self.ensure_turn_file_baseline(&path);
        if let Some(pending) = self.turn_file_changes.get_mut(&path) {
            pending.deleted = true;
            pending.latest.clear();
        }
    }

    fn track_tool_file_change(
        &mut self,
        kind: ToolKind,
        status: ToolCallStatus,
        location: Option<&Path>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = location else {
            return;
        };
        match kind {
            // Claude often edits on disk and only notifies via ToolCall (no
            // `fs/write_text_file`) — snapshot early, re-read on Completed.
            ToolKind::Edit | ToolKind::Move => {
                self.ensure_turn_file_baseline(path);
                if matches!(status, ToolCallStatus::Completed) {
                    self.refresh_turn_file_latest(path);
                    cx.emit(AgentPanelEvent::WorkspaceTouched);
                }
            }
            ToolKind::Delete => {
                self.ensure_turn_file_baseline(path);
                if matches!(status, ToolCallStatus::Completed) {
                    self.note_file_delete(path.to_path_buf());
                    cx.emit(AgentPanelEvent::WorkspaceTouched);
                }
            }
            _ => {}
        }
    }

    /// After an Edit/Move tool finishes, pull the on-disk text for +/- counts.
    fn refresh_turn_file_latest(&mut self, path: &Path) {
        if !self.tracking_file_changes() {
            return;
        }
        let latest = std::fs::read_to_string(path).unwrap_or_default();
        if let Some(pending) = self.turn_file_changes.get_mut(path) {
            pending.latest = latest;
            pending.deleted = false;
        }
    }

    fn flush_turn_file_changes(&mut self, cx: &mut Context<Self>) {
        // Shell create/edit/delete never sends WriteFile / Edit — merge git delta.
        if let Some(baseline) = self.turn_git_baseline.take() {
            for delta in worktree_deltas_since_turn(&self.roots, &baseline) {
                let deleted = delta.after.is_empty() && !delta.before.is_empty();
                self.turn_file_changes.insert(
                    delta.path.clone(),
                    PendingFileChange {
                        path: delta.path,
                        baseline: delta.before,
                        latest: delta.after,
                        deleted,
                    },
                );
            }
        }
        let pending = std::mem::take(&mut self.turn_file_changes);
        if pending.is_empty() {
            return;
        }
        let mut files: Vec<FileChangeRow> = pending.into_values().map(|p| p.row()).collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        // Drop no-ops (write same content, or delete already-empty).
        files.retain(|f| f.added > 0 || f.removed > 0 || f.deleted);
        if files.is_empty() {
            return;
        }
        // User count already includes this turn's bubble.
        let turn = self
            .entries
            .iter()
            .filter(|e| matches!(e, Entry::User { .. }))
            .count()
            .saturating_sub(1) as u32;
        self.remember_file_changes(turn, &files, cx);
        self.push_entry(
            Entry::FileChanges {
                files,
                expanded: false,
            },
            cx,
        );
        // Shell edits never went through WriteFile — refresh Source Control.
        cx.emit(AgentPanelEvent::WorkspaceTouched);
    }

    /// Persist the card on the chat index so it survives `session/load` / restart.
    fn remember_file_changes(
        &mut self,
        turn: u32,
        files: &[FileChangeRow],
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.session.clone() else {
            return;
        };
        let Some(chat) = self.chats.iter_mut().find(|c| c.id == id) else {
            return;
        };
        chat.file_changes.retain(|c| c.turn != turn);
        chat.file_changes.push(AgentFileChangesCard {
            turn,
            files: files.iter().map(FileChangeRow::stored).collect(),
        });
        chat.file_changes.sort_by_key(|c| c.turn);
        cx.emit(AgentPanelEvent::ChatsChanged);
    }

    /// Splice stored **N Files** cards into the current transcript.
    ///
    /// Safe to call repeatedly while history is still streaming: strips any
    /// prior splice and rebuilds from `AgentChat.file_changes`.
    fn restore_file_change_cards(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.session.clone() else {
            return;
        };
        let Some(chat) = self.chats.iter().find(|c| c.id == id) else {
            return;
        };
        if chat.file_changes.is_empty() {
            self.pending_file_change_restore = false;
            return;
        }
        let max_turn = chat
            .file_changes
            .iter()
            .map(|c| c.turn)
            .max()
            .unwrap_or(0);
        let mut by_turn: HashMap<u32, Vec<FileChangeRow>> = HashMap::new();
        for card in &chat.file_changes {
            by_turn.insert(
                card.turn,
                card.files
                    .iter()
                    .cloned()
                    .map(FileChangeRow::from_stored)
                    .collect(),
            );
        }
        // Drop a partial splice from an earlier attempt (SessionOpened raced).
        let old: Vec<_> = self
            .entries
            .drain(..)
            .filter(|e| !matches!(e, Entry::FileChanges { .. }))
            .collect();
        let mut out = Vec::with_capacity(old.len() + by_turn.len());
        let mut user_turn = 0u32;
        for entry in old {
            if matches!(entry, Entry::User { .. }) {
                if user_turn > 0
                    && let Some(files) = by_turn.remove(&(user_turn - 1))
                {
                    out.push(Entry::FileChanges {
                        files,
                        expanded: false,
                    });
                }
                out.push(entry);
                user_turn += 1;
            } else {
                out.push(entry);
            }
        }
        if user_turn > 0
            && let Some(files) = by_turn.remove(&(user_turn - 1))
        {
            out.push(Entry::FileChanges {
                files,
                expanded: false,
            });
        }
        self.entries = out;
        // Card for turn N needs N+1 user bubbles before we know the turn ended.
        if user_turn >= max_turn.saturating_add(1) {
            self.pending_file_change_restore = false;
        }
        cx.notify();
    }

    /// Replay must not snapshot today's disk into a historical turn.
    fn tracking_file_changes(&self) -> bool {
        self.silent.is_none()
            && !matches!(self.opening, Some(SessionOpen::Load(_)))
            && !self.pending_file_change_restore
    }

    fn toggle_file_changes(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(Entry::FileChanges { expanded, .. }) = self.entries.get_mut(ix) {
            *expanded = !*expanded;
            self.touched(ix, cx);
            cx.notify();
        }
    }

    /// Latest non-empty **N Files** card (sticky strip above the composer).
    fn latest_file_changes(&self) -> Option<(usize, &[FileChangeRow], bool)> {
        self.entries.iter().enumerate().rev().find_map(|(ix, e)| match e {
            Entry::FileChanges { files, expanded } if !files.is_empty() => {
                Some((ix, files.as_slice(), *expanded))
            }
            _ => None,
        })
    }

    /// Source Control discarded these paths — drop them from **N Files** cards.
    pub fn note_discarded_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let drop = |p: &Path| paths.iter().any(|d| d == p);
        self.prune_file_change_paths(drop, cx);
    }

    /// Discard All in a repo — drop every card row under that root.
    pub fn note_discarded_repo(&mut self, root: &Path, cx: &mut Context<Self>) {
        let drop = |p: &Path| p.starts_with(root);
        self.prune_file_change_paths(drop, cx);
    }

    fn prune_file_change_paths(
        &mut self,
        mut drop: impl FnMut(&Path) -> bool,
        cx: &mut Context<Self>,
    ) {
        for entry in &mut self.entries {
            if let Entry::FileChanges { files, .. } = entry {
                files.retain(|f| !drop(&f.path));
            }
        }
        for chat in &mut self.chats {
            for card in &mut chat.file_changes {
                card.files.retain(|f| !drop(&f.path));
            }
            chat.file_changes.retain(|c| !c.files.is_empty());
        }
        cx.emit(AgentPanelEvent::ChatsChanged);
        cx.notify();
    }

    /// Pinned on top of the composer (input) so it stays reachable while chatting.
    fn render_sticky_file_changes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (ix, files, expanded) = self.latest_file_changes()?;
        let panel = cx.entity().downgrade();
        Some(render_file_changes(&panel, ix, files, expanded, cx))
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
            AgentEvent::SessionOpened {
                id,
                modes,
                config_options,
            } => {
                let was_load = matches!(self.opening, Some(SessionOpen::Load(_)));
                self.session = Some(id.clone());
                self.transcript_of = Some(id.clone());
                // A brand-new chat is active but stays out of the index (and
                // so is not reopened) until its first prompt.
                self.active = Some(id.clone());
                self.opening = None;
                self.status = Status::Ready;
                // Keep prior pickers if this open omits options (avoids the
                // Mode/Model row vanishing for a frame, then popping back).
                if let Some(modes) = modes {
                    self.modes = Some(modes);
                }
                if let Some(opts) = config_options {
                    self.config_options = Some(opts);
                }
                self.apply_prefs(cx);
                self.usage = self.usage_by_chat.get(&id).copied();
                // Claude only sends usage_update mid-turn — seed size so the
                // header is not blank on open / switch.
                self.ensure_usage_placeholder();
                // ACP replay has no file-change cards — splice ours back in.
                // History `Update`s can still be in the queue *after* this
                // event on cold start, so keep retrying until all turns land.
                if was_load {
                    self.turn_file_changes.clear();
                    self.turn_git_baseline = None;
                    self.pending_file_change_restore = true;
                    self.restore_file_change_cards(cx);
                }
                self.maybe_start_pending_title(cx);
                self.emit_promptability_changed(cx);
            }
            AgentEvent::ConfigOptions(options) => {
                self.config_options = Some(options);
                self.ensure_usage_placeholder();
            }
            AgentEvent::SessionOpenFailed { open, message } => {
                self.opening = None;
                self.pending_title = None;
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
                    if self.pending_file_change_restore {
                        self.restore_file_change_cards(cx);
                    }
                    self.maybe_start_pending_title(cx);
                }
            }
            AgentEvent::TurnEnded(reason) => {
                // Home chat only — silent jobs end via SilentTurnEnded.
                let ok = matches!(reason, StopReason::EndTurn);
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
                self.home_turn_finished(ok, cx);
            }
            AgentEvent::SilentTurnEnded { ok, text } => {
                if let Some(pending) = &mut self.silent {
                    *pending.text_mut() = text;
                }
                self.silent_job_finished(ok, cx);
            }
            AgentEvent::TurnFailed(message) => {
                self.push_notice(message, true, cx);
                self.home_turn_finished(false, cx);
            }
            AgentEvent::AuthRequired(methods) => {
                self.disconnect(cx);
                self.status = Status::LoginRequired(methods);
            }
            AgentEvent::Permission(ask) => {
                // Silent-session tool asks are cancelled in the ACP client by
                // session id; anything that reaches here is for the open chat.
                self.ask_permission(ask, cx);
                cx.emit(AgentPanelEvent::PermissionRequested);
            }
            AgentEvent::ReadFile(read) => cx.emit(AgentPanelEvent::ReadFile(read)),
            AgentEvent::WriteFile(write) => cx.emit(AgentPanelEvent::WriteFile(write)),
            AgentEvent::Closed(error) => {
                if self.silent.is_some() {
                    self.finish_silent(false, cx);
                }
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
                    if let Some(pending) = &mut self.silent {
                        pending.text_mut().push_str(&text);
                    } else {
                        self.append_text(false, &text, cx);
                    }
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                // Thoughts are not the commit message — ignore during Suggest.
                if self.silent.is_some() {
                    return;
                }
                if let Some(text) = block_text(&chunk.content) {
                    self.append_text(true, &text, cx);
                }
            }
            SessionUpdate::ToolCall(_) | SessionUpdate::ToolCallUpdate(_)
                if self.silent.is_some() =>
            {
                self.abort_silent(cx);
            }
            SessionUpdate::ToolCall(call) => self.upsert_tool_call(call, cx),
            SessionUpdate::ToolCallUpdate(update) => self.update_tool_call(update, cx),
            SessionUpdate::Plan(_) if self.silent.is_some() => {}
            SessionUpdate::Plan(plan) => {
                if let Some(ix) = self.entries.iter().rposition(|e| matches!(e, Entry::Plan(_))) {
                    self.entries[ix] = Entry::Plan(plan.entries);
                    self.touched(ix, cx);
                } else {
                    self.push_entry(Entry::Plan(plan.entries), cx);
                }
            }
            // Only arrives while `session/load` replays history.
            SessionUpdate::UserMessageChunk(_) if self.silent.is_some() => {}
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
            SessionUpdate::CurrentModeUpdate(update) => {
                if let Some(modes) = &mut self.modes {
                    modes.current_mode_id = update.current_mode_id;
                }
            }
            SessionUpdate::ConfigOptionUpdate(update) => {
                self.config_options = Some(update.config_options);
                // Model / context size may have changed before the next turn.
                self.ensure_usage_placeholder();
            }
            SessionUpdate::UsageUpdate(update) => {
                let snap = ContextUsage {
                    used: Some(update.used),
                    size: update.size,
                };
                self.usage = Some(snap);
                if let Some(id) = self.session.clone() {
                    self.usage_by_chat.insert(id, snap);
                }
                cx.notify();
            }
            SessionUpdate::CompactionUpdate(update) if self.silent.is_some() => {
                self.compacting = matches!(update.status, CompactionStatus::InProgress);
            }
            SessionUpdate::CompactionUpdate(update) => {
                match update.status {
                    CompactionStatus::InProgress => {
                        self.compacting = true;
                    }
                    CompactionStatus::Completed => {
                        self.compacting = false;
                        self.push_notice("Context compacted.", false, cx);
                    }
                    CompactionStatus::Failed => {
                        self.compacting = false;
                        self.push_notice("Context compaction failed.", true, cx);
                    }
                    CompactionStatus::Cancelled => {
                        self.compacting = false;
                    }
                    _ => {}
                }
                cx.notify();
            }
            // Summary chunks / notices / other metadata the panel does not show.
            _ => {}
        }
    }

    /// Replayed user chunks merge into one bubble.
    fn append_user(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.entries.len().checked_sub(1)
            && let Entry::User {
                text: existing,
                refs,
            } = &self.entries[ix]
        {
            let refs = refs.clone();
            self.entries[ix] = Entry::User {
                text: format!("{existing}{text}").into(),
                refs,
            };
            self.touched(ix, cx);
        } else {
            self.push_entry(
                Entry::User {
                    text: text.to_owned().into(),
                    refs: Vec::new(),
                },
                cx,
            );
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
            let tracked = if let Entry::Tool(tool) = &mut self.entries[ix] {
                tool.title = call.title.into();
                tool.kind = call.kind;
                tool.status = call.status;
                tool.location = location.or(tool.location.take());
                Some((tool.kind, tool.status, tool.location.clone()))
            } else {
                None
            };
            if let Some((kind, status, loc)) = tracked {
                self.track_tool_file_change(kind, status, loc.as_deref(), cx);
            }
            self.touched(ix, cx);
            return;
        }
        let tool = ToolEntry {
            id: call.tool_call_id,
            title: call.title.into(),
            kind: call.kind,
            status: call.status,
            location,
            permission: None,
        };
        self.track_tool_file_change(tool.kind, tool.status, tool.location.as_deref(), cx);
        self.push_entry(Entry::Tool(tool), cx);
    }

    fn update_tool_call(&mut self, update: ToolCallUpdate, cx: &mut Context<Self>) {
        let Some(ix) = self.tool_index(&update.tool_call_id) else {
            // Updates for unseen calls still deserve a row.
            let fields = update.fields;
            let tool = ToolEntry {
                id: update.tool_call_id,
                title: fields.title.unwrap_or_else(|| "Tool call".into()).into(),
                kind: fields.kind.unwrap_or(ToolKind::Other),
                status: fields.status.unwrap_or(ToolCallStatus::Pending),
                location: fields
                    .locations
                    .and_then(|l| l.into_iter().next())
                    .map(|l| l.path),
                permission: None,
            };
            self.track_tool_file_change(tool.kind, tool.status, tool.location.as_deref(), cx);
            self.push_entry(Entry::Tool(tool), cx);
            return;
        };
        let tracked = if let Entry::Tool(tool) = &mut self.entries[ix] {
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
            Some((tool.kind, tool.status, tool.location.clone()))
        } else {
            None
        };
        if let Some((kind, status, loc)) = tracked {
            self.track_tool_file_change(kind, status, loc.as_deref(), cx);
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
        self.pending_file_change_restore = false;
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

fn build_chat_title_prompt(transcript: &str) -> String {
    format!(
        "Write a short chat title for this conversation.\n\n\
         Rules:\n\
         - Reply with ONLY the title (a few words).\n\
         - No quotes, no markdown fences, no preamble or explanation.\n\
         - Do not use tools.\n\n\
         Transcript:\n{transcript}\n"
    )
}

fn build_improve_prompt(draft: &str) -> String {
    format!(
        "Rewrite this user message into a clearer prompt for a coding agent.\n\n\
         Rules:\n\
         - Fix spelling and grammar; expand lazy shorthand when the intent is clear.\n\
         - Keep the original intent and scope — do not invent files, requirements, or constraints.\n\
         - Reply with ONLY the improved prompt text.\n\
         - No quotes, no markdown fences, no preamble or explanation.\n\
         - Do not use tools.\n\n\
         Draft:\n{draft}\n"
    )
}

/// Rebuild composer content after Improve: keep `@` tokens, replace prose.
///
/// Tokens are placed before the rewritten text (Improve never sees them).
fn improved_composer_content(improved: &str, tokens: &[InlineToken]) -> InputContent {
    if tokens.is_empty() {
        return InputContent::new(improved);
    }
    let mut text = String::new();
    let mut spans = Vec::with_capacity(tokens.len());
    for token in tokens {
        if !text.is_empty() {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(token.text());
        spans.push((start..text.len(), token.clone()));
    }
    if !improved.is_empty() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(improved);
    }
    let mut content = InputContent::new(text);
    for (range, token) in spans {
        // Ranges were built from `token.text()`; mismatch would be a bug.
        content = content
            .with_token(range, token)
            .expect("improved_composer_content: token text matches range");
    }
    content
}

/// First line, short — same ceiling as the first-prompt fallback title.
fn scrub_chat_title(raw: &str) -> String {
    let scrubbed = crate::git::scrub_commit_message(raw);
    let line = scrubbed.lines().next().unwrap_or("").trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}

/// Strip fences / quotes / “here’s an improved…” wrappers from an Improve reply.
fn scrub_improved_prompt(raw: &str) -> String {
    let mut s = crate::git::scrub_commit_message(raw);
    const LABELS: &[&str] = &[
        "Improved prompt:",
        "improved prompt:",
        "Improved:",
        "improved:",
        "Here's the improved prompt:",
        "Here's an improved prompt:",
        "Here is the improved prompt:",
        "Here is an improved prompt:",
    ];
    for label in LABELS {
        if let Some(rest) = s.strip_prefix(label) {
            s = rest.trim().to_owned();
            break;
        }
    }
    s.trim().to_owned()
}

#[derive(Clone)]
struct ConfigSelect {
    config_id: String,
    name: String,
    current: String,
    choices: Vec<(String, String)>,
}

/// One Model select choice after parsing family / version / context.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelChoice {
    family: String,
    id: String,
    version: Option<String>,
    /// Normalized context label (`1M`, `200K`, …) when the name/id carries one.
    context: Option<String>,
}

/// Model option split into family + version (+ context) when names parse that way.
#[derive(Clone)]
struct ModelFamilySplit {
    config_id: String,
    choices: Vec<ModelChoice>,
    family: String,
    version_label: Option<String>,
    /// Display label for the current context (`1M` / `200K`), when applicable.
    context_label: Option<String>,
}

/// If `silent_model` is still offered, overwrite the Model select in `configs`.
fn apply_silent_model_override(
    configs: &mut [(String, SilentConfigValue)],
    model: Option<&ConfigSelect>,
    silent_model: Option<&str>,
) {
    let Some(want) = silent_model.filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(model) = model else {
        return;
    };
    if !model.choices.iter().any(|(id, _)| id == want) {
        return;
    }
    if let Some((_, value)) = configs.iter_mut().find(|(id, _)| *id == model.config_id) {
        *value = SilentConfigValue::Select(want.to_owned());
    }
}

/// Best Model option id for a family (+ optional version / context).
fn pick_silent_model_id(
    choices: &[ModelChoice],
    family: &str,
    version: Option<&str>,
    prefer_ctx: Option<&str>,
) -> Option<String> {
    let in_family: Vec<&ModelChoice> = choices.iter().filter(|c| c.family == family).collect();
    if in_family.is_empty() {
        return None;
    }
    if let Some(ver) = version {
        return in_family
            .iter()
            .copied()
            .filter(|c| c.version.as_deref() == Some(ver))
            .find(|c| display_context_label(choices, c).as_deref() == prefer_ctx)
            .or_else(|| {
                in_family
                    .iter()
                    .copied()
                    .find(|c| c.version.as_deref() == Some(ver))
            })
            .map(|c| c.id.clone());
    }
    in_family
        .iter()
        .copied()
        .find(|c| display_context_label(choices, c).as_deref() == prefer_ctx)
        .or_else(|| in_family.first().copied())
        .map(|c| c.id.clone())
}

fn find_select(
    options: &Option<Vec<SessionConfigOption>>,
    category: Option<SessionConfigOptionCategory>,
    id_fallback: &str,
) -> Option<ConfigSelect> {
    let options = options.as_ref()?;
    let opt = options.iter().find(|o| {
        category
            .as_ref()
            .is_some_and(|c| o.category.as_ref() == Some(c))
            || o.id.0.as_ref() == id_fallback
    })?;
    select_from_option(opt)
}

fn select_from_option(opt: &SessionConfigOption) -> Option<ConfigSelect> {
    let SessionConfigKind::Select(select) = &opt.kind else {
        return None;
    };
    let choices: Vec<(String, String)> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(opts) => opts
            .iter()
            .map(|o| (o.value.to_string(), o.name.clone()))
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|g| {
                g.options
                    .iter()
                    .map(|o| (o.value.to_string(), o.name.clone()))
            })
            .collect(),
        _ => Vec::new(),
    };
    if choices.is_empty() {
        return None;
    }
    Some(ConfigSelect {
        config_id: opt.id.to_string(),
        name: opt.name.clone(),
        current: select.current_value.to_string(),
        choices,
    })
}

/// `"1M"` / `"200k"` / `"[1m]"` → normalized label, or `None` if not a context token.
fn normalize_context_token(tok: &str) -> Option<String> {
    let clean = tok
        .trim()
        .trim_matches(|c| matches!(c, '[' | ']' | '(' | ')'));
    if clean.is_empty() {
        return None;
    }
    let lower = clean.to_ascii_lowercase();
    if lower == "1m" {
        return Some("1M".into());
    }
    if let Some(digits) = lower.strip_suffix('k')
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
    {
        return Some(format!("{digits}K"));
    }
    if let Some(digits) = lower.strip_suffix('m')
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
    {
        return Some(format!("{digits}M"));
    }
    None
}

/// Strip a trailing context token from a display name: `"Sonnet 4.6 1M"` →
/// (`"Sonnet 4.6"`, `Some("1M")`).
fn strip_context_suffix(display: &str) -> (String, Option<String>) {
    let trimmed = display.trim();
    let Some((head, last)) = trimmed.rsplit_once(char::is_whitespace) else {
        return (trimmed.to_owned(), None);
    };
    match normalize_context_token(last) {
        Some(label) => (head.trim_end().to_owned(), Some(label)),
        None => (trimmed.to_owned(), None),
    }
}

/// Context from a model value id (`…[1m]`, `…-1m`, `…[200k]`).
fn context_from_model_id(id: &str) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    if let Some(start) = lower.rfind('[')
        && let Some(end) = lower[start + 1..].find(']')
    {
        let inner = &lower[start + 1..start + 1 + end];
        if let Some(label) = normalize_context_token(inner) {
            return Some(label);
        }
    }
    for suffix in ["-1m", "_1m"] {
        if lower.ends_with(suffix) {
            return Some("1M".into());
        }
    }
    None
}

/// `"Sonnet 4.5"` → (`Sonnet`, `Some("4.5")`); `"Opus 5"` → (`Opus`, `Some("5")`).
fn parse_model_family_version(display: &str) -> (String, Option<String>) {
    let trimmed = display.trim();
    let without_claude = trimmed
        .strip_prefix("Claude ")
        .or_else(|| trimmed.strip_prefix("claude "))
        .unwrap_or(trimmed)
        .trim();
    // Trailing version: digits, optional .digits (not part of a longer token).
    let bytes = without_claude.as_bytes();
    let mut i = bytes.len();
    while i > 0 && (bytes[i - 1].is_ascii_digit() || bytes[i - 1] == b'.') {
        i -= 1;
    }
    if i == 0 || i == bytes.len() || !bytes[i - 1].is_ascii_whitespace() {
        return (without_claude.to_owned(), None);
    }
    let version = without_claude[i..].trim();
    let family = without_claude[..i].trim();
    if family.is_empty()
        || version.is_empty()
        || !version.chars().next().is_some_and(|c| c.is_ascii_digit())
        || version.ends_with('.')
    {
        return (without_claude.to_owned(), None);
    }
    (family.to_owned(), Some(version.to_owned()))
}

fn parse_model_choice(id: &str, name: &str) -> ModelChoice {
    let (base, ctx_name) = strip_context_suffix(name);
    let (family, version) = parse_model_family_version(&base);
    let context = ctx_name.or_else(|| context_from_model_id(id));
    ModelChoice {
        family,
        id: id.to_owned(),
        version,
        context,
    }
}

/// When ≥2 families have versions, expose family + version pickers.
fn model_family_split(options: &Option<Vec<SessionConfigOption>>) -> Option<ModelFamilySplit> {
    let model = find_select(options, Some(SessionConfigOptionCategory::Model), "model")?;
    let parsed: Vec<ModelChoice> = model
        .choices
        .iter()
        .map(|(id, name)| parse_model_choice(id, name))
        .collect();
    let versioned_families: std::collections::BTreeSet<&str> = parsed
        .iter()
        .filter(|c| c.version.is_some())
        .map(|c| c.family.as_str())
        .collect();
    if versioned_families.is_empty() {
        return None;
    }
    // Need at least one family with 2+ versions, or 2+ versioned families.
    let worth_splitting = versioned_families.len() >= 2
        || parsed.iter().any(|c| {
            c.version.is_some()
                && parsed
                    .iter()
                    .filter(|o| o.family == c.family && o.version.is_some())
                    .map(|o| o.version.as_deref())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    >= 2
        });
    if !worth_splitting {
        return None;
    }
    let current = parsed
        .iter()
        .find(|c| c.id == model.current)
        .cloned()
        .or_else(|| parsed.first().cloned())?;
    let context_label = display_context_label(&parsed, &current);
    Some(ModelFamilySplit {
        config_id: model.config_id,
        choices: parsed,
        family: current.family,
        version_label: current.version,
        context_label,
    })
}

/// Label shown for the current choice's context when siblings differ.
fn display_context_label(all: &[ModelChoice], current: &ModelChoice) -> Option<String> {
    let siblings: Vec<&ModelChoice> = all
        .iter()
        .filter(|c| c.family == current.family && c.version == current.version)
        .collect();
    if siblings.len() < 2 {
        return current.context.clone();
    }
    let has_explicit = siblings.iter().any(|c| c.context.is_some());
    if !has_explicit {
        return None;
    }
    Some(
        current
            .context
            .clone()
            .unwrap_or_else(|| "200K".into()),
    )
}

/// `(model_id, label)` context choices for the current family+version.
fn context_choices(split: &ModelFamilySplit) -> Vec<(String, String)> {
    let siblings: Vec<&ModelChoice> = split
        .choices
        .iter()
        .filter(|c| c.family == split.family && c.version == split.version_label)
        .collect();
    if siblings.len() < 2 || !siblings.iter().any(|c| c.context.is_some()) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for c in siblings {
        let label = c.context.clone().unwrap_or_else(|| "200K".into());
        if !out.iter().any(|(_, l)| l == &label) {
            out.push((c.id.clone(), label));
        }
    }
    if out.len() < 2 {
        return Vec::new();
    }
    out
}

#[cfg(test)]
fn context_choice_id(split: &ModelFamilySplit, context: &str) -> Option<String> {
    context_choices(split)
        .into_iter()
        .find(|(_, label)| label == context)
        .map(|(id, _)| id)
}

fn is_fast_option(opt: &SessionConfigOption) -> bool {
    let id = opt.id.0.as_ref().to_ascii_lowercase();
    let name = opt.name.to_ascii_lowercase();
    id.contains("fast") || name.contains("fast")
}

fn find_fast_option(options: &Option<Vec<SessionConfigOption>>) -> Option<&SessionConfigOption> {
    options.as_ref()?.iter().find(|o| is_fast_option(o))
}

fn fast_is_on(opt: &SessionConfigOption) -> bool {
    match &opt.kind {
        SessionConfigKind::Boolean(b) => b.current_value,
        SessionConfigKind::Select(select) => {
            let cur = select.current_value.to_string().to_ascii_lowercase();
            matches!(cur.as_str(), "on" | "true" | "yes" | "1")
        }
        _ => false,
    }
}

fn fast_select_value(opt: &SessionConfigOption, on: bool) -> Option<String> {
    let SessionConfigKind::Select(select) = &opt.kind else {
        return None;
    };
    let choices: Vec<(String, String)> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(opts) => opts
            .iter()
            .map(|o| (o.value.to_string(), o.name.to_ascii_lowercase()))
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|g| {
                g.options
                    .iter()
                    .map(|o| (o.value.to_string(), o.name.to_ascii_lowercase()))
            })
            .collect(),
        _ => Vec::new(),
    };
    let want = if on {
        ["on", "true", "yes", "1"]
    } else {
        ["off", "false", "no", "0"]
    };
    choices
        .iter()
        .find(|(id, name)| {
            let id_l = id.to_ascii_lowercase();
            want.iter()
                .any(|w| id_l == *w || name == w || name.contains(w))
        })
        .map(|(id, _)| id.clone())
}

/// One-line preview for a queued prompt in the composer strip.
fn queued_preview(item: &QueuedPrompt) -> String {
    let mut line = item
        .text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    if line.is_empty() {
        line = item.title.clone();
    }
    if !item.refs.is_empty() {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&format!("(+{})", item.refs.len()));
    }
    if line.is_empty() {
        line = "(attachment)".into();
    }
    let mut out: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        out.push('…');
    }
    out
}

/// Claude ACP still lists `auto` for every model; the SDK only supports it on
/// Claude 4.6+. Hide it in the picker when the current model is older.
/// ponytail: Claude-specific heuristic until ACP exposes `supportsAutoMode`.
fn claudeish_model_id(model_id: &str) -> bool {
    let l = model_id.to_ascii_lowercase();
    ["claude", "sonnet", "opus", "haiku", "fable"]
        .iter()
        .any(|p| l.contains(p))
}

/// `(major, minor)` from ids like `sonnet-4-5`, `claude-haiku-4-5-20251001`, `opus-5`.
fn parse_claude_model_version(model_id: &str) -> Option<(u32, u32)> {
    let l = model_id.to_ascii_lowercase();
    let mut nums = l
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok());
    let major = nums.next()?;
    // Date suffixes (`20251001`) are not a minor version.
    let minor = nums.next().filter(|&n| n < 100).unwrap_or(0);
    Some((major, minor))
}

fn model_supports_auto_mode(model_id: Option<&str>) -> bool {
    let Some(id) = model_id.filter(|s| !s.is_empty()) else {
        return true;
    };
    if !claudeish_model_id(id) {
        return true;
    }
    match parse_claude_model_version(id) {
        // Claude catalog: Auto permission mode requires 4.6+.
        Some((major, minor)) if major < 4 || (major == 4 && minor < 6) => false,
        _ => true,
    }
}

/// Drop modes the current model cannot use (today: Claude `auto` on &lt;4.6).
fn filter_mode_choices(
    choices: &[(String, String)],
    model_id: Option<&str>,
) -> Vec<(String, String)> {
    choices
        .iter()
        .filter(|(id, _)| id != "auto" || model_supports_auto_mode(model_id))
        .cloned()
        .collect()
}

/// Icon for the current session mode (Ask / Auto / Plan / accept edits / …).
fn mode_icon(id: &str, name: &str) -> LucideIcon {
    let blob = format!("{id} {name}").to_ascii_lowercase();
    if blob.contains("plan") {
        LucideIcon::ListTodo
    } else if blob.contains("auto") {
        LucideIcon::Sparkles
    } else if blob.contains("accept")
        || blob.contains("code")
        || (blob.contains("edit") && !blob.contains("ask"))
    {
        LucideIcon::Pencil
    } else if blob.contains("ask") {
        LucideIcon::ShieldQuestionMark
    } else {
        LucideIcon::Shield
    }
}

/// Lucide for an ACP config option that is not Mode/Model/Context/Fast.
fn config_option_icon(opt: &SessionConfigOption) -> LucideIcon {
    if matches!(
        opt.category,
        Some(SessionConfigOptionCategory::ThoughtLevel)
    ) {
        return LucideIcon::Brain;
    }
    let id = opt.id.0.as_ref().to_ascii_lowercase();
    let name = opt.name.to_ascii_lowercase();
    let blob = format!("{id} {name}");
    if blob.contains("agent")
        || blob.contains("persona")
        || blob.contains("subagent")
        || blob.contains("skill")
    {
        LucideIcon::Users
    } else if blob.contains("effort")
        || blob.contains("thought")
        || blob.contains("think")
        || blob.contains("reason")
    {
        LucideIcon::Brain
    } else if blob.contains("context") || blob.contains("window") {
        LucideIcon::Maximize2
    } else {
        LucideIcon::SlidersHorizontal
    }
}

/// Dedicated ACP Context / window-size select (not the Model list).
fn find_context_select(options: &Option<Vec<SessionConfigOption>>) -> Option<ConfigSelect> {
    let options = options.as_ref()?;
    let opt = options.iter().find(|o| {
        if is_fast_option(o) {
            return false;
        }
        let id = o.id.0.as_ref().to_ascii_lowercase();
        let name = o.name.to_ascii_lowercase();
        (id.contains("context") || name.contains("context") || name.contains("window"))
            && select_from_option(o).is_some_and(|s| {
                s.choices.iter().any(|(cid, cname)| {
                    normalize_context_token(cname).is_some()
                        || normalize_context_token(cid).is_some()
                        || context_from_model_id(cid).is_some()
                })
            })
    })?;
    select_from_option(opt)
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
        Entry::User { text, refs } => {
            let has_text = !text.trim().is_empty();
            Message::new()
                .alignment(MessageAlignment::End)
                .content(
                    MessageContent::new().bubble(
                        Bubble::new()
                            .with_variant(BubbleVariant::Secondary)
                            .child(
                                v_flex()
                                    .gap_1()
                                    .when(has_text, |col| {
                                        // Same selectable TextView as agent replies (plain `div`
                                        // text is not mouse-selectable in GPUI).
                                        col.child(
                                            div().text_sm().child(
                                                TextView::markdown(
                                                    SharedString::from(format!("agent-user-{ix}")),
                                                    text.clone(),
                                                )
                                                .selectable(true)
                                                .style(crate::theme::text_view_style(cx)),
                                            ),
                                        )
                                    })
                                    .when(!refs.is_empty(), |col| {
                                        col.child(
                                            h_flex().gap_1().flex_wrap().children(
                                                refs.iter().enumerate().map(|(rix, r)| {
                                                    // Images: thumb only (no Lucide, no "@name").
                                                    if let Some(thumb) = &r.thumb {
                                                        return render_image_thumb(
                                                            thumb,
                                                            SharedString::from(format!(
                                                                "agent-ref-img-{ix}-{rix}"
                                                            )),
                                                            theme.border,
                                                        );
                                                    }
                                                    // Files/folders: Material icon + basename;
                                                    // snippets (terminal): Lucide terminal.
                                                    let icon = if r.snippet {
                                                        Icon::new(LucideIcon::SquareTerminal)
                                                            .text_color(theme.muted_foreground)
                                                            .into_any_element()
                                                    } else {
                                                        material_path_icon(
                                                            &r.path, r.is_dir, false, cx,
                                                        )
                                                    };
                                                    h_flex()
                                                        .items_center()
                                                        .gap_1()
                                                        .px_1p5()
                                                        .py_0p5()
                                                        .border_1()
                                                        .border_color(theme.border)
                                                        .bg(theme.muted.opacity(0.35))
                                                        .text_xs()
                                                        .child(icon)
                                                        .child(
                                                            div()
                                                                .min_w_0()
                                                                .truncate()
                                                                .child(r.name.clone()),
                                                        )
                                                        .into_any_element()
                                                }),
                                            ),
                                        )
                                    }),
                            ),
                    ),
                )
                .into_any_element()
        }
        Entry::Agent(state) => div()
            .w_full()
            .min_w_0()
            .text_sm()
            .child(
                TextView::new(state).style(crate::theme::text_view_style(cx)),
            )
            .into_any_element(),
        Entry::Thought(state) => div()
            .w_full()
            .min_w_0()
            .pl_2()
            .border_l_2()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                // Explicit style: parent muted color would otherwise disable
                // code-block highlighting (Kit "inverted surface" path).
                TextView::new(state).style(crate::theme::text_view_style(cx)),
            )
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
        // Sticky strip above the composer owns this UI — keep a zero-height
        // placeholder so MessageScroller indices stay aligned with `entries`.
        Entry::FileChanges { .. } => div().into_any_element(),
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
    // Tool / file-change rows are bordered cards — keep outer padding
    // tight so consecutive shell tools don't float apart. Bubbles /
    // markdown keep a touch more air via scroller pb_2.
    let entry_py = matches!(entry, Entry::Tool(_));
    // File-change rows live above the composer — no mid-stream padding.
    if matches!(entry, Entry::FileChanges { .. }) {
        return body;
    }
    div()
        .id(("agent-entry", ix))
        .w_full()
        .px_3()
        .when(entry_py, |d| d.py_0p5())
        .when(!entry_py, |d| d.py_1())
        .child(body)
        .into_any_element()
}

fn file_change_basename(path: &Path) -> SharedString {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}

fn render_file_changes(
    panel: &WeakEntity<AgentPanel>,
    ix: usize,
    files: &[FileChangeRow],
    expanded: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let n = files.len();
    let label: SharedString = if n == 1 {
        "1 File".into()
    } else {
        format!("{n} Files").into()
    };
    let chevron = if expanded {
        LucideIcon::ChevronDown
    } else {
        LucideIcon::ChevronRight
    };
    v_flex()
        .id(("agent-file-changes", ix))
        .w_full()
        .flex_shrink_0()
        .min_w_0()
        .gap_1()
        .px_3()
        .py_1()
        .border_t_1()
        .border_color(theme.border)
        .child(
            h_flex()
                .id(("agent-file-changes-toggle", ix))
                .w_full()
                .items_center()
                .gap_1()
                .min_w_0()
                .cursor_pointer()
                .on_click({
                    let panel = panel.clone();
                    move |_, _, cx| {
                        let _ = panel.update(cx, |p, cx| p.toggle_file_changes(ix, cx));
                    }
                })
                .child(Icon::new(chevron).small().text_color(theme.muted_foreground))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label),
                ),
        )
        .when(expanded, |col| {
            col.children(files.iter().enumerate().map(|(fix, file)| {
                let path = file.path.clone();
                let name = file_change_basename(&file.path);
                let added = file.added;
                let removed = file.removed;
                h_flex()
                    .id(SharedString::from(format!("agent-file-change-{ix}-{fix}")))
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .py_0p5()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted.opacity(0.35)))
                    .on_click({
                        let panel = panel.clone();
                        let path = path.clone();
                        move |_, _, cx| {
                            let _ = panel.update(cx, |_, cx| {
                                cx.emit(AgentPanelEvent::OpenPaths(vec![path.clone()]));
                            });
                        }
                    })
                    .child(material_path_icon(&path, false, false, cx))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_sm()
                            .child(name),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .when(added > 0, |row| {
                                row.child(
                                    div()
                                        .text_color(theme.success)
                                        .child(format!("+{added}")),
                                )
                            })
                            .when(removed > 0, |row| {
                                row.child(
                                    div()
                                        .text_color(theme.danger)
                                        .child(format!("-{removed}")),
                                )
                            })
                            .when(added == 0 && removed == 0 && file.deleted, |row| {
                                row.child(
                                    div()
                                        .text_color(theme.danger)
                                        .child("deleted"),
                                )
                            }),
                    )
            }))
        })
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
        .gap_1()
        .px_2()
        .py_0p5()
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
        if self.compacting {
            return Some("Compacting…".into());
        }
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
        // Session title only — turn activity lives in the transcript footer.
        let title: SharedString = self
            .active
            .as_ref()
            .and_then(|id| self.chats.iter().find(|c| &c.id == id))
            .map(|c| c.title.as_str())
            .filter(|t| !t.is_empty())
            .unwrap_or("New chat")
            .into();
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
                    .child(title),
            )
            .when_some(self.usage, |row, u| {
                let pct = match (u.used, u.size > 0) {
                    (Some(used), true) => (used.saturating_mul(100)) / u.size,
                    _ => 0,
                };
                let color = if pct >= 95 {
                    theme.danger
                } else if pct >= 80 {
                    theme.warning
                } else {
                    theme.muted_foreground
                };
                row.child(
                    div()
                        .id("agent-context-usage")
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(color)
                        .child(format_context_usage(u)),
                )
            })
            .child(
                Button::new("agent-new-chat")
                    .ghost()
                    .small()
                    .icon(LucideIcon::Plus)
                    .tooltip("New Chat")
                    .disabled(!can_new_chat)
                    .when(can_new_chat, |b| b.cursor_pointer())
                    .on_click(cx.listener(|this, _, window, cx| this.new_chat(window, cx))),
            )
            .when(self.info.as_ref().is_none_or(|i| i.can_load), |row| {
                row.child(self.render_history(cx))
            })
            .when(
                find_select(
                    &self.config_options,
                    Some(SessionConfigOptionCategory::Model),
                    "model",
                )
                .is_some(),
                |row| row.child(self.render_agent_settings(cx)),
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

    /// Accordion under the header (N Files pattern — not Kit Accordion).
    /// Collapsed = slim 5h/Week strip. Expanded = full details at content
    /// height (`flex_shrink_0`, no scroll) so the transcript keeps `flex_1`.
    fn render_plan_limits(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let l = self.plan_limits.as_ref()?;
        if l.session.is_none() && l.week.is_none() {
            return None;
        }
        let theme = cx.theme();
        let open = self.plan_usage_open;
        let panel = cx.entity().downgrade();
        let limit_color = |pct: f32| {
            if pct >= 90.0 {
                theme.danger
            } else if pct >= 70.0 {
                theme.warning
            } else {
                theme.muted_foreground
            }
        };
        let compact_bar = |id: &'static str, label: &'static str, w: &limits::Window| {
            let pct = w.percent.clamp(0.0, 100.0);
            let color = limit_color(pct);
            h_flex()
                .id(id)
                .flex_1()
                .min_w_0()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(
                    Progress::new(SharedString::from(format!("{id}-bar")))
                        .flex_1()
                        .color(color)
                        .value(pct),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(color)
                        .child(format!("{pct:.0}%")),
                )
        };
        let chevron = if open {
            LucideIcon::ChevronDown
        } else {
            LucideIcon::ChevronRight
        };

        // Strip stays in-flow (always one row). Expanded detail is painted over
        // the transcript via `render_plan_limits_overlay` — not as a sibling
        // that steals `flex_1` from chat.
        Some(
            h_flex()
                .id("agent-plan-usage-toggle")
                .w_full()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .cursor_pointer()
                .on_click({
                    let panel = panel.clone();
                    move |_, _, cx| {
                        _ = panel.update(cx, |this, cx| {
                            this.plan_usage_open = !this.plan_usage_open;
                            cx.notify();
                        });
                    }
                })
                .child(
                    Icon::new(chevron)
                        .small()
                        .flex_shrink_0()
                        .text_color(theme.muted_foreground),
                )
                .when_some(l.session.as_ref(), |row, w| {
                    row.child(compact_bar("agent-limit-session", "5h", w))
                })
                .when_some(l.week.as_ref(), |row, w| {
                    row.child(compact_bar("agent-limit-week", "Week", w))
                })
                .into_any_element(),
        )
    }

    /// Expanded plan usage — absolute over the transcript, not in the column flow.
    fn render_plan_limits_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.plan_usage_open {
            return None;
        }
        let l = self.plan_limits.as_ref()?;
        if l.session.is_none() && l.week.is_none() {
            return None;
        }
        let theme = cx.theme();
        let limit_color = |pct: f32| {
            if pct >= 90.0 {
                theme.danger
            } else if pct >= 70.0 {
                theme.warning
            } else {
                theme.muted_foreground
            }
        };
        let detail_bar = |id: SharedString, label: SharedString, w: &limits::Window| {
            let pct = w.percent.clamp(0.0, 100.0);
            let color = limit_color(pct);
            let resets = w
                .resets_at
                .as_ref()
                .map(|at| format!("Resets {}", limits::format_resets_local(at)));
            v_flex()
                .w_full()
                .gap_1()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_sm()
                                .text_color(color)
                                .child(format!("{pct:.0}%")),
                        ),
                )
                .when_some(resets, |col, text| {
                    col.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(text),
                    )
                })
                .child(
                    Progress::new(SharedString::from(format!("{id}-detail")))
                        .w_full()
                        .color(color)
                        .value(pct),
                )
        };
        let plan_subtitle: SharedString = match l.subscription.as_deref() {
            Some(plan) => format!("{plan} subscription usage").into(),
            None => "Claude subscription usage".into(),
        };

        let mut body = v_flex()
            .id("agent-plan-usage-detail")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .w_full()
            .gap_3()
            .px_3()
            .pt_2()
            .pb_2()
            .bg(theme.sidebar)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(plan_subtitle),
            );
        if let Some(w) = l.session.as_ref() {
            body = body.child(detail_bar(
                "agent-limit-session".into(),
                "5-hour limit".into(),
                w,
            ));
        }
        if let Some(w) = l.week.as_ref() {
            body = body.child(detail_bar(
                "agent-limit-week".into(),
                "Weekly · all models".into(),
                w,
            ));
        }
        if let Some(w) = l.week_opus.as_ref() {
            body = body.child(detail_bar(
                "agent-limit-opus".into(),
                "Weekly · Opus".into(),
                w,
            ));
        }
        if let Some(w) = l.week_sonnet.as_ref() {
            body = body.child(detail_bar(
                "agent-limit-sonnet".into(),
                "Weekly · Sonnet".into(),
                w,
            ));
        }
        if !l.breakdown.is_empty() {
            body = body.child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .pt_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("What's using your week"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Approximate share of weekly usage by product"),
                    )
                    .children(l.breakdown.iter().map(|row| {
                        let pct = row.percent.clamp(0.0, 100.0);
                        let color = limit_color(pct);
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_color(theme.foreground)
                                    .child(row.display_name.clone()),
                            )
                            .child(
                                Progress::new(SharedString::from(format!(
                                    "agent-limit-bd-{}",
                                    row.key
                                )))
                                .w(px(80.))
                                .color(color)
                                .value(pct),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(color)
                                    .w(px(36.))
                                    .child(format!("{pct:.0}%")),
                            )
                    })),
            );
        }
        Some(body.into_any_element())
    }

    /// Mode: prefer ACP `configOptions` (category mode), else legacy `modes`.
    fn render_mode_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let model_id = find_select(
            &self.config_options,
            Some(SessionConfigOptionCategory::Model),
            "model",
        )
        .map(|m| m.current);
        if let Some(opt) = find_select(
            &self.config_options,
            Some(SessionConfigOptionCategory::Mode),
            "mode",
        ) {
            let config_id = opt.config_id.clone();
            // Name/icon from the full agent list (current may be filtered out of picks).
            let current_name = opt
                .choices
                .iter()
                .find(|(id, _)| id == &opt.current)
                .map(|(_, n)| n.as_str())
                .unwrap_or("");
            let icon = mode_icon(&opt.current, current_name);
            let choices = filter_mode_choices(&opt.choices, model_id.as_deref());
            return self.render_bottom_select(
                "agent-mode",
                icon,
                &opt.name,
                current_name,
                &opt.current,
                &choices,
                self.config_controls_locked(),
                cx,
                move |this, id, cx| {
                    // Keep legacy mode state in sync for the header/tests.
                    if let Some(modes) = &mut this.modes {
                        modes.current_mode_id = id.clone().into();
                    }
                    this.set_config_value(config_id.clone(), id, cx);
                },
            );
        }
        let Some(modes) = self.modes.clone() else {
            return div().into_any_element();
        };
        let current = modes.current_mode_id.to_string();
        let raw: Vec<(String, String)> = modes
            .available_modes
            .iter()
            .map(|m| (m.id.to_string(), m.name.clone()))
            .collect();
        let current_name = raw
            .iter()
            .find(|(id, _)| id == &current)
            .map(|(_, n)| n.as_str())
            .unwrap_or(current.as_str());
        let icon = mode_icon(&current, current_name);
        let choices = filter_mode_choices(&raw, model_id.as_deref());
        self.render_bottom_select(
            "agent-mode",
            icon,
            "Mode",
            current_name,
            &current,
            &choices,
            self.config_controls_locked(),
            cx,
            |this, id, cx| this.set_mode(id, cx),
        )
    }

    /// Model family (Sonnet / Opus / …) when names parse with versions.
    fn render_model_family_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(split) = model_family_split(&self.config_options) else {
            return div().into_any_element();
        };
        let mut families = Vec::new();
        for c in &split.choices {
            if !families.iter().any(|f| f == &c.family) {
                families.push(c.family.clone());
            }
        }
        let choices: Vec<(String, String)> = families
            .into_iter()
            .map(|f| (f.clone(), f))
            .collect();
        let config_id = split.config_id.clone();
        let all = split.choices.clone();
        let prefer_ctx = split.context_label.clone();
        self.render_bottom_select(
            "agent-model-family",
            LucideIcon::Cpu,
            "Model",
            &split.family,
            &split.family,
            &choices,
            self.config_controls_locked(),
            cx,
            move |this, family, cx| {
                let id = all
                    .iter()
                    .filter(|c| c.family == family)
                    .find(|c| {
                        display_context_label(&all, c).as_ref() == prefer_ctx.as_ref()
                    })
                    .or_else(|| all.iter().find(|c| c.family == family))
                    .map(|c| c.id.clone());
                let Some(id) = id else {
                    return;
                };
                this.set_config_value(config_id.clone(), id, cx);
            },
        )
    }

    /// Versions for the current family (`4.5`, `4.6`, …).
    fn render_model_version_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(split) = model_family_split(&self.config_options) else {
            return div().into_any_element();
        };
        let mut versions: Vec<(String, String)> = Vec::new();
        for c in &split.choices {
            if c.family != split.family {
                continue;
            }
            let Some(ver) = &c.version else {
                continue;
            };
            if versions.iter().any(|(_, v)| v == ver) {
                continue;
            }
            // Prefer the choice matching current context when several share a version.
            let id = split
                .choices
                .iter()
                .filter(|o| o.family == split.family && o.version.as_deref() == Some(ver.as_str()))
                .find(|o| display_context_label(&split.choices, o) == split.context_label)
                .or_else(|| {
                    split.choices.iter().find(|o| {
                        o.family == split.family && o.version.as_deref() == Some(ver.as_str())
                    })
                })
                .map(|o| o.id.clone())
                .unwrap_or_else(|| c.id.clone());
            versions.push((id, ver.clone()));
        }
        if versions.is_empty() {
            return div().into_any_element();
        }
        let current = split
            .choices
            .iter()
            .find(|c| c.family == split.family && c.version == split.version_label)
            .and_then(|c| {
                versions
                    .iter()
                    .find(|(_, v)| Some(v.as_str()) == c.version.as_deref())
                    .map(|(id, _)| id.clone())
            })
            .or_else(|| versions.first().map(|(id, _)| id.clone()))
            .unwrap_or_default();
        let config_id = split.config_id.clone();
        let version_label = versions
            .iter()
            .find(|(id, _)| id == &current)
            .map(|(_, n)| n.as_str())
            .unwrap_or(current.as_str());
        self.render_bottom_select(
            "agent-model-version",
            LucideIcon::Tag,
            "Version",
            version_label,
            &current,
            &versions,
            self.config_controls_locked(),
            cx,
            move |this, id, cx| this.set_config_value(config_id.clone(), id, cx),
        )
    }

    /// Context window (`200K` / `1M` / …) when the agent advertises variants.
    fn render_context_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(opt) = find_context_select(&self.config_options) {
            let config_id = opt.config_id.clone();
            let label = opt
                .choices
                .iter()
                .find(|(id, _)| id == &opt.current)
                .map(|(_, n)| n.as_str())
                .unwrap_or(opt.current.as_str());
            return self.render_bottom_select(
                "agent-context",
                LucideIcon::Maximize2,
                "Context",
                label,
                &opt.current,
                &opt.choices,
                self.config_controls_locked(),
                cx,
                move |this, id, cx| this.set_config_value(config_id.clone(), id, cx),
            );
        }
        let Some(split) = model_family_split(&self.config_options) else {
            return div().into_any_element();
        };
        let choices = context_choices(&split);
        if choices.len() < 2 {
            return div().into_any_element();
        }
        let current = split
            .context_label
            .as_ref()
            .and_then(|label| {
                choices
                    .iter()
                    .find(|(_, l)| l == label)
                    .map(|(id, _)| id.clone())
            })
            .or_else(|| choices.first().map(|(id, _)| id.clone()))
            .unwrap_or_default();
        let label = choices
            .iter()
            .find(|(id, _)| id == &current)
            .map(|(_, n)| n.as_str())
            .unwrap_or(current.as_str());
        let config_id = split.config_id.clone();
        self.render_bottom_select(
            "agent-model-context",
            LucideIcon::Maximize2,
            "Context",
            label,
            &current,
            &choices,
            self.config_controls_locked(),
            cx,
            move |this, id, cx| this.set_config_value(config_id.clone(), id, cx),
        )
    }

    /// Fast mode as a Kit Switch labeled "Fast" (On/Off select or boolean).
    fn render_fast_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(opt) = find_fast_option(&self.config_options) else {
            return div().into_any_element();
        };
        let checked = fast_is_on(opt);
        let busy = self.config_controls_locked();
        let panel = cx.entity().downgrade();
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id("agent-fast-mode-wrap")
            .gap_1()
            .items_center()
            .child(Icon::new(LucideIcon::Zap).small().text_color(muted))
            .child(
                Switch::new("agent-fast-mode")
                    .label("Fast")
                    .small()
                    .checked(checked)
                    .disabled(busy)
                    .tooltip(SharedString::from(opt.name.clone()))
                    .when(!busy, |s| s.cursor_pointer())
                    .on_click(move |on, _, cx| {
                        let on = *on;
                        _ = panel.update(cx, |this, cx| this.set_fast_mode(on, cx));
                    }),
            )
            .into_any_element()
    }

    fn render_flat_model_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(opt) =
            find_select(&self.config_options, Some(SessionConfigOptionCategory::Model), "model")
        else {
            return div().into_any_element();
        };
        let config_id = opt.config_id.clone();
        let label = opt
            .choices
            .iter()
            .find(|(id, _)| id == &opt.current)
            .map(|(_, n)| n.as_str())
            .unwrap_or(opt.current.as_str());
        self.render_bottom_select(
            "agent-model",
            LucideIcon::Cpu,
            &opt.name,
            label,
            &opt.current,
            &opt.choices,
            self.config_controls_locked(),
            cx,
            move |this, id, cx| this.set_config_value(config_id.clone(), id, cx),
        )
    }

    /// Header Agent settings — stacked labeled rows (more knobs later).
    fn render_agent_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let locked = self.config_controls_locked();
        let panel = cx.entity().downgrade();
        Popover::new("agent-settings")
            .anchor(Anchor::TopRight)
            .trigger(
                Button::new("agent-settings-button")
                    .ghost()
                    .small()
                    .icon(LucideIcon::SlidersHorizontal)
                    .tooltip("Agent settings")
                    .disabled(locked)
                    .when(!locked, |b| b.cursor_pointer()),
            )
            .content(move |_state: &mut PopoverState, _, cx| {
                let Some(entity) = panel.upgrade() else {
                    return div().into_any_element();
                };
                entity.update(cx, |this, cx| this.render_agent_settings_body(cx))
            })
            .into_any_element()
    }

    /// Settings body: one row per knob — `Label → controls…`.
    fn render_agent_settings_body(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .w(px(360.))
            .gap_2()
            .p_2()
            .child(self.render_utility_model_setting_row(cx))
            .into_any_element()
    }

    /// `Utility model → [Chat model|family] [version]` (Improve / titles / Git Suggest).
    fn render_utility_model_setting_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let locked = self.config_controls_locked();
        let mute = cx.theme().muted_foreground;
        let silent_id = self.settings.silent_model.as_deref();
        let split = model_family_split(&self.config_options);
        let silent_choice = silent_id.and_then(|id| {
            split
                .as_ref()
                .and_then(|s| s.choices.iter().find(|c| c.id == id))
        });

        let model_picker = if let Some(split) = split.as_ref() {
            let mut families = Vec::new();
            for c in &split.choices {
                if !families.iter().any(|f| f == &c.family) {
                    families.push(c.family.clone());
                }
            }
            let mut choices = Vec::with_capacity(families.len() + 1);
            choices.push((String::new(), "Chat model".into()));
            choices.extend(families.into_iter().map(|f| (f.clone(), f)));
            let current = silent_choice
                .map(|c| c.family.clone())
                .unwrap_or_default();
            let label = if current.is_empty() {
                "Chat model"
            } else {
                current.as_str()
            };
            self.render_bottom_select(
                "agent-settings-utility-model",
                LucideIcon::Cpu,
                "Model",
                label,
                &current,
                &choices,
                locked,
                cx,
                |this, id, cx| {
                    if id.is_empty() {
                        this.set_silent_model(None, cx);
                    } else {
                        this.set_silent_model_family(id, cx);
                    }
                },
            )
        } else if let Some(opt) = find_select(
            &self.config_options,
            Some(SessionConfigOptionCategory::Model),
            "model",
        ) {
            let mut choices = Vec::with_capacity(opt.choices.len() + 1);
            choices.push((String::new(), "Chat model".into()));
            choices.extend(opt.choices);
            let current = silent_id.unwrap_or("").to_owned();
            let label = if current.is_empty() {
                "Chat model"
            } else {
                choices
                    .iter()
                    .find(|(id, _)| id == &current)
                    .map(|(_, n)| n.as_str())
                    .unwrap_or(current.as_str())
            };
            self.render_bottom_select(
                "agent-settings-utility-model",
                LucideIcon::Cpu,
                "Model",
                label,
                &current,
                &choices,
                locked,
                cx,
                |this, id, cx| {
                    this.set_silent_model(if id.is_empty() { None } else { Some(id) }, cx);
                },
            )
        } else {
            div().into_any_element()
        };

        let version_picker = split.as_ref().and_then(|split| {
            let family = silent_choice?.family.as_str();
            let mut versions: Vec<(String, String)> = Vec::new();
            for c in &split.choices {
                if c.family != family {
                    continue;
                }
                let Some(ver) = &c.version else {
                    continue;
                };
                if versions.iter().any(|(_, v)| v == ver) {
                    continue;
                }
                versions.push((ver.clone(), ver.clone()));
            }
            if versions.is_empty() {
                return None;
            }
            let current = silent_choice
                .and_then(|c| c.version.clone())
                .unwrap_or_default();
            let label = if current.is_empty() {
                "—"
            } else {
                current.as_str()
            };
            Some(self.render_bottom_select(
                "agent-settings-utility-version",
                LucideIcon::Tag,
                "Version",
                label,
                &current,
                &versions,
                locked,
                cx,
                |this, ver, cx| this.set_silent_model_version(ver, cx),
            ))
        });

        h_flex()
            .w_full()
            .items_center()
            .gap_1()
            .child(
                div()
                    .id("agent-settings-utility-label")
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(mute)
                    .child("Utility model"),
            )
            .child(model_picker)
            .children(version_picker)
            .into_any_element()
    }

    /// Effort / other selects (not Mode, Model, Fast, or Context).
    fn render_extra_config_pickers(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(options) = &self.config_options else {
            return Vec::new();
        };
        let busy = self.config_controls_locked();
        let context_id = find_context_select(&self.config_options).map(|s| s.config_id);
        options
            .iter()
            .filter(|o| {
                !matches!(
                    o.category,
                    Some(SessionConfigOptionCategory::Mode | SessionConfigOptionCategory::Model)
                ) && o.id.0.as_ref() != "mode"
                    && o.id.0.as_ref() != "model"
                    && !is_fast_option(o)
                    && context_id.as_deref() != Some(o.id.0.as_ref())
            })
            .filter_map(|o| {
                let opt = select_from_option(o)?;
                let icon = config_option_icon(o);
                let config_id = opt.config_id.clone();
                let popover_id: SharedString =
                    format!("agent-config-{}", opt.config_id).into();
                let label = opt
                    .choices
                    .iter()
                    .find(|(id, _)| id == &opt.current)
                    .map(|(_, n)| n.as_str())
                    .unwrap_or(opt.current.as_str());
                Some(self.render_bottom_select(
                    popover_id,
                    icon,
                    opt.name,
                    label,
                    &opt.current,
                    &opt.choices,
                    busy,
                    cx,
                    move |this, id, cx| this.set_config_value(config_id.clone(), id, cx),
                ))
            })
            .collect()
    }

    /// Composer-row Popover select (opens upward).
    #[allow(clippy::too_many_arguments)]
    fn render_bottom_select(
        &self,
        popover_id: impl Into<SharedString>,
        icon: LucideIcon,
        tooltip: impl Into<SharedString>,
        label: impl Into<SharedString>,
        current: &str,
        choices: &[(String, String)],
        busy: bool,
        cx: &mut Context<Self>,
        on_pick: impl Fn(&mut Self, String, &mut Context<Self>) + Clone + 'static,
    ) -> AnyElement {
        let popover_id: SharedString = popover_id.into();
        let tooltip: SharedString = tooltip.into();
        let label: SharedString = label.into();
        let panel = cx.entity().downgrade();
        let current = current.to_owned();
        let choices = choices.to_vec();
        let trigger_id: SharedString = format!("{popover_id}-button").into();
        Popover::new(popover_id.clone())
            .anchor(Anchor::BottomLeft)
            .trigger(
                Button::new(trigger_id)
                    .ghost()
                    .small()
                    .icon(icon)
                    .label(label)
                    .tooltip(tooltip)
                    .disabled(busy)
                    .when(!busy, |b| b.cursor_pointer()),
            )
            .content(move |_state: &mut PopoverState, _, cx| {
                let popover = cx.weak_entity();
                let popover_id = popover_id.clone();
                v_flex()
                    .w(px(200.))
                    .children(choices.iter().map(|(id, name)| {
                        let selected = id == &current;
                        let (panel, popover) = (panel.clone(), popover.clone());
                        let on_pick = on_pick.clone();
                        let id = id.clone();
                        ListItem::new(SharedString::from(format!("{popover_id}-{id}")))
                            .selected(selected)
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                if let Some(p) = popover.upgrade() {
                                    p.update(cx, |p, cx| p.dismiss(window, cx));
                                }
                                let id = id.clone();
                                let on_pick = on_pick.clone();
                                _ = panel.update(cx, |this, cx| on_pick(this, id, cx));
                            })
                            .child(div().text_sm().child(name.clone()))
                    }))
            })
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
                        let usage = panel
                            .upgrade()
                            .and_then(|p| p.read(cx).usage_by_chat.get(&chat.id).copied());
                        let (open_panel, rename_panel, delete_panel) =
                            (panel.clone(), panel.clone(), panel.clone());
                        let (open_popover, rename_popover, delete_popover) =
                            (popover.clone(), popover.clone(), popover.clone());
                        let (open_id, rename_id, delete_id) =
                            (chat.id.clone(), chat.id.clone(), chat.id.clone());
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
                                                h_flex()
                                                    .gap_2()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(relative_time(chat.updated, now))
                                                    .when_some(usage, |row, u| {
                                                        row.child(format_context_usage(u))
                                                    }),
                                            ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "agent-chat-rename-{}",
                                            chat.id
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(LucideIcon::Pencil)
                                        .tooltip("Rename Chat")
                                        .disabled(busy)
                                        .when(!busy, |b| b.cursor_pointer())
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            if let Some(p) = rename_popover.upgrade() {
                                                p.update(cx, |p, cx| p.dismiss(window, cx));
                                            }
                                            let id = rename_id.clone();
                                            _ = rename_panel.update(cx, |this, cx| {
                                                this.confirm_rename_chat(id, window, cx);
                                            });
                                        }),
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
                            let button = Button::new(SharedString::from(format!("agent-login-{i}")))
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
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let busy = self.turn_running;
        let activity = self.activity_label();
        let body = if self.entries.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .p_4()
                .text_center()
                .text_xs()
                .text_color(muted)
                .child(format!("Ask {} about this workspace.", self.agent_name))
                .into_any_element()
        } else {
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
            // ~half the pane below the latest row so long chats can sit mid-
            // viewport (Cursor-style), not glued to the composer.
            .with_bottom_space(relative(0.5))
            .size_full()
            .into_any_element()
        };
        // In-chat activity (not only the header spinner) so a long transcript
        // still shows that a turn is running at the bottom of the pane.
        v_flex()
            .size_full()
            .child(div().flex_1().min_h_0().w_full().child(body))
            .when(busy, |col| {
                col.child(
                    h_flex()
                        .id("agent-chat-busy")
                        .w_full()
                        .flex_shrink_0()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .child(Spinner::new().small())
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(muted)
                                .child(activity.unwrap_or_else(|| "Working…".into())),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_composer(&self, ready: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        // Open-chat turn only — Suggest / title silent jobs must not look like
        // the chat is busy. Improve keeps its own Stop + button spinner.
        let chat_busy = self.turn_running;
        let improving = matches!(self.silent, Some(SilentTurn::Improve { .. }));
        let queueing = chat_busy;
        let draft_plain = {
            let content = self.input.read(cx).content();
            text_without_tokens(content.text(), content.tokens())
        };
        let draft = {
            let input = self.input.read(cx);
            !draft_plain.is_empty()
                || !input.tokens().is_empty()
                || !self.attachments.is_empty()
        };
        // Send stays available while a user turn runs — it queues. Stop is
        // separate. Suggest/title do not switch Send→Queue.
        let can_send = ready && draft && !improving;
        let can_improve = ready
            && !chat_busy
            && self.silent.is_none()
            && !draft_plain.is_empty();
        let stop = (chat_busy || improving).then(|| {
            // Lucide `Square` is stroke-only (hollow); stop needs a solid block.
            Button::new("agent-stop")
                .danger()
                .small()
                .cursor_pointer()
                .child(
                    div()
                        .size_2()
                        .flex_shrink_0()
                        .bg(theme.button_danger_foreground),
                )
                .label("Stop")
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
                .into_any_element()
        });
        let improve = Button::new("agent-improve")
            .ghost()
            .small()
            .icon(LucideIcon::PencilSparkles)
            .tooltip("Improve prompt")
            .loading(improving)
            .disabled(!can_improve && !improving)
            .when(can_improve, |b| {
                b.cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.request_improve_prompt(cx);
                    }))
            })
            .into_any_element();
        let send = Button::new("agent-send")
            .primary()
            .small()
            .icon(LucideIcon::ArrowUp)
            .label(if queueing { "Queue" } else { "Send" })
            .disabled(!can_send)
            .when(can_send, |b| {
                b.cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
            })
            .into_any_element();
        let actions = h_flex()
            .gap_1()
            .items_center()
            .child(improve)
            .children(stop)
            .child(send);
        let hint = if queueing {
            if self.outbound.is_empty() {
                "@ mention · Enter to queue".to_owned()
            } else {
                format!(
                    "{} queued · Enter to queue",
                    self.outbound.len()
                )
            }
        } else {
            "@ mention · Enter to send".to_owned()
        };
        let panel = cx.entity().downgrade();
        let muted = theme.muted_foreground;
        let border = theme.border;
        let chips = (!self.attachments.is_empty()).then(|| self.render_attachment_chips(cx));
        let mention = self.mention.as_ref().map(|m| self.render_mention_menu(m, cx));
        let roots = self.roots.clone();
        // Drop target for tree / editor-tab / OS file drags → attach chips.
        // Outer composer + prompt wrapper: GPUI hits the deepest matching
        // listener, so the Textarea area still attaches.
        v_flex()
            .id("agent-composer")
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(border)
            .drag_over::<TreeEntryDrag>(|this, _, _, cx| this.bg(cx.theme().drop_target))
            .on_drop(cx.listener(|_, d: &TreeEntryDrag, _, cx| {
                cx.emit(AgentPanelEvent::AttachPath(d.path.clone()));
            }))
            .drag_over::<EditorTabDrag>(|this, _, _, cx| this.bg(cx.theme().drop_target))
            .on_drop(cx.listener(|_, d: &EditorTabDrag, _, cx| {
                cx.emit(AgentPanelEvent::AttachPath(d.path.clone()));
            }))
            .drag_over::<ExternalPaths>(|this, _, _, cx| this.bg(cx.theme().drop_target))
            .on_drop(cx.listener(|_, paths: &ExternalPaths, _, cx| {
                for path in paths.paths() {
                    cx.emit(AgentPanelEvent::AttachPath(path.clone()));
                }
            }))
            .children(chips)
            .children(mention)
            .child(
                div()
                    .id("agent-prompt-drop")
                    .w_full()
                    .drag_over::<TreeEntryDrag>(|this, _, _, cx| this.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(|_, d: &TreeEntryDrag, _, cx| {
                        cx.emit(AgentPanelEvent::AttachPath(d.path.clone()));
                    }))
                    .drag_over::<EditorTabDrag>(|this, _, _, cx| this.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(|_, d: &EditorTabDrag, _, cx| {
                        cx.emit(AgentPanelEvent::AttachPath(d.path.clone()));
                    }))
                    .drag_over::<ExternalPaths>(|this, _, _, cx| this.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(|_, paths: &ExternalPaths, _, cx| {
                        for path in paths.paths() {
                            cx.emit(AgentPanelEvent::AttachPath(path.clone()));
                        }
                    }))
                    .child(
                        // `@` mentions are Kit inline tokens inside the prompt Textarea.
                        Textarea::new(&self.input)
                            .token(move |ctx, _, cx| render_mention_token(ctx, &roots, cx))
                            .on_paste(move |item, _window, cx| {
                                panel
                                    .update(cx, |panel, cx| panel.handle_composer_paste(item, cx))
                                    .unwrap_or(false)
                            }),
                    ),
            )
            // Config band then attach/hint/Send. Always reserve one control-row
            // height so first options / session open do not shove the textarea.
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        v_flex()
                            .id("agent-composer-config-band")
                            .w_full()
                            .min_h(px(theme::AGENT_COMPOSER_CONFIG_ROW_H))
                            .justify_center()
                            .when(self.has_composer_config_controls(), |band| {
                                band.child(self.render_composer_config_row(cx))
                            }),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        Button::new("agent-attach-file")
                                            .ghost()
                                            .small()
                                            .icon(LucideIcon::Paperclip)
                                            .tooltip("Attach files… (or drop from tree / tab / OS)")
                                            .disabled(chat_busy || improving)
                                            .when(!(chat_busy || improving), |b| {
                                                b.cursor_pointer()
                                            })
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.emit(AgentPanelEvent::AttachFromDisk)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(hint),
                                    ),
                            )
                            .child(actions),
                    ),
            )
            .when(!self.outbound.is_empty(), |col| {
                col.child(self.render_outbound_queue(cx))
            })
            .into_any_element()
    }

    /// Waiting prompts under the composer — remove with ghost `CircleX`.
    fn render_outbound_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let n = self.outbound.len();
        v_flex()
            .id("agent-outbound-queue")
            .w_full()
            .gap_1()
            .p_2()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(if n == 1 {
                        "Queued — sends when this turn ends".to_owned()
                    } else {
                        format!("Queued ({n}) — send in order when this turn ends")
                    }),
            )
            .children(self.outbound.iter().enumerate().map(|(ix, item)| {
                let preview = queued_preview(item);
                h_flex()
                    .id(SharedString::from(format!("agent-outbound-{ix}")))
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(preview),
                    )
                    .child(
                        Button::new(SharedString::from(format!("agent-outbound-remove-{ix}")))
                            .ghost()
                            .small()
                            .icon(LucideIcon::CircleX)
                            .tooltip("Remove from queue")
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_outbound(ix, cx);
                            })),
                    )
            }))
            .into_any_element()
    }

    fn has_composer_config_controls(&self) -> bool {
        self.modes.is_some()
            || self
                .config_options
                .as_ref()
                .is_some_and(|opts| !opts.is_empty())
    }

    /// Mode / Model / Version / Context / Fast / Effort — one horizontal row
    /// (scroll if narrow) so height stays stable and does not wrap-jump.
    fn render_composer_config_row(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .id("agent-composer-config")
            .w_full()
            .min_w_0()
            .h(px(theme::AGENT_COMPOSER_CONFIG_ROW_H))
            .gap_1()
            .items_center()
            .overflow_x_scroll()
            .when(
                find_select(
                    &self.config_options,
                    Some(SessionConfigOptionCategory::Mode),
                    "mode",
                )
                .is_some()
                    || self.modes.is_some(),
                |row| row.child(self.render_mode_picker(cx)),
            )
            .when(model_family_split(&self.config_options).is_some(), |row| {
                row.child(self.render_model_family_picker(cx))
                    .child(self.render_model_version_picker(cx))
            })
            .when(
                model_family_split(&self.config_options).is_none()
                    && find_select(
                        &self.config_options,
                        Some(SessionConfigOptionCategory::Model),
                        "model",
                    )
                    .is_some(),
                |row| row.child(self.render_flat_model_picker(cx)),
            )
            .when(
                find_context_select(&self.config_options).is_some()
                    || model_family_split(&self.config_options)
                        .is_some_and(|s| context_choices(&s).len() >= 2),
                |row| row.child(self.render_context_picker(cx)),
            )
            .when(find_fast_option(&self.config_options).is_some(), |row| {
                row.child(self.render_fast_toggle(cx))
            })
            .children(self.render_extra_config_pickers(cx))
            .into_any_element()
    }

    fn render_mention_menu(&self, menu: &MentionMenu, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let selected = menu.selected;
        // Items are direct children of the scroll container so
        // `ScrollHandle::scroll_to_item` indexes match ↑↓ selection.
        v_flex()
            .id("agent-mention-menu")
            .w_full()
            .max_h(px(200.))
            .gap_0()
            .py_0p5()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .overflow_y_scroll()
            .track_scroll(&menu.scroll)
            .when(menu.hits.is_empty(), |col| {
                col.child(
                    div()
                        .px_2()
                        .py_0p5()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("No matches"),
                )
            })
            .children(menu.hits.iter().enumerate().map(|(ix, hit)| {
                let label = if hit.is_dir {
                    format!("{}/", hit.label.trim_end_matches('/'))
                } else {
                    hit.label.clone()
                };
                let is_dir = hit.is_dir;
                let is_root = self.roots.iter().any(|r| r == &hit.path);
                let icon = material_path_icon(&hit.path, is_dir, is_root, cx);
                ListItem::new(SharedString::from(format!("agent-mention-{ix}")))
                    .w_full()
                    .selected(ix == selected)
                    .cursor_pointer()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_1()
                            .px_1p5()
                            .py_0p5()
                            .child(icon)
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_xs()
                                    .child(label),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_mention(ix, window, cx);
                    }))
            }))
            .into_any_element()
    }

    fn render_attachment_chips(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        // Bottom-align so short file/folder chips sit on the same baseline as
        // taller image thumbs (not floating at the top of the row).
        h_flex()
            .w_full()
            .gap_2()
            .flex_wrap()
            .items_end()
            .children(self.attachments.iter().enumerate().map(|(ix, attachment)| {
                let remove = Button::new(SharedString::from(format!("agent-attach-x-{ix}")))
                    .ghost()
                    .xsmall()
                    .icon(LucideIcon::CircleX)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.remove_attachment(ix, cx);
                    }));
                let body: AnyElement = if let Some(Some(thumb)) = self.attachment_thumbs.get(ix) {
                    render_image_thumb(
                        thumb,
                        SharedString::from(format!("agent-attach-img-{ix}")),
                        theme.border,
                    )
                } else {
                    let label = attachment.label();
                    let icon = match attachment {
                        Attachment::Snippet { .. } => Icon::new(LucideIcon::SquareTerminal)
                            .text_color(theme.muted_foreground)
                            .into_any_element(),
                        Attachment::File { path, .. } => {
                            let is_root = self.roots.iter().any(|r| r == path);
                            material_path_icon(path, path.is_dir(), is_root, cx)
                        }
                        Attachment::Image { path: Some(p), .. } => {
                            material_path_icon(p, false, false, cx)
                        }
                        Attachment::Image { label, .. } => {
                            material_path_icon(Path::new(label.as_str()), false, false, cx)
                        }
                    };
                    h_flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_1()
                        .bg(theme.muted.opacity(0.35))
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .child(icon)
                        .child(div().min_w_0().truncate().child(label))
                        .into_any_element()
                };
                // Kit Attachment pattern: × rides the upper trailing corner.
                div()
                    .id(SharedString::from(format!("agent-attach-{ix}")))
                    .relative()
                    .flex_none()
                    .pt(px(6.))
                    .pr(px(6.))
                    .child(body)
                    .child(div().absolute().top_0().right_0().child(remove))
                    .into_any_element()
            }))
            .into_any_element()
    }
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.flush_pending_improve(window, cx);
        let ready = matches!(self.status, Status::Ready);
        // Keep the transcript on screen while reconnecting / replaying into it.
        let show_transcript = ready
            || (!self.entries.is_empty()
                && matches!(
                    self.status,
                    Status::Starting | Status::Opening | Status::Suspended
                ));
        // Mount the composer with the transcript so Ready doesn't shove history
        // up when Send appears (Send stays disabled until the session is ready).
        let show_composer = show_transcript
            || matches!(
                self.status,
                Status::Ready | Status::Starting | Status::Opening | Status::Suspended
            );
        v_flex()
            .id("agent-panel")
            // So `⌘N` / `Ctrl+N` maps to NewAgentChat while this panel (composer) is focused.
            .key_context("Agent")
            .size_full()
            .min_w_0()
            .bg(cx.theme().sidebar)
            .on_action(cx.listener(|this, _: &crate::app::NewAgentChat, window, cx| {
                this.new_chat(window, cx);
            }))
            .child(self.render_header(cx))
            .children(self.render_plan_limits(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .relative()
                    .child(if show_transcript {
                        self.render_transcript(cx)
                    } else {
                        self.render_state(cx)
                    })
                    // After transcript so the expand paints on top of chat.
                    .children(self.render_plan_limits_overlay(cx)),
            )
            // **N Files** sits on top of the composer (the chat input), not under usage.
            .children(self.render_sticky_file_changes(cx))
            .when(show_composer, |col| col.child(self.render_composer(ready, cx)))
    }
}

#[cfg(test)]
mod silent_model_override_tests {
    use super::{apply_silent_model_override, ConfigSelect};
    use crate::acp::SilentConfigValue;

    fn model_select(current: &str) -> ConfigSelect {
        ConfigSelect {
            config_id: "model".into(),
            name: "Model".into(),
            current: current.into(),
            choices: vec![
                ("sonnet-4-5".into(), "Sonnet 4.5".into()),
                ("haiku-4-5".into(), "Haiku 4.5".into()),
            ],
        }
    }

    fn model_value(configs: &[(String, SilentConfigValue)]) -> Option<&str> {
        configs.iter().find_map(|(id, v)| {
            if id != "model" {
                return None;
            }
            match v {
                SilentConfigValue::Select(s) => Some(s.as_str()),
                _ => None,
            }
        })
    }

    #[test]
    fn overrides_when_silent_model_is_offered() {
        let mut configs = vec![
            ("mode".into(), SilentConfigValue::Select("code".into())),
            ("model".into(), SilentConfigValue::Select("sonnet-4-5".into())),
        ];
        apply_silent_model_override(
            &mut configs,
            Some(&model_select("sonnet-4-5")),
            Some("haiku-4-5"),
        );
        assert_eq!(model_value(&configs), Some("haiku-4-5"));
        assert!(matches!(
            configs[0].1,
            SilentConfigValue::Select(ref s) if s == "code"
        ));
    }

    #[test]
    fn leaves_chat_model_when_unset_or_invalid() {
        let mut configs = vec![("model".into(), SilentConfigValue::Select("sonnet-4-5".into()))];
        apply_silent_model_override(&mut configs, Some(&model_select("sonnet-4-5")), None);
        assert_eq!(model_value(&configs), Some("sonnet-4-5"));

        apply_silent_model_override(
            &mut configs,
            Some(&model_select("sonnet-4-5")),
            Some("missing-id"),
        );
        assert_eq!(model_value(&configs), Some("sonnet-4-5"));
    }

    #[test]
    fn pick_silent_model_id_prefers_family_then_version() {
        use super::{pick_silent_model_id, ModelChoice};
        let choices = vec![
            ModelChoice {
                family: "Sonnet".into(),
                id: "sonnet-4-5".into(),
                version: Some("4.5".into()),
                context: None,
            },
            ModelChoice {
                family: "Sonnet".into(),
                id: "sonnet-4-6".into(),
                version: Some("4.6".into()),
                context: None,
            },
            ModelChoice {
                family: "Haiku".into(),
                id: "haiku-4-5".into(),
                version: Some("4.5".into()),
                context: None,
            },
        ];
        assert_eq!(
            pick_silent_model_id(&choices, "Haiku", None, None).as_deref(),
            Some("haiku-4-5")
        );
        assert_eq!(
            pick_silent_model_id(&choices, "Sonnet", Some("4.6"), None).as_deref(),
            Some("sonnet-4-6")
        );
    }
}

#[cfg(test)]
mod mode_filter_tests {
    // Avoid `use super::*` — it pulls gpui_kit's `#[test]` macro into scope.
    use super::filter_mode_choices;

    #[test]
    fn hides_auto_below_claude_4_6() {
        let choices = vec![
            ("ask".into(), "Ask".into()),
            ("auto".into(), "Auto".into()),
            ("plan".into(), "Plan".into()),
        ];
        let ids = |model: &str| {
            filter_mode_choices(&choices, Some(model))
                .into_iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("sonnet-4-5"), vec!["ask", "plan"]);
        assert_eq!(ids("claude-haiku-4-5-20251001"), vec!["ask", "plan"]);
        assert_eq!(ids("sonnet-4-6"), vec!["ask", "auto", "plan"]);
        assert_eq!(ids("opus-5"), vec!["ask", "auto", "plan"]);
        // Non-Claude agents keep Auto.
        assert_eq!(ids("gpt-4o"), vec!["ask", "auto", "plan"]);
    }
}

#[cfg(test)]
mod improve_prompt_tests {
    use super::{build_improve_prompt, improved_composer_content, scrub_improved_prompt};
    use gpui_kit::component::input::InlineToken;

    #[test]
    fn scrub_strips_fences_and_preamble() {
        assert_eq!(
            scrub_improved_prompt("```\nFix the overlay pushing chat\n```"),
            "Fix the overlay pushing chat"
        );
        assert_eq!(
            scrub_improved_prompt("Improved prompt: Make expand absolute over the transcript"),
            "Make expand absolute over the transcript"
        );
        assert_eq!(
            scrub_improved_prompt("Here's an improved prompt:\nKeep attachments; rewrite text only"),
            "Keep attachments; rewrite text only"
        );
        assert_eq!(scrub_improved_prompt("\"Tighten the error message\""), "Tighten the error message");
        assert!(scrub_improved_prompt("   ").is_empty());
    }

    #[test]
    fn improve_prompt_asks_for_plain_rewrite() {
        let p = build_improve_prompt("fix overlay push chat");
        assert!(p.contains("Draft:\nfix overlay push chat"));
        assert!(p.contains("ONLY the improved prompt"));
        assert!(p.contains("do not invent"));
    }

    #[test]
    fn improved_content_keeps_inline_tokens() {
        let a = InlineToken::new("/ws/a.rs", "a.rs").with_label("a.rs");
        let b = InlineToken::new("/ws/b/", "b/").with_label("b/");
        let content = improved_composer_content("Please fix the overlay.", &[a.clone(), b.clone()]);
        assert_eq!(content.text().as_ref(), "a.rs b/ Please fix the overlay.");
        let tokens = content.tokens();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token(), &a);
        assert_eq!(tokens[1].token(), &b);
        assert_eq!(&content.text()[tokens[0].range()], "a.rs");
        assert_eq!(&content.text()[tokens[1].range()], "b/");
    }

    #[test]
    fn improved_content_without_tokens_is_plain() {
        let content = improved_composer_content("Tighten the error message", &[]);
        assert_eq!(content.text().as_ref(), "Tighten the error message");
        assert!(content.tokens().is_empty());
    }
}
