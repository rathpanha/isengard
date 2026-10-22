# Design rules

These rules keep the UI consistent. They are requirements, not suggestions:
every new or changed UI must follow them, and any new pattern the user approves
must be added here in the same change. When a rule and existing code disagree,
fix the code (or ask the user before changing the rule).

History goes in [changelog.md](changelog.md) only — do not add a Changelog
section here.

## 1. Foundation

- **Components:** build UI from GPUI Kit components only. Don't hand-build a
  widget the library already has. When a pattern repeats, wrap it once in
  `src/ui/components.rs` and reuse it (see §4).
- **Shape — block style:** no rounded corners anywhere in the app. The theme
  sets `radius = 0` and `radius_lg = 0` (`src/theme.rs`); never pass a custom
  radius. (The macOS window frame and Dock mask are drawn by the OS and are
  outside our control.)
- **Font:** JetBrains Mono Nerd Font for everything — UI and editor
  (`theme::FONT_FAMILY`). UI base size 14px (the rem base), editor size from
  settings.
- **Color:** only semantic theme colors from `cx.theme()` — `foreground`,
  `muted_foreground`, `background`, `border`, `danger`, `sidebar`, … No raw
  hex/rgb in UI code. Opacity variations of a theme color are fine
  (e.g. `danger.opacity(0.15)`). Exceptions: the file tree uses Material Icon
  Theme SVGs with their upstream fill colours (see §5); the terminal ANSI
  16-colour table may keep documented defaults in `terminal::colors` (bg/fg/
  cursor mapped from `cx.theme()` where possible).
- **Dark only:** the app is dark mode permanently — no light theme, no toggle.
  Editor syntax colours come from `assets/themes/ayu-darker-highlight.json`
  (GPL-3.0); editor/gutter background is forced to `theme.sidebar` (same as the
  file tree).

## 2. Spacing

- Use GPUI's rem-based helpers (`gap_2`, `px_3`, `mr_2`, …), not `px(..)`,
  except for documented physical sizes.
- **Edge symmetry:** an element at the trailing edge of a container sits the
  same distance from that edge as the content at the leading edge. Example: the
  tab's close × is `mr_2` from the right border; the Material file icon lives
  inside the tab's padded content (not `Tab::prefix`) so Kit's 12px left pad
  insets it the same distance.
- Related controls in a row: `gap_3`; tightly grouped items (e.g. key caps):
  `gap_1`; sections on a page: `gap_8`.

## 3. Interaction states

