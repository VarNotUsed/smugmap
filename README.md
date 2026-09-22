<h1 align="center">smugmap</h1>

<p align="center">
  <b>Demand-page S3 files into any unmodified binary — works where FUSE can't.</b>
</p>

<p align="center">
  <a href="https://github.com/VarNotUsed/smugmap/actions/workflows/ci.yml"><img src="https://github.com/VarNotUsed/smugmap/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/smugmap"><img src="https://img.shields.io/crates/v/smugmap.svg" alt="Crates.io"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/platform-linux-lightgrey.svg" alt="Platform: Linux">
</p>

**smugmap** is a tiny `LD_PRELOAD` shim that lets any unmodified Linux binary read files directly from S3 — as if they were local. Only the pages the program actually touches are ever fetched.

It intercepts `open()`, `mmap()`, `pread()` and friends, hands back a synthetic fd backed by anonymous memory, and uses Linux `userfaultfd` to fault pages in from S3 on demand.

No FUSE. No kernel module. No root.

## ✨ Features

- 🚀 **Zero code changes** — works with any existing binary via `LD_PRELOAD`
- 📦 **Demand-paged from S3** — only the bytes you read are ever transferred
- 🔐 **SigV4 out of the box** — reads standard AWS environment variables
- 🪶 **Tiny** — a single `.so`, ~200 KB, no runtime dependencies
- ☁️ **Runs where FUSE can't** — AWS Lambda, rootless containers, CI runners, Kubernetes without host-path mounts
- ⚡ **Fast enough for interactive workloads** — p50 Range GET ~24 ms cold, ~3 ms warm

## 🚀 Quick Start

```bash
# 1. Configure AWS credentials (any standard method works)
export AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_REGION=eu-central-1

# 2. Point smugmap at your S3 object
cat > /tmp/smugmap.json <<'EOF'
[{"pattern":"*.db","url":"s3://my-bucket/analytics.db"}]
EOF

# 3. Run any binary — smugmap intercepts the reads
SMUGMAP_CONFIG=/tmp/smugmap.json \
LD_PRELOAD=/usr/local/lib/smugmap.so \
  sqlite3 /remote/analytics.db "SELECT count(*) FROM events"
```

That query hits a handful of 4 KB pages out of a 50 GB database. The rest stays in S3.

## 📦 Installation

**Download** a prebuilt `.so` from the [Releases](../../releases) page (x86_64 and aarch64 Linux builds).

**Or build from source:**

```bash
make build
sudo make install
```

**Requirements:**
- Linux with `vm.unprivileged_userfaultfd=1` (default on modern kernels and AWS Lambda ARM64)
- Rust 1.70+ (build only)

## ⚙️ Configuration

`SMUGMAP_CONFIG` points to a JSON file mapping filename globs to URLs:

```json
[
  { "pattern": "*.db",   "url": "s3://my-bucket/analytics.db" },
  { "pattern": "*.gguf", "url": "https://...presigned...",     "readahead": 4 }
]
```

### Config fields

| Field       | Required | Description                                                                 |
|-------------|----------|-----------------------------------------------------------------------------|
| `pattern`   | ✅       | Glob matched against the opened filename                                    |
| `url`       | ✅       | `s3://bucket/key` (SigV4 from env) or any HTTPS URL that supports `Range`   |
| `readahead` | ❌       | Extra pages to prefetch on each fault (default: `0`)                        |

### Environment variables

| Variable                                                        | Purpose                                    |
|-----------------------------------------------------------------|--------------------------------------------|
| `SMUGMAP_CONFIG`                                                | Path to the JSON config file               |
| `SMUGMAP_QUIET`                                                 | Set to `1` to suppress `[smugmap]` logs    |
| `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_REGION`    | Used to sign `s3://` URLs                  |
| `AWS_SESSION_TOKEN`                                             | Added to SigV4 signing when present        |

### URL modes

- **`s3://…`** — the recommended path. smugmap signs Range requests with SigV4 using the standard AWS environment variables.
- **`https://…`** — plain Range GETs. Perfect for presigned URLs when no credentials are available at runtime:
  ```bash
  URL=$(aws s3 presign s3://my-bucket/analytics.db --expires-in 3600)
  ```

> ⚠️ Config files may contain presigned URLs. Protect them with `chmod 600`.

## 💡 Use Cases

The sweet spot is a **large file** with **sparse reads** and a binary you can't (or don't want to) modify.

### LLM inference on Lambda

Run `llama.cpp` without downloading model weights to `/tmp`:

```bash
SMUGMAP_CONFIG=/tmp/config.json LD_PRELOAD=smugmap.so \
  llama-cli -m /remote/model.gguf -p "Hello" -n 128
```

