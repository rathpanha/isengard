#!/usr/bin/env bash
# Build Isengard.app (+ .dmg and .tar.gz) for macOS. No Apple Developer ID:
# the app is signed with MACOS_SIGN_IDENTITY (a self-signed certificate, see
# scripts/create-macos-cert.sh) or, when unset, ad-hoc (`-`).
#
# Env:
#   VERSION             default: Cargo.toml version
#   TARGETS             default: "aarch64-apple-darwin x86_64-apple-darwin"
#                       (two targets -> universal binary; one -> single arch)
#   MACOS_SIGN_IDENTITY codesign identity name or SHA-1 (default "-" = ad-hoc)
#   MACOS_KEYCHAIN      keychain that holds the identity (default: search list)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
TARGETS="${TARGETS:-aarch64-apple-darwin x86_64-apple-darwin}"
IDENTITY="${MACOS_SIGN_IDENTITY:--}"

DIST="$ROOT/dist/macos"
rm -rf "$DIST"
mkdir -p "$DIST"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

BINS=()
for target in $TARGETS; do
  cargo build --release --target "$target"
  BINS+=("$ROOT/target/$target/release/isengard")
done

if [[ ${#BINS[@]} -gt 1 ]]; then
  ARCH="universal"
  lipo -create -output "$TMP/isengard" "${BINS[@]}"
else
  # Not a release artifact: the updater only looks for "-macos-universal".
  ARCH="${TARGETS%%-apple-darwin}"
  cp "${BINS[0]}" "$TMP/isengard"
fi

# The app must run on Macs with nothing from Homebrew installed: only system
# libraries (/usr/lib, /System) may be linked dynamically.
if otool -L "$TMP/isengard" | grep -E '^\s' | grep -vE '^\s(/usr/lib/|/System/)'; then
  echo "error: binary links non-system libraries (listed above)" >&2
  exit 1
fi

APP="$TMP/stage/Isengard.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TMP/isengard" "$APP/Contents/MacOS/isengard"
chmod 755 "$APP/Contents/MacOS/isengard"
cp "$ROOT/assets/logo/Isengard.icns" "$APP/Contents/Resources/Isengard.icns"
cp "$ROOT/LICENSE" "$ROOT/docs/credits.md" "$APP/Contents/Resources/"
sed -e "s/\${VERSION}/$VERSION/g" "$ROOT/packaging/macos/Info.plist.in" \
  >"$APP/Contents/Info.plist"

# A signature is mandatory on Apple Silicon. A *stable* identity keeps the
# Keychain's "Always Allow" valid across updates; ad-hoc changes every build.
SIGN_ARGS=(--force --deep --sign "$IDENTITY")
[[ -n "${MACOS_KEYCHAIN:-}" ]] && SIGN_ARGS+=(--keychain "$MACOS_KEYCHAIN")
[[ "$IDENTITY" != "-" ]] && SIGN_ARGS+=(--timestamp=none)
codesign "${SIGN_ARGS[@]}" "$APP"
codesign --verify --deep --strict "$APP"
codesign -dv "$APP" 2>&1 | sed -n 's/^\(Identifier\|Signature\|Authority\|TeamIdentifier\)/  \1/p'
echo "archs: $(lipo -archs "$APP/Contents/MacOS/isengard")"

NAME="isengard-${VERSION}-macos-${ARCH}"

# Tarball: what the in-app updater and scripts/install-macos.sh download.
# COPYFILE_DISABLE keeps AppleDouble ("._*") files out of the archive.
COPYFILE_DISABLE=1 tar -C "$TMP/stage" -czf "$DIST/$NAME.tar.gz" Isengard.app

# DMG: drag-to-Applications image for people who download from a browser.
# No `-quiet`: GHA runners sometimes fail hdiutil with no message (and leave a
# `diskimages-helper` orphan). Retry a few times — same flake hit v0.2.0 CI.
mkdir -p "$TMP/dmg"
cp -R "$APP" "$TMP/dmg/Isengard.app"
ln -s /Applications "$TMP/dmg/Applications"
dmg_ok=0
for attempt in 1 2 3; do
  echo "hdiutil create (attempt $attempt)…"
  if hdiutil create -volname "Isengard" -srcfolder "$TMP/dmg" -ov -format UDZO \
    "$DIST/$NAME.dmg"; then
    dmg_ok=1
    break
  fi
  echo "hdiutil failed (attempt $attempt); sleeping before retry…" >&2
  rm -f "$DIST/$NAME.dmg"
  sleep $((attempt * 5))
done
if [[ "$dmg_ok" -ne 1 ]]; then
  echo "error: hdiutil create failed after retries" >&2
  exit 1
fi

(
  cd "$DIST"
  shasum -a 256 ./* >SHA256SUMS
)

echo "Artifacts in $DIST:"
ls -la "$DIST"
