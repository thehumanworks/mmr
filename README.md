# mmr

`mmr` is a Rust CLI for reading local AI coding history from Claude, Codex, Cursor, Grok, and Pi and returning machine-readable JSON for projects, sessions, messages, exports, and continuity briefs.

The code stays storage-free: it reads each source's local transcript files directly, normalizes them in memory, and exposes a small read/query surface.

## Supported local history sources

| Source | Default history root | Project identity used by mmr |
| --- | --- | --- |
| Codex | `~/.codex/` | canonical working-directory path |
| Claude | `~/.claude/projects/` | encoded project directory name, with stored `cwd` used when present |
| Cursor | `~/.cursor/projects/` | encoded project directory name |
| Grok | `~/.grok/sessions/` | canonical cwd from `summary.json` or percent-decoded project directory |
| Pi | `~/.pi/agent/sessions/` | session `cwd`, while project listings still expose the parent directory name |

Use `SIMPLEMMR_HOME=/path/to/home` to point `mmr` at a different home directory than the current shell's `HOME`.

## Requirements

- Rust toolchain with Edition 2024 support (`Cargo.toml` sets `edition = "2024"`).
- A recent stable cargo if the host default is older. In this environment, `cargo +stable ...` is the safe fallback.

Examples:

```bash
cargo +stable run -- projects
cargo +stable test
```

## Quick start

List all known projects across all sources:

```bash
cargo run -- projects
```

List sessions for the current working directory's project:

```bash
cargo run -- sessions
```

List messages for the current working directory's project:

```bash
cargo run -- messages
```

Export the full current project's transcript as chronological JSON:

```bash
cargo run -- export
```

Generate a continuity brief from the latest matching session:

```bash
cargo run -- remember --project /path/to/proj
```

## Core workflows

### `projects`

`projects` returns aggregated project metadata:

- `name`
- `source`
- `original_path`
- `session_count`
- `message_count`
- `last_activity`

Examples:

```bash
cargo run -- projects
cargo run -- --source grok projects
cargo run -- projects --sort-by message-count --order desc --limit 20
```

### `sessions`

By default, `sessions` tries to auto-discover the current working directory as the active project scope.

- If auto-discovery succeeds, results are scoped to that project.
- If auto-discovery fails, `sessions` falls back to all projects.
- If auto-discovery succeeds but the project has no matching history, the response is an empty JSON list instead of widening scope.
- `--all` disables cwd auto-discovery.

Examples:

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- --source codex sessions --project /Users/test/codex-proj
```

### `messages`

`messages` uses the same cwd auto-discovery rules as `sessions`, then applies message-specific selectors:

- `--session <id>` filters to one session.
- `--latest [N]` returns the latest `N` messages from the latest session in scope.
- `--from-message-index <N>` / `--to-message-index <N>` slice a zero-based window after filtering and sorting.
- Standard pagination uses `--limit` and `--offset`.

Important behavior:

- Default invocation is `--limit 50 --sort-by timestamp --order asc`.
- For ascending timestamp order, pagination is applied from the newest window and then returned in chronological order.
- `next_page`, `next_offset`, and `next_command` are populated for paginated `messages` results, but not for `--latest`.
- When `--session` is supplied without `--project`, the CLI skips cwd auto-discovery and searches all matching projects instead.

Examples:

```bash
cargo run -- messages
cargo run -- messages --all --limit 100
cargo run -- messages --session sess-123
cargo run -- messages --latest
cargo run -- messages --latest 5
cargo run -- messages --from-message-index 10 --to-message-index 20
```

The canonical behavior contract for this command lives in [`specs/messages.md`](specs/messages.md).

### `export`

`export` returns an `ApiMessagesResponse` containing all messages for one project in chronological order.

- `cargo run -- export --project /path/to/proj` queries the explicit project across all sources unless `--source` is set.
- `cargo run -- export` infers the project from the current working directory.

When `export` infers from cwd:

- Codex, Grok, and Pi use the canonical path directly.
- Claude and Cursor use the same path encoded as a project directory name with `/` replaced by `-` and a leading `-`.

Examples:

```bash
cargo run -- export
cargo run -- --source pi export --project /Users/test/pi-proj
```

### `remember`

`remember` builds a stateless continuity brief from previously recorded sessions.

- Default selection: latest matching session.
- `remember all`: include all matching sessions.
- `remember session <id>`: include one specific session.
- Default output format: Markdown (`-O md`).
- JSON output is available with `-O json`.

Examples:

```bash
cargo run -- remember --project /path/to/proj
cargo run -- remember all --project /path/to/proj
cargo run -- remember session sess-123 --project /path/to/proj
cargo run -- remember --project /path/to/proj -O json
```

Backend requirements:

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL`
- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: working Codex CLI/app-server auth in the current environment