A 512-token query on a 7B GGUF touches ~2.5 GB out of 4.5 GB. The rest never gets pulled.

### SQLite / DuckDB on S3

Query multi-gigabyte databases with zero local disk:

```bash
SMUGMAP_CONFIG=/tmp/config.json LD_PRELOAD=smugmap.so \
  sqlite3 /remote/analytics.db "SELECT ..."
```

A point query on a 50 GB database reads a handful of B-tree pages — that's the download.

### Geospatial

GDAL reading one tile out of a 200 GB GeoTIFF pulls exactly one tile:

```bash
SMUGMAP_CONFIG=/tmp/config.json LD_PRELOAD=smugmap.so \
  gdal_translate -srcwin 0 0 256 256 /remote/world.tif out.png
```

> 📂 Runnable examples for each of these live in [`examples/`](examples/).

## 📊 Performance

Numbers from the spike this crate grew out of:

| Metric                          | Value            | Notes                                       |
|---------------------------------|------------------|---------------------------------------------|
| S3 Range GET latency (p50 cold) | ~24 ms           | Fresh connection to S3                      |
| S3 Range GET latency (p50 warm) | ~3 ms            | Same keep-alive connection                  |
| 7B GGUF query (512 tokens)      | ~2.5 GB fetched  | Out of 4.5 GB on disk                       |
| Fault size                      | 4 KB             | Configurable via `readahead`                |

Your mileage depends on the access pattern. Random reads on a huge file are exactly where this pays off. **Full scans just download the file — use `aws s3 cp` for that.**

## 🔍 How It Works

```
┌─────────────┐   open("/remote/foo.db")   ┌──────────────┐
│  your app   │────────────────────────────▶│   smugmap    │
│ (sqlite3,   │                             │ (LD_PRELOAD) │
│  llama.cpp, │◀────── magic fd ────────────│              │
│  gdal, ...) │                             └──────┬───────┘
└──────┬──────┘                                    │
       │                                           │ HEAD /foo.db
       │  mmap(fd, ...)                            ▼
       │──────────────────▶┌──────────────┐   ┌───────┐
       │                   │  anon mmap   │   │  S3   │
       │                   │  + uffd      │   └───┬───┘
       │                   └──────┬───────┘       │
       │  read page N              │              │
       │──────────────────────────▶│              │
       │                           │ page fault   │
       │                           │─────────────▶│
       │                           │ GET bytes=…  │
       │                           │◀─────────────│
       │                           │ UFFDIO_COPY  │
       │◀──────────────────────────│              │
```

1. **`open("/remote/foo.db")`** — matches a pattern → HEAD request for size → magic fd returned.
2. **`fstat(fd)`** — synthetic struct with the real size.
3. **`mmap(fd, ...)`** — anonymous memory allocated and registered with `userfaultfd`.
4. **Page fault** — background thread issues a Range GET → `UFFDIO_COPY` fills the page → caller unblocks.
5. **`pread`/`read`** — straight Range GET (fallback for binaries that don't `mmap`).

Everything else falls through to the real libc.

## 🆚 Why Not FUSE?

| Solution                     | Needs `/dev/fuse` | Needs root | Works on Lambda | Works in rootless containers |
|------------------------------|:-----------------:|:----------:|:---------------:|:----------------------------:|
| s3fs / goofys / Mountpoint   | ✅                | Often ✅   | ❌              | ❌                           |
| NFS / EFS mount              | —                 | ✅         | ⚠️ (EFS only)   | ❌                           |
| Custom S3 SDK in the app     | ❌                | ❌         | ✅              | ✅                           |
| **smugmap**                  | ❌                | ❌         | ✅              | ✅                           |

`userfaultfd` has been available unprivileged since Linux 5.7. `LD_PRELOAD` needs nothing on the host.

## ⚠️ Limitations

- **Linux only** — `userfaultfd` is a Linux kernel feature.
- **Read-only** — writes fall through to the underlying filesystem.
- **Not for full scans** — every fault is a network round trip; use `aws s3 cp` if you'll read the whole file.
- **Needs `vm.unprivileged_userfaultfd=1`** — the default on modern kernels and Lambda ARM64.

## 🗺️ Roadmap

Read-heavy workloads are the focus. The following land when there's demand — [open an issue](../../issues) if that's you:

- Write support
- GCS and Azure Blob backends
- Adaptive readahead based on access pattern

## 🤝 Contributing

PRs welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: see [SECURITY.md](SECURITY.md).

## 📄 License

[MIT](LICENSE)