- **Button / notification variants:** use Kit's semantic variants as shipped
  (`.primary()`, `.danger()`, `.ghost()`, `.link()`, `.outline()`, …) — no
  custom colours on buttons. Kit's destructive role is `.danger()` (there is
  no `.destructive()`).
  - **Icon-only dismiss / chrome** (tab ×, remove from recent, terminal × /
    minimize, sidebar toggle, tree New File/Folder): Kit's default `.ghost()`
    — no danger tint on the ×.
  - **In-flight async actions** (git Pull/Push/Commit/Refresh/discard, search
    while a scan runs, …): Spinner only on the control that started the work;
    siblings may `.disabled(true)` without `.loading()`. Kit only paints a
    spinner when the button already has an `.icon()` — label-only busy
    buttons must set a placeholder icon (e.g. `Loader`) while loading.
    Do **not** grey out with `.disabled(true)` alone for the acting control.
    Status text ("Searching…") does not replace a loading control when the
    control itself started the work.
  - **Choice dialogs** (`open_choice_dialog`): Cancel · secondary · primary —
    Cancel `.outline()`; discard secondary ("Don't Save", "Quit Without
    Saving") `.danger().outline()`; safe primary ("Save", "Save All")
    `.primary()`.
  - **Toasts:** `components::notify_success` / `notify_error` — custom
    content row (`h_flex` + `items_center`) because Kit's typed
    `Notification::success`/`error` absolutely places the icon at
    `top: 18px`, which does not vertically centre with one-line text.
- **Modal dialogs** sit in the middle of the window — wrap every
  `open_dialog` with `components::center_dialog`. Context menus stay
  click-anchored. **No enter/exit motion and no panel drop shadow.** The
  dialog host is clipped to the Linux CSD content frame (not the full
  viewport) so the dim overlay cannot paint into the transparent
  window-shadow padding. Patch lives in `vendor/gpui-component`. Toasts keep
  their motion.
- **Pointer cursor** on everything clickable (buttons, tabs, tree/list rows,
  checkboxes, links); disabled items and plain text keep the default arrow,
  text areas the I-beam. GPUI Kit only does this itself for `link()`/`text()`
  buttons, so add `.cursor_pointer()` to every other clickable component
  (for list/tree rows and checkboxes: `.when(!disabled, |el|
  el.cursor_pointer())`). Shared components in `src/ui/components.rs`
  already include it.
- **Tooltips** on every icon-only button, naming the action ("Close",
  "Remove from recent"). Kit's managed tooltip can stick if the trigger
  unmounts while a delayed show is in flight (or on keyboard close while
  hovered). Call `window.dismiss_tooltip(cx)` on any action that removes the
  hovered control: workspace switch, tab/terminal close, welcome recent
  remove/open, conflict Accept. Click paths also get a Kit-level hide +
  deferred re-hide (vendored `managed_tooltip`).

## 4. Shared components (`src/ui/components.rs`)

| Component | Use for |
| --- | --- |
| `center_dialog(dialog, window)` | Wrap every `open_dialog` builder so the modal sits in the middle of the window (GPUI Kit defaults to top ~10%). Not for context menus. |
| `notify_success` / `notify_error` / `notify_warning` | One-line toasts with icon + text vertically centred (workaround for Kit typed-notification icon layout). Warning = something waits on the user. |

Add a row here whenever a new shared component is created.

## 5. Patterns

- **Dirty tabs:** unsaved buffer shows a 6px square in `theme.blue` before
  the close × (block style — no round “dot”).
- **Panel header row height:** sidebar chrome (Explorer / Git / Search),
  explorer + git toolbars, editor tab strip, and Edit / Preview mode bar
  all use `theme::PANEL_HEADER_ROW_H` (36px, Kit `Tab::large()`). Two
  horizontal bands line up across the sidebar | editor split when a
  preview-capable tab is open.
- **Markdown / SVG preview:** `.md` and `.svg` tabs show an Edit / Preview
  `ButtonGroup` under the tab bar (`.outline().small()` — same size as
  conflict Accept Current / Incoming). Markdown preview uses
  `TextView::markdown`; SVG preview renders the buffer via
  `Image::from_bytes(Svg)` (live, including unsaved edits). Toggle also via
  View > Toggle Preview or `⌘⇧V` / `Ctrl+Shift+V` (disabled while the
  terminal has focus so paste keeps that chord). Distinct from **preview
  tabs** (italic, temporary).
- **Image preview:** raster images (png/jpg/gif/webp/…) open as a centred
  `img(path)` pane (no text editor). Status bar shows "Image". SVG is not in
  this list — it uses Edit/Preview above.
- **Preview tabs:** single-click a file in the tree → italic preview tab
  (only one; the next preview replaces it unless dirty, then it pins) and
  **keeps Tree focus** so Delete / F2 still hit the selection. Double-click
  the file or the tab, or edit the buffer → permanent and focus moves to the
  editor. File > Open File / CLI opens permanent (editor focused).
- **Editor context menu:** right-click in a text editor → Kit defaults
  (Cut/Copy/Paste/Select All, plus Go to Definition / Code Actions when
  available) plus **Add Selection to Agent** (same as `⌘L` / Edit menu).
- **Tab context menu:** right-click a tab → Close, Close Others (disabled when
  only one tab), Close All. Dirty tabs get one Save All / Don't Save dialog
  for Close Others / Close All. Implemented as Kit `Tab`s in an `h_flex` (not
  `TabBar`) because `TabBar::children` requires bare `Tab` and cannot host
  `ContextMenu`.
- **Tab reorder:** drag a tab within its own strip (editor among editor;
  terminal among terminal). Drop target shows a left `drag_border`; trailing
  empty zone accepts a drop at the end. No cross-strip drag, no
  detach-to-window, no editor splits. Order persists via existing session
  save (`open_files` / terminal cwds).
- **Tree drag-to-move:** drag a file/folder row onto a folder row (file row →
  its parent; blank panel space → first root). Hovered target gets the
  `drop_target` bg; preview is icon + name. No drag for placeholder/create/
  rename rows or multi-root roots; self, own-subtree and same-folder drops
  are rejected via `can_drop` (no highlight). Instant, no confirm; name
  collision / fs error → toast.
  Folder rows expand/collapse on **click** (mouse-up), not mouse-down: the row's
  mouse-down selects + focuses and stops propagation so Kit's mouse-down toggle
  never runs, which keeps a drag from expanding the folder (`ToggleDir`).
  Multi-root: root rows are draggable too but only **reorder** (drop on another
  root = insert before it; blank space = last); a root can't be moved into a
  folder and a file can't reorder roots. No-op positions get no highlight. Every row consumes the drop (no `can_drop`):
  a refused drop is ignored inside the handler, so it can't fall through to the
  panel's blank-space handler or leave the hover painted.
- **File icons (tree + editor tabs + agent mentions):** full Material Icon
  Theme under `assets/icons/material/` (MIT; see its `LICENSE`) with
  associations from `assets/icons/material-icons.json`, via
  `file_tree::tree_icon`. Same helper for tree rows, open editor tabs, Agent
  `@` mention rows, and file/folder attachment chips in the composer and user
  bubbles (raster attaches stay as image thumbs). Render with `img()` so
  upstream fills survive — GPUI Kit `Icon`/`svg()` alpha-masks and re-tints.
  Do not replace Lucide for other UI chrome.
- **File tree indent guides:** vertical 1px lines in each ancestor column
  (`sidebar_border`) through every descendant row (including the last child),
  so a folder's rail runs the full height of its contents. Multi-root: col 0
  stops before the next workspace root (no line between roots). Each deeper
  level adds one `INDENT_COL`. Guide segments use a 1px vertical overlap;
  ListItem `py_0`/`px_0` so hover is full-bleed to the panel edge; row content
  keeps `px_3`. A narrow panel truncates labels instead of compressing the tree.
- **Gitignored entries:** still listed; icon + label at 60% opacity (not
  hidden). Matched via `ignore` + each root's `.gitignore` tree.
- **Title bar:** macOS shows the centred window title; Windows/Linux show the
  in-window `AppMenuBar`. No theme toggle (dark only).
- **Keyboard shortcut hints:** one `Kbd` key cap per key with `gap_1`
  (`⌘` `O`, `Ctrl` `O`) — see `welcome::shortcut_keys`. Never a single cap with
  the keys run together.
- **Empty states:** don't show chrome that has nothing to say. The file tree
  only while a folder is open and the sidebar is not minimized; the welcome
  screen replaces the workspace when nothing is open; the terminal panel is
  omitted entirely when minimized (no empty strip). Status bar only while a
  folder/workspace is open (panel toggles; path / Ln·Col / language when a
  file tab is active) — not on welcome. Language servers: one status-bar
  `BrainCircuit` icon (badge from live servers). Popover lists only installed /
  remembered servers plus the active tab’s default (not every language). Row:
  status mark (fixed 7×7 **square**, block style — not a circle or collapsed
  bar) + name; Download when not app-local, Trash when app-local. A system
  PATH binary must not hide Install after Remove. Status-bar `BrainCircuit`
  badge is the same square (Kit’s `Badge::dot` is round — do not use it).
  Install downloads into the app data dir (not the user toolchain).
- **Bottom terminal panel:** Kit `v_resizable("editor-term-split")` under the
  **editor column only** (file tree stays full height in `main-split`); when
  the sidebar is hidden, under the full editor body. Requires an open
  folder/workspace (not available on welcome). Open/closed + height remembered
  in `config.terminal_visible` / `terminal_height` (default closed, 200px).
  Text selection: drag (double-click word, triple-click line); inverted
  fg/bg highlight. Copy/paste match OS terminals: macOS `⌘C` / `⌘V`;
  Linux/Windows `Ctrl+Shift+C` / `Ctrl+Shift+V` (plain `Ctrl+C` still goes
  to the PTY; `Ctrl+Shift+V` wins over Toggle Preview while the terminal is
  focused). Successful copy toasts "Copied". Hovering a URL shows a continuous
  underline clear of the glyphs (`font_size` + 5px) + pointer cursor;
  Ctrl/Cmd+click opens `http(s)://` / `www.` / OSC-8 links in the system
  browser.
  Header: tab strip (title = folder name; drag to reorder within the strip) +
  `Plus` (new) + `Minus` (minimize).
  Toggle (`Ctrl+\`` / status-bar) shows/hides without spawning; if no sessions
  yet, creates the first. **New Terminal** (`Ctrl+Shift+\`` / `Plus` / View menu)
  always spawns a tab. Multi-root: root picker only when creating a session
  (↑↓ / Enter / Cancel), not when un-minimizing. Tab cwds, visibility, and
  panel height are stored per workspace in `sessions` (shells respawn).
- **Agent panel (right):** Kit `h_resizable("agent-split-…")` around the
  whole workspace body, so the panel runs full height beside sidebar +
  editor + terminal. Only while a folder/workspace is open; omitted
  entirely when hidden (no empty strip). Width + visibility per workspace in
  `sessions` (default hidden, 380px). Toggle: status-bar `Bot` (`.selected`
  while shown), View > Toggle Agent Panel, `⌘⌥B` / `Ctrl+Alt+B`; header
  `PanelRightClose` hides. Header: `PANEL_HEADER_ROW_H`, `border_b`, 7×7
  status square (success ready, warning starting/working, danger failed,
  muted otherwise) + **session title** (active chat title, or `New chat`;
  activity suffix while busy; Compacting… during agent compaction) +
  muted/warning/danger `text_xs` context used/size (e.g. `53K / 200K`;
  `— / 200K` seeded on open/switch until ACP `usage_update`) + ghost
  `Plus` New Chat. Chat-list rows append last-known used/size beside the
  relative time when available (process memory only). Under the header, an optional `border_b` strip of
  two Kit `Progress` bars (`5h`, `Week`, `text_xs` label + percent):
  `theme.muted_foreground` <70%, `theme.warning` 70–90%, `theme.danger`
  ≥90%; hidden until data arrives. Background
  `theme.sidebar` like the other panels. Header also has a ghost
  `MessagesSquare` chat list (Popover, 300px): rows are Kit `ListItem`s —
  title (`text_sm`, truncated) over relative time (`text_xs` muted), active
  row `.selected`, ghost `Pencil` rename (dialog + `Input`; outline
  **Generate** runs a silent ACP turn for a title for **any** chat when
  Ready (uses the open transcript, a per-session cache from chats opened
  earlier, or loads that chat then generates) — Cancel outline · Generate
  outline · Rename primary) + ghost
  `Trash` (confirm: Cancel outline · Delete danger); rows and New Chat
  disabled while a turn runs. Empty list: muted "No chats in this
  workspace yet." Loading a saved chat shows Spinner + "Loading chat…"
  until the replay lands; reconnecting keeps the old transcript on screen.
  A background workspace's agent asking for approval raises a warning toast
  (`notify_warning`). Transcript in Kit
  `MessageScroller` (tail follow + jump-to-latest): user text in a
  `Secondary` `Bubble` aligned end as selectable `TextView` markdown (same
  selection as agent replies); agent replies as streamed
  `TextView` markdown (selectable); thoughts muted `text_xs` with a left
  `border_l_2` rule; tool calls as bordered rows (Lucide kind icon, title,
  muted path, right status: Kit `Spinner` while running, `Check` success,
  `CircleX` danger) — consecutive tools (e.g. shell execute) sit tight
  (`MessageScroller` row `pb_2`, tool entry `py_0p5` / card `py_0p5`, not
  section `pb_8`);   after a turn that changed files (git dirty delta since turn start —
  including shell create/edit/delete — and/or ACP write / Edit/Move/
  Delete tools), a full-width **N Files** strip for the latest turn is
  pinned on top of the composer / chat input (`border_t`, `px_3` like
  header/usage — not an inset rounded card; above Send, not under usage
  and not mid-transcript); starts collapsed; chevron expands; Material path
  icon + basename, green `+N` / red `-M`; row click opens one; Source Control
  discard drops matching paths from the strip; stored on
  `AgentChat.file_changes` and restored after ACP `session/load`); plan as
  7×7 status
  squares + text (done = muted + line-through);
  notices `text_xs` muted or danger. **Permission prompts
  are inline** on the tool row (not a modal — several can be open while
  updates keep streaming): one button per agent option, `.small()`,
  allow-once `.primary()`, allow-always `.outline()`, reject
  `.danger().outline()`.   Composer: optional wrap-row of attachment chips
  (Material icon + basename / `file:n-m`, or 40×40 image thumbs — no leading
  `@` on chip text) with ghost `CircleX` remove — chips are from the Attach
  (`Paperclip`) button, paste, **drag-drop** onto the prompt (file-tree
  row, editor tab, or OS file explorer — `drop_target` highlight), or
  **`⌘L` / `Ctrl+L`** / Edit > Add Selection to Agent / editor right-click
  **Add Selection to Agent** (selection → `file:N-M` chip, or whole file if
  the caret has no range); never from `@` mentions; `@` in
  the textarea opens a slim bordered mention menu (`text_xs`, scrollable
  ~200px): empty `@` lists every root child except build/deps skips
  (`.git`, `node_modules`, `target`, …); typing fuzzy-filters (≤8 hits);
  ↑↓ move (scrolls selection into view); click/Enter replaces the `@query`
  with a Kit **inline token inside the prompt Textarea** (Material
  folder/file icon + basename, or a line-height image thumb — not a path
  string and not the paperclip chip row); send resolves those to ACP
  resource/image blocks; →/Tab dives (query gets a trailing `/` and lists
  that folder); empty `@` lists nested folders (not only top-level) plus
  root files; Esc closes; then auto-growing
  `Textarea` (Enter sends when the menu is closed, Shift+Enter newline;
  paste image / file paths become chips); then **two** composer strips:
  (1) session config row when ACP advertises options — each tap is ghost
  `.small()` with a Lucide icon + current value (tooltip = ACP option
  name): Mode icon follows the selection (`ShieldQuestionMark` Ask,
  `Sparkles` Auto, `ListTodo` Plan, `Pencil` code/accept-edits, else
  `Shield`); Model family `Cpu` (+ Version `Tag` when names parse, else
  one Model `Cpu` — not `Bot`, which is the panel/status toggle); Mode
  list omits Claude `auto` when the selected model is older than 4.6
  (agent still advertises it); Context `Maximize2` when size variants exist; Fast
  `Zap` + Kit `Switch` labeled `Fast`; other options by ACP name with
  heuristic icons (`Brain` thought/effort, `Users` agent/persona, else
  `SlidersHorizontal`) — Popover upward, `ListItem`s,
  disabled while a turn runs or the session is not Ready; Prefer
  `configOptions` over legacy `modes`. Config band is always
  `AGENT_COMPOSER_CONFIG_ROW_H` (36px) with horizontal scroll (no wrap)
  so open/switch/first options do not change composer height; last
  options are kept across reconnect and not cleared when an open omits
  them. (2) action
  row: ghost `.small()` Attach (`Paperclip` — OS multi-file picker;
  folders via `@`) + muted hint (`Enter to send`, or `Enter to queue` /
  `N queued · Enter to queue` while busy) + primary Send (`ArrowUp`,
  label **Queue** while a turn runs — always enqueues when busy) and,
  while busy, a sibling `.danger()` Stop (solid square block, not Lucide
  stroke `Square`). Queued prompts stay out of the transcript until
  drained FIFO when the current turn ends (cleared on disconnect /
  suspend) so the in-flight reply stays one bubble; while waiting they
  show in a bordered strip **under** the composer (muted header + one
  truncated line per item + ghost `CircleX` remove).
  Attachment chips bottom-align in the wrap row (file/folder chips share a
  baseline with taller image thumbs); ghost `CircleX` remove sits on the
  upper trailing corner of each chip/thumb (not inline in the flex row).
  Composer stays mounted while a
  transcript is shown / session is opening (Send disabled until Ready) so
  history does not jump when the agent finishes connecting. User bubbles
  show attachment refs the same way (Material icon + basename, or image
  thumb only — never Lucide Folder/File/Image, never a leading `@`). While a turn runs, a Kit `Spinner` + activity
  label also sits at the bottom of the transcript pane (in addition to the
  header spinner), so a long chat still shows work in progress. Non-ready
  states are one centred column: Spinner + "Starting…"; not installed →
  primary Install (`Download`, loading while npm runs) + danger error line;
  login → one button per agent login method (first primary, others outline)
  + outline Retry; failed → danger message + primary Retry.
