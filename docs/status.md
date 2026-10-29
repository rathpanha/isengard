# Current status

Shipping editor (v0.1.16): welcome, multi-root workspaces, file tree, tabs,
tree-sitter editor, LSP Install, Bun + `bun fmt`, terminal, search, Source
Control, Claude Agent panel (ACP), and in-app updates on Linux / macOS /
Windows. Early phased plan (egui → Kit → Agent v1) is finished — do not treat
“Phase 5” as open work. Gaps and deferred items are only under **Known
issues** and **Roadmap** below.

What works today:

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
  `bun add --cwd …/lsp/pkg/…` for npm-packaged servers, which then run as
  `bun <script>` (Node is no longer used; the Bun package cache stays under
  `…/isengard/runtime/bun-cache`). Existing npm-era installs keep working until
  Remove + Install; they run on Bun too once it is present. Only the agent
  adapter is verified on Bun so far — the language servers are untested.
  `pylsp` / `clangd` / etc. stay PATH-only (no Install).
  **Bun** (pinned 1.4.4, `lsp/bin/bun`): first launch without it (not app-local,
  not on `PATH`, not `~/.bun/bin`) shows a non-dismissable **Set up Bun** dialog
  (Install / Quit). Windows `.cmd` shims are not moved to Bun yet.
  Status bar: one LSP icon → short list with Install / Remove (Remove deletes
  app-local files only; ready/PATH probes are cached — never re-run every
  paint). Go-to-definition defers cross-file opens (avoids GPUI nested-update
  panic). Code Actions (editor context menu / Kit), Format Document
  (`⇧⌘I` / `Ctrl+Shift+I`; format-on-save default on via `lsp.format_on_save`;
  sends the tab's detected indent, footer shows `Spaces: N` / `Tab Size: N`).
  **`bun fmt`** formats js/ts/jsx/tsx/json/css/html/yaml/md first (Prettier-
  compatible; project `.prettierrc` / `.editorconfig` win, else the configured
  `tab_size`); other files use the language server. No formatter → plain save,
  `bun fmt` error → warning + plain save. Runs synchronously (no timeout yet),
  Rename Symbol (F2 in editor, dialog), `textDocument/didSave` after write.
  Saving `config.json` with `lsp` changes restarts servers and re-attaches
  open tabs. `config.json` → `lsp`. Untitled buffers skip
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
- **File watching:** every workspace root is watched (macOS/Windows: one
  recursive watch per root; Linux: per-dir walk off the UI thread so Open
  Folder does not beachball). Edits made outside Isengard (agent shell tools,
  `git checkout`, formatters, other editors) — debounced 300 ms — reload
  **clean** open tabs, refresh loaded tree directories on create/remove/rename
  (expand state kept), and refresh Source Control. A tab with unsaved edits is
  never overwritten: one warning toast per divergence, and Save still writes
  the buffer.
- **Save / close:** Save, Save All, close-tab and quit confirmations when
  unsaved (New File is buffer-only until Save; save dialog uses active tree
  dir / root) or while an agent turn or a terminal foreground process (Unix) is running
  (Quit Anyway / Keep Working). Quitting terminates each terminal's foreground
  program (SIGTERM, then SIGKILL after ~1s) and hangs up the shell, so dev servers
  don't keep their port.
  Linux in-window close × confirms unsaved changes. Save All
  keeps going past a failed file, lists every failure in one toast, and then
  cancels the pending close / quit / switch.
- **Agent panel:** right-hand panel (View > Toggle Agent Panel /
  `⌘⇧B` / `Ctrl+Shift+B` / status-bar `Bot`) chatting with an external agent.
  over the Agent Client Protocol, like Zed's external agents. A finished or failed
  run raises an OS notification + sound when the app is in the background
  (`src/notify.rs`; Mixkit pop sound, logo icon; Linux `notify-send`+`paplay`; macOS
  native `UNUserNotificationCenter` from the app bundle — app icon, listed under
  Isengard in System Settings > Notifications, asks permission on first use — with
  `osascript` only for unbundled `cargo run` builds, plus `afplay`; no Windows yet). Default is
  Claude Agent (`@agentclientprotocol/claude-agent-acp`, pinned 0.94.0):
  **Install** runs `bun add --cwd …/isengard/agents/claude` (never global) and
  the adapter launches as `bun <script>`, else `claude-agent-acp` on `PATH`. Uses the user's
  Claude Code login — no API key in Isengard. Not signed in → login buttons
  run the agent's own `--cli auth login …` in a terminal tab; when that
  program exits the panel reconnects by itself (Retry stays as a fallback).
  Custom agent via `config.json` `agent.command` / `args` / `env`. Streams
  markdown replies + thoughts, tool-call rows (kind icon, path, status;
  2+ consecutive tools — thoughts between them included — collapse into one
  card, see design.md),
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
  advertised (built-in Claude always advertises it — session `_meta` sets
  flag-tier `model: opusplan`, no terminal `/model opusplan` needed; off by
  default — session open pins the concrete Model unless prefs say on;
  Opus in Plan Mode, Sonnet otherwise; Mode change re-pins
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
  `config.json` `agent.command` / `agent.silent_model`.
  Switching workspace
  parks the old panel: a
  running turn finishes in the background (approval requests toast), then its
  agent stops; coming back resumes the chat (`session/resume`, no replay)
  with the transcript still in memory. After a restart the active chat
  reopens via `session/load` (history replays). Verified end to end against
  the real adapter on Linux (read + shell tool turn; load replays and the
  model keeps context; resume does not replay).
- **Updates (Linux, macOS, Windows):** checks GitHub releases at startup (≤ once
  a day; opt out with `updates.check: false`) and via Isengard > Check for
  Updates…. Dialog: Skip this version / Later / Update now. Self-installed
  copies (writable dir) download, verify (minisign signature + SHA-256) and
  swap in place, then offer Restart now (unsaved-changes prompt first;
  workspace reopens). On macOS an `Isengard.app` in a writable folder (e.g.
  `/Applications`) swaps the whole bundle (no password, no Gatekeeper prompt).
  Windows portable install renames the running `.exe` aside and replaces it.
  RPM/DEB installs download + verify, then **Install now** asks for the sudo
  password in a masked dialog, runs `sudo -S <manager> install -y <file>` with
  live output (wrong password re-asks) and restarts into the new binary; Copy
  command / Open in terminal remain as fallbacks. Dev builds and `/usr/local`
  installs only link to the release page. **Needs one-time release setup:**
  `minisign -G`, secrets `MINISIGN_KEY` / `MINISIGN_PASSWORD`, public key into
  `update::PUBLIC_KEY` — until then the updater never installs.
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

- Agent process switch in the panel — still deferred (one agent today; change
  via `config.json`). See Roadmap.
- Bun: only the Claude adapter is verified on Bun; npm-packaged language
  servers are untested there. Windows `.cmd` shims are not rewritten to Bun
  yet. `bun fmt` has no timeout.
- Agent OS notification + sound: no Windows yet.
- Terminal scrollback UI is still minimal.
- Windows installer / updater: not yet smoke-tested on a clean PC (needs a
  second release to exercise update-to).

UI test coverage is an agent rule
([`.cursor/rules/ui-test-coverage.mdc`](../.cursor/rules/ui-test-coverage.mdc)),
not a product TODO. License is **GPL-3.0-only** — see
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Release

- **v0.1.16 — Linux + macOS + Windows** (Linux `.deb` / `.rpm` / tarball via
  `scripts/package-linux.sh`; macOS universal `.dmg` / tarball via
  `scripts/package-macos.sh`; Windows x86_64 `.zip` via
  `scripts/package-windows.ps1`; all from
  [`.github/workflows/release.yml`](../.github/workflows/release.yml)).
  Tag must match `Cargo.toml`; notes at `docs/releases/<tag>.md`. App id
  `app.isengard.edtir`.
- **Windows (first release v0.1.12):** `scripts/package-windows.ps1` +
  `windows` CI job + `scripts/install-windows.ps1` + in-app updater
  (`InstallKind::WinExe`); unsigned (see [architecture.md](architecture.md)).
  Still needs a smoke test on a clean PC (updater needs a second release to
  update to). Only x86_64.
- **macOS:** no Apple Developer ID (see [architecture.md](architecture.md) for
  the signing / Gatekeeper trade-offs); signed with the self-signed cert from
  secrets `MACOS_CERT_P12` / `MACOS_CERT_PASSWORD`. The CI keychain-trust step
  and the x86_64 cross-build ran in the v0.1.10 release (arm64 verified on a
  real Mac; the Intel half has not been run on an Intel Mac).
- Notes: [docs/releases/v0.1.16.md](releases/v0.1.16.md). Human install:
  root [README.md](../README.md).

## Roadmap

Agent panel v1 is done (Claude via ACP, settings, Opus Plan, silent utility
jobs, multi-root, session restore). Claude stays the default “already
configured” path. Extra agents (Cursor via ACP, like Zed) are optional.

### External agents (Cursor ACP) — phased

Do **not** treat this as a second Install-button clone of Claude. Zed’s model
is spawn `agent acp` + ACP. Isengard already hosts ACP; much of the panel UX
is Claude-shaped (plan usage, Opus Plan, `bun add` adapter, terminal auth).

| Phase | Goal | Status |
| --- | --- | --- |
| **0** | Smoke: point `config.json` `agent.command` / `args` at Cursor CLI (`agent` + `["acp"]`, or full path). One real turn (read/edit/permission/cancel). Record `initialize` caps vs Claude. Keep Claude subscription path working unchanged. | **in progress** — Claude baseline OK (app-local 0.94.0 `initialize`: load/resume/delete/close, images, embeddedContext, additionalDirectories, auth `terminal` / `claude-ai-login`). Cursor CLI `agent` **not installed** (Cursor.app alone is not enough) |
| **1** | Auth that isn’t Claude: handle `cursor_login` / “run `agent login`” empty state; keep `auth.terminal` for Claude | blocked on 0 |
| **2** | Process-level agent switch in the panel (Claude vs Cursor vs custom), persist per workspace, reconnect. Replaces today’s config-only switch. | deferred (was the only roadmap item) |
| **3** | Capability-gated UI: Opus Plan / plan-limits / subagent-model / Claude Install only when that agent advertises them; silent jobs (Improve / titles / Git Suggest) only if multi-session works | after 2 |
| **4** | Optional Cursor extensions: `cursor/ask_question` (blocking, highest priority), then `create_plan` / `update_todos`, then `task` / `generate_image` | after 0–2 proven |
| **5** | Docs only for Cursor install — do **not** app-local `bun add` Cursor; CLI comes from Cursor | with 2 |

**Claude vs Cursor gaps (living):** Cursor = user-installed CLI + Cursor
subscription; no Isengard plan-usage accordion; modes ask/plan/agent (not
Opus Plan); auth `cursor_login` not Claude Keychain; MCP from `.cursor/mcp.json`
(agent-side); proprietary `cursor/*` methods for rich UX. Core chat/tools/
permissions should work on stock ACP once Phase 0 passes.

### Other deferred

- (none beyond the table above)
