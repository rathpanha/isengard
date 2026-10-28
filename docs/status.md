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
  untitled-workspace prompts). Workspace file `settings` overlays
  `editor.fontSize` / `font_size`, `editor.tabSize` / `tabSize`,
  `editor.formatOnSave` (does not rewrite user `config.json`). File
  **Save As…** (`⌘⇧S` / `Ctrl+Shift+S`) writes a copy and rebinds the tab
  (original file untouched). Session restore (tabs with cursor/scroll +
  expanded tree dirs) per folder/workspace in `config.json` `sessions`
  (tabs + dirty/untitled text, sidebar strip, search query, git
  drafts/selection; terminal layout/cwds only — clean shells; cold start
  still welcome) — see design for restore rules. Save cancelled
  mid-close/quit toasts instead of failing silently.
- **File tree:** Kit `Tree` while a workspace is open; lazy load; dirs first,
  case-insensitive; hides `.git` / `.DS_Store`; full Material Icon Theme;
  gitignored rows shown muted; dirty git paths tint the label (modified/
  rename → warning, new/untracked → success, deleted/conflict → danger;
  parent folders roll up); indent guides (design owns the pixel rules).
  Context menu: files Reveal in File Manager (Finder / File Explorer) / Rename /
  Delete; folders New File / New Folder / Reveal / Rename / Delete (multi-root roots also Remove Folder from Workspace).
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
  LSP until saved. Command-only code actions run via
  `workspace/executeCommand` (and `workspace/applyEdit` when the server
  pushes edits).
- **Terminal:** multi-tab bottom panel while a folder/workspace is open (not
  on welcome). Toggle show/hide (`Ctrl+\``); New Terminal (`Ctrl+Shift+\`` /
  header `+`) always adds a tab. Multi-root asks for root only when creating
  a session, not when restoring a minimized panel. Tab cwds, visibility,
  and height persist per workspace session (fresh shells on restore — no
  scrollback); drag tabs within the strip to reorder. Mouse drag selects
  (double = word, triple = line); copy/paste via OS terminal chords
  (`⌘C`/`⌘V` or `Ctrl+Shift+C`/`V`, chord wins over Toggle Preview when
  focused); Tab / Shift+Tab go to the shell (not focus traversal); URL hover continuous underline + pointer; Ctrl/Cmd+click opens.
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
  dir / root) or while an agent turn or a terminal foreground process (Unix) is running
  (Quit Anyway / Keep Working). Quitting terminates each terminal's foreground
  program (SIGTERM, then SIGKILL after ~1s) and hangs up the shell, so dev servers
  don't keep their port.
  Linux in-window close × confirms unsaved changes. Save All
  keeps going past a failed file, lists every failure in one toast, and then
  cancels the pending close / quit / switch.
