#!/usr/bin/env bash
# Generate a deterministic-ish Ed25519 keypair for the e2e curl pipeline.
#
# Prints two files relative to the working directory:
#   - $OUT_DIR/ed25519_private.pem   PKCS#8 PEM-encoded private key
#   - $OUT_DIR/ed25519_public.bin    32 raw bytes of the public key
#   - $OUT_DIR/ed25519_public.b64    Base64 (standard, no newline) of the 32 bytes
#
# Used by `e2e-p0-curl.sh` to register a signing key with the project
# (Contract C4) and then mint a JWT signed by the private key (Contract C2).
#
# Usage:
#   ./scripts/gen-ed25519.sh /tmp/feedbackmonk-e2e-keys
#
# Requires: openssl 3+ (Ed25519 support).

set -euo pipefail

OUT_DIR="${1:-./.e2e-keys}"
mkdir -p "$OUT_DIR"

PRIV_PEM="$OUT_DIR/ed25519_private.pem"
PUB_PEM="$OUT_DIR/ed25519_public.pem"
PUB_BIN="$OUT_DIR/ed25519_public.bin"
PUB_B64="$OUT_DIR/ed25519_public.b64"

openssl genpkey -algorithm ED25519 -out "$PRIV_PEM" 2>/dev/null
openssl pkey -in "$PRIV_PEM" -pubout -out "$PUB_PEM" 2>/dev/null

# Extract the 32 raw public-key bytes by stripping the PKCS#8 DER prefix.
# Ed25519 SubjectPublicKeyInfo is a 12-byte prefix + 32-byte key:
#   30 2A 30 05 06 03 2B 65 70 03 21 00 || <32 bytes>
openssl pkey -in "$PRIV_PEM" -pubout -outform DER 2>/dev/null | tail -c 32 > "$PUB_BIN"

# Base64 (standard, no line wrapping) for the registration request body.
# openssl base64 wraps at 64 cols by default; tr strips newlines.
base64 -w 0 < "$PUB_BIN" > "$PUB_B64" 2>/dev/null \
  || openssl base64 -in "$PUB_BIN" | tr -d '\n' > "$PUB_B64"

echo "OK: ed25519 keypair in $OUT_DIR"
echo "  private (PEM): $PRIV_PEM"
echo "  public  (PEM): $PUB_PEM"
echo "  public  (bin, 32 raw bytes): $PUB_BIN"
echo "  public  (base64, no padding-strip): $PUB_B64"
