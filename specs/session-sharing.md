# Session Sharing

This specification defines the public command contract for moving one selected
native provider session between machines, optionally over a trusted SSH peer.

## Principles

- A peer is explicit per invocation. `mmr` does not maintain a host registry,
  query `known_hosts`, inspect Tailscale, or discover peers.
- Read/query commands are local only. There is no peer-read flag; history from
  another machine arrives through `share` and `import`.
- Directional movement is literal:
  - `share` is run on the source machine.
  - `import` is run on the destination machine.
  - `ingest events` imports provider history into normalized local events.
- No legacy command or flag aliases are retained unless a maintainer explicitly
  asks for compatibility.
- stdout is one JSON object on success. Human diagnostics and warnings go to
  stderr.

## SSH Target Syntax

Targets are passed directly to SSH:

- `mini`
- `user@mini`
- `user@mini:22`
- `ssh://user@mini:22`

If the local SSH configuration cannot resolve or authenticate the target, the
command fails. That failure is the expected validation path.

## Peer Protocol

`mmr` reaches a peer by running `ssh -o BatchMode=yes ... -- <target> mmr peer
<command>` with one JSON request on stdin. The peer protocol version is 1.

- `mmr peer status [--host <target>]` reports `protocol_version`, `mmr_version`,
  `capabilities` (`["teleport-pack"]`) and `sources`.
- `mmr peer teleport-pack` is hidden. `import session --from` calls it to
  package one native session on the peer.

A peer failure is structured JSON with a nonzero exit. A peer that lacks the
`peer` subcommand or a requested peer command reports `peer_mmr_unavailable`.

## Source-Side Sharing

Command:

```bash
mmr share session [SESSION|latest] [--project <path>] [--to <target-or-file-url>] [--via auto|ssh|http|file] [--bind <addr:port>] [--timeout <seconds>] [--dry-run]
```

Selector rules:

- `SESSION` selects an explicit source session id.
- `latest`, `--latest`, or an omitted selector selects the latest session in
  scope.
- `SESSION` and `--session <id>` are aliases within this new command family only;
  they are not compatibility shims for removed commands.
- `--project` and global `--source` narrow scope before latest selection.
- conflicting selectors fail with a usage error.

Transport rules:

- `auto` infers SSH for normal SSH targets and file for `file://` destinations.
- `ssh` streams a bundle to remote `mmr import bundle --to - --apply` when remote
  `mmr` is available. If remote `mmr` is unavailable, the command may stage the
  bundle in the remote inbox and return `status: "partial"` plus the exact next
  command for the destination.
- `file` writes an atomic bundle into the provided inbox directory.
- `http` starts a one-shot local listener and prints an `mmtp://` locator.

Output command value: `share/session`.

## Destination-Side Import

### `import session`

Command:

```bash
mmr import session --from <ssh-target> (--session <id>|--latest) [--project <path>] [--read-only|--apply] [--force]
```

Behavior:

- asks the remote peer to package one selected native provider session.
- caches the received bundle locally.
- by default (or with `--read-only`), verifies and prints the bundle messages
  without applying native provider files.
- `--apply` explicitly restores native files. `--force` requires `--apply`.
- verifies artifact hashes and the returned bundle identity before caching.
- `--read-only` and `--apply` together are a usage error.
- stale remote peer protocols fail with structured peer failure JSON.

Output command value: `import/session`.

### `import bundle`

Command:

```bash
mmr import bundle [LOCATOR] [--to <locator>] [--project <path>] [--read-only|--apply] [--force] [-O json|md] [--dry-run]
```

Locators:

- local bundle path.
- inbox directory or ready bundle path.
- `mmtp://` or `http://` one-shot locator.
- stdin marker `-`, only valid with `--apply`.

Behavior:

- the default and `--read-only` verify and print metadata/messages using the
  existing read/cache response. Native provider files remain untouched.
- `--apply` is required for native restoration, including stdin streams.
- apply mode writes native provider files and skips normalized event ingestion
  unless the implementation opts into a safe importer for that provider.
- `--force` permits replacing existing native files only with `--apply`.
- missing or conflicting locators fail with usage JSON.

Output command value: `import/bundle`.

## Event Ingestion

Command:

```bash
mmr --source codex ingest events --project <path> [--source-root <root>]
mmr --source claude ingest events --project <path> [--source-root <root>]
mmr --source cursor ingest events --project <path> [--source-root <root>]
```

`ingest events` is the only public command for importing provider history into
normalized local store events. `import` is reserved for session/bundle material.

## Removed Public Names

The public CLI rejects:

- `--host` and `--remote` on read/query commands.
- the old top-level event-import argv under `import`.
- the removed transport-name namespace and all of its subcommands.

Hidden peer diagnostics and implementation module names may retain older
internal names until the private code is renamed. They are not user-facing
contract.

## Verification Contract

Minimum checks for changes to this surface:

```bash
cargo fmt
cargo test --test peer_import_loopback_contract -- --nocapture
cargo test --test cli_contract peer -- --nocapture
cargo test --test cli_contract share -- --nocapture
cargo test --test cli_contract import -- --nocapture
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Live peer validation should use an SSH/Tailscale target with the same command
surface installed:

```bash
mmr peer status --host mini
mmr import session --from mini --session latest --project /Users/mish/projects/mmr --read-only
mmr share session latest --project /Users/mish/projects/mmr --to mini --dry-run
```

## Portable File Workflow And Migration

`share session --to file:///absolute/inbox` uses the existing provider collector
and publishes an inbox entry with `bundle.mmr`, `bundle.sha256` and `ready` after
writing `bundle.mmr.partial`. Copy the entry directory to another machine and
run `mmr import bundle /copied/entry`. The entry must be ready and its sidecar
checksum valid. A standalone `bundle.mmr` can also be copied/read; it validates
internal artifact hashes but has no external sidecar check. `.mmr` is JSON, not
ZIP. The format retains provider-native artifacts and normalized read output;
no native Claude/Codex conversion is required to read it.

Imports may cache under `~/.mmr/teleport/cache`, but default imports do not
install provider files or ingest local search events. Messages retain source
project provenance; `--project` remaps only explicit native apply. No SSH,
server, account or cloud storage is required for this workflow.

Migration: older builds applied imports when no mode was specified. Add
`--apply` to scripts that intentionally restore native sessions. Existing
`--read-only` scripts and explicit `--apply` streams keep their behavior. A bare
`--force` is now a usage error, so it cannot silently imply native restoration.
