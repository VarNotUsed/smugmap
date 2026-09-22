# DuckDB on S3 via smugmap

Run analytical queries on a Parquet file in S3 — DuckDB reads only the column
chunks it needs, smugmap fetches only those bytes.

## Quick start

```sh
# 1. Build smugmap
cargo build --release

# 2. Upload the demo dataset to S3
aws s3 cp examples/duckdb/demo.parquet s3://your-bucket/demo.parquet

# 3. Set your bucket in config.json
#    [{"pattern":"*.parquet","url":"s3://your-bucket/demo.parquet"}]

# 4. Set credentials
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=eu-central-1   # optional, default: us-east-1

# 5. Query
SMUGMAP_CONFIG=examples/duckdb/config.json \
  LD_PRELOAD=./target/release/libsmugmap.so \
  duckdb -c "SELECT dept, avg(salary) FROM '/tmp/demo.parquet' GROUP BY dept ORDER BY avg(salary) DESC"
```

## How it works

smugmap intercepts `open()`, `read()`, and `mmap()` at the libc level. When DuckDB
opens `/tmp/demo.parquet`, smugmap recognises the filename from the config and
transparently serves it from S3 via HTTP Range requests — fetching only the bytes
DuckDB actually reads. On a wide Parquet file this means only the queried columns
are transferred.
