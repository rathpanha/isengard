# Current status

Phases 1–4 of the original plan are done and the UI has been ported from egui
to GPUI Kit. Phase 5 is the Agent panel v1 (external ACP agents, Claude Agent
by default) — see **Agent panel** below.

Working features (verified by screenshots on macOS: welcome screen, file tree,
tabs, highlighted editor, status bar):

- **Welcome:** Start is two columns (workspace | folder/file) with a vertical
  rule; New File (`⌘N` / `Ctrl+N`) opens an untitled buffer (save dialog only
  on Save / dirty close). Recent workspaces then recent folders (max 8 each,
  × to remove; empty lists hidden). Details in [design.md](design.md).
- **Workspaces:** multi-root folders in one window; `.isengard-workspace` JSON
  (relative folder paths); Add Folder / Open / Save As / Close Folder or
  Close Workspace (label tracks single-folder vs named/multi-root); root
  context menu remove; switching or quitting closes tabs (unsaved +
  untitled-workspace prompts). File **Save As…** (`⌘⇧S` / `Ctrl+Shift+S`)
  writes a copy and rebinds the tab (original file untouched). Session
  restore (tabs with cursor/scroll + expanded tree dirs) per
  folder/workspace in `config.json` `sessions` (tabs + dirty/untitled
  text, sidebar strip, search query, git drafts/selection; terminal
  layout/cwds only — clean shells; cold start still welcome) — see design
  for restore rules. Save cancelled mid-close/quit toasts instead of
  failing silently.
- **File tree:** Kit `Tree` while a workspace is open; lazy load; dirs first,
  case-insensitive; hides `.git` / `.DS_Store`; full Material Icon Theme;
  gitignored rows shown muted; dirty git paths tint the label (modified/
  rename → warning, new/untracked → success, deleted/conflict → danger;
  parent folders roll up); indent guides (design owns the pixel rules).
  Context menu: files Rename / Delete; folders New File / New Folder /
  Rename / Delete (multi-root roots also Remove Folder from Workspace).
  Right-click empty space in the tree panel → New File / New Folder at
  the workspace root (sole root when single-root; first root when multi).
  Mini toolbar above the tree: New File / New Folder (selection's dir,
  else root) so create still works when the tree has no blank space.
  Rename / New File / New Folder are inline in the tree (Enter/blur
  commit, Escape cancel). Drag a row onto a folder (a file row → its
  folder; blank space → first root) to move it; open tabs follow;
  self/descendant and same-folder drops are refused. Multi-root: drag a root
  row onto another root (insert before) or blank space (last) to reorder —
  the first root is the agent's primary folder, so it reconnects (chat
  resumes) and the order is saved to the `.isengard-workspace` file. Rows
  inside a folder stay sorted (dirs first, A–Z); no manual order. Clicking a tree row focuses the Tree (preview
  open does not steal focus); `F2` rename; `Delete` / `Backspace` → OS
  Trash. Double-click focuses the editor.
- **Tabs:** Kit `Tab`s in `h_flex` (not `TabBar` — needed for per-tab
  `ContextMenu`); Material file icon + name as tab children; Close / Close
  Others / Close All; preview tabs; dirty mark (cleared by undoing back to
  the saved text); Markdown/SVG Edit–Preview;
  raster image preview tabs; drag within the strip to reorder (persists in
  session). Details in design.
