#!/usr/bin/env bash
# Build .deb + .rpm (+ binary tarball) for the current Linux host.
# Requires: cargo release binary at target/release/isengard, and `nfpm` on PATH
#   (https://nfpm.goreleaser.com/ — or run via the GitHub Actions workflow).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
  x86_64|amd64) ARCH="amd64"; RPM_ARCH="x86_64"; TARBALL_ARCH="x86_64" ;;
  aarch64|arm64) ARCH="arm64"; RPM_ARCH="aarch64"; TARBALL_ARCH="aarch64" ;;
  *)
    echo "unsupported arch: $ARCH_RAW" >&2
    exit 1
    ;;
esac

BIN="$ROOT/target/release/isengard"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN — run: cargo build --release" >&2
  exit 1
fi

if ! command -v nfpm >/dev/null 2>&1; then
  echo "nfpm not found. Install: https://nfpm.goreleaser.com/install/" >&2
  exit 1
fi

DIST="$ROOT/dist/linux"
rm -rf "$DIST"
mkdir -p "$DIST"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
# nfpm wants literal arch values; rpm uses different arch names than deb.
sed -e "s/\${VERSION}/$VERSION/g" -e "s/\${ARCH}/$ARCH/g" \
  "$ROOT/packaging/nfpm.yaml" >"$TMP/nfpm.deb.yaml"
sed -e "s/\${VERSION}/$VERSION/g" -e "s/\${ARCH}/$RPM_ARCH/g" \
  "$ROOT/packaging/nfpm.yaml" >"$TMP/nfpm.rpm.yaml"

chmod +x "$ROOT/packaging/linux/postinstall.sh"

nfpm package -f "$TMP/nfpm.deb.yaml" -p deb -t "$DIST"
nfpm package -f "$TMP/nfpm.rpm.yaml" -p rpm -t "$DIST"

STAGE="$TMP/isengard-${VERSION}-linux-${TARBALL_ARCH}"
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/icons/hicolor/512x512/apps" "$STAGE/share/doc/isengard"
cp "$BIN" "$STAGE/bin/isengard"
cp "$ROOT/packaging/linux/app.isengard.edtir.desktop" "$STAGE/share/applications/"
cp "$ROOT/assets/logo/logo-512.png" "$STAGE/share/icons/hicolor/512x512/apps/app.isengard.edtir.png"
cp "$ROOT/LICENSE" "$ROOT/docs/credits.md" "$STAGE/share/doc/isengard/"
cat >"$STAGE/README.txt" <<EOF
Isengard ${VERSION} (Linux ${TARBALL_ARCH})

Install the binary somewhere on PATH, e.g.:
  sudo install -m755 bin/isengard /usr/local/bin/isengard
  sudo cp share/applications/app.isengard.edtir.desktop /usr/local/share/applications/
  sudo cp share/icons/hicolor/512x512/apps/app.isengard.edtir.png \\
    /usr/local/share/icons/hicolor/512x512/apps/

App id / Wayland: app.isengard.edtir
License: GPL-3.0-only (see share/doc/isengard/)
EOF
tar -C "$TMP" -czf "$DIST/isengard-${VERSION}-linux-${TARBALL_ARCH}.tar.gz" \
  "isengard-${VERSION}-linux-${TARBALL_ARCH}"

(
  cd "$DIST"
  shasum -a 256 ./* >SHA256SUMS
)

echo "Artifacts in $DIST:"
ls -la "$DIST"
