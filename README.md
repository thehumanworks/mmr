# mmr

`mmr` lets an agent fetch a project's earlier context, whichever harness
produced it: Claude Code, Codex, Cursor, Grok or Pi. It reads each harness's own
history files on this machine, so nothing needs to be set up, and its read
commands write nothing.

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
- A C compiler toolchain, because the TLS crypto library (`aws-lc-sys`, pulled
  in by `reqwest`) compiles C code.
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

## Let Your Agents Use It

mmr is mainly for coding agents: what earlier sessions in a project decided,
tried and left unfinished, whichever agent ran them. Install the bundled skill
so agents reach for it without being told:

```bash
mmr skill install   # ~/.agents/skills/mmr and ~/.claude/skills/mmr
```

The skill makes an agent check history when it starts or resumes work, or
before it explains why code is the way it is. A request that does not sound
like a history question ("I'm about to deploy this, anything to watch out
for?") does not trigger it. For that, give Claude Code the recent sessions at
startup with a SessionStart hook in `~/.claude/settings.json` (about 4 KB of
context per session start):

```json
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "echo 'Earlier agent sessions in this project, newest first (from mmr; read one with: mmr read session <session_id>):'; mmr list sessions --limit 8" }] }]
  }
}
```

[evals/cross_harness](evals/cross_harness/README.md) measures both setups
against a project with history from all five agents.

## Use

Run these inside the project, or pass `--project <name-or-path>`:

```bash
mmr find "retry limit"           # search this project's history, newest first
mmr --source codex read session <session-id>   # read a match (each match prints this)
mmr list sessions                # this project's sessions, newest first
mmr recall                       # the previous session in this project
mmr context project --limit 20   # session list plus the newest messages, in one call
```

- `find` is case-insensitive and searches every harness. `--project` or `--all`
  widens it and `--session` narrows it. Each match carries a `read_command`.
- `--source <claude|codex|cursor|grok|pi>` narrows any command to one harness.
- Output is JSON; add `--pretty` to indent it. Paged results include a
  `next_command` that runs as printed.
- `summarize` and `compact` condense history with an external model and need an
  API key. See [docs/mmr-command-taxonomy.md](docs/mmr-command-taxonomy.md).
- For agents: `mmr skill load` prints the agent skill, and
  `mmr mcp --transport stdio` serves the fetch commands as MCP tools (see
  [docs/mmr-mcp.md](docs/mmr-mcp.md)).

Try the whole workflow on synthetic history (Python 3, standard library only):

```bash
cargo build --locked
python3 examples/find_read_context.py
# Or: python3 examples/find_read_context.py /absolute/path/to/mmr
```

Expected output:

```text
find: Keep the retry limit at three.
read with: mmr --source codex read session example-session
read user: Keep the retry limit at three.
read assistant: Noted for the next session.
context: 1 session, 2 messages
Native histories unchanged; nothing was set up or written.
Temporary home removed.
```

The example creates an isolated temporary HOME/project, clears inherited provider
configuration, and verifies native files remain byte-for-byte unchanged.

## Move a session to another machine

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
intended. No cloud account or shared store is needed.

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
