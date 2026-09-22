# SQLite on S3 via smugmap

Query a SQLite database stored in S3 — no download, no code changes, just LD_PRELOAD.

## Quick start

```sh
# 1. Build smugmap
cargo build --release

# 2. Upload the demo database to S3
aws s3 cp examples/sqlite/demo.db s3://your-bucket/demo.db

# 3. Set your bucket in config.json
#    [{"pattern":"*.db","url":"s3://your-bucket/demo.db"}]

# 4. Set credentials
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=eu-central-1   # optional, default: us-east-1

# 5. Query
SMUGMAP_CONFIG=examples/sqlite/config.json \
  LD_PRELOAD=./target/release/libsmugmap.so \
  sqlite3 /tmp/demo.db "SELECT * FROM products ORDER BY price DESC"
```

## How it works

smugmap intercepts `open()`, `read()`, and `mmap()` at the libc level. When sqlite3
opens `/tmp/demo.db`, smugmap recognises the filename from the config and transparently
serves it from S3 via HTTP Range requests — fetching only the bytes sqlite3 actually reads.
