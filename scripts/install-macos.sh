#!/usr/bin/env bash
# Install (or update) Isengard on macOS from the latest GitHub release:
#   curl -fsSL https://raw.githubusercontent.com/rathpanha/isengard/master/scripts/install-macos.sh | bash
#
# Files fetched by curl carry no quarantine flag, so Gatekeeper never blocks
# the app (it is not Apple-notarized). The download is checked against the
# release's SHA256SUMS, and its minisign signature when `minisign` is installed.
#
# Env: VERSION (default: latest release), INSTALL_DIR (default: /Applications,
#      or ~/Applications when that is not writable).
set -euo pipefail

REPO="rathpanha/isengard"
# Must match PUBLIC_KEY in src/update.rs.
MINISIGN_PUBKEY="RWQbB4mA17dCElWr4j6a897OzAzdEm5LyFvTfGCEd+2EKyoin6HtzQZH"

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "this installer is for macOS"

if [[ -n "${VERSION:-}" ]]; then
  TAG="v${VERSION#v}"
else
  TAG="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
  [[ -n "$TAG" ]] || die "could not find the latest release"
fi
ASSET="isengard-${TAG#v}-macos-universal.tar.gz"
BASE="https://github.com/$REPO/releases/download/$TAG"

INSTALL_DIR="${INSTALL_DIR:-/Applications}"
if [[ ! -w "$INSTALL_DIR" ]]; then
  INSTALL_DIR="$HOME/Applications"
  mkdir -p "$INSTALL_DIR"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading Isengard ${TAG#v}…"
curl -fL --progress-bar -o "$TMP/$ASSET" "$BASE/$ASSET" \
  || die "no macOS build in release $TAG"
curl -fsSL -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS" || die "release has no SHA256SUMS"

if command -v minisign >/dev/null 2>&1; then
  curl -fsSL -o "$TMP/SHA256SUMS.minisig" "$BASE/SHA256SUMS.minisig" \
    || die "release has no SHA256SUMS.minisig"
  minisign -Vq -P "$MINISIGN_PUBKEY" -m "$TMP/SHA256SUMS" -x "$TMP/SHA256SUMS.minisig" \
    || die "SHA256SUMS signature does not verify"
else
  echo "note: minisign not installed; skipping signature check (brew install minisign)"
fi

EXPECTED="$(awk -v f="./$ASSET" '$2 == f || $2 == "*" f { print $1 }' "$TMP/SHA256SUMS")"
[[ -n "$EXPECTED" ]] || die "$ASSET is not listed in SHA256SUMS"
ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | awk '{ print $1 }')"
[[ "$ACTUAL" == "$EXPECTED" ]] || die "checksum mismatch for $ASSET"

tar -xzf "$TMP/$ASSET" -C "$TMP"
[[ -d "$TMP/Isengard.app" ]] || die "archive has no Isengard.app"
# No-op for curl downloads; protects against odd setups.
xattr -dr com.apple.quarantine "$TMP/Isengard.app" 2>/dev/null || true

# Stage beside the destination, then swap, so a failed copy never loses the app.
rm -rf "$INSTALL_DIR/.Isengard.app.new"
cp -R "$TMP/Isengard.app" "$INSTALL_DIR/.Isengard.app.new"
rm -rf "$INSTALL_DIR/Isengard.app"
mv "$INSTALL_DIR/.Isengard.app.new" "$INSTALL_DIR/Isengard.app"

echo "Installed $INSTALL_DIR/Isengard.app"
