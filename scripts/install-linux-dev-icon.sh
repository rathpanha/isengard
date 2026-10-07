#!/usr/bin/env bash
# Install a user-local .desktop + hicolor icon so Wayland compositors show the
# Isengard logo for `cargo run` (app_id = dev.isengard.editor).
# X11 also gets WindowOptions::icon from the binary; this script still helps
# alt-tab / launcher entries.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_ID="dev.isengard.editor"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
ICON_DIR="$DATA/icons/hicolor/512x512/apps"
APP_DIR="$DATA/applications"
BIN="$ROOT/target/debug/isengard"
ICON_SRC="$ROOT/assets/logo/logo-512.png"

if [[ ! -f "$ICON_SRC" ]]; then
  echo "missing $ICON_SRC" >&2
  exit 1
fi

mkdir -p "$ICON_DIR" "$APP_DIR"
cp "$ICON_SRC" "$ICON_DIR/$APP_ID.png"

cat >"$APP_DIR/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Isengard
Comment=The already configured editor
Exec=$BIN %F
Icon=$APP_ID
Terminal=false
Categories=Development;TextEditor;
StartupWMClass=$APP_ID
StartupNotify=true
EOF

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f "$DATA/icons/hicolor" 2>/dev/null || true
fi

echo "Installed $APP_DIR/$APP_ID.desktop"
echo "Icon: $ICON_DIR/$APP_ID.png"
echo "Re-run after moving the repo. Then: cargo run"
