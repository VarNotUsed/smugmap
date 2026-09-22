# SQLite on S3 via smugmap

Query a SQLite database stored in S3 — no download, no code changes, just LD_PRELOAD.

> **Read-only.** `INSERT`/`UPDATE`/`DELETE` will fail with `attempt to write a readonly database`. sqlite3 automatically opens the file read-only when it sees smugmap reject the write-mode open.

## Quick start

```sh
# 1. Build smugmap
cargo build --release

# 2. Upload the demo database to S3
aws s3 cp examples/sqlite/demo.db s3://your-bucket/demo.db

# 3. Set credentials (any AWS SDK env style works)
export AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_REGION=eu-central-1

# 4. Query
SMUGMAP_CONFIG_JSON='[{"pattern":"*.db","url":"s3://your-bucket/demo.db"}]' \
  LD_PRELOAD=./target/release/libsmugmap.so \
  sqlite3 /tmp/demo.db "SELECT * FROM products ORDER BY price DESC"
```

## How it works

smugmap intercepts `open()`, `read()`, and `mmap()` at the libc level. When sqlite3
opens `/tmp/demo.db`, smugmap recognises the filename from the config and transparently
serves it from S3 via HTTP Range requests — fetching only the bytes sqlite3 actually reads.