- **External file changes:** a clean open tab silently reloads when its file
  changes on disk; a tab with unsaved edits keeps them and shows one
  `notify_warning` ("<name> changed on disk — kept your unsaved edits"). The
  tree keeps every folder's expand state when it re-lists after a
  create/remove/rename. No dialogs, no auto-save.
- **Sidebar toggle:** file tree can be hidden while the workspace stays open.
  Width + visibility remembered per workspace in `sessions` (default 260px /
  visible). Toggle via status-bar icon, View > Toggle Sidebar, or `⌘B` /
  `Ctrl+B`.
- **Project Search (sidebar):** left panel switches between Explorer (file
  tree), Search, and Source Control (same width — not a second column).
  Toggle via the sidebar top strip (`FolderTree` / `Search` / `GitBranch`,
  `.small()`), View > Explorer / Search / Source Control, Edit > Find in
  Files, or `⌘⇧F` / `Ctrl+Shift+F`.
  Find + replace fields; toggles for case / whole word / regex (Lucide
  `CaseSensitive` / `WholeWord` / `Regex`). Results list: file headers
  (Material `tree_icon` + workspace-relative path) + match rows (`Ln` +
  line preview with the match span on `theme.warning` fill);
  click opens a preview tab, selects the hit, and fills every match in that
  file (`RangeDecoration`). Selecting non-empty text in an editor soft-fills
  other exact occurrences in that buffer (`accent` @ ~22% — distinct from
  search’s `warning` hits). Replace in File applies
  to the selected result's file; Replace All confirms first (choice dialog).
  Open editor buffers win over disk for search/replace; closed files write
  UTF-8 to disk. Honors `.gitignore` (even without a `.git` dir); skips
  binaries / media extensions / well-known build dirs (`node_modules`,
  `target`, …) / files >1 MiB. Caps at 10k matches / 2k files; UI paints at
  most 500 hit rows (shows truncated).
