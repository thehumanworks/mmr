# mmr

`mmr` is a Rust CLI for browsing local AI coding-session history from Claude
Code, Codex, Cursor, Grok, and Pi.

## Install From GitHub

Install the latest `main` build with Cargo:

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

## Download A Prebuilt Binary

The [Build workflow](.github/workflows/build.yml) builds `mmr` for four
targets on every push to `main`, every pull request and every `v*` tag:

| Platform                   | Archive                                |
| -------------------------- | -------------------------------------- |
| Linux x86_64 (glibc 2.35+) | `mmr-x86_64-unknown-linux-gnu.tar.gz`  |
| Linux arm64 (glibc 2.35+)  | `mmr-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel (11+)          | `mmr-x86_64-apple-darwin.tar.gz`       |
| macOS Apple silicon (11+)  | `mmr-aarch64-apple-darwin.tar.gz`      |

Each archive holds the `mmr` binary and this README, next to a `.sha256`
checksum file. A `v*` tag attaches the archives and a combined `SHA256SUMS` to
its [GitHub release](https://github.com/thehumanworks/mmr/releases). Builds of
`main` and of pull requests are workflow artifacts on the
[Actions run page](https://github.com/thehumanworks/mmr/actions/workflows/build.yml)
(GitHub sign-in required).

Install the latest release:

```bash
target=aarch64-apple-darwin   # pick your archive from the table
base=https://github.com/thehumanworks/mmr/releases/latest/download
curl -fsSLO "$base/mmr-$target.tar.gz"
curl -fsSLO "$base/mmr-$target.tar.gz.sha256"
shasum -a 256 -c "mmr-$target.tar.gz.sha256"
tar -xzf "mmr-$target.tar.gz" mmr
mkdir -p ~/.local/bin && mv mmr ~/.local/bin/
```

The macOS binaries are not notarized. `curl` downloads run as-is; if you
download with a browser, clear the quarantine flag first with
`xattr -d com.apple.quarantine mmr`.

## Install With Nix

The repository is a Nix flake with a package, an overlay, a home-manager
module and a NixOS module:

```bash
nix run github:thehumanworks/mmr -- --help
nix profile add github:thehumanworks/mmr
```

```nix
# home-manager
imports = [ inputs.mmr.homeModules.default ];
programs.mmr.enable = true;

# NixOS
imports = [ inputs.mmr.nixosModules.default ];
programs.mmr.enable = true;
```

Both modules can also run the HTTP MCP server (`services.mmr-mcp`). See
[docs/mmr-nix.md](docs/mmr-nix.md) for every option.

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

To recover a decision when you do not know its session, run these in the project:

```bash
mmr init                             # create/update the local indexed store
mmr find "retry"                     # inspect matching lines and citations
mmr retrieve "retry" --max-sessions 1 # select a readable native session
mmr --source <source> read session <source_session_id> --project <project_name>
mmr context project --limit 20        # collect bounded raw context for an agent
```

Use `source`, `source_session_id`, and `project_name` from `retrieve`'s
`selected_sessions` for `read`; `find`'s `session_id` is an internal store ID.
If you already know the native session ID, go straight to `read` without `init`.
`context project` returns session metadata and a bounded message array locally;
it does not call a model.

Try the whole workflow on synthetic history (Python 3, standard library only):

```bash
cargo build --locked
python3 examples/find_read_context.py
# Or: python3 examples/find_read_context.py /absolute/path/to/mmr
```

Expected output:

```text
find: Keep the retry limit at three.
retrieve: codex/example-session
read user: Keep the retry limit at three.
read assistant: Noted for the next session.
context: 1 session, 2 messages
Native histories unchanged; only the temporary local store was initialized.
Temporary home removed.
```

The example creates an isolated temporary HOME/project, clears inherited provider
configuration, and verifies native files remain byte-for-byte unchanged. The
current file-backed sync simulation used by `init` stays inside that HOME.

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
