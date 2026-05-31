# mmr

`mmr` is a Rust CLI for browsing local AI coding-session history from Claude
Code, Codex, Cursor, Grok, and Pi.

## Install From GitHub

After this repository is public, install the latest `main` build with Cargo:

```bash
cargo install --git https://github.com/thehumanworks/mmr.git --locked
```

For reproducible installs, pin a tag or commit:

```bash
cargo install --git https://github.com/thehumanworks/mmr.git --tag <tag> --locked
cargo install --git https://github.com/thehumanworks/mmr.git --rev <commit-sha> --locked
```

Requirements:

- Rust 1.85 or newer.
- A C compiler toolchain, because `mmr` builds bundled SQLite through
  `rusqlite`.
- Normal platform TLS/build prerequisites for Rust networking crates.

Verify the install:

```bash
mmr --help
mmr skill load
```

## Development

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