- **Source Control (sidebar):** strip order Explorer | Source Control |
  Search. Stacked flush sections (no cards) — one per unique Git worktree
  (shared `.git` → one section); bottom-border separators only *between*
  repos (none for a single repo or under the last). Per
  section: repo name + branch, outlined Pull / Push (`Pull (N)` /
  `Push (N)` when behind / ahead of upstream), multi-line commit
  `Textarea` + outlined Suggest + Commit (disabled while message empty
  or nothing selected), collapsible changes list with select-all +
  per-file checkboxes (default none selected; Commit/`Suggest` use
  selection only; `git add -- <paths>` then commit). Suggest: when the
  workspace Agent panel has a Ready ACP session, one-shot prompt (any
  connected agent — not Claude-only) fills the box from selected-path
  diff + recent subjects; otherwise the path heuristic draft. Suggest
  spins while the turn runs; never auto-commits; tools/cancel fall back
  to the heuristic. Change rows:
  checkbox + Material `tree_icon` + path, status letter on the right,
  ghost Open File + Undo discard (confirms). The change whose
  diff/conflict tab is active is highlighted (`ListItem::selected` +
  accent row fill) — separate from commit checkboxes. Discard-all on the
  changes header (confirms; `reset --hard` + `clean -fd`). Header Refresh
  is ghost icon-only. Pull is `--ff-only`. Click a change opens a
  **read-only** side-by-side preview tab (equal HEAD | Working Tree
  panes; locked scroll; opens scrolled to the first change; deleted
  lines danger-fill / inserted success-fill; overview ruler ticks; not
  editable). System `git` on PATH; errors toast.
  Refresh on view open / workspace change / after actions / Refresh /
  agent worktree touches (`fs/write_text_file`, Edit/Delete tool done,
  end-of-turn shell file delta).
  **Conflicts:** detect in-progress merge / rebase / cherry-pick
  (`MERGE_HEAD` / rebase dirs / `CHERRY_PICK_HEAD`). Persistent status
  line under Pull·Push, e.g. `Merge in progress — 2 conflicts`
  (`theme.danger` while any `U`; `theme.warning` + `ready to commit`
  when the op remains but conflicts are cleared). Conflict rows sort
  first. Row click / Open open a read-only **Current | Incoming**
  side-by-side tab (index stages `:2:` / `:3:`; locked scroll; opens at
  the first change; deleted/inserted fills + overview ticks; resolved
  hunks mute). Each unresolved hunk shows an outlined **Accept** chip
  aligned with the text column of that hunk’s first line (conflict panes
  hide line numbers + folding; scroll-tracked overlay). Last hunk writes
  the merge + `git add`. Toolbar (`.small()`): status `N/M changes` +
  whole-file **Accept Current** / **Accept Incoming** (`checkout
  --ours|--theirs` + `add`). Accept lives in the editor only — not on
  panel change rows. After resolve, conflict tab closes; open worktree
  buffers reload. While an op is in progress or any `U` remains: disable
  Pull, Push, Discard, Discard all; Suggest disabled. Commit stays
  available for merges (selection → `git add` then commit) but
  **refuses** if any selected path still contains conflict markers
  (`<<<<<<<` / `=======` / `>>>>>>>`); Commit disabled during rebase /
  cherry-pick (continue from the terminal). Empty commit box seeds from
  `MERGE_MSG` when a merge starts. Not: Accept Both, abort buttons,
  persistent index staging, branch switch, stash, amend.
