# mmr

`mmr` is a Rust CLI for querying, exporting, summarizing, optimizing, merging, and syncing local Claude and Codex conversation history.

## What `mmr` reads

`mmr` works from local JSONL history files under your home directory:

- Claude: `~/.claude/projects/**/*.jsonl`
- Codex: `~/.codex/sessions/**/*.jsonl`
- Codex archives: `~/.codex/archived_sessions/**/*.jsonl`

The `sync` workflow also maintains a local manifest at `~/.config/mmr/sync-manifest.json`.

## Setup

1. Install a recent Rust toolchain with Edition 2024 support.
2. Make sure Claude and/or Codex local history exists under your home directory.
3. Configure any AI backend you plan to use:
   - Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
   - Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL`
   - Codex: authenticated `codex exec` environment
4. For cloud sync, prepare a Cloudflare R2 endpoint, bucket, and access keys.

## Common workflows

All query commands accept the global `--source claude|codex` flag. When `--source` is omitted, `mmr` uses `MMR_DEFAULT_SOURCE` if set, otherwise both sources.

| Workflow | Command | Notes |
| --- | --- | --- |
| List projects | `cargo run -- projects` | Returns machine-readable JSON. |
| Inspect the current project's sessions | `cargo run -- sessions` | Auto-discovers the current working directory as the default project scope. |
| Search all sessions across every project | `cargo run -- sessions --all` | Bypasses cwd scoping. |
| Inspect one session's messages | `cargo run -- messages --session sess-123` | Messages are returned in chronological order. |
| Export a project's full message stream | `cargo run -- export --project /path/to/proj` | Returns the same JSON shape as `messages`. |
| Export the current directory's project | `cargo run -- export` | Merges Claude and Codex messages and sorts by timestamp ascending. |
| Generate a continuity brief | `cargo run -- remember --project /path/to/proj` | Uses the latest session by default and returns Markdown by default. |
| Generate an optimized agent prompt | `cargo run -- prompt "Add sync docs" --target codex --project /path/to/proj` | Returns plain text, not JSON. |
| Preview a merge plan | `cargo run -- merge --from-session sess-a --to-session sess-b --dry-run` | Non-mutating JSON response. |

### Project scoping defaults

- `sessions` and `messages` auto-discover the current working directory as the default project scope.
- If cwd auto-discovery fails for `sessions` or `messages`, those commands fall back to all projects and both sources.
- `export`, `remember`, and `prompt` also default to the current working directory when `--project` is omitted.
- `export` resolves the cwd differently per source:
  - Codex uses the canonical path.
  - Claude uses the same path with `/` replaced by `-` and a leading `-`.

## `remember` and `prompt`

### `remember`

`remember` summarizes prior work for a project.

- Default selector: latest session
- Other selectors: `all`, `session <session-id>`
- Default output format: Markdown
- JSON output: `-O json`
- Backend selection: `--agent cursor|codex|gemini`
- Default backend: `MMR_DEFAULT_REMEMBER_AGENT` if set, otherwise Cursor

Examples:

```bash
cargo run -- remember --project /path/to/proj
cargo run -- remember all --project /path/to/proj
cargo run -- remember session sess-123 --project /path/to/proj -O json
cargo run -- remember --instructions "Return only a keyword." --project /path/to/proj
```

`--instructions` replaces the default output-formatting section of the system prompt while preserving the Memory Agent identity and transcript-input description.

### `prompt`

`prompt` generates a prompt for another coding agent.

- Target agent: `--target claude|codex`
- Backend optimizer: `--agent cursor|codex|gemini`
- Default backend: same resolution as `remember`
- Output: plain text on stdout
- Clipboard: best-effort copy via the local clipboard; failures are ignored

Context resolution is intentionally simple:

- If `mmr` can load prior session transcripts for the project, `prompt` uses those transcripts as its context.
- If no session transcripts are found, it falls back to lightweight codebase context built from `rg --files` plus up to a few keyword matches from the query.

Examples:

```bash
cargo run -- prompt "Add tests for merge dry-run" --target codex --project /path/to/proj
cargo run -- prompt "Write a concise release note" --target claude
```

## Sync runbook

`mmr sync` is the operational workflow for backing up local conversation history to cloud storage. The implementation currently uses Cloudflare R2-style configuration.

### What gets synced

- Claude history under `~/.claude/projects`
- Codex history under `~/.codex/sessions` and `~/.codex/archived_sessions`
- Only `.jsonl` files are uploaded or downloaded
- A local manifest is stored at `~/.config/mmr/sync-manifest.json`

### Initial setup

Run the interactive initializer:

```bash
cargo run -- sync init
```

This writes `~/.config/mmr/sync.toml` and, on Unix, saves it with `0600` permissions.

Default config shape:

```toml
[storage]
provider = "r2"
endpoint = "https://<account_id>.r2.cloudflarestorage.com"
bucket = "my-history-bucket"
access_key_id = "R2_ACCESS_KEY_ID"
secret_access_key = "R2_SECRET_ACCESS_KEY"
region = "auto"

