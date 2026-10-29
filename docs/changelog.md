# Changelog

Newest first. Append **one** bullet per change here only — never duplicate in
status, architecture, or design. See [README.md](README.md).

- **2026-10-29** — Cursor Agent: auto-allow tool permissions (AllowAlways
  preferred) and default launch `--force acp` — Mode does not gate
  approval on ACP, so every tool was prompting.
- **2026-10-29** — Composer Mode/Model/… menus: 240px viewport +
  `overflow_y_scroll` when the catalog is long (wheel was hitting the
  config row’s horizontal scroller instead of the list).
- **2026-10-29** — Cursor model switch: cache Fast/Context/Effort per model so
  the composer does not wait on Cursor’s catalog re-fetch; Utility settings
  also show Context + Fast. Debug/Multitask stay IDE-only — ACP agent only
  advertises Agent/Plan/Ask.
- **2026-10-29** — Cursor ACP: advertise `_meta.parameterizedModelPicker` so
  sessions expose separate Model / Fast / Context / Effort config options
  (same composer chrome as Claude) instead of flat variant strings.
- **2026-10-29** — Source Control Suggest is one `PencilSparkles` menu:
  Generate (heuristic) + Claude + Cursor (compact popover).
- **2026-10-29** — OpusPlan Switch is Claude-only; Cursor keeps the composer
  Model picker (sticky `opusplan` prefs no longer hide it or pin the alias).
- **2026-10-29** — Agent Switch control is the header logo + `ChevronsUpDown`
  (not a separate Bot / ArrowLeftRight buried in the trailing icons).
- **2026-10-29** — Agent choice is per workspace session (`sessions.*.agent`),
  not `config.json` `agent.command`; restart restores the last agent (no
  re-picker).
- **2026-10-29** — Agent panel: empty built-in slot shows Claude | Cursor
  picker (auto-Install on pick); header Material logo + Switch Agent popover
  remounts the other slot; Cursor ACP Phase 2 (Claude ↔ Cursor) done.
- **2026-10-29** — Cursor ACP Phase 1 start: Agent-typed `cursor_login` shows a
  Login button (`cursor-agent login` in the terminal, then `authenticate` on
  reconnect); Claude vs Cursor chat lists are stored in separate session slots.
- **2026-10-29** — Cursor ACP Phase 0: app-local Install of pinned Cursor CLI
  (`…/agents/cursor/`), `agent.command = "cursor-agent"` resolve + panel Install
  copy; recorded `initialize` caps vs Claude (no embeddedContext / resume /
  delete; auth `cursor_login` only).
- **2026-10-29** — Cursor ACP roadmap: Install like Claude (pinned CLI into
  `…/agents/cursor/`, not user PATH / Cursor.app); Phase 0 = app-local smoke.
- **2026-10-29** — `@` mention walk skips `graphify-out/` so generated graph
  files no longer exhaust the walk budget (fixes
  `nested_folder_query_matches_in_this_repo` locally).
- **2026-10-29** — Release prep v0.1.17 (terminal scrollback, OpusPlan
  fix): version bump, `docs/releases/v0.1.17.md`, status release lines.
- **2026-10-29** — Terminal scrollback: mouse wheel / trackpad and
  Shift+PageUp/PageDown scroll history (10,000 lines); selection, links and
  cursor follow the scrolled viewport; typing or pasting snaps to the bottom.
  Apps using the alternate screen or mouse reporting still get the wheel.
- **2026-10-29** — OpusPlan Switch starts off: opening a Claude chat now pins
  the concrete Model (or `opusplan` only if the saved pref says so), so the
  flag setting or a terminal `"model":"opusplan"` no longer turns it on.