- **Notifications:** bottom-right (`theme.notification.placement =
  BottomRight` in `theme::apply`), with `margins.bottom` clearing the status
  bar (`STATUS_BAR_HEIGHT` + 16px edge). Don't set placement/margins per toast.
  Runtime failures that would otherwise only hit the log (dialog errors, PTY
  spawn, save/open failures, cancelled save during close/quit) use
  `notify_error` so the user sees why nothing happened.
- **Close Folder vs Close Workspace:** File menu label follows
  `Workspace::is_named_workspace()` (workspace file or multi-root →
  "Close Workspace"; single folder → "Close Folder"). Disabled on welcome
  with nothing open.
- **Links vs buttons:** navigation-like actions on the welcome screen
  (Open Folder, recent folders) are `Button::link()`; commands in dialogs are
  regular buttons with one `primary`.
- **Secondary text:** paths and hints use `muted_foreground` and a smaller
  size (`text_sm`/`text_xs`).
- **Workspaces:**
  - Naming: a single folder shows its name; a workspace file or several folders
    show `<name> (Workspace)` (`Untitled (Workspace)` before it is saved). Used
    for the window / title-bar title only (no duplicate label above the tree).
  - Session restore: reopening a folder or workspace (welcome recent, Open, or
    CLI) restores its last open tabs (cursor + scroll) — including Source
    Control diff/conflict preview tabs when those paths are still dirty —
    and expanded tree directories from `config.json` `sessions`. Multi-root
    roots stay collapsed unless they were expanded when saved. Unsaved
    edits are not restored; cold start still shows the welcome screen.
  - Window geometry: `config.json` stores last window width/height/origin and
    maximized flag; launch opens with that frame (centered if no origin yet).
    Saved on close and debounced while resizing.
  - Multi-root tree: each root is a top-level item; a root missing on disk is
    shown disabled as `<name> (missing)` with a folder icon.
  - Git dirty tint (same palette as Source Control badges): file label
    `warning` for modify/rename/copy, `success` for untracked/added,
    `danger` for delete/conflict; folders use a roll-up (conflict >
    modify family > untracked). Updated whenever Git status reloads.
  - Actions on a tree item live in its right-click menu (`Tree::context_menu`):
    files — Rename, Delete; folders — New File…, New Folder…, Rename,
    Delete; multi-root roots also get "Remove Folder from Workspace".
    Right-click empty space in the tree panel (not on a row) → New File /
    New Folder under the workspace root (single-root folder, or first
    root when multi). Mini toolbar at the top of the tree (`justify_between`,
    `border_b`, height `PANEL_HEADER_ROW_H`): left = folder / workspace
    `display_name` (`text_sm`, medium, truncated); right = ghost New File /
    New Folder icons (`assets::IconName::FilePlus` / `FolderPlus`,
    embedded via `icon_assets!` + `AppAssets` in `main.rs` — not in
    default Kit `Assets`). Creates in the selected directory (or
    parent of a selected file); with no selection, the first workspace
    root. Window title still shows the same name (plus ` (Workspace)` when
    named/multi-root).
    With the tree focused (`Tree` key context): `F2` renames; `Delete` /
    `Backspace` moves to the OS Trash (toast, no confirm). **Rename**,
    **New File**, and **New Folder** are inline in the tree (Kit `Input`
    on the row or a temporary create row; Enter/blur commits, Escape
    cancels; empty create name cancels).
