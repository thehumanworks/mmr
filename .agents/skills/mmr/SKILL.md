---
name: mmr
description: "Read and search local AI coding-session history (Claude, Codex, Cursor, Grok, Pi), recover project context, or carry a selected session between machines as a file. Use for mmr setup, usage, and troubleshooting."
---

# mmr

`mmr` is the local Rust CLI for retrieving the history needed for a task.
Check `mmr <command> --help` for the installed version's contract. stdout is JSON
unless a command explicitly requests another format. Check the exit status:
structured failures can also emit JSON on stdout; diagnostics use stderr.

## Read Local Context

```bash
mmr list sessions --project /path/to/project
mmr read session <session-id>
mmr recall --project /path/to/project
```

`recall` selects the previous stable session, holding back the newest by default.
`list sessions` and `read project` infer the current project when no scope is
given; pass `--project` when the scope matters. Omit `--source` for all providers unless `MMR_DEFAULT_SOURCE` sets a default,
or use `--source claude|codex|cursor|grok|pi`; `all` is not a source value.

When you do not know which session holds something, run `mmr find "phrase"` in
the project. It needs no setup, is case-insensitive, searches every harness,
and returns matches newest first. Run a match's `read_command` (or pass its
`source` and `session_id` to `mmr --source <source> read session <session_id>`)
to read the whole session. Use `--project <path>` or `--all` to widen the scope.
If access to history is restricted, report the missing context instead of
reading protected provider files as a fallback.

Use `mmr context project --limit 20` for bounded raw messages and session metadata
without model calls. In the source checkout, run `python3
examples/find_read_context.py` after `cargo build --locked` to try this flow with
an isolated synthetic HOME and verify native histories remain unchanged.

## Carry Context As A File

```bash
# Source machine: choose the session, then export to an inbox directory.
mmr --source codex share session <session-id> --to file:///absolute/path/to/inbox
# Copy the directory reported as inbox_path to the destination machine.
mmr import bundle /copied/entry
mmr import bundle /copied/entry -O md | jq -r .text
```

The entry contains `bundle.mmr` (JSON, not ZIP), `bundle.sha256`, and `ready`.
Default import verifies and reads messages, caching under `~/.mmr/teleport/cache`
without installing native histories. Preserve source project provenance. No
SSH, server, central account, or Claude/Codex conversion is needed to read it.
Native artifacts may contain private conversation text and paths; transfer only
when intended by the user.

To deliberately restore a provider-native session, pass `--apply --project
/local/project`. `--force` requires `--apply`. Older builds applied imports by
default; scripts that need restoration must add `--apply`. Explicit
`--read-only` remains supported. The same read-only default applies to
`import session --from <ssh-target>`; SSH is an optional transport.

## Additional Capabilities

- `mmr context project` prepares project context without calling a model.
- `mmr summarize` and `mmr compact` send selected text to configured external
  model services. Use only when that processing is intended.
- `mmr mcp --transport stdio` exposes the existing CLI-backed tools to a client.
- `mmr sync` currently uses a file-backed remote simulation, not hosted GitHub,
  S3 or R2. File handoffs do not require sync.
- `mmr skill load` prints this bundle; `mmr skill install [--local]` replaces its
  installed copy.

For deeper continuity extraction, read `session-mining/SKILL.md`. In the mmr
source repository, follow `AGENTS.md` and the focused MCP, native-bundle and CLI
skills; verify with fixture-only contract tests and the full Rust checks.
