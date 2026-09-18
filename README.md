# smugmap

> Demand-page S3 files into any unmodified binary. Works where FUSE can't.

LD_PRELOAD shim that intercepts `open()` and `mmap()`, serving pages from S3 on demand via userfaultfd. Only the bytes your program actually reads are ever fetched.

**Works in:** AWS Lambda, unprivileged Docker, Kubernetes without host-path mounts, CI runners.

## Quick Start

```bash
# Generate a presigned URL (valid for 1 hour)
URL=$(aws s3 presign s3://my-bucket/analytics.db --expires-in 3600)

# Write config
cat > /tmp/config.json <<EOF
[{"pattern":"*.db","url":"$URL"}]
EOF

# Query a 50 GB database on S3 — fetches only the pages it needs
SMUGMAP_CONFIG=/tmp/config.json LD_PRELOAD=/usr/local/lib/smugmap.so \
  sqlite3 /remote/analytics.db "SELECT count(*) FROM events"
```

## Installation

Download a pre-built `.so` from [Releases](../../releases), or build from source:

```bash
make build
sudo make install
```

Requires Linux (userfaultfd is a Linux kernel feature).

## Configuration

`SMUGMAP_CONFIG` points to a JSON file mapping glob patterns to presigned URLs:

```json
[
  { "pattern": "*.db",   "url": "https://..." },
  { "pattern": "*.gguf", "url": "https://...", "readahead": 4 }
]
```

| Field | Required | Description |
|-------|----------|-------------|
| `pattern` | yes | Glob matched against the opened filename |
| `url` | yes | Presigned URL or any HTTP URL supporting Range requests |
| `readahead` | no | Pages to prefetch after each fault (default: 0) |

**Security:** Config files contain presigned URLs — set permissions with `chmod 600`.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `SMUGMAP_CONFIG` | Path to JSON config file |
| `SMUGMAP_QUIET` | Set to `1` to suppress stderr logging |

## Use Cases

**LLM inference on Lambda** — run llama.cpp without downloading model weights to `/tmp`:
```bash
SMUGMAP_CONFIG=/tmp/config.json LD_PRELOAD=smugmap.so llama-cli -m /remote/model.gguf -p "Hello" -n 128
```

**SQLite on S3** — query large databases, fetch only the pages your query touches:
```bash
SMUGMAP_CONFIG=/tmp/config.json LD_PRELOAD=smugmap.so sqlite3 /remote/analytics.db "SELECT ..."
```

**Geospatial** — GDAL on cloud-hosted GeoTIFFs, tile-by-tile:
```bash
SMUGMAP_CONFIG=/tmp/config.json LD_PRELOAD=smugmap.so gdal_translate -srcwin 0 0 256 256 /remote/world.tif out.png
```

## How It Works

1. `open()` — matched path → HEAD request for file size → returns magic fd
2. `mmap()` — allocates anonymous memory, registers with userfaultfd
3. Page fault → background thread fetches 4 KB Range from URL → fills page
4. `pread()` — direct Range fetch (fallback for binaries that don't use mmap)

## Why Not FUSE?

FUSE-based solutions (s3fs, Mountpoint-S3, goofys) require `/dev/fuse`, which is unavailable in Lambda, unprivileged containers, and most CI environments. smugmap uses only `userfaultfd` (available as unprivileged since Linux 5.7) and LD_PRELOAD — no kernel modules, no special privileges.

## Limitations

- Linux only (userfaultfd is a Linux kernel feature)
- Read-only (writes pass through to the real filesystem)
- Requires `unprivileged_userfaultfd=1` (default on modern kernels and Lambda ARM64)

## Roadmap

- **v2:** SigV4 auth from `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` — no presigning needed

## License

MIT
