#!/usr/bin/env bash
# Cross-platform entry for the one-time *dev* app-icon setup.
# macOS / Windows need no install (see messages). Linux Wayland needs a .desktop.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

case "$(uname -s)" in
Linux*)
  exec "$ROOT/scripts/install-linux-dev-icon.sh"
  ;;
Darwin*)
  echo "macOS: Dock icon is set at runtime (branding::set_dock_icon). Nothing to install."
  echo "Then: cargo run"
  ;;
MINGW*|MSYS*|CYGWIN*)
  echo "Windows: icon is embedded into the .exe by build.rs (assets/logo/Isengard.ico)."
  echo "Nothing to install. Then: cargo run"
  ;;
*)
  echo "Unknown OS '$(uname -s)'. See docs/architecture.md (Development setup)." >&2
  exit 1
  ;;
esac
