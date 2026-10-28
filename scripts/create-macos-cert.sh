#!/usr/bin/env bash
# One-time: create the self-signed code-signing certificate used for release
# builds, free of Apple's $99 program. Signing every release with the SAME
# certificate keeps the macOS Keychain's "Always Allow" for the Claude Code
# login valid across updates (an ad-hoc signature changes every build).
#
# Usage: scripts/create-macos-cert.sh [output dir]   (default ~/.isengard-signing)
# Then store the printed values as GitHub secrets MACOS_CERT_P12 and
# MACOS_CERT_PASSWORD. BACK UP the .p12: a new certificate means every user is
# asked for Keychain access once again.
set -euo pipefail

NAME="Isengard Self-Signed"
OUT="${1:-$HOME/.isengard-signing}"
# macOS's own LibreSSL writes a .p12 that `security import` accepts.
OPENSSL=/usr/bin/openssl

if [[ -e "$OUT/isengard.p12" ]]; then
  echo "$OUT/isengard.p12 already exists; refusing to replace the signing identity." >&2
  exit 1
fi
mkdir -p "$OUT"
chmod 700 "$OUT"

PASSWORD="$("$OPENSSL" rand -hex 16)"
cat >"$OUT/openssl.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$OUT/openssl.cnf" \
  -keyout "$OUT/key.pem" -out "$OUT/cert.pem"
"$OPENSSL" pkcs12 -export -inkey "$OUT/key.pem" -in "$OUT/cert.pem" \
  -name "$NAME" -passout "pass:$PASSWORD" -out "$OUT/isengard.p12"
rm -f "$OUT/key.pem" "$OUT/openssl.cnf"
chmod 600 "$OUT/isengard.p12"
echo "$PASSWORD" >"$OUT/password.txt"
chmod 600 "$OUT/password.txt"

echo
echo "Created $OUT/isengard.p12 (valid 10 years)."
echo "GitHub secrets to add (repo Settings > Secrets and variables > Actions):"
echo "  MACOS_CERT_PASSWORD = $PASSWORD"
echo "  MACOS_CERT_P12      = contents of: base64 -i $OUT/isengard.p12 | pbcopy"
echo "  (optional) MACOS_SIGN_IDENTITY = $NAME   [defaults to this name]"