- **Welcome "Start" actions:** two columns separated by a vertical
  `Separator` — left: Open Workspace…, New Workspace…; right: Open Folder…
  (`⌘O` / `Ctrl+O`), Open File…, New File (`⌘N` / `Ctrl+N` outside Agent).
  With the Agent composer focused, `⌘N` / `Ctrl+N` is New Chat (`Agent` key
  context); File → New File stays New File. File menu keeps
  the same order stacked (workspace group, separator, folder/file group).
  Distinct Kit icons: `LayoutDashboard`, `FolderClosed`, `FolderOpen`, `File`,
  `FileText`.
- **Menu actions vs focus:** Title-bar/`AppMenuBar` restores the prior
  focus then defers `dispatch_action`, then dismiss restores focus again.
  Explorer moves focus to the Tree (stable). Git left focus on the chrome
  button (ephemeral) — Close was dropped after paint. Fix: focus the app
  handle when opening Git; Close defers one tick; app-level `cx.on_action`
  fallbacks use a `WeakEntity<IsengardApp>` (window root is Kit `Root`,
  not the app).
- **Menus by state** (omit, don’t grey out — fewer dead options):
  - **Welcome (no folder):** File = open/new only (+ Save / Close Tab if
    untitled tabs). No Add Folder, Save Workspace As, Close Folder. Edit /
    View menus hidden until a tab or folder exists (font zoom is
    editor-only — not on empty welcome).
  - **Folder or workspace open:** File also Add Folder to Workspace…,
    Save Workspace As…, Close Folder / Close Workspace. Edit includes
    Find in Files. View includes sidebar / Explorer / Search / Source
    Control / terminal / Agent panel.
  - **Tabs open:** Save, Save All, Close Tab, Edit clipboard/find, Toggle
    Preview, Increase / Decrease / Reset Font Size.