- **Agent panel (v1):** right-hand panel (View > Toggle Agent Panel /
  `⌘⇧B` / `Ctrl+Shift+B` / status-bar `Bot`) chatting with an external agent.
  over the Agent Client Protocol, like Zed's external agents. A finished or failed
  run raises an OS notification + sound when the app is in the background
  (`src/notify.rs`; Mixkit pop sound, logo icon; `notify-send`+`paplay`, `osascript`+`afplay`; no Windows yet). Default is
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
  permission buttons, Stop (`.danger()`, `session/cancel`), New Chat.
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
  `agent.command` is set). Polls every 5 min; failures back off (5→60s, or
  1→15 min after HTTP 429). The last good numbers are saved to
  `<cache dir>/isengard/plan-limits.json` (windows that have since reset are
  dropped on load) and shown **dimmed** at startup / while refreshes fail,
  with a "Showing saved numbers" note in the expanded view. No numbers yet
  while rate limited → a one-line "Plan usage unavailable — rate limited,
  retrying" note. Empty or partial API payloads keep the last good bars.
  Compaction is agent-side (Isengard advertises
  `ClientSessionCapabilities.compaction`); we show Compacting… / a short
  notice — we do not summarize or reset the window ourselves.
  Header ghost `SlidersHorizontal` Agent settings (row: Utility model →
  Model → Version for Improve / titles / Git Suggest → global
  `agent.silent_model`; **Chat model** inherits the open chat). Composer:
  config row (icon+label taps:
  Mode icon tracks Ask/Auto/Plan/ accept-edits — Auto omitted for Claude
  models older than 4.6; Model `Cpu` / Version/Context; Fast Switch;
  Effort/agent/… on the left; OpusPlan Switch pinned right when
  advertised (Opus in Plan Mode, Sonnet otherwise; Mode change re-pins
  alias when on / concrete Model when off — blocks Claude settings
  `opusplan` bleed; not in Model picker; hides Model/Version/Context
  while on)) sits in a fixed-height band above Attach / Improve
  (`PencilSparkles` silent rewrite on a throwaway session — open chat
  context untouched, chat not marked `turn_running`; Suggest/title leave
  Send alone; Improve swaps to `CircleX` cancel — silent-only, no chat
  Stop; `@` chips + paperclip chips kept; review then Send) / Queue+Stop
  on user turns only (horizontal scroll; disabled until Ready). Picks are remembered per folder/workspace
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
- **Updates (Linux):** checks GitHub releases at startup (≤ once a day; opt out
  with `updates.check: false`) and via Isengard > Check for Updates…. Dialog:
  Skip this version / Later / Update now. Self-installed copies (writable dir)
  download, verify (minisign signature + SHA-256) and swap the binary, then
  offer Restart now (unsaved-changes prompt first; workspace reopens). RPM/DEB
  installs download + verify, then **Install now** asks for the sudo password in a
  masked dialog, runs `sudo -S <manager> install -y <file>` with live output
  (wrong password re-asks) and restarts into the new binary; Copy command /
  Open in terminal remain as fallbacks. Dev builds
  and `/usr/local` installs only link to the release page. **Needs one-time
  release setup:** `minisign -G`, secrets `MINISIGN_KEY` / `MINISIGN_PASSWORD`,
  public key into `update::PUBLIC_KEY` — until then the updater never installs.
- **Chrome:** native macOS menu bar; in-window `AppMenuBar` on Windows/Linux
  (Isengard = About + Check for Updates only; File only with a folder/workspace open; no Edit / View menus — other actions are
  shortcut-only) — see design. Status bar while a workspace is
  open (panel toggles; path / Ln·Col when a file tab is active).
  Notifications bottom-right. Logo on welcome + platform app icon (see
  architecture Development setup / icon matrix). Main window size/position
  (+ maximized) restored from `config.json` on launch.
- **Theme:** JetBrains Mono Nerd Font; square corners; **dark mode only**;
  editor font-size zoom.

## Known issues / TODO

None open for the editor shell. Agent switch is the only deferred item
(below). UI test coverage is an agent rule
([`.cursor/rules/ui-test-coverage.mdc`](../.cursor/rules/ui-test-coverage.mdc)),
not a product TODO. License is **GPL-3.0-only** — see
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Release

- **v0.1.9 — Linux first** (`.deb` / `.rpm` / tarball via `scripts/package-linux.sh`
  + [`.github/workflows/release-linux.yml`](../.github/workflows/release-linux.yml)).
  Tag must match `Cargo.toml`; notes at `docs/releases/<tag>.md`. App id
  `app.isengard.edtir`. macOS / Windows installers not shipping yet.
- Notes: [docs/releases/v0.1.9.md](releases/v0.1.9.md). Human install:
  root [README.md](../README.md).

## Roadmap — Agent panel after v1

- Process-level agent switch in the panel — **deferred** (one agent today;
  change via `config.json` / Open Config File…). Mode/model pickers + chat
  rename + auto titles shipped.