- **Editor:** Kit `Editor` + tree-sitter for every language Kit vendors via
  `tree-sitter-languages` (GraphQL/TSX highlight queries patched — Kit ships
  empty/delta queries); line numbers, indent guides, search; unknown
  extensions stay Plain Text. Syntax: Ayu Darker; editor/gutter bg = sidebar.
  Markdown/agent code blocks: same tokens on muted gray (`theme::text_view_style`).
  **IntelliSense (LSP):** not bundled into the app binary. **Install** downloads
  into `dirs::data_dir()/isengard/lsp/` (bin + pkg) — never `npm -g` / brew /
  pip / rustup into the user toolchain. Resolve: app-local first, then system
  `PATH`. Recipes: native release assets (rust-analyzer, taplo, marksman, zls,
  lua-language-server), `GOBIN=…/lsp/bin go install` (gopls), and
  `npm install --prefix …/lsp/pkg/…` for Node servers (needs system `node`/`npm`
  as runtime only). `pylsp` / `clangd` / etc. stay PATH-only (no Install).
  Status bar: one LSP icon → short list with Install / Remove (Remove deletes
  app-local files only; ready/PATH probes are cached — never re-run every
  paint). Go-to-definition defers cross-file opens (avoids GPUI nested-update
  panic). Code Actions (editor context menu / Kit), Format Document
  (`⇧⌘I` / `Ctrl+Shift+I`; format-on-save default on via `lsp.format_on_save`),
  Rename Symbol (F2 in editor, dialog), `textDocument/didSave` after write.
  Saving `config.json` with `lsp` changes restarts servers and re-attaches
  open tabs. `config.json` → `lsp`; Open Config File…. Untitled buffers skip
  LSP until saved.
- **Terminal:** multi-tab bottom panel while a folder/workspace is open (not
  on welcome). Toggle show/hide (`Ctrl+\``); New Terminal (`Ctrl+Shift+\`` /
  header `+`) always adds a tab. Multi-root asks for root only when creating
  a session, not when restoring a minimized panel. Tab cwds, visibility,
  and height persist per workspace session (fresh shells on restore — no
  scrollback); drag tabs within the strip to reorder. Mouse drag selects
  (double = word, triple = line); copy/paste via OS terminal chords
  (`⌘C`/`⌘V` or `Ctrl+Shift+C`/`V`, chord wins over Toggle Preview when
  focused); URL hover continuous underline + pointer; Ctrl/Cmd+click opens.
  Scrollback UI still minimal.
- **Sidebar:** hide/show without closing the workspace (View > Toggle
  Sidebar / `⌘B` / `Ctrl+B` / status-bar icon); width + visibility persist
  per workspace session. Explorer ↔ Search ↔ Source Control via top strip
  (`⌘⇧F` / Find in Files for Search; View > Source Control for Git).
- **Project search:** parallel `ignore` walk + `regex` / `aho-corasick`
  prefilter; cancels in-flight scans on new query; skips `node_modules`/
  `target`/media/binaries and files >1 MiB; open buffers preferred over disk;
  Replace in File / Replace All (confirm); click match to open (result row +
  editor highlight).   Results list paints at most 500 hits. Selecting text in
  an editor soft-highlights other exact occurrences in that file
  (debounced; skipped for buffers >512 KiB).
- **Source Control:** CLI `git`; stacked multi-root repos (flush sections,
  border only between repos); checkboxes for commit selection (select-all
  + per-file); commit box height auto 1→6 rows + drag floor to 20; Suggest
  icons — heuristic (`WandSparkles`) and Claude (Material icon; enabled
  only while Agent `can_silent_prompt`, throwaway session, no heuristic
  fallback; disabled = no click; scrub drops Co-Authored-By /
  Generated-with / Claude-Session trailers) / Commit from selection;
  Open File + discard;
  Pull(N)/Push(N); side-by-side diff preview (scroll locked; active row
  highlighted in the change list; restored across sessions while still
  dirty); refreshes when the agent touches the worktree (no manual
  Refresh). Merge/rebase/cherry-pick surfaced as a status line + `U` rows
  first (red while conflicts remain, yellow when cleared); conflict opens
  Current | Incoming tab (locked scroll; Accept chip on each hunk +
  whole-file Accept); Commit refuses leftover markers
  (rebase/cherry-pick → terminal). No persistent staging, branch UI, or
  Accept Both.
