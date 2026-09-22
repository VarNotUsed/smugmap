# llama.cpp on S3 via smugmap

Run inference on a GGUF model stored in S3 — no full download, smugmap
demand-pages only the weights llama.cpp actually reads.

## Quick start

```sh
# 1. Build smugmap
cargo build --release

# 2. Stream a model from Hugging Face directly into S3 — no local copy needed
curl -L https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf \
  | aws s3 cp - s3://your-bucket/tinyllama.gguf

# 3. Set your bucket in config.json
#    [{"pattern":"*.gguf","url":"s3://your-bucket/tinyllama.gguf","readahead":4}]

# 4. Set credentials
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=eu-central-1   # optional, default: us-east-1

# 5. Run inference
SMUGMAP_CONFIG=examples/llama/config.json \
  LD_PRELOAD=./target/release/libsmugmap.so \
  llama-cli -m /tmp/tinyllama.gguf -p "Hello, world" -n 128
```

The `"readahead": 4` in the config prefetches 4 extra pages per fault, which
matches llama.cpp's sequential access pattern through the weight tensors.

## How it works

smugmap intercepts `open()`, `read()`, and `mmap()` at the libc level. When
llama.cpp opens `/tmp/tinyllama.gguf`, smugmap recognises the filename from the
config and transparently serves it from S3 via HTTP Range requests — fetching
only the bytes llama.cpp actually reads. A 7B model (4GB) cold-starts in
seconds instead of minutes.
