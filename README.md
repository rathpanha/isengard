# Isengard

**The already configured editor.**

Cross-platform desktop code editor built in Rust on [GPUI Kit](https://gpui-kit.com).
Ships for **Linux** (`.deb`, `.rpm`, and a binary tarball), **macOS** (universal
`.dmg` / tarball, not notarized) and **Windows** (x86_64 zip, not code-signed).

## Why Isengard

- **Already configured** — format on save, per-file indent detection, dark Ayu
  theme, Material file icons. Open a folder and work; edit `config.json` only
  when you want to change a default.
- **Workspaces that remember** — multi-root folders; tabs, sidebar, terminal
  layout, and agent chats restore per workspace.
- **Claude Agent built in** — Agent panel over ACP. Install from the panel,
  sign in with your Claude Code login (no API key stored in Isengard). `@`
  files, permissions, plan usage, and tool runs live in the UI.
- **Model controls that matter** — pick the chat model, a separate utility
  model for quiet jobs (titles, Improve, Git Suggest), and a Fast / Opus Plan
  switch without digging through Claude’s own settings.
- **Tools stay in the app** — Bun, language servers, and the agent adapter
  install under Isengard’s data folder — not `npm -g` / brew into your
  toolchain. Bun also formats JS/TS/JSON/CSS/HTML/YAML/Markdown on save.
- **Linux, macOS, and Windows** — Check for Updates… where the install allows.

What works and what is still rough: [docs/status.md](docs/status.md).

## Install (Linux)

Grab the asset for your CPU from the
[GitHub Releases](https://github.com/rathpanha/isengard/releases) page.

| Distro family | Package |
| --- | --- |
| Debian / Ubuntu / Mint / … | `isengard_*_amd64.deb` (or `_arm64`) |
| Fedora / RHEL / openSUSE / … | `isengard-*.x86_64.rpm` (or `*.aarch64.rpm`) |
| Any (Arch, Void, manual) | `isengard-*-linux-*.tar.gz` |

```sh
# Debian/Ubuntu
sudo apt install ./isengard_*_amd64.deb

# Fedora
sudo dnf install ./isengard-*.x86_64.rpm

# Tarball
tar -xzf isengard-*-linux-*.tar.gz
sudo install -m755 isengard-*/bin/isengard /usr/local/bin/isengard
# …copy .desktop + icon from share/ (see README.txt inside the archive)
```

App id / Wayland window class: `app.isengard.edtir` (icons and `.desktop` are
included in the packages).

Verify downloads with `SHA256SUMS` on the release.

## Install (macOS)

Isengard is not notarized by Apple (that needs a paid developer account), so
macOS treats a browser download as untrusted. Pick one:

**Terminal (no warnings):**

```sh
curl -fsSL https://raw.githubusercontent.com/rathpanha/isengard/master/scripts/install-macos.sh | bash
```

Files downloaded by `curl` are not quarantined, so Isengard opens normally.

**Disk image:** download `isengard-*-macos-universal.dmg`, drag Isengard to
Applications, then open **System Settings → Privacy & Security** and click
**Open Anyway** (or run `xattr -dr com.apple.quarantine /Applications/Isengard.app`).

Once installed, **Check for Updates…** updates Isengard in place with no
prompts. The first time the Claude Agent reads your Claude Code login, macOS
asks for Keychain access once — choose **Always Allow**.

## Install (Windows)

Isengard is not code-signed (that needs a paid certificate), so a zip downloaded
in a browser shows SmartScreen's "Windows protected your PC" (**More info → Run
anyway**). To avoid it, install from PowerShell:

```powershell
irm https://raw.githubusercontent.com/rathpanha/isengard/master/scripts/install-windows.ps1 | iex
```

It installs to `%LOCALAPPDATA%\Programs\Isengard` (no administrator rights),
adds a Start Menu shortcut, and puts the folder on your `PATH`. **Check for
Updates…** then updates Isengard in place. Only 64-bit Intel/AMD Windows is
built.

### After install

- **Bun** may need a one-time Install from the first-run dialog (or use a Bun
  already on `PATH` / `~/.bun/bin`).
- **Language servers** are not inside the package. Use the status-bar LSP menu
  → **Install** (downloads under the app data dir — e.g. Linux
  `~/.local/share/isengard/lsp/`, macOS
  `~/Library/Application Support/isengard/lsp/` — never into your system
  toolchain).
- **Claude Agent** is optional: Install / Login from the Agent panel. No API
  key is stored in Isengard; you sign in with the agent’s own flow.

## Build from source

See [docs/architecture.md](docs/architecture.md) (Rust via rustup, Linux
system packages, `cargo run` / `cargo build --release`).

Package on Linux after a release build:

```sh
cargo build --release
./scripts/package-linux.sh   # needs `nfpm` on PATH → writes dist/linux/
```

Package on macOS (`.app`, `.dmg`, `.tar.gz` → `dist/macos/`):

```sh
./scripts/package-macos.sh   # universal; TARGETS=aarch64-apple-darwin for one arch
```

## License

**GPL-3.0-only** — [LICENSE](LICENSE). Third-party credits:
[docs/credits.md](docs/credits.md).

Forks and private copies are fine. **Pull requests are not accepted** —
[CONTRIBUTING.md](CONTRIBUTING.md).

## Report a bug

Found a problem? [Open an issue](https://github.com/rathpanha/isengard/issues/new/choose)
and fill in the template. Isengard is maintained best-effort, so an issue
**may not get picked up or fixed**.

## Architecture

Rust desktop app on **GPUI Kit** (Zed’s GPUI). The binary is the editor shell;
**Bun**, language servers, and the Claude Agent adapter install on demand into
the app data folder and run as separate processes (LSP / ACP / PTY). No Tokio
— threads + `flume`. Full stack, process map, on-disk layout, and `src/` map:
[docs/architecture.md](docs/architecture.md).

## Docs

| Doc | What |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | Stack, process map, bootstrap, `src/` map |
| [docs/status.md](docs/status.md) | What works / deferred |
| [docs/design.md](docs/design.md) | UI rules |
| [docs/credits.md](docs/credits.md) | Third-party recognition |
| [docs/changelog.md](docs/changelog.md) | History |
