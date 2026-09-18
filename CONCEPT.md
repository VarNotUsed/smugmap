# smugmap

> Serve any large file from S3 — transparently, on demand, to any unmodified binary.

---

## The Idea

Most programs that work with large files use `mmap()` — they ask the OS to map a file into memory and let the kernel page in the data on demand as they access it. This is how SQLite reads databases, how llama.cpp loads model weights, how GDAL reads geospatial files.

**smugmap hijacks this.**

Using two Unix primitives:

1. **LD_PRELOAD** — a shared library loaded before any other, which lets us replace `open()`, `mmap()`, `fstat()` and `pread()` with our own implementations. The target binary never knows.

2. **userfaultfd** — a Linux kernel feature where a userspace thread registers a memory region and handles page faults itself. Instead of the kernel reading from disk, *we* get called for every 4KB the program tries to access.

Combined: when a program opens a file, we intercept the call, return a fake fd backed by anonymous memory, and register that memory with userfaultfd. When the program touches any byte, the kernel wakes our background thread, we fetch the corresponding 4KB chunk from S3, and write it into the page. The program sees it as if the file was always there.

**Zero code changes to the target binary. Zero local disk. Only the pages actually accessed are ever fetched.**

---

## The Two Primitives

### LD_PRELOAD

```
LD_PRELOAD=/usr/local/lib/smugmap.so your-binary --flag arg
```

smugmap intercepts:
- `open(path, ...)` — if path matches a configured pattern (e.g. `*.db`, `*.gguf`), return a magic fd instead of opening a real file. HEAD the S3 object for its size.
- `fstat(fd, ...)` — return a synthetic `stat` with the correct `st_size`.
- `mmap(fd, ...)` — allocate anonymous memory of the right size, register with userfaultfd.
- `pread(fd, ...)` — direct S3 range fetch (fallback for binaries that don't use mmap).
- `munmap(fd, ...)` — cleanup.

### userfaultfd

After intercepting `mmap()`, smugmap:
1. Creates a userfaultfd file descriptor.
2. Registers the anonymous memory region.
3. Starts a background thread blocking on the uffd.
4. When the binary touches an unmapped page → kernel delivers `UFFD_EVENT_PAGEFAULT` to our thread.
5. Thread fetches `bytes=[offset, offset+4096)` from S3 via an HTTP Range request.
6. Copies bytes into the page via `UFFDIO_COPY` → wakes the faulting thread.
7. Binary continues as if the page was always there.

---

## What You Can Do With It

**LLM inference without local weights**
```bash
SMUGMAP_BUCKET=my-models SMUGMAP_KEY=llama-3-8b.gguf SMUGMAP_PATTERN="*.gguf" \
  LD_PRELOAD=smugmap.so llama-server -m /remote/model.gguf
```
Only the layers actually visited during inference are fetched. A 7B model doing a 512-token query might touch 2–3GB out of 4.5GB of weights.

**SQLite from S3**
```bash
SMUGMAP_BUCKET=my-data SMUGMAP_KEY=analytics.db SMUGMAP_PATTERN="*.db" \
  LD_PRELOAD=smugmap.so sqlite3 /remote/analytics.db "SELECT count(*) FROM events"
```
A 50GB database, a point query that reads 10 pages — only those 10 pages are fetched.

**Geospatial data**
```bash
SMUGMAP_BUCKET=geodata SMUGMAP_KEY=world.tif SMUGMAP_PATTERN="*.tif" \
  LD_PRELOAD=smugmap.so gdal_translate -srcwin 1000 1000 256 256 /remote/world.tif out.png
```
A 200GB GeoTIFF, only the requested tile is transferred.

**The pattern:** large file, sparse access, read-only, unmodified binary. If it fits, smugmap works.

---

## Why This Doesn't Exist Yet

- **NFS/FUSE**: require kernel modules or `/dev/fuse`. Lambda doesn't have either.
- **Custom S3 SDKs**: require modifying the application code.
- **Presigned URL + wget**: downloads the whole file first.
- **CRIU**: uses userfaultfd for process migration, not for storage backends.

The combination of LD_PRELOAD + userfaultfd as a generic demand-paging shim for remote storage has not been packaged as a standalone library.

---

## Implementation

**Language:** C (no runtime — essential for LD_PRELOAD interposition)  
**S3 auth:** libcurl + AWS SigV4, or presigned URL (configurable)  
**Configuration:** environment variables

| Variable | Description |
|----------|-------------|
| `SMUGMAP_BUCKET` | S3 bucket name |
| `SMUGMAP_KEY` | S3 object key |
| `SMUGMAP_PATTERN` | File pattern to intercept (e.g. `*.gguf`, `*.db`) |
| `SMUGMAP_REGION` | AWS region (default: `AWS_REGION` env) |
| `SMUGMAP_PRESIGNED_URL` | Optional: use a presigned URL instead of SigV4 |

**Performance knobs:**
- Connection keep-alive (one curl handle per fault-handler thread, not per request)
- Optional read-ahead: prefetch the next N pages after a fault

---

## Proof of Concept (done, in `../ai-labor`)

A working spike exists that validates the two critical unknowns:

1. **userfaultfd available in AWS Lambda ARM64?** → ✅ Yes (`/proc/sys/vm/unprivileged_userfaultfd = 1`)
2. **S3 range-request latency tolerable?** → ✅ p50 = 24ms (improvable with keep-alive to ~3ms)

The spike (`llambda`) uses this exact mechanism to run llama.cpp on Lambda without downloading model weights. The code for `smugmap.so` is ~300 lines of C.

---

## What to Build Next

1. **Generalize** — rename, remove llama-specific hardcoding, clean env var API
2. **Fix latency** — curl keep-alive (one handle per thread, reused across page faults)
3. **Demo 1** — LLM inference on Lambda (from the spike)
4. **Demo 2** — `sqlite3` querying a 10GB database on S3, zero local disk
5. **README + benchmarks** — the framing matters as much as the code
6. **Ship** — GitHub, Hacker News: *"We built a demand-paging shim for S3 using LD_PRELOAD + userfaultfd"*

---

## What This Is Not

- Not a FUSE filesystem (no kernel module, works in sandboxed environments like Lambda)
- Not a caching layer (pages stay in the OS page cache naturally; smugmap doesn't manage eviction)
- Not multi-backend yet (S3 only for now — add GCS/Azure based on actual demand)
- Not suitable for write-heavy workloads (read-only focus, writes go through normal paths)