- **File watching:** every workspace root is watched; edits made outside
  Isengard (agent shell tools, `git checkout`, formatters, other editors) —
  debounced 300 ms — reload **clean** open tabs, refresh loaded tree
  directories on create/remove/rename (expand state kept), and refresh Source
  Control. A tab with unsaved edits is never overwritten: one warning toast
  per divergence, and Save still writes the buffer.
- **Save / close:** Save, Save All, close-tab and quit confirmations when
  unsaved (New File is buffer-only until Save; save dialog uses active tree
  dir / root). Linux in-window close × confirms unsaved changes. Save All
  keeps going past a failed file, lists every failure in one toast, and then
  cancels the pending close / quit / switch.
- **Agent panel (v1):** right-hand panel (View > Toggle Agent Panel /
  `⌘⇧B` / `Ctrl+Shift+B` / status-bar `Bot`) chatting with an external agent.
  over the Agent Client Protocol, like Zed's external agents. Default is
  Claude Agent (`@agentclientprotocol/claude-agent-acp`, pinned 0.94.0):
  **Install** runs `npm install --prefix …/isengard/agents/claude` (needs
  Node.js; never global), else `claude-agent-acp` on `PATH`. Uses the user's
  Claude Code login — no API key in Isengard. Not signed in → login buttons
  run the agent's own `--cli auth login …` in a terminal tab; when that
  program exits the panel reconnects by itself (Retry stays as a fallback).
  Custom agent via `config.json` `agent.command` / `args` / `env`. Streams
  markdown replies + thoughts, tool-call rows (kind icon, path, status),
  nested subagent rows (Explore/Agent progress while the main turn waits —
  ACP `clientCapabilities.subagents`; Agent settings **Subagent model**
  sets Claude’s `CLAUDE_CODE_SUBAGENT_MODEL`),
  end-of-turn **N Files** change summary as a full-width strip on top of
  the composer (above the chat input; latest turn; collapsed by default;
  `+/-` from git worktree delta since turn start — covers shell
  create/edit/delete — plus Write/Edit/Delete tool paths; expand then
  row-click to open; Git discard removes those paths; strip state persists
  in the workspace session and is spliced back after `session/load` /
  editor restart), plan checklist, inline
  permission buttons (Approve Plan’s keep-planning option labeled **Keep
  planning and View the plan**, outline not danger — opens the plan `.md`
  in Preview), Stop (`.danger()`, `session/cancel`), New Chat.
  While a turn runs, Spinner + activity (tool title, or Thinking/Writing/…)
  appear at the bottom of the transcript only (header stays the session
  title; status square still turns warning). Long chats keep ~half-pane
  scroll room under the latest
  message so the end is not glued to the composer.   Composer: type `@` for a
  slim file/folder picker (filter; click/Enter
  inserts an inline token in the prompt Textarea — Material folder/file
  icon + basename, or image thumb; send resolves to ACP blocks; →/Tab
  dives; empty `@` / filters see workspace roots + nested folders via a
  plain FS walk); type `/` for agent slash commands from ACP
  `available_commands_update` (filter; Enter inserts `/name`). Chip row is
  Attach / paste / drag-drop (tree row, editor
  tab, or OS explorer onto the prompt) / `⌘L`·`Ctrl+L` / editor or terminal
  right-click Add Selection to Agent (editor selection or whole file;
  terminal selection → `Terminal` chip) only — bottom-aligned;
  raster paths and pastes show thumbnails; Paperclip opens the OS
  multi-file picker. Mention tokens and paperclip chips also show as
  icon/name or thumbs in the user bubble.
  Composer is shown while history loads / session opens (Send disabled
  until Ready) to avoid layout jump. Files embed when allowed; folders are
  resource links;
  images go as ACP `image` when the agent allows (ICO/BMP/TIFF re-encoded to
  PNG for Claude). Agent file reads use the open buffer
  (unsaved text) before disk; writes go to disk and reload the open tab as
  saved. Visibility + width persist per workspace. Shell tools run inside
  Claude Agent (not Isengard's Terminal panel) — ACP client `terminal/*` is
  unused by the pinned adapter, so we do not implement it.
  **Multi-root:** the agent runs in the first root and gets the other roots
  as ACP `additionalDirectories` (new/load/resume; skipped if the agent does
  not advertise it). Adding/removing a folder reconnects the live agent
  (after a running turn) and resumes the chat with the new roots.
  **Chats per folder/workspace:** each folder/workspace has its own panel and
  chat index (`sessions.<key>.agent_chats` / `agent_active`; only chats
  started in Isengard — not `claude` CLI chats). Header chat list
  (`MessagesSquare`): title (first-prompt placeholder, then silent AI title
  on a throwaway session after the first turn; Claude sends no
  `session_info_update`) + relative time, click to open, pencil rename
  (manual or AI Generate for any chat — live, cached, or load-then-generate),
  delete with confirm. Header: session title + context used/size
  (`— / {window}` on open / switch until ACP `usage_update`, then live fill)
  + ghost `Plus` New Chat (`⌘N` / `Ctrl+N` while the Agent composer is
  focused; elsewhere still New File).
  Under the header, Claude plan usage accordion (chevron + 5h/Week; expand
  overlays the transcript for plan name, resets, optional Opus/Sonnet,
  product breakdown) from Anthropic's undocumented OAuth usage endpoint
  using the Claude Code token (macOS Keychain, else
  `~/.claude/.credentials.json`); access tokens are refreshed via
  `platform.claude.com` when expired/401 and written back (refresh tokens
  rotate). Best effort — hidden until the first good fetch (or when a custom
  `agent.command` is set). Failed fetches with no bars yet retry on a
  5→60s backoff; empty or partial API payloads keep the last good bars
  instead of blanking them.
  Compaction is agent-side (Isengard advertises
  `ClientSessionCapabilities.compaction`); we show Compacting… / a short
  notice — we do not summarize or reset the window ourselves.
  Header ghost `SlidersHorizontal` Agent settings (row: Utility model →
  Model → Version for Improve / titles / Git Suggest → global
  `agent.silent_model`; **Chat model** inherits the open chat). Composer:
  config row (icon+label taps:
  Mode icon tracks Ask/Auto/Plan/ accept-edits — Auto omitted for Claude
  models older than 4.6; Model `Cpu` / Version/Context; Fast Switch;
  Effort/agent/…) sits in a fixed-height band above Attach / Improve
  (`PencilSparkles` silent rewrite on a throwaway session — open chat
  context untouched, chat not marked `turn_running`; Suggest/title leave
  Send alone; Improve uses Stop + `.loading`; `@` chips + paperclip chips
  kept; review then Send) / Queue+Stop (horizontal scroll; disabled until
  Ready). Picks are remembered per folder/workspace
  (`sessions.<key>.agent_prefs`, seeded from global last-used `agent_prefs`)
  and re-applied to every chat opened — new, loaded, or after a restart.
  Follow-ups while a turn runs go into an outbound FIFO queue (strip
  under the composer with remove; drained when the turn ends).
  (`session/set_config_option`). Process-level agent still comes from
  `config.json` `agent.command` / `agent.silent_model` (Open Config File…).
  Switching workspace
  parks the old panel: a
  running turn finishes in the background (approval requests toast), then its
  agent stops; coming back resumes the chat (`session/resume`, no replay)
  with the transcript still in memory. After a restart the active chat
  reopens via `session/load` (history replays). Verified end to end against
  the real adapter on Linux (read + shell tool turn; load replays and the
  model keeps context; resume does not replay).
- **Chrome:** native macOS menu bar; in-window `AppMenuBar` on Windows/Linux
  (Isengard / File / Edit / View). Menu items omit by state (welcome vs
  folder/workspace vs tabs) — see design. Status bar while a workspace is
  open (panel toggles; path / Ln·Col when a file tab is active).
  Notifications bottom-right. Logo on welcome + platform app icon (see
  architecture Development setup / icon matrix). Main window size/position
  (+ maximized) restored from `config.json` on launch.
- **Theme:** JetBrains Mono Nerd Font; square corners; **dark mode only**;
  editor font-size zoom.

## Known issues / TODO

- LSP: servers are not inside the app binary (app-local download dir is
  separate). Command-only code actions (`workspace/executeCommand`) are not
  run — edit-bearing quick fixes / refactors are.
- Terminal scrollback UI still minimal (selection/copy/paste/URL-click work).
  Workspace switch clears all terminal tabs (re-prompts for a root if the
  panel was open).
- UI integration tests (`src/app/ui_tests.rs`) cover edit/undo dirty mark,
  Save, Save As, Save All with a failing file, dirty-session restore across
  folder switch, quit’s untitled multi-root workspace prompt, tab close with
  the unsaved dialog, file-tree create/rename/move, terminal toggle + new
  tab, Agent panel toggle/restore per folder, and agent file read/write
  against open buffers, chats per folder across switch + restart (resume vs
  load), and a turn finishing in the background after a switch. Agents must
  extend these for every new UI feature ([architecture.md](architecture.md) ·
  [`.cursor/rules/ui-test-coverage.mdc`](../.cursor/rules/ui-test-coverage.mdc)).
  `acp::client` tests drive a fake in-memory agent (prompt, file I/O,
  permission).
- File watching (`notify`, recursive on every root; skips `.git` /
  `node_modules` / `target`): on Linux inotify watches every directory, so a
  huge tree can hit the watch limit (logged, watching degrades). A reload of
  a clean tab resets its cursor/scroll/undo (same as the agent-write reload).
  Deleted files keep their open buffer. Changes made while the app is closed
  are not noticed.
- No "Open Recent" in the native menu (welcome screen only). Tree New File
  creates on disk after the inline name (distinct from menu New File /
  `⌘N`, which stays untitled until Save). Tree Delete always goes to Trash.
  Gitignore rules refresh on folder open / expand, not on every save of
  `.gitignore`. `.env` / `.gitignore` / lockfiles / Dockerfile use
  Bash/JSON/TOML aliases (no dedicated Dockerfile grammar compatible with
  GPUI's tree-sitter); GraphQL and TSX highlights are patched in
  `editor::highlights` because GPUI Kit ships an empty GraphQL query and
  only a TS-keyword delta for TSX.
- Workspaces: not yet verified by clicking — Add Folder, Save Workspace As,
  root context menu, switch dialogs. Workspace `settings` are stored but
  not applied. Session restore does not auto-open last workspace on launch
  (cold start stays welcome).
- Save All: an untitled buffer whose write fails after the Save dialog still
  stops the queue (on-disk files carry on and report together).
- Tree item ids are absolute paths; placeholder ids append `\0placeholder`.
- App icons for `cargo run`: macOS Dock + Windows `.exe` embed + Linux X11
  window icon are implemented; Wayland still needs
  `scripts/install-dev-icon.sh` once. Windows embed and Wayland panel matching
  are not verified on every machine/DE.
- `package.metadata.bundle.identifier` (`dev.isengard.editor`) is a placeholder.
- Syntax highlight theme (`assets/themes/`, Ayu Darker) is GPL-3.0; the project
  as a whole is also **GPL-3.0-only** (`LICENSE`, `Cargo.toml`). See
  [CONTRIBUTING.md](../CONTRIBUTING.md) — forks are fine, PRs are not accepted.

## Roadmap — Agent panel after v1

- Process-level agent switch in the panel (today: `config.json` /
  Open Config File… only). Mode/model pickers + chat rename + auto titles
  shipped.
- Interactive QA: FakeAgent UI tests cover permission answer, chat delete,
  Install/Login empty-state buttons, rename, and prompt send. Live smoke
  verified on this machine; re-run after adapter bumps: (1) Install or PATH
  agent, (2) Login → Retry, (3) one prompt reply, (4) one real permission Ask,
  (5) rename + delete chat.