If `--agent` is omitted, `MMR_DEFAULT_REMEMBER_AGENT` applies when set; otherwise the default backend is Cursor with model `composer-2-fast` unless `--model` overrides it.

## Environment variables

### Query defaults

- `SIMPLEMMR_HOME=/path/to/home` overrides the history root that all source loaders read from.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for `sessions` and `messages`.
- `MMR_AUTO_DISCOVER_PROJECT=1` or unset keeps cwd auto-discovery enabled.
- `MMR_DEFAULT_SOURCE=claude|codex|cursor|grok|pi` supplies the default `--source` when the flag is omitted.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default `remember --agent` when the flag is omitted.

Empty or invalid values for `MMR_DEFAULT_SOURCE` and `MMR_DEFAULT_REMEMBER_AGENT` are treated as unset.

## Source-specific matching notes

- Claude project directories are encoded names such as `-Users-mish-proj`, but Claude records can still expose a canonical `cwd`, so `--project /actual/path` works when that path is present in the transcript.
- Cursor project directories are also encoded names such as `-Users-mish-proj`, and the current loader keeps that encoded value as both `project_name` and `project_path`. In practice, direct `--project` filtering for Cursor matches the encoded project directory name, not the decoded filesystem path.
- Grok stores project directories percent-encoded under `~/.grok/sessions/`, but `mmr` decodes them and prefers `summary.json.info.cwd` when present.
- Pi project listings expose the parent directory name under `~/.pi/agent/sessions/`, while filtering and exports use the `cwd` stored in session records.

See the raw source-layout references in:

- [`docs/references/schemas/claude/message_schema.md`](docs/references/schemas/claude/message_schema.md)
- [`docs/references/schemas/codex/message_schema.md`](docs/references/schemas/codex/message_schema.md)
- [`docs/references/schemas/cursor/message_schema.md`](docs/references/schemas/cursor/message_schema.md)
- [`docs/references/schemas/grok/message_schema.md`](docs/references/schemas/grok/message_schema.md)
- [`docs/references/schemas/pi/message_schema.md`](docs/references/schemas/pi/message_schema.md)

## Troubleshooting

### `sessions` or `messages` unexpectedly return an empty list

You may be in a directory that auto-discovered successfully but has no matching history. Try one of:

```bash
cargo run -- sessions --all
cargo run -- messages --all
cargo run -- messages --project /explicit/project/path
```

### Cursor results do not appear when filtering by a real filesystem path

The current Cursor loader matches encoded project directory names, not decoded paths, for direct `--project` lookups. Use the project name reported by:

```bash
cargo run -- --source cursor projects
```

or run `export` from inside the target project directory so the cwd encoding is handled automatically.

### You need hermetic reads during tests or scripting

Point the CLI at a fixture home instead of your real local histories:

```bash
SIMPLEMMR_HOME=/tmp/mmr-home cargo run -- projects
```

### Subprocess project arguments do not match as expected

Pass `--project` and its value as separate arguments. Avoid embedding literal shell quotes in a single argument such as `--project="/path"`.