[sync]
quiet_start = "03:00"
quiet_end = "09:00"
quiet_timezone = "Europe/London"
interval_minutes = 15

[sources]
claude = true
codex = true
```

Environment overrides are applied at runtime for:

- `MMR_SYNC_ENDPOINT`
- `MMR_SYNC_BUCKET`
- `MMR_SYNC_ACCESS_KEY_ID`
- `MMR_SYNC_SECRET_ACCESS_KEY`

### Daily commands

```bash
cargo run -- sync status
cargo run -- sync push --dry-run
cargo run -- sync push
cargo run -- sync pull --dry-run
cargo run -- sync pull
```

Behavior notes:

- `sync status`, `sync push`, and `sync pull` write machine-readable JSON to stdout.
- `sync init`, `sync install`, and `sync uninstall` print human-readable status text.
- `sync push` uploads new or modified local `.jsonl` files and updates the manifest after a real run.
- `sync pull` is non-destructive: it downloads only files that do not exist locally.
- If a local file exists but differs from the remote version, `sync pull` keeps the local file and reports a conflict instead of overwriting it.
- `--dry-run` reports what would happen without transferring files.

### Background daemon

Install a user-level background runner after `sync.toml` exists:

```bash
cargo run -- sync install --interval 15
```

- Linux: writes `~/.config/systemd/user/mmr-sync.service` and `~/.config/systemd/user/mmr-sync.timer`
- macOS: writes `~/Library/LaunchAgents/com.mmr.sync.plist`
- Both platforms write `~/.config/mmr/sync-daemon.sh`
- `sync uninstall` removes those installed artifacts
- Install and uninstall are supported only on macOS and Linux

The daemon wrapper checks the configured quiet hours before running `mmr sync push`. Manual `sync push` commands are not delayed by quiet hours.

## Environment variables

### CLI defaults

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE=codex|claude` changes the default source filter when `--source` is omitted
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` changes the default backend for `remember --agent` and `prompt --agent`

### Backend auth

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- Gemini test or proxy endpoint: `GEMINI_API_BASE_URL`
- Cursor: `CURSOR_API_KEY`

## Troubleshooting and common pitfalls

- `mmr sessions` and `mmr messages` are project-scoped by default. Use `--all` if you expected global results.
- `mmr remember` returns Markdown unless you pass `-O json`.
- `mmr prompt` returns raw prompt text, not a JSON object.
- Clipboard copy for `prompt` can fail silently on headless CI or machines without a usable display server.
- `mmr sync pull` will not overwrite a modified local history file; look for reported conflicts instead.
- When calling `mmr` from scripts, pass `--project` and its value as separate arguments rather than embedding quotes inside one argument.

## Contributor notes

For repository structure, verification requirements, and contributor-facing command guidance, see [`AGENTS.md`](./AGENTS.md) and the ADRs under [`adrs/`](./adrs/).
