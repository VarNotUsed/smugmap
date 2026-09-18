#!/usr/bin/env bash
set -euo pipefail

# Demo: query a large SQLite database on S3 without downloading it.
# Usage: S3_BUCKET=my-bucket S3_KEY=analytics.db QUERY="SELECT count(*) FROM events" ./demo/sqlite.sh

: "${S3_BUCKET:?set S3_BUCKET}"
: "${S3_KEY:?set S3_KEY}"
: "${QUERY:=SELECT count(*) FROM sqlite_master}"

SMUGMAP_SO="${SMUGMAP_SO:-/usr/local/lib/smugmap.so}"

URL=$(aws s3 presign "s3://$S3_BUCKET/$S3_KEY" --expires-in 3600)
CONFIG=$(mktemp)
printf '[{"pattern":"*.db","url":"%s"}]\n' "$URL" > "$CONFIG"
chmod 600 "$CONFIG"

trap 'rm -f "$CONFIG"' EXIT

SMUGMAP_CONFIG="$CONFIG" LD_PRELOAD="$SMUGMAP_SO" \
  sqlite3 "/remote/$S3_KEY" "$QUERY"
