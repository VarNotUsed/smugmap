#!/usr/bin/env bash
set -euo pipefail

# Demo: run llama.cpp inference on a model hosted on S3 without downloading weights.
# Usage: S3_BUCKET=my-models MODEL_KEY=llama-3-8b.gguf PROMPT="Hello" ./demo/llama.sh

: "${S3_BUCKET:?set S3_BUCKET}"
: "${MODEL_KEY:?set MODEL_KEY}"
: "${PROMPT:=Hello}"
: "${N_TOKENS:=128}"

SMUGMAP_SO="${SMUGMAP_SO:-/usr/local/lib/smugmap.so}"

URL=$(aws s3 presign "s3://$S3_BUCKET/$MODEL_KEY" --expires-in 3600)
CONFIG=$(mktemp)
printf '[{"pattern":"*.gguf","url":"%s","readahead":4}]\n' "$URL" > "$CONFIG"
chmod 600 "$CONFIG"

trap 'rm -f "$CONFIG"' EXIT

SMUGMAP_CONFIG="$CONFIG" LD_PRELOAD="$SMUGMAP_SO" \
  llama-cli -m "/remote/$MODEL_KEY" -p "$PROMPT" -n "$N_TOKENS"
