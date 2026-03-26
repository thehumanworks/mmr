# mmr

`mmr` is a Rust CLI for querying local AI coding history from Codex, Claude, and Cursor.

It reads transcript files from the local machine, normalizes them into a common in-memory model, and returns machine-readable results on `stdout`. Human-facing hints and errors are written to `stderr`.

## What it ingests

`mmr` resolves the home directory with `SIMPLEMMR_HOME` first and falls back to the process `HOME`.

| Source | Files read | Notes |
| --- | --- | --- |
| Codex | `$HOME/.codex/sessions/**/*.jsonl` and `$HOME/.codex/archived_sessions/**/*.jsonl` | Project identity comes from `session_meta.payload.cwd`. |
| Claude | `$HOME/.claude/projects/<project>/*.jsonl` and nested `subagents/*.jsonl` | Project directories typically use the slash-to-hyphen naming form that Claude stores under `.claude/projects`. |
| Cursor | `$HOME/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl` | Cursor agent transcripts are grouped by project and session directory. |

The loaders parse JSONL defensively: malformed lines are skipped so valid history can still be queried.

## Quick start

Run commands from the repository root during development:

```bash
cargo run -- projects
cargo run -- sessions
cargo run -- messages
cargo run -- export
```

Representative workflows:

```bash
# List projects across all sources
cargo run -- projects

# List sessions for the current working directory's project by default
cargo run -- sessions

# Search everything instead of the cwd project
cargo run -- sessions --all

# Get messages for one session
cargo run -- messages --session sess-123 --source claude

# Export all messages for the current working directory's project
cargo run -- export

# Generate a continuity brief from the latest matching session
cargo run -- remember --project /path/to/proj
```

## Command guide

### `projects`

- Returns project summaries with per-project message and session counts.
- Uses all sources by default unless `--source` is provided or `MMR_DEFAULT_SOURCE` is set.
- Returns JSON on `stdout`.

Example:

```bash
cargo run -- --source cursor projects
```

### `sessions`

- Lists sessions for a project.
- When `--project` is omitted, `mmr` tries to auto-discover the current working directory's project.
- Use `--all` to bypass cwd auto-discovery and search across all projects.
- If cwd auto-discovery fails, `mmr` falls back to global results.
- If cwd auto-discovery succeeds but that project has no history, the result is empty instead of widening scope silently.

Examples:

```bash
# Default to the cwd project when it can be resolved
cargo run -- sessions

# Force a specific project
cargo run -- sessions --project /path/to/proj

# Search all projects
cargo run -- sessions --all
```

### `messages`

- Returns normalized message records with per-item `session_id`, `source`, and `project_name`.
- Uses the same cwd-project default as `sessions` when `--project` is omitted.
- `--session` narrows to one session ID.
- When `--session` is provided without `--project`, `mmr` bypasses cwd auto-discovery and searches all projects instead.
- Pagination metadata is included in the response as `next_page`, `next_offset`, and `next_command`.

Important pagination constraint:

- With the default `--sort-by timestamp --order asc`, pagination is computed from the newest matching window, then returned in chronological order.
- If `next_page` is `true`, prefer rerunning the exact `next_command` emitted in the JSON response.

Examples:

```bash
# Latest messages for the current project's history
cargo run -- messages

# Search by session ID across all projects
cargo run -- messages --session sess-123

# Page through results
cargo run -- messages --limit 100 | jq -r '.next_command'
```

### `export`

- Returns the same response shape as `messages`, but always exports the full message set for one project.
- Without `--project`, `export` infers the project from the current working directory:
  - Codex uses the canonical filesystem path.
  - Claude and Cursor use the slash-to-hyphen project form derived from that path.
- Results are sorted chronologically ascending.

Examples:

```bash
# Export the cwd project from all sources
cargo run -- export

# Export one project and pipe only the message array
cargo run -- export --project /path/to/proj | jq '.messages'
```

### `remember`

- Builds a stateless continuity brief from prior session transcripts.
- Uses the current working directory as the default project when `--project` is omitted.
- Selectors:
  - no selector: latest matching session
  - `all`: all matching sessions
  - `session <id>`: one specific session
- Output defaults to Markdown. Use `-O json` for structured JSON.

Backend behavior:

- Default backend: Cursor.
- `MMR_DEFAULT_REMEMBER_AGENT` can change the default to `cursor`, `codex`, or `gemini`.
- Cursor uses `composer-2-fast` unless `--model` is provided.
- Codex uses the local Codex client and currently defaults to `gpt-5.4-mini` with medium reasoning effort.
- Gemini uses `GOOGLE_API_KEY` or `GEMINI_API_KEY`; `GEMINI_API_BASE_URL` is supported for alternate endpoints and tests.

Examples:

```bash
# Latest session, Markdown output
cargo run -- remember --project /path/to/proj

# One specific session as JSON
cargo run -- remember session sess-123 --project /path/to/proj -O json

# Override the default output instructions
cargo run -- remember --project /path/to/proj --instructions "Return only a checklist."
```

## Environment variables

| Variable | Effect |
| --- | --- |
| `SIMPLEMMR_HOME` | Override the home directory used when reading local transcript files. |
| `MMR_AUTO_DISCOVER_PROJECT=0` | Disable cwd project auto-discovery for `sessions` and `messages`. |
| `MMR_AUTO_DISCOVER_PROJECT=1` or unset | Keep cwd project auto-discovery enabled. |
| `MMR_DEFAULT_SOURCE=codex|claude|cursor` | Supply the default `--source` when the flag is omitted. Invalid or empty values are treated as unset. |
| `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` | Supply the default `remember --agent` when the flag is omitted. Invalid or empty values are treated as unset. |
| `CURSOR_API_KEY` | Required when using the Cursor remember backend. |
| `GOOGLE_API_KEY` or `GEMINI_API_KEY` | Required when using the Gemini remember backend. |
| `GEMINI_API_BASE_URL` | Optional alternate Gemini base URL; used by integration tests with a mock server. |

## Troubleshooting and common pitfalls

### A query unexpectedly returned an empty list

Check these first:

1. `pwd` - `sessions` and `messages` default to the current working directory's project.
2. `cargo run -- projects` - confirm the project name that `mmr` actually sees.
3. Retry with `--all` to bypass cwd scoping.
4. If you know the exact session ID, use `messages --session <id>`.

### I am not sure which `--project` value to pass

Prefer copying the exact project identifier from `mmr projects`.

This is especially useful for Claude and Cursor, where stored project names often use the slash-to-hyphen directory encoding instead of the raw filesystem path.

### My script passes `--project`, but matching breaks

Pass `--project` and the project value as separate arguments.

Good:

```bash
mmr messages --project /path/to/proj
```

Also good in subprocess APIs:

```python
["mmr", "messages", "--project", "/path/to/proj"]
```

Avoid embedding quotes inside one argument:

```python
["mmr", "messages", '--project="/path/to/proj"']
```

### I only want machine-readable output

- `projects`, `sessions`, `messages`, and `export` write JSON to `stdout`.
- `remember` writes Markdown to `stdout` by default; use `-O json` to get JSON instead.
- Hints and diagnostics belong on `stderr`.

## Development

Common verification commands:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

## Additional references

- `AGENTS.md` - repository workflow, contracts, and verification expectations.
- `adrs/002-cwd-scoped-defaults.md` - why `sessions` and `messages` default to the cwd project.
- `docs/references/session-lookup-invariants.md` - session lookup rules for `messages --session`.
- `docs/references/schemas/` - source-specific raw transcript schema notes.