- **New File:** opens an in-memory `Untitled-N` tab (welcome or workspace); no
  file on disk until Save. Save dialog defaults to the workspace target dir
  (tree selection, else active root). Close tab / quit asks Save / Don't Save
  when dirty. Linux title-bar × runs the same quit confirmation as the WM close.
- **Workspaces before folders** wherever both appear (Start actions, recent
  lists, menus).
- **Recent lists:** one titled section per kind ("Recent workspaces", then
  "Recent folders"); a section is hidden when empty, and when all are empty
  a single "Recent" section says so. Rows: link-style name, muted parent path,
  ghost close × to remove.
- **Logo:** use `branding::LOGO_MARK` (block "I" + amber cursor) on the dark
  UI; never recolor or round it. Brand colors (logo only, not UI): tile
  `#0b0b0c`, mark `#ececec`, cursor `#f5a524`.
- **Slogan:** welcome subtitle and About / bundle blurb —
  "The already configured editor" (no settings-panel cosplay). About may add
  one short supporting line; don't invent a second tagline.
- **Config, not Preferences UI:** durable knobs (e.g. LSP enable/command)
  live in `config.json`. Expose **Open Config File…** in the Isengard menu;
  do not add a settings / preferences panel.

- Status-bar toggle buttons: active = info-blue icon (`theme.info`), never the gray `selected` fill.
