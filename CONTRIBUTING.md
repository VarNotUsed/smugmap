# Contributing

PRs welcome. Keep them small and focused.

## Build & test

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Integration test needs Linux with `vm.unprivileged_userfaultfd=1` and Python 3
(for the tiny `test/serve.py` HTTP server). See `.github/workflows/ci.yml` for
the exact sequence CI runs.

## Style

- No unrequested abstractions or dependencies. If a few lines do the job,
  write the few lines.
- New non-trivial logic ships with a runnable check (a `test_*` or an
  `assert`-based `demo()`).
- Keep diffs minimal — reviewers should be able to read the whole change.

## Reporting bugs

Open an issue with: kernel version, `LD_PRELOAD` target binary, config
snippet (with URLs redacted), and the stderr output with `SMUGMAP_QUIET`
unset.
