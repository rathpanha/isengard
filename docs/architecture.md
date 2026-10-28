# Architecture

## Development setup

Stable Rust via [rustup](https://rustup.rs/). First `cargo run` compiles GPUI
and takes several minutes; later builds are fast. The `block v0.1.6`
future-incompat notice comes from a dependency — ignore it.

```sh
cargo run                       # welcome screen; isengard must be warning-free
cargo run -- /path/to/folder    # open a folder
cargo run -- /path/to/file.js   # open a file (+ parent folder)
cargo check                      # agents: after src/ edits — zero isengard warnings
cargo test                       # CI uses RUST_TEST_THREADS=1 (FakeAgent TLS races)
cargo clippy --all-targets      # must be warning-free
cargo build --release
```

**Linux packages (first release target):** after `cargo build --release` on
Linux, `./scripts/package-linux.sh` (needs [nfpm](https://nfpm.goreleaser.com/))
writes `.deb`, `.rpm`, and a `.tar.gz` under `dist/linux/`, plus `SHA256SUMS`.
Version comes from `VERSION` or `Cargo.toml`. Contents: `/usr/bin/isengard`,
`.desktop` + hicolor icon for `app.isengard.edtir`, `LICENSE` / credits under
`/usr/share/doc/isengard/`. Tag `v*` runs
[`.github/workflows/release.yml`](../.github/workflows/release.yml)
(tag must equal `Cargo.toml`; notes = `docs/releases/<tag>.md`) and attaches
those assets to a GitHub Release.

On macOS, `./scripts/package-macos.sh` builds `Isengard.app` (hand-assembled
from `packaging/macos/Info.plist.in`, `Isengard.icns`, a `lipo` universal binary
of `aarch64`/`x86_64-apple-darwin`; `TARGETS=…` overrides) and writes
`isengard-<ver>-macos-universal.{tar.gz,dmg}` + `SHA256SUMS` under
`dist/macos/`. The same workflow's `macos` job (runner `macos-14`) runs it and
feeds the shared `publish` job (Linux still publishes if macOS fails).
There is **no Apple Developer ID / notarization** (costs $99/yr), so:
- The app must still be code-signed (Apple Silicon refuses unsigned code). A
  *self-signed* certificate (`scripts/create-macos-cert.sh`, one time) is used
  so the signature identity is stable across releases — the Keychain's "Always
  Allow" for the Claude Code token then survives updates. Secrets
  `MACOS_CERT_P12` (base64 `.p12`) / `MACOS_CERT_PASSWORD`;
  `scripts/ci-macos-keychain.sh` imports it into a throwaway keychain and marks
  it trusted for code signing (codesign ignores untrusted identities). Without
  the secret the build is signed ad-hoc (`-`) and the Keychain re-asks after
  each update. Back the `.p12` up: a new certificate = a new identity.
- Gatekeeper blocks quarantined (browser-downloaded) copies until the user does
  System Settings > Privacy & Security > Open Anyway. Files fetched by `curl`
  or written by the app's own updater are not quarantined, so
  `scripts/install-macos.sh` (curl one-liner in the README) and in-app updates
  run without any prompt. `install-macos.sh` embeds the minisign public key —
  keep it equal to `update::PUBLIC_KEY`.

A Finder / Dock launch on macOS gets a bare `PATH`, so the agent adapter
(`env: node: No such file or directory`) and terminal tools (`oh-my-posh`) fail
while `cargo run` works. `shell_env::import_login_path` (first thing in `main`)
reads the login shell's `PATH` (`$SHELL -ilc`, markers around `$PATH`), merges
it in front of the current `PATH` and `set_var`s it — skipped when stdin is a
tty (started from a terminal). The shell start-up (~1 s) is paid once: the
result is cached in `<cache dir>/isengard/login-path`, later launches apply the
cache instantly and refresh it from a background thread for the next launch.
The interactive terminal also starts its shell with `-l` on macOS
(`terminal/pty.rs`) so `~/.zprofile` runs, like Terminal.app. Linux needs
neither: desktop sessions already pass the login environment to launched apps.

Agents: see `.cursor/rules/cargo-warning-free.mdc`. Finish no `src/` task while
`cargo check` still prints `isengard` warnings — fix the code; do not disable
or filter warning output. `block v0.1.6` is upstream, not this package.

Settings: `dirs::config_dir()/isengard/config.json` (macOS:
`~/Library/Application Support/isengard/config.json`). Delete to reset.

### macOS

1. Xcode Command Line Tools (`xcode-select --install`).
2. Rust via Homebrew's keg-only `rustup` (or rustup directly). If `cargo` is
   missing after Homebrew install:

   ```sh
   export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
   ```

3. App icon: automatic at runtime (`branding::set_dock_icon`) for `cargo run`.
   Bundled `.app` builds (`scripts/package-macos.sh`) use `Isengard.icns`;
   the `[package.metadata.bundle]` table in `Cargo.toml` is only for an
   optional manual `cargo bundle`.
4. Release builds for both architectures need
   `rustup target add aarch64-apple-darwin x86_64-apple-darwin`.

### Linux (Fedora example)

1. Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, then
   `source "$HOME/.cargo/env"` (or open a new shell).
2. Link/build deps (adjust names on Debian/Ubuntu — usually `*-dev` instead of
   `*-devel`):

   ```sh
   sudo dnf install -y gcc clang cmake pkgconf-pkg-config \
     fontconfig-devel freetype-devel \
     libxkbcommon-devel libxkbcommon-x11-devel \
     libxcb-devel libX11-devel libXcursor-devel \
     wayland-devel alsa-lib-devel openssl-devel
   ```

3. App icon:
   - **X11:** automatic via `WindowOptions::icon` (`icon-1024.png`).
   - **Wayland:** one-time user `.desktop` + hicolor icon so the compositor can
     match `app_id` (`app.isengard.edtir`):

     ```sh
     ./scripts/install-dev-icon.sh   # or install-linux-dev-icon.sh
     ```

     Re-run if you move the repo (Exec path is absolute to `target/debug/isengard`).

### Windows

1. [rustup](https://rustup.rs/) with the **MSVC** toolchain
   (`x86_64-pc-windows-msvc`), not GNU.
2. [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   with the **Desktop development with C++** workload (MSVC + Windows SDK).
   Needed for linking GPUI and for `embed-resource` / `rc.exe` (`.exe` icon).
3. In a **Developer PowerShell** or a shell where `cargo` and the MSVC tools are
   on `PATH`:

   ```powershell
   cargo run
   ```

4. App icon: automatic — `build.rs` embeds `assets/logo/Isengard.ico` into every
   Windows binary (`cargo run` / `cargo build`). No install script. Optional
   reminder: `.\scripts\install-dev-icon.ps1`.
5. Release builds are GUI-subsystem (`windows_subsystem = "windows"` in
   `main.rs`, release only) so no console window opens behind the editor.
   `.\scripts\package-windows.ps1` writes `dist\windows\isengard-<ver>-windows-x86_64.zip`
   (`isengard.exe`, `LICENSE`, `credits.md`); the workflow's `windows` job
   (`windows-latest`) builds and packages only — tests/clippy run on the Linux
   and macOS jobs because the suite assumes Unix paths in places.

**Windows distribution has no code-signing certificate** (costs money). A
browser-downloaded zip gets SmartScreen's "Windows protected your PC"
(More info > Run anyway); files fetched by PowerShell or written by the app's own
updater carry no internet mark. So `scripts/install-windows.ps1`
(`irm … | iex`, README) checks SHA256SUMS (+ minisign when installed), unpacks
into `%LOCALAPPDATA%\Programs\Isengard` (no admin), adds a Start Menu shortcut
and the folder to the user `PATH`. It embeds the minisign public key — keep it
equal to `update::PUBLIC_KEY`. In-app updates (`InstallKind::WinExe`,
`classify_windows`: writable folder and not a `target\` build) extract
`isengard.exe` from `…-windows-x86_64.zip`, rename the running exe to
`isengard.exe.old` (Windows allows renaming, not overwriting, a running exe),
move the new one in, restore on failure, then restart; `cleanup_old_exe` removes
the `.old` at the next launch. Free signing is possible later through the
SignPath Foundation (open-source projects) — not applied for yet.

### App icon matrix (`cargo run`)

| Platform | Mechanism |
| --- | --- |
| macOS | `branding::set_dock_icon` (runtime Dock image) |
| Windows | `.exe` resource via `build.rs` + `assets/windows/isengard.rc` |
| Linux X11 | `WindowOptions::icon` |
| Linux Wayland | `WindowOptions::app_id` + `scripts/install-dev-icon.sh` |

`./scripts/install-dev-icon.sh` detects the OS and either installs (Linux) or
prints that nothing is needed (macOS / Git Bash on Windows).

## Source tree

```
src/
├── main.rs              app bootstrap: `AppAssets` (default Kit icons +
│                        FilePlus/FolderPlus/FolderTree/Search/CaseSensitive/
│                        WholeWord/Regex/Replace/ReplaceAll), gpui_kit::init,
│                        fonts, theme, key bindings, menus, window
│                        (QuitMode::LastWindowClosed)
├── app/                 IsengardApp entity (one GPUI view; split by concern)
│   ├── mod.rs           struct + `new`, shared types (`Switch`, `SidebarView`), module map
│   ├── actions.rs       `actions!`, tab path actions, key bindings, menu bridge
│   ├── agent.rs         Agent panel toggle, OS attach picker, file I/O, login → terminal
│   ├── tab.rs           `EditorTab` / `TabBody` / `DiffKind`
│   ├── workspace.rs     open/switch/persist workspace, recents, titles
│   ├── session.rs       session + window geometry restore/persist
│   ├── files.rs         open/save/close tabs, untitled, font/preview
│   ├── tree.rs          inline create/rename/delete in the file tree
│   ├── lsp.rs           LSP attach, install/remove UI, event pump
│   ├── search.rs        project search/replace + selection soft-highlight
│   ├── git.rs           Source Control diff/conflict tabs
│   ├── watch.rs         watcher pump: reload clean tabs, refresh tree + Git
│   ├── terminal.rs      bottom PTY panel show/create/cwd
│   ├── terminal_cwd.rs  multi-root “Open Terminal in…” picker
│   ├── chrome.rs        sidebar toggle, About, config, quit
│   ├── update.rs        update-available / download / restart / install-command dialogs
│   ├── render.rs        title/sidebar/editor/status-bar paint + `Render`
│   └── welcome.rs       welcome screen (start actions + recent folders)
├── update.rs            release lookup (GitHub API), install-kind detection,
│                        asset pick, download + minisign/SHA-256 verify,
│                        in-place tarball install (no UI; unit-tested)
├── watch.rs             `FsWatcher`: non-recursive watches per dir (skips
│                        `node_modules`/… so Linux inotify does not blow up)
│                        → flume of `FsChange`
├── theme.rs             bundled font loading + Theme overrides (font, radius 0, sizes)
├── branding.rs          logo mark, APP_ID, Dock / window icon helpers
├── menus.rs             native menus + AppMenuBar (Windows/Linux)
├── config.rs            AppConfig (serde JSON) + recents + sessions + window geometry
├── workspace.rs         Workspace (root folders + optional file): naming,
│                        root_for/relative_label, load/save .isengard-workspace
├── editor/
│   ├── document.rs      read_text / write_text / file_name / is_image
│   ├── language.rs      Language enum: from_path, highlighter_name, display_name
│   ├── highlights.rs    patches weak GPUI Kit highlight queries (GraphQL, TSX)
│   └── tabs.rs          TabList<T>: order/activate/reorder (unit-tested)
├── lsp/                 Language Server Protocol (IntelliSense)
│   ├── host.rs          workspace servers, didOpen/Change/Close, diagnostics pump
│   ├── client.rs        stdio JSON-RPC client (flume + background thread)
│   ├── bridge.rs        Kit Completion/Hover/DefinitionProvider + showDocument
│   ├── registry.rs      Language → default ServerSpec + config overrides
│   ├── install.rs       app-local download / resolve (bin then PATH)
│   ├── protocol.rs      Content-Length framing
│   └── uri.rs           file:// path ↔ Uri
├── acp/                 Agent Client Protocol host (no UI)
│   ├── client.rs        `AgentConnection`: own thread, `block_on`, flume
│   │                    commands in / `AgentEvent`s out (+ reply senders)
│   ├── registry.rs      `config.agent` command, else built-in Claude Agent
│   ├── install.rs       app-local `npm install --prefix …/agents/claude`
│   ├── files.rs         `slice_lines` for `fs/read_text_file` ranges
│   └── testing.rs       (tests) in-memory `FakeAgent` + `install` hook
├── agent/
│   ├── panel.rs         `AgentPanel`: transcript (`MessageScroller` + half-pane bottom space),
│   │                    tool rows, permission buttons, composer + `@` menu
│   ├── limits.rs        Claude plan usage (5h/week + optional Opus/Sonnet +
│   │                    product breakdown + subscription label) via OAuth;
│   │                    refreshes/persists Claude Code tokens; merge keeps last good
│   ├── mention.rs       `@` query parse + workspace file/folder listing
│   └── prompt.rs        `Attachment` → ACP `ContentBlock`s (`resource` /
│                        `resource_link` / `image`) from agent prompt caps
├── file_tree/
│   ├── node.rs          FsNode (File/Dir), lazy load_one_level, set_expanded, find_mut
│   ├── ignore.rs        GitIgnoreIndex — dim gitignored paths (show, don't hide)
│   ├── indent_guides.rs vertical tree lines from flat entry depths
│   └── icons.rs         Material Icon Theme (full set) → tree_icon(path, …)
├── search/              project Find & Replace (sidebar Search view)
│   ├── engine.rs        parallel WalkBuilder + regex/aho-corasick; cancel;
│   │                    size/ext/dir skips; open-buffer override; replace
│   └── panel.rs         SearchPanel UI (inputs, toggles, capped results)
├── git/                 minimal Source Control (sidebar Git view)
│   ├── engine.rs        CLI `git`: discover, porcelain, `RepoOp`,
│   │                    commit/pull/push/discard, accept ours/theirs,
│   │                    write resolved conflict, marker guard,
│   │                    `file_sides` / conflict stages
│   ├── diff_highlight.rs line ranges + conflict hunks via `similar`
│   └── panel.rs         GitPanel: stacked repos, conflict banner, list
├── terminal/            bottom integrated PTY panel (adapted from gpui-terminal)
│   ├── panel.rs         TerminalPanel: Kit chrome, show/hide, restart on switch
│   ├── pty.rs           portable-pty session + default shell / cwd
│   ├── view.rs          TerminalView: paint, select/copy/paste, URL click
│   ├── state.rs         alacritty_terminal Term wrapper
│   ├── render.rs        cell → GPUI paint (selection invert)
│   ├── links.rs         URL detect + open (Ctrl/Cmd+click)
│   ├── input.rs         keystroke → bytes
│   ├── colors.rs        ANSI / theme palette
│   └── …                mouse, box_drawing, clipboard, event
└── ui/
    ├── components.rs    shared compositions encoding docs/design.md rules
    │                    (center_dialog)
    ├── file_tree.rs     FileTreePanel: FsNode -> TreeItem sync, TreeEvent handling, render;
    │                    tree path actions (RenamePath, DeletePath, NewFileIn, …)
    └── tab_drag.rs      editor/terminal tab drag payloads + preview
```

## How it fits together

- `gpui_kit::open_window` wraps `IsengardApp` in a `Root` (needed for dialogs,
  notifications, tooltips, menus). Never add a second Root.
- **Actions**: declared with `actions!(isengard, [...])` in `app/actions.rs`,
  bound in `app::init` (`secondary-*` = Cmd on macOS, Ctrl elsewhere), handled
  by `.on_action(cx.listener(..))` on the root div, which `track_focus`es
  `IsengardApp::focus_handle`. Actions only reach the handlers if focus is
  inside the view — `focus_active_editor` refocuses the editor or the root.
  A global `Quit` fallback in `app::init` quits without checking (used when a
  dialog holds focus). Action types are re-exported at `crate::app::*` for
  menus/UI. New `IsengardApp` methods go in the matching `app/*.rs` concern
  file (child `impl` blocks can touch private fields); do not grow `mod.rs`.
- **Tabs**: `TabList<EditorTab>`; each `EditorTab` owns an `Entity<EditorState>`
  plus two subscriptions: `InputEvent::Change` → mark modified (+ LSP
  `didChange`), and `observe` → re-render (keeps Ln/Col current).
- **LSP**: `IsengardApp::lsp` (`LspHost`) resolves `Language` +
  `AppConfig.lsp` via `lsp::registry::resolve` (built-in defaults +
  per-language enable/command/args). Spawn uses `lsp::install::resolve_command`
  (**app-local** `data_dir/isengard/lsp/bin` first, then system `PATH`).
  Lazy-start on first matching file open; full-document `didChange`;
  `publishDiagnostics` → Kit `diagnostics_mut`. Providers hang on
  `EditorState::lsp_mut()`. Cross-file go-to-def uses Kit `show_document` →
  deferred `open_file` + cursor (never nest entity updates inside the editor
  lease — that panicked). Workspace switch shuts servers down. No Tokio — stdio
  + flume like the terminal. Start is async so the status bar can show yellow
  while spawning. Missing server → Install dialog (`lsp::install` download
  recipes) or toast. Status bar: one icon → short Popover
  (`catalog_servers(focus)` = known/live + active-tab language only — not the
  full registry). Install writes under `…/isengard/lsp/` (ureq + archive
  extract, or `go install`/`npm install --prefix` scoped to that tree — never
  global `-g`/brew/pip/rustup). Remove → `remove_server` / `run_uninstall`
  (delete app-local bin + pkg only). Server map is hand-maintained in
  `lsp/registry.rs` + `lsp/install.rs`. Edit `config.json` by hand (no in-app open action;
  no settings panel).
- **Workspace**: `IsengardApp::workspace` is the source of truth for open
  folders; after changing it call `workspace_changed` (re-syncs the tree via
  `FileTreePanel::set_folders` and the titles) and `persist_workspace` (writes
  the file if there is one). Switching goes through `request_switch(Switch)` →
  optional "save untitled workspace" dialog → `confirm_unsaved_then` →
  `apply_switch` (persists session, closes tabs, records recents, restores
  the new workspace's session). Confirmations use
  `open_choice_dialog` (Cancel / secondary / primary).
  `RemoveWorkspaceFolder(PathBuf)` is a data-carrying action
  (`#[derive(Action)] #[action(namespace = isengard, no_json)]`) dispatched from
  the tree's context menu. Same pattern for `RenamePath` / `DeletePath` /
  `NewFileIn` / `NewFolderIn`. These five are defined in `ui/file_tree.rs`
  (the dispatcher) and handled in `app/` — `ui/` never imports `app`. Blank-space New File/Folder uses a panel
  `ContextMenuExt` around the tree (row menus suppress it). A thin
  toolbar above the tree dispatches the same actions (parent from
  selection, else first root). Selection shortcuts bind only in the Kit
  `Tree` key context: `f2` rename; `delete` / `backspace` → OS Trash
  (`trash` crate).
- **File tree**: `FsNode` is the source of truth. Unloaded/empty directories get
  a disabled placeholder child (`"Loading…"` / `"(empty)"`) because GPUI Kit's
  `TreeItem::is_folder()` is just "has children". On `TreeEvent::Expanded` the
  node loads its children and the `TreeItem`s are rebuilt via `set_items`
  (selection is restored by id). Tree item ids are absolute paths. Row icons
  come from `file_tree::tree_icon` (full Material Icon Theme via `img()` +
  `material-icons.json`), not Lucide.   Create/rename/delete refresh the parent
  dir (or `workspace_changed` when a root moves/vanishes); open tabs under a
  renamed path are remapped. Inline rename / new-file / new-folder live on
  `IsengardApp::tree_edit` (`TreeEditTarget` + `InputState`); create injects
  a temporary `\0creating` row under the parent via
  `FileTreePanel::set_creating`.
- **Terminal:** `IsengardApp::terminal` is an `Entity<TerminalPanel>` (PTY
  session lazy on first show). When the tree is open, `v_resizable("editor-term-split")`
  is the right child of `main-split` (under editor only; tree full height).
  Without a tree, the same vertical split wraps the body. Panel omitted when
  minimized; `AppConfig.terminal_visible` + `terminal_height` restore open
  state and height (`resizable_panel().size`, saved on toggle / resize end).
  `apply_switch` restarts the shell in the first folder cwd (else home).
  Requires at least one workspace folder (hidden on welcome / Close
  Workspace). `ToggleTerminal` shows/hides existing tabs; `NewTerminal`
  always `add_session`. Multi-root create uses `pick_terminal_cwd` before
  `add_terminal_in`. Layout sizes live on `WorkspaceSession`
  (`sidebar_width` / `sidebar_visible` / `terminal_height`); resizable
  element ids include the session key so Kit state does not leak across
  folders. `ToggleSidebar` / `⌘B`/`Ctrl+B`.
- **Agent panel:** one `Entity<AgentPanel>` per folder/workspace in
  `IsengardApp::agents` (keyed by session key, with its subscription);
  `IsengardApp::agent` is the current one, shown as the right child of
  `h_resizable("agent-split-{session key}")` around the whole body (sidebar +
  editor + terminal) while a folder is open. `apply_switch` →
  `switch_agent_panel`: old panel `suspend()` (running turn continues; agent
  dropped when it ends → `Status::Suspended`, transcript kept), new panel
  reused from the map or created from that key's saved `agent_chats` /
  `agent_active` / `agent_prefs` (empty → global `AppConfig::agent_prefs`).
  `agent_visible` / `agent_width` / chat index / prefs live on
  `WorkspaceSession`; `ChatsChanged` / `PrefsChanged` from a background panel
  patch only those fields of its saved session. Header `SlidersHorizontal`
  Agent settings popover uses labeled rows (`Utility model → Model →
  Version`); `SettingsChanged` writes global `AppConfig::agent` (including
  `silent_model` for Improve / title / Git Suggest throwaway sessions) and
  syncs other cached panels.
  `AgentPrefs`
  = composer picks (config option id → select value / bool, plus legacy
  `modes` id), recorded in `set_config_value` / `set_config_bool` /
  `set_mode` and re-sent by `apply_prefs` on every `SessionOpened` (model
  first; values the agent no longer offers are skipped). Silent jobs copy
  chat configs then override Model when `agent.silent_model` is still
  offered. The panel owns `acp::AgentConnection`
  (lazy `ensure_started` on first show / restore / return) and pumps its
  events with `cx.spawn` + `recv_async` (tests: `try_recv` + scheduler timer
  — GPUI's test scheduler rejects cross-thread wakeups); a `generation`
  counter drops events from replaced connections. Chats:   after `Connected`
  the panel sends `SessionOpen::Resume(id)` when `transcript_of` is still
  that chat, else `Load(id)` (agent replays it as updates, user chunks
  merged into one bubble; **N Files** cards are not in ACP history — they
  live on `AgentChat.file_changes` and are spliced in after Load), else
  `New`. One open chat per connection
  (`session/close` the previous); updates are filtered by session id;
  `opens_in_flight` ignores answers to superseded opens. Roots: the
  connection gets every workspace folder (`roots[0]` = session cwd, rest =
  `additional_directories` when the agent advertises the capability);
  `add_folders` / remove folder call `sync_agent_roots` → `set_roots`
  (reconnect + resume, deferred past a running turn). A new chat joins
  the index on its first prompt; a chat that fails to load is dropped from
  the index. The connection runs the SDK's
  connection future on a dedicated OS thread under
  `futures::executor::block_on` — no Tokio, and nothing blocks GPUI's
  executors. Requests that need the editor carry a flume reply sender and
  are answered from a task spawned on the SDK connection so streaming
  continues meanwhile; the panel re-emits file requests as
  `AgentPanelEvent`s for `app/agent.rs` (buffer-first read, write + reload
  tab). Auth: we advertise `auth.terminal`; `authRequired` (-32000) → login
  buttons run `<agent command> <method args>` via
  `TerminalPanel::add_command_session` (one-off tabs are not persisted).
  We do **not** advertise ACP client `terminal/*` — Claude Agent 0.94.0
  runs Bash in-process and never calls those methods. Transcript-footer
  `activity_label` shows the in-progress tool title (or Thinking / Writing)
  while a turn runs (header keeps the session title only). Composer
  attachments (`agent::mention` + `agent::prompt`): `@` opens a
  filtered file/folder menu from workspace roots (plain FS walk + `SKIP_DIRS`,
  not gitignore; each root gets a fair fuzzy-walk budget so secondary roots
  stay searchable; empty query lists roots + nested dirs + root files;
  trailing `/` browses a folder; else fuzzy, capped); pick emits
  `AttachPath` (buffer-first read; raster paths →
  `image_from_path` → ACP `image` + thumbnail; ICO/BMP/TIFF re-encoded to
  PNG for the wire). Paperclip →
  `prompt_for_paths` (OS multi-file; `directories: false` — folders via `@`)
  → same attach path (each image is a separate chip). Drag-drop onto the
  prompt (`TreeEntryDrag` / `EditorTabDrag` / external paths) and
  `⌘L`/`Ctrl+L` (`AddSelectionToAgent`: editor selection → `File` +
  `LineRange`, or whole buffer if empty; terminal selection → `Snippet`)
  use the same chip path. Composer mounts during
  `Starting`/`Opening` when a transcript exists (Send gated on `Ready`).
  Paste image/paths also become chips. `session/prompt`
  sends `Vec<ContentBlock>` (`resource` when `embeddedContext`, else
  `<context>` text, else `resource_link`; images when `image`). Caps from
  `AgentInfo` after `initialize`. `apply_switch` calls
  `reset_agent_for_workspace` (drops the agent: the SDK kills its process
  group). Tests: `AgentConnection::spawn_with` takes any `ConnectTo<Client>`;
  `acp::testing::FakeAgent` serves `Channel::duplex` connections with shared
  chat state (new/load/resume/close/delete), and `acp::testing::install`
  makes panels on the test thread use it; `agent::prompt` unit-tests block
  assembly.
- **File watching:** `workspace_changed` → `restart_fs_watch` (new
  `watch::FsWatcher` over `workspace.folders()`, `fs_watch_gen` bump so an old
  pump stops; skipped under `cfg!(test)`). `FsWatcher` walks the tree and
  registers a **non-recursive** watch per directory, never descending into
  `.git` / `node_modules` / `target` / `.next` / `dist` / `__pycache__` /
  `.venv` / `venv` (Linux inotify is one watch per dir — recursive watch on a
  root would register under those trees and hit `max_user_watches`). New dirs
  get a watch on create. The pump task `recv_async`s, sleeps 300 ms, drains,
  then `apply_fs_changes`: per open text tab, compare disk with `saved` —
  equal → nothing, clean → `reload_open_file_from_disk` (keeps selection/
  scroll; undo still clears via Kit `set_value`), dirty → one warning in
  `disk_conflicts`, missing + clean → close tab, missing + dirty → warn and
  keep. Window activation runs `reconcile_open_tabs_with_disk`. Parents of
  create/remove/rename → `FileTreePanel::refresh_loaded_dirs`
  (`FsNode::refresh_children`: loaded dirs only — never `refresh_dir` here).
  Then `GitPanel::refresh`.
- **Updates:** `update.rs` (pure) + `app/update.rs` (UI). Startup check (≤1×/day,
  `config.updates.last_check`, off via `updates.check`, skipped under
  `cfg!(test)`) and Isengard > Check for Updates… call GitHub
  `releases/latest`; a newer, non-skipped version opens the availability dialog.
  `detect_install_kind`: dev `target/` build or unwritable dir → **Manual**
  (button opens the release page); exe under `/usr/bin` with dnf/zypper/apt →
  **Package** (download the `.rpm`/`.deb`, verify, then a dialog offers Copy
  command / Open in terminal — `sudo <mgr> install '<path>'` is *typed, not run*,
  so the user enters their own password); otherwise writable → **InPlace**
  (download tarball, verify, write `.isengard.update` beside the exe and rename
  over it; on macOS an `Isengard.app` whose parent folder is writable is
  **MacApp** — `install_app_bundle` unpacks `…-macos-universal.tar.gz` into a
  hidden sibling folder, `codesign --verify`s it, then swaps the whole bundle
  (old one moved aside, restored if the swap fails), then "Restart now" → `restart_into` sets `relaunch_exe`, goes through
  `confirm_quit`, and `quit()` re-spawns the exe on the open workspace/folder;
  plain Quit / window close clears the flag). Verification: `SHA256SUMS` +
  `SHA256SUMS.minisig` must verify against `update::PUBLIC_KEY`, then the asset
  hash must match; with an empty `PUBLIC_KEY` nothing is installed (link only).
  Release signing: `release.yml` signs `SHA256SUMS` with secrets
  `MINISIGN_KEY` / `MINISIGN_PASSWORD` (generate once with `minisign -G`, paste
  the public key line into `PUBLIC_KEY`; losing the key means shipping a build
  with a new key). Asset names are restricted to `[A-Za-z0-9._+~-]` because they
  reach a shell command.
- **Dialogs**: `window.open_dialog` with a `DialogFooter` of `Button`s; buttons
  capture a `WeakEntity<IsengardApp>` and call back into it. Always wrap with
  `ui::components::center_dialog` (Kit defaults to top ~10%; we centre). Close
  × buttons (tabs, recent rows, dialogs, toasts) use Kit's default ghost style
  — no custom red hover. Vendored `gpui-component` Dialog patch: no enter
  motion (`ANIMATION_DURATION = 0`), no panel drop shadow, and the host box is
  sized/positioned to the CSD content frame so the dim overlay cannot fill the
  transparent window-shadow padding on Linux. Same vendor: `Input` /
  `Editor::context_menu` receives the default Cut/Copy/… menu so callers can
  append (editor adds Add Selection to Agent) without rebuilding it.
- **Theme**: `theme::apply` always uses `ThemeMode::Dark` (it reloads the
  theme config) and then overrides fonts, sizes, `radius`/`radius_lg = 0`, and
  `highlight_theme` (Ayu Darker tokens + editor/gutter bg = `theme.sidebar` so
  it matches the file tree), `notification.placement = BottomRight`, and
  `notification.margins.bottom` above the status bar. Re-run it after any
  font-size change. `theme::text_view_style` applies that same highlight
  theme + editor background to Markdown `TextView`s (agent chat, preview).
  `theme.font_size` (14px) is the rem base for the whole UI;
  `mono_font_size` is the editor size from config.
- **Logo / icons** (`assets/logo/`): `logo.svg` is the master (1024, near-black
  tile with rounded corners, rx 229). `logo-mark.svg` is the shapes only (welcome screen,
  `branding::LOGO_MARK`). `icon-macos.svg` puts the tile on Apple's 824px grid
  with a transparent margin and the same rounded corners. Exports:
  `Isengard.icns`, `icon-1024.png` (macOS Dock, X11 window icon, cargo-bundle),
  `logo-512.png` (Linux hicolor via `scripts/install-dev-icon.sh`),
  `Isengard.ico` (Windows `.exe` via `build.rs` + `assets/windows/isengard.rc` +
  `embed-resource`). App id `branding::APP_ID` (`app.isengard.edtir`) is set
  on the window for Wayland `.desktop` matching. See **App icon matrix** above.
  Colors: tile `#0b0b0c`, mark `#ececec`, cursor `#f5a524`. The group is
  optically centred (halfway between bbox centre and area centroid). To
  re-export after editing an SVG: render PNGs with a transparent background
  using `resvg` (e.g. a tiny `resvg = "=0.45.1"` CLI; `qlmanage` fills
  transparency with white, don't use it), build the iconset sizes 16–512
  @1x/@2x, `iconutil -c icns`, and pack PNGs 16–256 into the .ico.
- **Fonts**: five JetBrains Mono Nerd Font weights are embedded with
  `include_bytes!` (~13 MB) and registered via `cx.text_system().add_fonts`.
  Family name: `"JetBrainsMono Nerd Font"`. Attribution and license paths:
  [credits.md](credits.md).

## Conventions

- Follow GPUI Kit's own agent guides (cloned docs: `skills/gpui-kit` in
  github.com/longbridge/gpui-component; online: https://gpui-kit.com/llms.txt,
  per-component `https://gpui-kit.com/component/<name>.md`). Key rules: never
  invent APIs — grep the crate source in `~/.cargo/registry/src/*/gpui-component-0.7.1`
  / `gpui-base-0.7.1` / `gpui-pre-0.3.8`; colors from `cx.theme()`, no raw
  hex/rgb; prefer rem-based helpers (`p_2()`, `gap_3()`) over `px(..)`.
- Repeated elements need domain-derived `ElementId`s (we use paths), not indexes.
- Side effects live on `IsengardApp`; UI modules take `Context<IsengardApp>` and
  call its `pub(crate)`/`pub` methods via `cx.listener`.
- Functions that build elements while `cx` is borrowed return `AnyElement`
  (edition 2024 `impl Trait` captures all lifetimes, which otherwise blocks
  later `cx` use).
- Errors: `anyhow` + `.with_context` in non-UI code; shown via
  `notify_error` (a `Notification::error`). Never `unwrap` on I/O.
- **Tests:** `use gpui_kit::*` brings in gpui's `test` macro, which shadows
  `#[test]`. In test modules of files that glob-import `gpui_kit`, import the
  needed items explicitly instead of `use super::*`.
- **UI tests:** `src/app/ui_tests.rs` drives the real `IsengardApp` headlessly
  (`#[gpui_kit::test]`, dev-dep `gpui-kit/test-support`): keystrokes via
  `window.input` / `press`, dialog buttons by id (`dialog-primary` /
  `dialog-secondary`), temp-dir fixtures. Under `cfg(test)`
  `AppConfig::default_path()` is `None`, so tests never touch the user config.
  Never block inside `background_spawn` (e.g. `flume::recv`): the test
  scheduler runs background tasks inline and hangs — use `recv_async`.
  `Harness::has` renders a frame first and only sees registered Kit controls
  (e.g. `Button` ids), not plain `div().id(..)`. The harness points
  `agent.command` at a missing path so tests never launch a real agent.
  **Required for every new/changed UI feature** in the same change — see
  [`.cursor/rules/ui-test-coverage.mdc`](../.cursor/rules/ui-test-coverage.mdc).
- Commits go on `master` (user's choice). Only commit when the user asks.
- Doc updates follow [README.md](README.md) (living topic file + one changelog
  bullet).

## Version pinning

`gpui-kit = "0.7.1"` pins an exact GPUI snapshot (`gpui-pre =0.3.8`). GPUI's API
changes between snapshots; upgrade `gpui-kit` as a whole and re-check every
API used here. Tree-sitter languages come from the `tree-sitter-languages`
feature (JSON is included via base `tree-sitter`). Detection lives in
`Language` in `editor/language.rs`. Direct dep `cc = "~1.2.1"` keeps
`tree-sitter-sequel` (SQL) resolvable against `embed-resource`. Terminal deps:
`alacritty_terminal = "0.25.1"`, `portable-pty = "0.9"` (plus `parking_lot`,
`flume`, `ropey` for the adapted view). Project search: `regex = "1"`,
`aho-corasick = "1"`, existing `ignore = "0.4"` (parallel `WalkBuilder`,
`require_git(false)`, `max_filesize`). Source Control shells out to system
`git` on PATH (no `git2`/`gix`). LSP: `lsp-types = "0.97"` (same as Kit) +
`url = "2"` for `file://` URIs; file watching: `notify = "8"`; servers are external (app-local download dir
and/or PATH — not inside the app binary). Download deps: `ureq`, `flate2`,
`tar`, `zip`, `xz2`; updates add `sha2` + `minisign-verify`. `AppConfig.lsp` holds the master switch and `languages`
map. Agent panel: `agent-client-protocol = "=3.4.0"` (feature `process`;
schema `=1.12.0` comes with it) + `futures = "0.3"`; the Claude adapter is
pinned in `acp/install.rs` (`CLAUDE_VERSION`). Upgrade both together — the
3.x API and the adapter change quickly, and docs online often show the
older 0.x/2.x API. Do not depend on crates.io `gpui-terminal` — it targets a different GPUI;
our panel is a local adaptation.
