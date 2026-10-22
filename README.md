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

Start with the local history already on this machine:

```bash
mmr list sessions --project /path/to/project
mmr read session <session-id>
mmr recall --project /path/to/project
```

For indexed text search, run `mmr init` in the project, then `mmr find "query"`.
For a local MCP client, use `mmr mcp --transport stdio`.

Carry one session to another machine without SSH:

```bash
# Source: write a portable inbox entry; JSON output includes inbox_path.
mmr --source codex share session <session-id> --to file:///absolute/path/to/inbox
# Copy that entry directory using your usual file transfer, then on destination:
mmr import bundle /copied/entry
```

The default import verifies and prints messages, caching the bundle without
writing provider histories. Use `-O md` for a Markdown `text` field in the JSON response. Native restoration is
optional: `mmr import bundle /copied/entry --apply --project /local/project`.
**Migration:** imports used to apply by default; scripts that restore native
sessions must now pass `--apply`. `--force` also requires `--apply`.

`.mmr` bundles are JSON with provider-native artifacts and hashes, not ZIP files.
They can contain private conversation text and paths; transfer them only where
intended. No cloud account or shared store is needed. The current `sync` backend
is a file-backed GitHub simulation, not a hosted GitHub/S3/R2 service.

See [docs/mmr-command-taxonomy.md](docs/mmr-command-taxonomy.md) and
[docs/mmr-session-sharing.md](docs/mmr-session-sharing.md) for the current CLI
shape.

## Development

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Focused synthetic contracts: `cargo test --test mcp_contract` and
`cargo test --test portable_bundle_contract`. These use temporary histories;
do not substitute real runtime session directories during development.
