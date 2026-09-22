# smugmap

[![CI](https://github.com/VarNotUsed/smugmap/actions/workflows/ci.yml/badge.svg)](https://github.com/VarNotUsed/smugmap/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/smugmap.svg)](https://crates.io/crates/smugmap)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Point an unmodified binary at an S3 URL and let it read the file as if it
were local. Only the bytes the program actually touches ever leave the
bucket.

smugmap is a small `LD_PRELOAD` shim. It intercepts `open`, `mmap`,
`pread` and friends, hands the caller a fake fd backed by anonymous memory,
and uses Linux `userfaultfd` to fault pages in from S3 on demand. No FUSE,
no kernel module, no root. It works in the places FUSE can't: AWS Lambda,
rootless containers, Kubernetes pods without host-path mounts, most CI
runners.

## Quick start

```bash
export AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_REGION=eu-central-1

cat > /tmp/smugmap.json <<'EOF'
[{"pattern":"*.db","url":"s3://my-bucket/analytics.db"}]
EOF

SMUGMAP_CONFIG=/tmp/smugmap.json LD_PRELOAD=/usr/local/lib/smugmap.so \
  sqlite3 /remote/analytics.db "select count(*) from events"
```

That query hits maybe a few dozen 4 KB pages out of a 50 GB database.
Everything else stays in S3.

## Install

Grab a pre-built `.so` from [Releases](../../releases), or build from
source:

```bash
make build
sudo make install
```

Linux only. Needs `vm.unprivileged_userfaultfd=1` — the default on modern
kernels and on Lambda ARM64.

## Config

`SMUGMAP_CONFIG` points at a JSON file. Each entry maps a filename glob to
a URL:

```json
[
  { "pattern": "*.db",   "url": "s3://my-bucket/analytics.db" },
  { "pattern": "*.gguf", "url": "https://...presigned...",     "readahead": 4 }
]
```

| Field | Required | Description |
|-------|----------|-------------|
| `pattern` | yes | Glob matched against the opened filename |
| `url` | yes | `s3://bucket/key` (SigV4 from env) or an HTTP URL that supports `Range` |
| `readahead` | no | Extra pages to prefetch on each fault (default: 0) |

Two URL flavors:

- `s3://…` — the usual case. smugmap signs Range requests with SigV4 using
  `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`, and
  `AWS_SESSION_TOKEN` if present.
- `https://…` — plain Range GETs. Handy when the runtime has no
  credentials, e.g. a presigned URL:
  ```bash
  URL=$(aws s3 presign s3://my-bucket/analytics.db --expires-in 3600)
  ```

The config file can contain presigned URLs — `chmod 600` it.

Two environment knobs on top of the AWS variables:

- `SMUGMAP_CONFIG` — path to the JSON file.
- `SMUGMAP_QUIET=1` — silence the `[smugmap] …` log lines on stderr.

## Where it earns its keep

The sweet spot is a large file with sparse read patterns and a binary you
can't (or don't want to) modify.

- **LLM inference on Lambda.** llama.cpp mmaps a GGUF file; a 512-token
  query on a 7B model touches maybe 2–3 GB out of 4.5 GB of weights. The
  rest never gets pulled.
- **SQLite / DuckDB on S3.** A point query on a 50 GB database reads a
  handful of B-tree pages. That's the download.
- **Geospatial.** GDAL reading one tile out of a 200 GB GeoTIFF pulls one
  tile.

Runnable examples for each live in [`examples/`](examples/).

## Numbers

From the spike this crate grew out of:

- **S3 Range GET latency** — p50 ~24 ms cold, ~3 ms warm on the same
  keep-alive connection.
- **7B GGUF, 512-token query** — ~2.5 GB fetched out of 4.5 GB on disk.
- **Page size** — 4 KB per fault. `readahead` amortizes latency when the
  access pattern is at all sequential.

Your mileage depends on the access pattern. Random reads on a huge file
are exactly where this pays off; a full scan just downloads the file, and
you should use `aws s3 cp` for that.

## How it works

Roughly:

1. `open("/remote/foo.db")` matches a pattern. smugmap does a `HEAD`,
   remembers the size, and hands back a magic fd.
2. `fstat` on that fd returns a synthetic struct with the real size.
3. `mmap` on that fd allocates anonymous memory and registers it with
   `userfaultfd`.
4. First read of a page → kernel delivers a fault to a background thread →
   thread issues a Range GET → `UFFDIO_COPY` fills the page → caller
   unblocks.
5. `pread`/`read` on the magic fd go straight to a Range GET, for binaries
   that don't mmap.

Everything else falls through to the real libc.

## Why not FUSE

s3fs, Mountpoint-S3 and goofys all need `/dev/fuse`, which isn't there in
Lambda, in unprivileged containers, or in most CI environments. smugmap
uses only `userfaultfd` (unprivileged since Linux 5.7) and `LD_PRELOAD`,
so there's nothing to install on the host.

## Limitations

- Linux only.
- Read-only. Writes fall through to the underlying filesystem.
- Needs `unprivileged_userfaultfd=1`. It's on by default nearly everywhere
  that matters.

## Roadmap

Read-heavy workloads are the focus. Write support and GCS/Azure backends
land when someone actually needs them — [open an
issue](../../issues) if that's you.

## License

MIT. See [LICENSE](LICENSE).
