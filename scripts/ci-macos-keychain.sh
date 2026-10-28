#!/usr/bin/env bash
# CI only: import the self-signed release certificate into a throwaway
# keychain and make codesign accept it. Prints the identity name for
# MACOS_SIGN_IDENTITY and the keychain path for MACOS_KEYCHAIN on stdout as
# `KEY=value` lines (append them to $GITHUB_ENV).
#
# Env: MACOS_CERT_P12 (base64 .p12), MACOS_CERT_PASSWORD, RUNNER_TEMP.
# Create the certificate once with scripts/create-macos-cert.sh.
set -euo pipefail

NAME="${MACOS_SIGN_IDENTITY:-Isengard Self-Signed}"
DIR="${RUNNER_TEMP:-$(mktemp -d)}"
KC="$DIR/isengard-signing.keychain-db"
KC_PASSWORD="$(/usr/bin/openssl rand -hex 16)"

printf '%s' "$MACOS_CERT_P12" | base64 --decode >"$DIR/cert.p12"
security create-keychain -p "$KC_PASSWORD" "$KC"
security set-keychain-settings -lut 3600 "$KC"
security unlock-keychain -p "$KC_PASSWORD" "$KC"
security import "$DIR/cert.p12" -k "$KC" -P "$MACOS_CERT_PASSWORD" -T /usr/bin/codesign >&2
security set-key-partition-list -S apple-tool:,apple: -s -k "$KC_PASSWORD" "$KC" >&2
security list-keychains -d user -s "$KC" login.keychain >&2

# A self-signed certificate is "not trusted" until told otherwise, and codesign
# ignores untrusted identities ("no identity found").
/usr/bin/openssl pkcs12 -in "$DIR/cert.p12" -clcerts -nokeys \
  -passin "pass:$MACOS_CERT_PASSWORD" -out "$DIR/cert.pem" >&2
sudo security add-trusted-cert -d -r trustRoot -p codeSign -k "$KC" "$DIR/cert.pem" >&2
rm -f "$DIR/cert.p12"

security find-identity -v -p codesigning "$KC" >&2
echo "MACOS_SIGN_IDENTITY=$NAME"
echo "MACOS_KEYCHAIN=$KC"