- **2026-10-29** — OpusPlan Switch no longer needs terminal `/model opusplan`:
  Claude Code only lists the `opusplan` alias when it is already the
  configured model, so the built-in Claude adapter's new/load/resume/silent
  sessions now carry `_meta.claudeCode.options.settings.model = "opusplan"`
  (flag tier — the user's own model is still what the session reports/runs).
  Also fixes a clippy `?` lint in `preferred_opusplan_id`.
- **2026-10-29** — Release prep v0.1.16 (Open Folder hang fix): version bump,
  `docs/releases/v0.1.16.md`, status release lines.
- **2026-10-29** — Open Folder no longer beachballs on large trees: `FsWatcher`
  returns before any directory walk; macOS/Windows use one recursive watch per
  root (Linux still walks off the UI thread).
- **2026-10-29** — Status roadmap: Cursor (and other) external ACP agents in
  phases 0–5 (smoke via `config.json`, auth, panel switch, capability-gated
  UI, optional `cursor/*` extensions); Claude remains default.
- **2026-10-29** — Docs audit vs codebase: architecture `src/` tree (bun,
  notify, shell_env, workspace/, slash, edits, selection_matches, ui_tests);
  README After-install paths + Bun; design update-dialog sudo + tree Reveal;
  credits download/notify crates; vendor README lists all Kit patches; status
  Windows smoke wording.
- **2026-10-29** — Status: drop stale “Phase 5” framing (Agent panel v1 is
  shipped); lead with current product; gather real gaps (Bun LSP verify,
  Windows notify/smoke, terminal scrollback); Updates line includes Windows.
- **2026-10-29** — README: “Why Isengard” feature blurb (defaults, workspaces,
  Claude Agent, model/utility/Opus Plan, app-local tools, platforms).
- **2026-10-29** — Docs: architecture overview (stack, process map, binary vs
  on-disk, data dirs) at the top of `architecture.md`; README Architecture
  blurb + docs index; credits row for Bun.
- **2026-10-29** — Release prep v0.1.15 (Bun runtime): version bump,
  `docs/releases/v0.1.15.md`, status release lines.
- **2026-10-29** — Bun replaces Node: app-local Bun (pinned 1.4.4) installed from
  a non-dismissable first-run dialog; agent adapter and npm-packaged language
  servers install with `bun add` and run as `bun <script>`; format-on-save and
  Format Document use `bun fmt` for js/ts/json/css/html/yaml/md (project config
  wins), language server for the rest. Indent detection now ignores odd steps.
- **2026-10-29** — Per-file indent detection (tabs / 2 / 4 / 8, else config
  `tab_size`); footer shows `Spaces: N` / `Tab Size: N`; LSP format-on-save and
  Format Document use the tab's indent instead of a hardcoded 4 spaces.
- **2026-10-29** — Release prep v0.1.14 (tool-run cards): version bump,
  `docs/releases/v0.1.14.md`, status release lines.
- **2026-10-29** — Agent chat: 2+ consecutive tool calls (thoughts between them
  included) collapse into one card (`N steps` / live title, collapsed by
  default, auto-open for permission prompts) so long turns stop flooding the
  transcript. Test: `agent_tool_runs_collapse_into_one_card`.
- **2026-10-29** — Added a GitHub bug-report issue template
  (`.github/ISSUE_TEMPLATE/`, blank issues off) and a "Report a bug" section in
  README / CONTRIBUTING, stating issues may not be picked up or fixed.
- **2026-10-29** — Release workflow: `publish` now requires the Linux, macOS and
  Windows jobs to all succeed (it used to publish whenever Linux passed, which
  shipped v0.1.13 without macOS files after the macOS job was cancelled while
  queued, so the macOS updater only offered "Open release page"). A failed job
  is fixed with "Re-run failed jobs".
- **2026-10-28** — Release prep v0.1.13 (macOS notification icon): version
  bump, `docs/releases/v0.1.13.md`, status release lines.
- **2026-10-28** — macOS "Agent finished" notifications now come from Isengard
  itself (`UNUserNotificationCenter` via `objc2-user-notifications`, only when
  running from an app bundle) so they show the app icon; `osascript` (which
  showed Script Editor's icon) remains the fallback for `cargo run`. macOS asks
  to allow notifications on first use. Test: native path is skipped without a
  bundle.
- **2026-10-28** — Renamed `.github/workflows/release-linux.yml` to
  `release.yml` (it builds Linux, macOS and Windows); doc links updated. Use
  `gh workflow run release.yml -f tag=<tag>` for manual runs.
- **2026-10-28** — Release prep v0.1.12 (first Windows release, Rename-chat
  Generate, silent jobs during a turn): version bump, `docs/releases/v0.1.12.md`,
  README / status release lines.
- **2026-10-28** — Improve, Git Suggest and chat-title Generate now work while
  a user turn is running (they run on throwaway sessions): `can_silent_prompt`,
  `can_suggest_commit`, `can_generate_title` and the Improve / history-row Rename
  buttons no longer require an idle chat (Generate that must load an unopened
  chat still does). `apply_update` no longer routes the open chat's stream into
  a running silent job (it would have swallowed the reply and aborted on tool
  calls); `start_silent` keeps a running turn's file tracking. FakeAgent's
  `hold` is now async so it can serve other sessions meanwhile. Test:
  `silent_jobs_run_while_a_user_turn_runs`.
- **2026-10-28** — Rename-chat dialog: Generate no longer closes the dialog and
  renames at once. It shows a spinner, fills the input with the generated title,
  and the chat is only renamed when the user clicks Rename (Cancel aborts the
  silent title turn). New `RenameChatView` + `RenameDialog` state in
  `agent/panel.rs`; UI test `rename_dialog_generate_fills_input_and_saves_only_on_rename`.
- **2026-10-28** — Windows release pipeline without a signing certificate:
  `scripts/package-windows.ps1` (x86_64 zip), `scripts/install-windows.ps1`
  (PowerShell installer, no SmartScreen mark), `windows` job in the release
  workflow, release builds use the GUI subsystem, in-app updater gains
  `InstallKind::WinExe` (rename running exe to `.old`, swap, restart; old file
  cleaned at launch). Tests: `classify_windows`, Windows asset pick, zip swap /
  missing-exe rollback, WinExe update dialog. Not yet run on Windows.
- **2026-10-28** — Release prep v0.1.11 (macOS launch + PATH fixes): version
  bump, `docs/releases/v0.1.11.md`, README / status release lines.
- **2026-10-28** — macOS apps launched from Finder/Dock now adopt the login
  shell's `PATH` (`src/shell_env.rs`), fixing "Claude Agent stopped … env: node:
  No such file" and `oh-my-posh: command not found` in the terminal. The
  login-shell PATH is cached (instant later launches, background refresh) and
  the macOS terminal shell now starts with `-l`. Tests: marker extraction, PATH
  merge, cache round-trip, real login-shell read.
- **2026-10-28** — Fix macOS crash at launch ("Library not loaded:
  liblzma.5.dylib"): CI's Homebrew `xz` was linked dynamically into the arm64
  slice. `xz2` now uses the `static` feature on macOS, and
  `scripts/package-macos.sh` fails if the binary links any non-system library.
  The v0.1.10 release assets were rebuilt from `master` via "Run workflow"
  (the tag itself still points at the pre-fix commit f646dae).
- **2026-10-28** — Release prep v0.1.10 (first macOS release): version bump,
  `docs/releases/v0.1.10.md`, README / status release lines.
- **2026-10-28** — macOS release pipeline without an Apple Developer ID:
  `scripts/package-macos.sh` (universal `Isengard.app`, `.dmg`, `.tar.gz`,
  signed with a self-signed cert or ad-hoc), `scripts/create-macos-cert.sh`,
  `scripts/ci-macos-keychain.sh`, `scripts/install-macos.sh` (curl installer),
  `macos` job in the release workflow (now named Release), README macOS
  install section. In-app updater gains `InstallKind::MacApp` (swap the whole
  bundle after `codesign --verify`, restart). Tests: `classify_macos`,
  macOS asset pick, bundle swap / bad-tarball rollback, MacApp update dialog.
- **2026-10-28** — Agent OpusPlan off now restores the model used before
  turning it on, persisted in `agent_prefs.model_before_opusplan` (survives
  restarts; was in-memory, so it fell back to the first list entry). Tests:
  Mode-picker path, agent Mode/Config updates, Switch click + lock, absent
  alias, restart/stale previous model; Improve cancel now asserts only the
  Improve session is cancelled (FakeAgent records `cancel <sid>`).
- **2026-10-28** — Disabled buttons no longer show the pointer hand: Git
  (Pull/Push/Commit/Discard/Refresh/Open), Search Replace and the chat title
  Generate button now add `cursor_pointer` only when enabled; `outline_btn`
  no longer sets it. Rule + disabled-color convention added to design.md.
- **2026-10-28** — Agent composer: Fast / Opus Plan Switch labels now gray
  with the icons while locked (drawn label, same gray as disabled Mode/Model
  buttons: `muted_foreground` @ 0.5); label
  renamed "Opus Plan".
- **2026-10-28** — Editor right-click on macOS now shows the drawn Kit menu
  instead of the system NSMenu (vendored `gpui-component` `NativeMenu::show`).
- **2026-10-28** — Removed the Open Config File action entirely (it never had
  a shortcut; edit `config.json` by hand). Agent Fast / OpusPlan Switch icons
  now turn gray with the Switch while the agent is running.
- **2026-10-28** — Agent OpusPlan: Mode changes sync model to the Switch —
  on → re-pin `opusplan`, off → re-pin concrete Model (blocks Claude
  `settings.json` `opusplan` bleed); tooltip shows Opus vs Sonnet.
- **2026-10-28** — Agent Improve: while rewriting, the Improve icon becomes ×
  (cancels only the silent session); chat Stop no longer appears for Improve.
- **2026-10-27** — Removed the Edit and View menus, Quit Isengard (still `⌘Q` / `Ctrl+Q`), Open Config File… from the Isengard menu, and
  all File items except Add Folder to Workspace…, Save Workspace As…, Close Folder/Workspace
  (File hidden on welcome). Removed actions keep their keyboard shortcuts.
- **2026-10-27** — Release: bump to 0.1.9 (settings popover outside-click fix, Approve Plan
  keeps Claude's own "No, keep planning" label).
- **2026-10-27** — Agent settings popover closes on outside click again (manual dismiss that
  skips clicks inside its nested model menus).
- **2026-10-27** — Approve Plan: reverted the "Keep planning and View the plan" relabel; Claude's
  own "No, keep planning" label, default reject style, and no plan Preview on click.
- **2026-10-27** — File tree: right-click **Reveal in File Manager** (Finder / File Explorer)
  opens the folder, or selects the file in its folder (`RevealPath`).
- **2026-10-27** — Release: bump to 0.1.8 (terminal Ctrl+C fix, quit protection for terminal
  processes, terminals stopped on quit).
- **2026-10-27** — Terminal: Ctrl+C reaches the shell again (Kit's `Root` bound it to Copy and
  the terminal swallowed it); `Terminal`-context `SendInterrupt` binding wins. Copy is Ctrl+Shift+C.
- **2026-10-27** — Quit: terminals' foreground programs are terminated on quit (SIGTERM,
  SIGKILL after ~1s) so servers don't hold their port; adds `libc` dependency.
- **2026-10-27** — Quit: the running-work prompt also fires when a terminal tab has a
  foreground process (PTY foreground group != shell; Unix only).
- **2026-10-27** — Release: bump to 0.1.7 (in-app package install, agent notifications, quit
  protection, rounded icon).
- **2026-10-27** — Logo: rounded-corner tile (rx 229) in `logo.svg` / `icon-macos.svg`;
  re-exported `icon-1024.png`, `logo-512.png`, `Isengard.ico`, `Isengard.icns`.
- **2026-10-27** — Notifications: use the Isengard logo icon and the Mixkit
  "Long pop" sound (`assets/sounds/agent-done.wav`) instead of the stock icon / clink sound.
- **2026-10-27** — Quit: closing the editor while an agent turn is running asks first
  (Quit Anyway / Keep Working), before the unsaved-files prompts.
- **2026-10-27** — Agent: removed the "What's using your week" per-product section from
  the plan usage detail.
- **2026-10-27** — Agent: when a run finishes (or fails) while Isengard is in the
  background, show an OS notification and play a sound (`src/notify.rs`).
- **2026-10-27** — Updates: the downloaded package/tarball is deleted once the update installs.
- **2026-10-27** — Updates: RPM/DEB installs get an **Install now** button — masked sudo
  password dialog, live package-manager output, then automatic restart.
- **2026-10-26** — Release: bump to 0.1.6 (terminal Tab fix, persisted plan usage bars).
- **2026-10-26** — Terminal: Tab / Shift+Tab now reach the shell (completion works).
  Kit's window `Root` bound them to focus traversal and moved focus out of the
  terminal; `Terminal`-context bindings (`SendTab` / `SendBackTab`) now win.
- **2026-10-26** — Agent: plan usage bars no longer vanish when the usage endpoint
  rate-limits (HTTP 429) — last numbers are saved to disk and shown dimmed, polling
  is 5 min with minutes-long backoff after a 429, and a "rate limited" note shows
  when there is nothing to display.
- **2026-10-26** — Release: bump to 0.1.5 (Check for Updates… fix, chat restore fix).
- **2026-10-26** — Updates: Check for Updates… did nothing (action had no root
  listener); now shows a "Checking…" toast, and update HTTP calls time out.
- **2026-10-26** — Agent: fixed restored chats cutting off the newest replies —
  splicing **N Files** cards added rows the transcript list did not count.
- **2026-10-26** — Release: bump to 0.1.4 (first build with the in-app updater).
- **2026-10-26** — Updates: daily GitHub release check, Isengard > Check for
  Updates…, signed (minisign) + checksummed in-place update for writable
  installs, assisted `sudo dnf|apt|zypper install` for RPM/DEB installs.
- **2026-10-26** — Git panel: each repo is an accordion section (click the header
  to fold to name / count / branch); state persists per workspace session.
- **2026-10-26** — Git: commit message box is disabled while Claude Suggest runs.
- **2026-10-26** — Agent: composer Textarea is disabled while Improve prompt runs.
- **2026-10-26** — Git: Claude Suggest runs in parallel across repos (ACP silent
  jobs keyed by job id; `can_suggest_commit`); one repo's Suggest no longer
  disables the others.
- **2026-10-26** — Release: bump to 0.1.3; CI requires tag == `Cargo.toml`,
  packages use that version, notes from `docs/releases/<tag>.md`.

- **2026-10-26** — Release workflow: fix nfpm download URL
  (`Linux_x86_64` / `Linux_arm64`, not `linux_amd64`).

- **2026-10-26** — CI: harden Add Selection to Agent UI test (call the
  handler; Kit `secondary-l` flaky on Ubuntu runners); ignore hidden
  terminal focus for Ctrl/Cmd+L.

- **2026-10-25** — Linux release packaging: root README, `.desktop` + nfpm
  `.deb`/`.rpm`/tarball (`scripts/package-linux.sh`), GitHub Actions release
  workflow, `docs/releases/v0.1.0.md`.

- **2026-10-25** — File watch: non-recursive per-dir watches that skip
  `node_modules`/build dirs (fixes Linux inotify blowups); status: only
  deferred item is agent switch; live smoke cleared.

- **2026-10-25** — App id `app.isengard.edtir`; file-watch edges (reload
  keeps selection/scroll, delete closes clean / warns dirty, window
  activation reconciles open tabs); more watch skip dirs; status drops
  LSP-bundle / GPL-as-open-TODO.

- **2026-10-25** — Workspace `settings` overlay (font / tab / format-on-save);
  LSP `executeCommand` + `applyEdit` for command-only code actions; drop
  click-verify / Wayland-icon TODOs (verified / ignore).

- **2026-10-25** — Status cleanup: drop Open Recent / terminal-scrollback
  TODOs; cold-start welcome is product design (not a gap); agent switch
  deferred while there’s only one agent.

- **2026-10-25** — Save All: untitled write failure after the Save dialog
  continues the queue (same as on-disk failures); one toast; `after` still
  blocked.

- **2026-10-25** — File Save As… (`⌘⇧S` / `Ctrl+Shift+S`); quit offers Save
  Workspace for untitled multi-root (same as switch); UI tests for Save As,
  dirty-session restore, and the quit workspace prompt.

- **2026-10-25** — UI tests for file-tree create/rename and terminal
  toggle/new-tab; close human “not verified” TODO; agent rule
  `ui-test-coverage` (every UI feature ships a `ui_tests` case).

- **2026-10-25** — LSP: Code Actions, Format Document + format-on-save
  (`lsp.format_on_save`), Rename Symbol (F2), `didSave`, restart servers when
  `lsp` in config changes.

- **2026-10-25** — Agent settings keep dropdown rows; `overlay_closable(false)`
  so nested model menus don’t get eaten; fold ACP `Default (recommended)`
  into inherit.

- **2026-10-25** — Subagent model picker: drop ACP `Default (recommended)`
  so it no longer doubles our empty **Default** (no override).

- **2026-10-25** — Agent subagent progress in chat (ACP `subagents` +
  child-session tool activity); Agent settings **Subagent model**
  (`CLAUDE_CODE_SUBAGENT_MODEL` / `_FORCE`).

- **2026-10-25** — Agent composer `/` slash-command suggestions from ACP
  `available_commands_update` (same picker pattern as `@`).

- **2026-10-25** — Approve Plan “keep planning” → **Keep planning and View
  the plan** (outline, not danger); opens the plan `.md` in Preview.

- **2026-10-25** — Agent permission options stack full-width (one per row),
  including Approve Plan — no side-by-side wrap.

- **2026-10-25** — Session restore: sidebar strip, search query, git
  drafts/selection/box height, dirty/untitled text; terminal layout/cwds
  only (fresh shells, no scrollback).

- **2026-10-25** — Git commit box: pixel height floor (auto 1→6 rows, drag
  to 20) instead of Kit `auto_grow` mid-drag — stops snap-back past the
  auto cap.

- **2026-10-25** — Agent rule: fix `isengard` warnings (do not mute Cargo);
  `.cursor/rules/cargo-warning-free.mdc` + docs; wire `can_prompt` into
  outbound drain so `cargo run` no longer warns `dead_code`.

- **2026-10-25** — Git Claude Suggest scrub strips Co-Authored-By /
  Generated-with / Claude-Session attribution; repo `.claude/settings.json`
  disables Claude Code commit/PR attribution for local + cloud sessions.

- **2026-10-23** — Agent header `SlidersHorizontal` settings: row layout
  `Utility model → Model → Version` (more rows later); global
  `agent.silent_model`; **Chat model** inherits the open chat.

- **2026-10-23** — Git Claude Suggest icon dims at 50% opacity when disabled
  (Material SVG does not pick up Kit’s muted disabled `fg`).

- **2026-10-22** — Silent jobs no longer freeze the open chat: Suggest/title
  keep Send as Send (home prompt can run on the open session); only Improve
  shows Stop; home vs silent turn end paths are split.

- **2026-10-22** — Silent jobs (Improve / title / Suggest) no longer set
  open-chat `turn_running` (throwaway ACP session was already separate;
  the UI was wrongly showing Working… / locking history).

- **2026-10-22** — Git Claude Suggest enables when the Agent panel is Ready
  (`PromptabilityChanged`); disabled (no click / no toast) otherwise.

- **2026-10-22** — Git commit Suggest is two icon buttons: heuristic
  (`WandSparkles`) and Claude (Material `claude` icon); AI no longer falls
  back to the heuristic.

- **2026-10-22** — Silent agent jobs (Improve, chat title, Git Suggest) run
  on a throwaway ACP session so the open chat’s context is not polluted.

- **2026-10-22** — Agent Improve icon is `PencilSparkles` (was `Sparkles`,
  same as Claude Auto mode); opted into `ExtraIcons` so it actually loads.

- **2026-10-22** — TSX syntax highlighting: re-register Kit's `tsx` grammar
  with TypeScript highlights + JSX captures (`assets/highlights/tsx_jsx.scm`);
  Kit had only the short `HIGHLIGHTS_QUERY` delta so `.tsx` looked plain.

- **2026-10-22** — Agent Improve keeps inline `@` mention chips (and
  paperclip attachments) when rewriting the draft; only plain text is
  replaced.

- **2026-10-22** — Plan usage accordion expands to full content height (no
  inner scroll); transcript keeps the remaining `flex_1` space.

- **2026-10-22** — Plan usage fetch refreshes the Claude OAuth access token
  (and persists the rotated refresh token) before calling `/oauth/usage`, so
  expired tokens no longer hide the bars until restart.

- **2026-10-22** — Plan usage accordion under the Agent header (chevron +
  5h/Week; expand capped at 240px so transcript height is preserved). Fetch
  backs off 5→60s on OAuth 429 when no bars yet.

- **2026-10-22** — Agent header shows session title only while busy (no
  activity suffix / Spinner); turn activity stays in the transcript footer.

- **2026-10-22** — Agent chats auto-title after the first turn (silent ACP
  turn; Claude sends no `session_info_update`); rename / Generate unchanged.

- **2026-10-22** — Agent toggle is `Ctrl+Shift+B` / `⌘⇧B` (dropped Ctrl+Alt);
  status-bar Sidebar / Terminal / Agent tooltips show the shortcut.

- **2026-10-22** — Agent interactive QA: FakeAgent UI tests for permission
  answer, chat delete, and Install/Login screens; live Claude smoke checklist
  in status.

- **2026-10-22** — Chat/Markdown code blocks: Ayu Darker tokens on muted gray
  background; Linux NativeMenu fallback no longer steals editor focus (selection
  highlight stays on right-click → Add Selection to Agent).

- **2026-10-22** — Terminal selection → Agent: `⌘L` / `Ctrl+L` and right-click
  Add Selection to Agent attach a `Terminal` snippet chip (same path as
  editor selection).

- **2026-10-22** — Agent transcript keeps ~half-pane empty scroll space below
  the latest message (Kit `MessageScroller::with_bottom_space`) so long chats
  are not glued to the composer.

- **2026-10-22** — Agent composer ghost **Improve** (`Sparkles`) left of Send:
  silent ACP rewrite into the draft for review; never auto-sends.

- **2026-10-22** — Plan usage bar tooltips show reset time in local date/time
  (e.g. `Thu Oct 22, 2026 3:10 PM`) instead of raw UTC ISO-8601.

- **2026-10-22** — Claude plan usage expand overlays the transcript instead of
  pushing chat down (absolute under the slim 5h/Week strip).

- **2026-10-22** — Claude plan usage bars no longer vanish when the OAuth
  usage endpoint returns HTTP 200 with null `five_hour` / `seven_day`
  (merge keeps the last good windows).

- **2026-10-22** — Editor right-click → Add Selection to Agent (same as
  `⌘L` / `Ctrl+L`); Kit `Editor::context_menu` now receives the default
  menu so apps can append without rebuilding Cut/Copy/Paste.

- **2026-10-22** — File tree drag: dropping on an invalid target (e.g. itself)
  clears the hover and can no longer fall through and move the item to the root.

- **2026-10-22** — File tree: folders toggle on click instead of mouse-down,
  so dragging a folder no longer expands it.

- **2026-10-22** — Multi-root workspaces: drag a root folder in the tree to
  reorder; the first root is the agent's primary folder, so moving a root to
  the top reconnects the agent there.

- **2026-10-22** — File tree: drag files/folders onto a folder (or blank
  space → root) to move them; open tabs are remapped.

- **2026-10-22** — `⌘L` / `Ctrl+L` (Edit > Add Selection to Agent) attaches
  the editor selection — or the whole file if nothing is selected — as a
  prompt chip and shows the Agent panel.

- **2026-10-22** — Agent prompt accepts drag-drop attach from the file tree,
  an editor tab, or the OS file explorer (chips; same path as Paperclip).

- **2026-10-22** — Agent `@` mentions are Kit inline tokens **inside** the
  prompt Textarea (folder/file icon + basename, or image thumb); paperclip
  chip row stays Attach / paste / drag-drop.

- **2026-10-22** — Agent **N Files** pins above the composer as a full-width
  layout strip (`border_t`, edge-to-edge — not an inset card); still drops
  paths on Source Control discard.

- **2026-10-22** — Agent **N Files** cards persist on `AgentChat.file_changes`
  and are spliced back after `session/load`; restore retries as history
  `Update`s arrive so a full editor restart no longer loses them (in-session
  close/reopen already kept the panel in memory).

- **2026-10-22** — Claude plan usage bars: normal band uses
  `muted_foreground` instead of green; yellow (≥70%) and red (≥90%) unchanged.

- **2026-10-22** — Gate `Command` import in `agent/limits.rs` to macOS so
  Linux `cargo run` is warning-free.

- **2026-10-22** — Modal dialogs: no enter animation / panel shadow, and the
  host is clipped to the Linux CSD content frame so the dim overlay no longer
  paints into the transparent window-shadow padding (looked like a slab under
  the chrome). Toasts keep motion.

- **2026-10-21** — Agent panel shows Claude plan usage (5-hour session +
  weekly) as two green/yellow/red bars under the header, polled every 2 min
  from Anthropic's OAuth usage endpoint with the existing Claude Code login.

- **2026-10-21** — Agent composer Mode / Model / Version / Context / Fast /
  Effort picks persist across restarts and new chats: saved per
  folder/workspace (`sessions.<key>.agent_prefs`) plus a global last-used
  fallback, re-applied on every session open (unknown values skipped).

- **2026-10-22** — Fix every `cargo clippy --all-targets` warning (let-chains,
  `clamp`, `contains`, type alias for diff ranges, `allow`s on the few
  wide-signature render helpers) so clippy is warning-free again.

- **2026-10-22** — Agent login reconnects by itself when the login terminal
  program exits (`add_command_session` takes an exit hook); Retry stays as a
  fallback. Roadmap drops auto-retry and "Import from Claude Code…".

- **2026-10-22** — File watching (`notify`): outside edits reload clean tabs,
  re-list loaded tree folders without losing expand state, and refresh Source
  Control; dirty tabs keep their edits and warn once.

- **2026-10-21** — Agent transcript tightens space between consecutive tool
  rows (shell execute…): scroller `pb_2`, tool/file-change entry padding
  reduced (was section-sized `pb_8` gaps).
- **2026-10-21** — Explorer tints dirty git paths (and parent folders):
  modified warning, new/untracked success, delete/conflict danger.
- **2026-10-21** — Agent **N Files** card starts collapsed; Review button
  removed (expand + row click to open).
- **2026-10-21** — Source Control auto-refreshes when the agent writes,
  finishes Edit/Delete tools, or ends a turn with shell file changes.
- **2026-10-21** — Agent **N Files** card also diffs the git worktree since
  turn start (shell `echo`/`rm`/…), not only Write/Edit tool RPCs.
- **2026-10-21** — Agent **N Files** summary also tracks Edit/Move tool
  completions (Claude often skips `fs/write_text_file` and only sends the
  tool row) — card appears under the turn’s reply.
- **2026-10-21** — Agent transcript shows an end-of-turn **N Files**
  summary (line `+/-`) for writes/deletes.
- **2026-10-21** — Status-bar toggles (sidebar, terminal, agent) show an
  info-blue icon when active instead of the gray `selected` background.
- **2026-10-21** — Agent Mode/Model band is a fixed 36px row (scroll, no
  wrap); open no longer clears options with `None` — stops the composer
  height jump when controls arrive.
- **2026-10-21** — `⌘N` / `Ctrl+N` in the Agent composer starts a new chat
  (not New File); File menu / editor focus keep New File.
- **2026-10-21** — Context used/size shows on session open/switch as
  `— / {window}` until the agent’s first `usage_update`.
- **2026-10-21** — Model picker uses `Cpu` (not `Bot`); Mode list hides
  Claude Auto when the selected model is older than 4.6.
- **2026-10-21** — Agent header / chat list show context used/size from
  ACP `usage_update`; advertise compaction so agents can notify when they
  auto-compact near the window limit.
- **2026-10-21** — Queued follow-ups show under the composer with
  per-item remove (`CircleX`).
- **2026-10-21** — Rename → Generate titles any chat (cache or load);
  Mode tap icon follows Ask / Auto / Plan / accept-edits.
- **2026-10-21** — Follow-ups while the agent is busy are queued (FIFO);
  Send stays as Queue beside Stop; drained when the turn ends.
- **2026-10-21** — Composer config taps show Lucide icons (Shield/Bot/Tag/
  Maximize2/Zap/…) so Mode vs Model vs Context vs agent are not label-only.
- **2026-10-21** — Composer config pickers move to their own wrapping row
  above Attach/Send so Mode/Model/Context/Fast no longer overflow.
- **2026-10-21** — Composer Fast is a Switch labeled Fast; Context picker
  when the model list has 200K/1M (etc.) variants for the selection.
- **2026-10-21** — Model picker splits family + version (Sonnet / 4.5);
  Fast mode shows as On/Off (was mislabeled Version).
- **2026-10-21** — New Chat uses `Plus`; rename dialog can AI-Generate a
  title from the open transcript (silent ACP turn).
- **2026-10-21** — Agent header shows session title; Mode / Model pickers
  sit on the composer row.
- **2026-10-21** — Agent chat rename (history Pencil) + ACP mode/model
  pickers when the session advertises them.
- **2026-10-21** — Git Suggest uses the Ready ACP agent for a commit-message
  draft (any agent; silent turn); falls back to the path heuristic when
  none is connected.
- **2026-10-21** — User chat bubbles are selectable (`TextView`), matching
  agent replies.
- **2026-10-21** — Agent attachment chips drop the leading `@` (basename /
  image thumb only); chat refs use Material icons, not Lucide.
- **2026-10-21** — Agent `@` / attachment chips use Material file icons
  (same as the tree); turn activity Spinner also shows at the bottom of
  the chat transcript.
- **2026-10-21** — `@` fuzzy search gives each workspace root its own walk
  budget so a huge primary folder cannot hide files in secondary roots.
- **2026-10-21** — Agent image attach re-encodes ICO/BMP/TIFF to PNG so
  Claude can read favicons and similar (UI thumb still uses the original).
- **2026-10-21** — `@` can attach multi-root workspace folders themselves
  (`@royal5g-system`); Harness UI test opens a two-root mock workspace.
- **2026-10-21** — `@` mention walk is plain FS (not gitignore) so nested
  folders always match; Harness UI test covers empty `@` + filter on a
  mock nested workspace.
- **2026-10-21** — Empty `@` lists nested workspace folders (not only
  top-level); browse-into-folder requires a trailing `/` (→/Tab).
- **2026-10-21** — Attachment remove (`CircleX`) sits on the top-right
  corner of each chip/thumb.
- **2026-10-21** — Attachment chips bottom-align with image thumbs; `@`
  click attaches folders again (→/Tab still dives).
- **2026-10-21** — Paperclip picks files only (no accidental `@Pictures/`
  folder); composer stays mounted while the session opens so chat history
  does not jump when Send appears.
- **2026-10-21** — Agent image attaches (disk / paste / multi-select) show
  40px thumbnails in the composer and user bubble (no longer file/folder
  chips); each selected image is its own preview.
- **2026-10-21** — `@` mention ↑↓ scrolls the selected row into view.
- **2026-10-21** — `@` root listing no longer clipped to 8 rows (fuzzy
  search still capped; build dirs still skipped).
- **2026-10-21** — Agent Stop uses a solid square (Lucide `Square` is hollow).
- **2026-10-21** — Agent Stop is `.danger()`; mentioned files/folders show
  as icon + name chips in the user bubble.
- **2026-10-21** — Agent Paperclip opens the OS file/folder picker
  (replaced active-file / selection attach buttons).
- **2026-10-21** — Slimmer `@` mention menu; Enter attaches folders too
  (→/Tab still dives in).
- **2026-10-21** — Agent composer `@` mention menu: workspace file/folder
  list (filter, dive into folders, Enter attaches).
- **2026-10-21** — Fix agent file attach: tree selection when no text tab;
  embed file text as `<context>` when the agent lacks `embeddedContext`;
  attach events no longer gated on panel Entity eq.
- **2026-10-21** — Agent composer attachments: active file / selection
  chips, paste image or paths; ACP `resource` / `resource_link` / `image`
  from agent prompt capabilities (`agent::prompt`, `base64`).
- **2026-10-21** — Dropped unused ACP client `terminal/*` (Claude Agent
  0.94.0 never calls it). Kept agent header live activity text (tool title /
  Thinking…).
- **2026-10-21** — Agent sees every root of a multi-root workspace (other
  roots sent as ACP `additionalDirectories`; folder add/remove reconnects).
- **2026-10-21** — Agent chats per folder/workspace: one panel per
  workspace with its own chat index in `sessions` (Isengard-started chats
  only), header chat list (open / delete), background turns finish after a
  switch then the agent stops, return resumes without replay, restart
  reloads via `session/load`. `acp::testing::FakeAgent` for tests;
  `notify_warning` toast.
- **2026-10-21** — Agent panel v1 (replaces the planned direct-API Claude
  panel): right-hand chat with external agents over the Agent Client
  Protocol (`agent-client-protocol` 3.4.0, own thread, no Tokio). Built-in
  Claude Agent (`@agentclientprotocol/claude-agent-acp` 0.94.0) installs
  app-locally and uses the Claude Code login; custom agents via
  `config.agent`. Streaming replies, tool rows, plan, inline permissions,
  Stop / New Chat, buffer-aware file read/write, terminal login. Dropped the
  unused `AppConfig.model`; `npm_install` shared with LSP installs; terminal
  can run one-off command tabs.
- **2026-10-20** — Tree path actions (`RenamePath`, `DeletePath`, `NewFileIn`,
  `NewFolderIn`, `RemoveWorkspaceFolder`) moved from `app/actions.rs` to
  `ui/file_tree.rs`; `ui/` no longer imports `app`. No behavior change.
- **2026-10-20** — Headless UI integration tests (`src/app/ui_tests.rs`) for
  dirty mark/undo, Save, Save All failure and tab-close dialog; config path is
  disabled under test; LSP event pump uses `recv_async` (blocking recv hung the
  test scheduler).
- **2026-10-20** — Dirty mark is derived from a saved-text rope snapshot, so
  undoing back to the saved text clears it; Save All no longer stops at the
  first failed file (one toast lists all failures; close/quit/switch still
  cancelled).
- **2026-10-20** — Moved welcome screen `src/ui/welcome.rs` → `src/app/welcome.rs`
  (it renders from `IsengardApp`); removes the `app` ↔ `ui::welcome` import
  cycle. No behavior change.
- **2026-10-20** — Agent tooling: optional local graphify code graph of `src/`
  (gitignored; scope in `.graphifyignore`, `docs/` excluded and read directly);
  graphify rules added to both `CLAUDE.md` and `AGENTS.md`.
- **2026-10-18** — Split monolith `app.rs` (~6k) into `src/app/` by concern
  (actions, tabs, workspace/session, files/tree, lsp, search, git, terminal,
  chrome, render); docs map updated. No behavior change.
- **2026-10-18** — LSP catalog: Install returns after Remove — `installed` means
  app-local (or live), not PATH; ready-cache pinned false while uninstall runs.
- **2026-10-18** — LSP status marks are block-style squares (status-bar badge +
  catalog rows), not Kit’s round `Badge::dot` / `rounded_full`.
- **2026-10-17** — LSP Install/Remove: dismiss the catalog popover and restore
  editor focus so File > Close Folder/Workspace keeps a live menu target.
- **2026-10-17** — LSP catalog: cache `is_server_ready` (PATH / rust-analyzer
  `--version`) so a missing server no longer stalls scroll and UI animation on
  every status-bar paint; skip re-spawn while start is in Error until retry.
- **2026-10-16** — Selection matches / scroll: ignore editor observe no-ops
  (scroll + blink) — only refresh matches when the selected range changes and
  only re-render the app when Ln/Col changes.
- **2026-10-16** — Selection matches: stop materializing the whole buffer on
  every cursor move (`value()`); clear immediately when idle, debounce scans
  (~50ms), skip files >512 KiB.
- **2026-10-16** — Drag to reorder editor tabs and terminal tabs within each
  strip (left `drag_border` insert; trailing drop zone; session order persists).
  Editor drag binds on `Tab` (not a parent wrapper) + unique `element_id`
  (Kit defaulted every standalone tab to id 0).
- **2026-10-16** — Stuck managed tooltips: Kit hide + deferred re-hide on
  click; also `dismiss_tooltip` on workspace switch, tab/terminal close,
  welcome recent open/remove, and conflict Accept (trigger unmount paths).
- **2026-10-14** — LSP Install downloads into `…/isengard/lsp/` (app-local
  first, PATH fallback); Remove deletes that tree only (no OS package cmds).
- **2026-10-16** — Selection matches: soft-highlight other exact occurrences
  of the selected text. Skip no-op `set`s (Kit always notifies). Fill uses
  Kit's hover wash (`selection` with `s: 0.1`) so matches match symbol hover.
- **2026-10-16** — Completion docs popover: content-sized list+docs (not
  stretched to full remaining width), sidebar-aware clamp, stack when tight,
  docs scroll at 240px max height (vendored Kit patch on `gpui-component`).
- **2026-10-16** — Auto-import completions: advertise `resolveSupport` /
  `labelDetailsSupport`, enable tsserver module-export prefs, resolve items
  for `additionalTextEdits`, show import source in `detail`, and apply
  import edits on Tab/Enter accept (restart the language server once).
- **2026-10-16** — Completion accept: synthesize/clamp `textEdit` to the
  typed segment after `.` so Enter keeps `a.map` (Kit's latch otherwise
  replaces the whole `a.map` with `map`). Tab accepts the open menu
  (Kit only bound Tab → indent).
- **2026-10-16** — Rank/filter completions by the segment after `.` (Kit
  latches `a.map` as the query; without this, `flatMap` outranks / hides `map`).
  Drop superseded completion replies so a slow `a.` response cannot overwrite
  the refined `a.map` menu.
- **2026-10-16** — Fix stale/wrong completions: sync `didChange` on the
  request path before completion (Kit raced ahead of the Change subscription),
  and treat queries ending in `.` as TriggerCharacter.
- **2026-10-16** — Fix completion popup empty: normalize Kit's bogus
  multi-char `triggerCharacter` to LSP `Invoked` before the server sees it.
- **2026-10-16** — Fix go-to-definition crash: defer cross-file open so we
  don't nest GPUI entity updates inside the editor.
- **2026-10-14** — LSP row status is a fixed circular dot (not a collapsed bar).
- **2026-10-14** — LSP popover lists only active-language + installed servers
  (not the full registry dump).
- **2026-10-14** — LSP status icon opens a catalog with Install / Remove icon
  actions (registry recipes; not bundled servers).
- **2026-10-14** — Opt in `BrainCircuit` / `Circle` Lucide assets so the LSP
  status-bar icon actually paints.
- **2026-10-14** — Status bar LSP is one icon; click opens server list to
  Remove (uninstall), with a status badge on the icon.
- **2026-10-14** — Toast messages wrap instead of clipping (long LSP install
  errors).
- **2026-10-14** — Missing language server offers an Install dialog (known
  recipes run rustup/npm/pip/…); otherwise a toast.
- **2026-10-14** — LSP registry covers common PATH servers; `config.json`
  `lsp` overrides + Isengard → Open Config File… (no bundled servers).
- **2026-10-14** — LSP IntelliSense MVP: stdio client + Kit providers
  (completion, hover, go-to-definition, diagnostics).
- **2026-10-14** — Diff/conflict preview tabs restore across sessions
  (while the path is still dirty).
- **2026-10-14** — Active diff/conflict tab highlights its Source Control
  change row.
- **2026-10-14** — Diff/conflict tabs open scrolled to the first change.
- **2026-10-14** — Conflict Accept chips align with the text column
  (folding off; Kit gutter + pad inset).
- **2026-10-12** — Conflict Accept chips sit at column 0 of each hunk
  (line numbers off on conflict panes).
- **2026-10-10** — Conflict Accept chips sit on each hunk line (not the
  toolbar); whole-file Accept unchanged.
- **2026-10-09** — Diff/conflict side-by-side panes lock scroll together.
- **2026-10-09** — Diff/conflict overview ticks fill the full scrollbar
  track width (no inset).
- **2026-10-09** — Diff/conflict overview marks pin top+bottom (no
  full-height stretch); track width = Kit scrollbar; merge banner turns
  warning yellow when conflicts are cleared.
- **2026-10-09** — Conflict Accept only in editor toolbar; Edit/Preview
  `ButtonGroup` uses `.small()` to match; demo uses a long conflict file.
- **2026-10-09** — Conflict open: Current | Incoming side-by-side tab
  (diff highlights + Accept toolbar).
- **2026-10-09** — Conflict rows: Accept Current / Incoming (whole
  file); Commit refuses paths that still contain conflict markers.
- **2026-10-09** — Source Control surfaces merge/rebase/cherry-pick
  conflicts (banner + `U` rows first; open file not diff; lock
  Pull/Push/Discard; seed `MERGE_MSG`) — no line/block resolver.
- **2026-10-08** — Panel header rows share `PANEL_HEADER_ROW_H` (36px):
  sidebar chrome, explorer/git toolbars, editor tabs (`Tab::large`), Edit/
  Preview bar — bands align across the split.
- **2026-10-08** — Close Folder/Workspace: stable app focus after Git tab,
  WeakEntity menu fallback, defer close past AppMenuBar dismiss (Git chrome
  focus was ephemeral and dropped the action).
- **2026-10-08** — Explorer tree toolbar: folder/workspace name left,
  New File / New Folder right (`justify_between`).
- **2026-10-08** — Font size menu items only when an editor tab is open
  (hidden on empty welcome).
- **2026-10-08** — Title-bar menus omit dead actions by state (welcome /
  folder / tabs); Close Folder vs Close Workspace; toast on dialog/PTY/
  save-cancel failures that previously failed silently.
- **2026-10-08** — Git Push toast uses git’s own success text (e.g.
  `Everything up-to-date` from stderr), not a synthetic "Pushed".
- **2026-10-08** — Git busy: disable sibling actions; spinner only on the
  button that started Pull/Push/Commit/discard.
- **2026-10-08** — Git label-only busy buttons (Pull/Push/Suggest/Commit)
  set a `Loader` icon so Kit `.loading()` actually paints a spinner.
- **2026-10-08** — In-flight Button `.loading()` rule in design.md; git +
  search replace use spinners; git checkboxes get `cursor_pointer`.
- **2026-10-08** — Git commit selection defaults to none selected.
- **2026-10-08** — Git commit selection: per-file + select-all checkboxes;
  Suggest/Commit use selection only (`git add -- paths`).
- **2026-10-08** — Git Pull/Push labels show behind/ahead counts vs
  upstream (`Pull (N)` / `Push (N)`).
- **2026-10-08** — Git Commit disabled while the message is empty/whitespace.
- **2026-10-08** — Git repo sections: bottom border only between repos
  (not under a single or last section).
- **2026-10-08** — Git panel Suggest fills commit box from change paths
  (Add/Update/Remove + optional docs:/test:; no AI, does not commit).
- **2026-10-08** — Git change rows: Open File icon (permanent tab, like
  the tree) next to discard.
- **2026-10-08** — Git diff overview ruler: absolute overlay (flex sibling
  blew out pane height); ticks still via document fractions.
- **2026-10-08** — Git side-by-side diff: overview-ruler ticks on each
  pane (Kit scrollbar has no marker API).
- **2026-10-08** — Git side-by-side diff: line highlights (delete/insert
  fills) via `similar`.
- **2026-10-08** — Git change preview: equal side-by-side HEAD | Working
  Tree panes, both read-only (replaces unified diff tab).
- **2026-10-08** — Git sections: flush layout, bottom border only (no cards).
- **2026-10-08** — Git changes: Material icons, status letter on the right,
  discard file / discard all (confirm); Refresh ghost icon-only.
- **2026-10-08** — Git panel UX: strip order Explorer|Git|Search; commit
  textarea; outlined buttons; bordered repo sections; collapsible changes;
  click opens read-only diff preview.
- **2026-10-08** — Minimal Source Control sidebar (stacked multi-root repos;
  status / commit-all / pull ff-only / push via CLI `git`).
- **2026-10-08** — Remember main window size/position/maximized in
  `config.json` across launches.
- **2026-10-08** — Project search speed: parallel walk, cancel on new query,
  skip build/media dirs & >1 MiB files, aho-corasick prefilter, paint ≤500 hits.
- **2026-10-08** — Project search: highlight match in result rows and in the
  editor on preview click (`RangeDecoration` fill + selection).
- **2026-10-08** — Project search file headers use Material `tree_icon`
  (same as tree/tabs).
- **2026-10-08** — Explorer / Search toggle back at the top of the sidebar
  (status bar only shows/hides the panel).
- **2026-10-08** — Sidebar icon buttons use `.small()` (was `.xsmall`);
  search find/replace rows use `gap_3`; status line only when non-empty.
- **2026-10-08** — Embed Lucide `FolderTree` / `Search` (+ search option icons).
- **2026-10-08** — Project Find & Replace in the left sidebar (Explorer ↔
  Search); `⌘⇧F` / Edit > Find in Files; replace-all confirm; `.gitignore`
  + open-buffer aware (`src/search`).
- **2026-10-08** — Toast icon/text vertically centred via custom notification
  content (`notify_success` / `notify_error`); tab/recent × stay `.ghost()`.
- **2026-10-08** — Kit semantic variants: discard dialog buttons
  `.danger().outline()`; success/error toasts typed (design §3).
- **2026-10-08** — Embed Lucide `FilePlus` / `FolderPlus` via `AppAssets`
  (default Kit icons + those two); tree toolbar uses them.
- **2026-10-08** — File tree toolbar icons: `FilePlus` / `FolderPlus`.
- **2026-10-08** — File tree mini toolbar: New File / New Folder (for when
  the tree has no blank space to right-click).
- **2026-10-08** — Right-click empty space in the file tree → New File /
  New Folder at the workspace root.
- **2026-10-08** — New File / New Folder are inline in the tree (same as
  rename); no name modal.
- **2026-10-08** — Removed Delete Permanent; tree Delete is Trash-only.
- **2026-10-08** — Tree click keeps Tree focus (preview open no longer steals
  it); Delete / F2 work after selecting a file.
- **2026-10-08** — File tree Delete → OS Trash.
- **2026-10-08** — File tree rename is inline in the row (no modal); New
  File/Folder still use a name dialog.
- **2026-10-08** — File tree: context menu New File/Folder (dirs), Rename /
  Delete; `F2` / `Delete`/`Backspace` on tree selection.
- **2026-10-08** — Terminal selection: `write_to_primary` only on
  linux/freebsd so macOS/`cargo run` compiles again.
- **2026-10-07** — New File in workspace stays in-memory until Save; Linux
  title-bar close runs unsaved-changes quit dialog.
- **2026-10-07** — Save / Save All no longer show a toast on success.
- **2026-10-07** — New File: welcome opens untitled buffer (save on demand);
  with a folder/workspace, auto-creates under active tree dir / root.
- **2026-10-07** — Terminal copy shows a "Copied" toast; URL hover underline
  sits farther under the glyphs (`font_size` + 5px).
- **2026-10-07** — Terminal paste: `TogglePreview` no longer steals
  `Ctrl+Shift+V` / `⌘⇧V` while Terminal focused; URL hover underline is one
  continuous stroke with a gap below the glyphs.
- **2026-10-07** — Terminal copy/paste use OS chords (Linux/Win
  Ctrl+Shift+C/V; macOS ⌘C/V); URL hover underline + pointer cursor.
- **2026-10-07** — Terminal: mouse selection (word/line clicks), copy/paste,
  Ctrl/Cmd+click to open URLs.
- **2026-10-07** — Notifications clear the status bar (`margins.bottom` =
  status-bar height + 16px).
- **2026-10-07** — Reverted no-animation experiment; Kit dialog/toast motion
  is default again (`reduce_motion` left off).
- **2026-10-07** — Cross-platform Development setup in architecture (macOS /
  Linux / Windows) + app-icon matrix; `scripts/install-dev-icon.sh` /
  `.ps1` entrypoints (Windows `.exe` icon already via `build.rs`).
- **2026-10-07** — Linux app icon for `cargo run`: `WindowOptions::{app_id,icon}`
  (X11) + `scripts/install-linux-dev-icon.sh` for Wayland panel matching.
- **2026-10-07** — Editor tab file icons: `tree_icon` + name as `Tab`
  children (`gap_2`, match file tree); avoid `Tab::prefix` (extra gap).
- **2026-10-07** — Sidebar width/visibility and terminal height persist per
  workspace/folder session (not globally).
- **2026-10-07** — Terminal tabs (cwds + visibility) persist in the workspace
  session and restore when reopening the editor.
- **2026-10-07** — Multi-tab terminals; root picker only when creating a
  session (New Terminal / first open), not when un-minimizing.
- **2026-10-07** — Multi-root terminal folder dialog: ↑↓ / Enter / Escape.
- **2026-10-07** — Terminal only while a folder/workspace is open (not on
  welcome).
- **2026-10-07** — Multi-root: opening the terminal asks which folder to use
  as the shell cwd.
- **2026-10-07** — Terminal open/closed state persisted in config
  (`terminal_visible`).
- **2026-10-07** — Panel chrome: status-bar Show/Minimize Terminal + Toggle
  Sidebar; terminal/sidebar sizes persisted in config across hide/show.
- **2026-10-07** — Terminal panel sits under the editor column only (file tree
  keeps full height).
- **2026-10-07** — Bottom integrated terminal: `v_resizable` panel, default
  shell via `portable-pty` + adapted `alacritty_terminal` view; View >
  Toggle Terminal / `Ctrl+\``.
- **2026-10-07** — Welcome Start: two columns (workspace | folder/file) with
  a vertical separator; unique icons per action.
- **2026-10-07** — Welcome Start: side-by-side rows + unique icons
  (LayoutDashboard / FolderClosed / FolderOpen / File / FileText).
- **2026-10-07** — Welcome + File menu Start grouping: Open/New Workspace,
  separator, then Open Folder / Open File / New File (`⌘N` / `Ctrl+N`).
- **2026-10-07** — Welcome + File menu: New Workspace… (pick folders, save
  `.isengard-workspace`) and New File… (`⌘N` / `Ctrl+N`; creates empty file).
- **2026-10-07** — Added [credits.md](credits.md): recognition for GPUI Kit,
  Zed/GPUI, Lucide, Tree-sitter, Ayu Darker / Ayu, fonts, Material Icon Theme,
  and direct crates.
- **2026-10-07** — Docs restructure: living hand-off under `docs/` (status,
  architecture, design) + single `changelog.md`; root `AGENTS.md` /
  `CLAUDE.md` are stubs. Removed duplicate DESIGN/CLAUDE changelogs.
- **2026-10-07** — Tab right-click: Close / Close Others / Close All (Kit
  `Tab`s in `h_flex` + `ContextMenu`; `TabBar` cannot host menus).
- **2026-10-07** — Slogan "The already configured editor" on welcome, About,
  and cargo-bundle short_description.
- **2026-10-07** — Session restore also keeps per-tab cursor (line/col) and
  editor scroll offset.
- **2026-10-07** — Session restore: multi-root expand respects collapsed roots
  (`expand_paths` applies the saved set; nested expands under a collapsed
  parent are not persisted).
- **2026-10-07** — Session restore: open tabs + expanded tree dirs persisted
  per folder/workspace in `config.json` (`sessions`); restored when reopening
  from welcome / Open / CLI. No dirty-buffer or cold-start auto-open.
- **2026-10-07** — File tree hover full-bleed (no tree `px`; inset on row content).
- **2026-10-07** — Dropped the file-tree header label (name lives in the title bar).
- **2026-10-07** — Indent guides: paint every ancestor column through all
  descendants (incl. last child) + 1px overlap; multi-root skips only the
  cross-root rail.
- **2026-10-07** — Dropped unused `document::is_svg` (SVG via `Language::Svg`).
- **2026-10-07** — File tree: depth-0 rows align with the workspace title
  (`px_3`); each nest is one `INDENT_COL` only (no extra `INDENT_BASE`).
  Multi-root still suppresses only the cross-root rail.
- **2026-10-07** — SVG Edit/Preview like Markdown (buffer → `Image::from_bytes`);
  View menu "Toggle Preview" covers both. Raster images remain image-only.
- **2026-10-07** — Image preview tabs (`img(path)` for png/jpg/gif/webp/…).
- **2026-10-07** — Notifications bottom-right (`theme.notification.placement`).
- **2026-10-07** — Markdown Edit/Preview toggle via Kit `TextView::markdown`
  (toolbar + View menu + `⌘⇧V` / `Ctrl+Shift+V`). Not the same as preview tabs.
- **2026-10-07** — Unsaved tab indicator: blue square (`theme.blue`; was yellow).
- **2026-10-07** — Close/remove × use Kit ghost default (removed red-hover
  `destructive_icon_button` / dialog override).
- **2026-10-07** — Modal dialogs centred in the window (`center_dialog`; Kit
  default was top ~10%). Context menus unchanged.
- **2026-10-07** — Unsaved tabs: yellow square indicator (`theme.yellow`).
- **2026-10-07** — Preview tabs (VS Code-style): tree single-click = preview,
  double-click file/tab or edit = pin.
- **2026-10-07** — Gitignored tree rows: 60% opacity.
- **2026-10-07** — File tree: indent columns don't shrink on narrow panels
  (labels truncate instead).
- **2026-10-07** — File tree indent guides + gitignore dimming (show muted,
  don't hide); `ignore` crate + `file_tree::{indent_guides,ignore}`.
- **2026-10-07** — Editor/gutter background = sidebar (matches file tree);
  Dockerfile/Containerfile detected (Bash grammar alias; status shows
  "Dockerfile"); `.dockerignore` → Bash.
- **2026-10-07** — Language aliases: `.env*` / ignore files → Bash; lockfiles →
  JSON/TOML/YAML; GraphQL highlight query patch (`assets/highlights/graphql.scm`)
  so `.gql`/`.graphql` are not plain white (kit ships empty highlights).
- **2026-10-07** — Project licensed GPL-3.0-only (`LICENSE`, `Cargo.toml`);
  `CONTRIBUTING.md` states forks OK, PRs not accepted.
- **2026-10-07** — Syntax tokens: Ayu Darker from
  [k4yt3x/zed-theme-ayu-darker](https://github.com/k4yt3x/zed-theme-ayu-darker)
  (GPL-3.0; `assets/themes/LICENSE`). Replaces MIT Ayu Dark / Gruvbox.
- **2026-10-07** — Syntax tokens: Ayu Dark (MIT from Zed's theme). Gruvbox
  removed. (Superseded same day by Ayu Darker.)
- **2026-10-07** — Dark mode only: removed light theme, View/title-bar toggle,
  and `AppConfig.dark_mode`. Syntax tokens were briefly Gruvbox Dark.
- **2026-10-06** — Title-bar sun/moon icon toggles light/dark (same as View
  menu); macOS title stays centred.
- **2026-10-06** — Editor/syntax colours: GitHub Dark + GitHub Light highlight
  themes (token colours only; editor chrome unchanged).
- **2026-10-06** — Enabled all GPUI Kit tree-sitter languages
  (`tree-sitter-languages`) and expanded `Language` detection accordingly.
- **2026-10-06** — File tree: full Material Icon Theme pack (1251 SVGs +
  `material-icons.json` associations) instead of a curated subset.
- **2026-10-06** — File tree Material icons rendered with `img()` so colours
  survive (GPUI `Icon`/`svg()` alpha-masks and tints).
- **2026-10-06** — File tree icons: curated Material Icon Theme subset
  (`assets/icons/material/`, MIT) via `file_tree::tree_icon`; Lucide kept
  for the rest of the UI.
- **2026-10-06** — View menu: trailing separator so macOS "Enter Full Screen"
  sits in its own section below font-size items.
- **2026-10-06** — Removed Next/Previous Tab from the View menu (the
  Ctrl+Tab / Ctrl+Shift+Tab shortcuts still work).
- **2026-10-06** — Pointer cursor on all clickable elements (dialog buttons,
  tabs, tree rows, destructive buttons); GPUI Kit only sets it for link/text
  buttons (docs/design.md §3).
- **2026-10-06** — Welcome screen: workspaces before folders ("Open Workspace…"
  first in Start; "Recent workspaces" above "Recent folders").
- **2026-10-06** — VS Code–style workspaces: multi-root folders, `.isengard-workspace`
  files (relative paths), Add Folder / Open / Save As / Close Workspace,
  root context menu, separate recent folders/workspaces lists, shared
  `open_choice_dialog`.
- **2026-10-06** — Added design rules doc. Extracted `destructive_icon_button`;
  the recent-folder remove × now matches the tab close × (red only on hover).
- **2026-10-06** — Welcome screen shortcut hint shows separate key caps (`⌘` `O`).
- **2026-10-06** — App logo (concept B, optically centred) on the welcome
  screen and as the app icon (macOS Dock at runtime, Windows .exe icon,
  cargo-bundle metadata).
- **2026-10-06** — Tab close button: muted ×, red only on hover; 12px right spacing
  matching the label padding.
- **2026-10-06** — Status bar is hidden entirely unless a file tab is active.
- **2026-10-06** — Ported the whole UI from egui/eframe to GPUI Kit 0.7.1
  (Tree, TabBar, Editor, Dialog, StatusBar, TitleBar, native menus). Switched
  to JetBrains Mono Nerd Font for UI and editor. Settings moved from eframe
  storage to a JSON file. Removed our own tree-sitter highlighter (GPUI Kit's
  Editor highlights). New: Save All, Edit menu, notifications.
- **2026-10-06** — Square (block) corners on all widgets, windows, menus.
- **2026-10-06** — Welcome screen with recent folders; file tree hidden until a
  folder is open; File > Open Recent / Close Folder.
- **2026-10-06** — Fix panic when closing the last tab via its × button.
- **2026-10-06** — Initial implementation of phases 1–4 on egui: layout, lazy
  file tree, tabbed editor, tree-sitter highlighting, save/close confirmation,
  shortcuts, status bar, persisted settings, `isengard <path>` CLI argument.
