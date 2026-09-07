# Contributing

Use Rust 1.96.1 or newer. Keep Cargo.lock committed for reproducible builds.

Before submitting a change, run:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Add regression tests for behavior changes. File tests must use temporary
directories, never the user's output directory. Keep tool descriptions,
README.md and AGENTS.md consistent. Palette changes require provenance and
tests for unique names/codes and shared 144/221 entries.

Report bugs with the OS, Rust version, MCP client, reproduction instructions,
and sanitized stderr logs. Do not include credentials or private artwork.
Keep pull requests focused; discuss larger features in an issue first.
