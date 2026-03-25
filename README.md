# mmr

`mmr` is a Rust CLI for browsing local AI coding history from Claude, Codex, and Cursor.

It reads each tool's on-disk transcripts, normalizes them into stable JSON responses, and
provides query commands for projects, sessions, messages, full-project exports, and
"remember" continuity briefs.

## What it reads

`mmr` loads history from the current user's home directory, or from `SIMPLEMMR_HOME` when
that environment variable is set.

| Source | Files and directories |
| --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl`, `~/.codex/archived_sessions/**/*.jsonl` |
| Claude | `~/.claude/projects/*/*.jsonl` and `~/.claude/projects/*/*/subagents/*.jsonl` |
| Cursor | `~/.cursor/projects/*/agent-transcripts/*/*.jsonl` |

Parsing is defensive: malformed JSONL lines are skipped so valid records from the same file
are still ingested.

## Quick start

This crate uses Rust edition 2024. Use a Rust toolchain that supports edition 2024 before
building or testing.

```bash
cargo build
cargo run -- projects
cargo run -- sessions
cargo run -- messages
```

`mmr` writes machine-readable results to `stdout` as JSON. Human-facing diagnostics and hints
go to `stderr`.

## Core commands

### List projects

```bash
cargo run -- projects
cargo run -- --source codex projects
cargo run -- --source cursor projects --limit 25
```

Returns `ApiProjectsResponse` with project-level counts and last activity metadata.

### List sessions

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- sessions --project /path/to/project
cargo run -- --source claude sessions --project -Users-me-proj
```

Without `--project` and without `--all`, `sessions` tries to auto-discover the current
working directory as the project scope. If discovery fails, it falls back to all projects.

### List messages

```bash
cargo run -- messages
cargo run -- messages --session sess-123
cargo run -- messages --project /path/to/project --limit 100
cargo run -- --source cursor messages --project -Users-me-proj
```

Returns `ApiMessagesResponse`:

- `messages`
- `total_messages`
- `next_page`
- `next_offset`
- `next_command` when another page is available

Important behavior:

- Message pagination selects the newest matching window first, then returns that page in
  chronological order.
- `mmr messages --session <id>` bypasses cwd project auto-discovery when `--project` is not
  provided and searches across all projects instead.

See [`docs/references/session-lookup-invariants.md`](docs/references/session-lookup-invariants.md)
for the full `--session` contract and related tests.

### Export a project's full transcript

```bash
cargo run -- export
cargo run -- export --project /path/to/project
cargo run -- --source codex export --project /path/to/project
```

`export` returns the same `ApiMessagesResponse` shape as `messages`, but with the full
matching transcript in ascending timestamp order.

When `--project` is omitted, `export` infers the project from the current working directory:

- Codex matches the canonical filesystem path directly.
- Claude and Cursor match the same path encoded with `/` replaced by `-` and a leading `-`.

### Generate a continuity brief

```bash
cargo run -- remember --project /path/to/project
cargo run -- remember all --project /path/to/project
cargo run -- remember session sess-123 --project /path/to/project
cargo run -- remember --agent gemini --instructions "Return only bullet points."
```

`remember` loads prior session transcripts and sends them to the selected backend:

- `cursor`
- `codex`
- `gemini`

By default, `remember` returns Markdown. Use `-O json` for structured JSON output.

## Environment variables

| Variable | Effect |
| --- | --- |
| `SIMPLEMMR_HOME` | Override the home directory that `mmr` scans for local history files |
| `MMR_AUTO_DISCOVER_PROJECT=0` | Disable cwd project auto-discovery for `sessions` and `messages` |
| `MMR_DEFAULT_SOURCE=codex\|claude\|cursor` | Supply the default `--source` when the flag is omitted |
| `MMR_DEFAULT_REMEMBER_AGENT=cursor\|codex\|gemini` | Supply the default `remember --agent` value |
| `GEMINI_API_KEY` / `GOOGLE_API_KEY` | Credentials for the Gemini backend |
| `GEMINI_API_BASE_URL` | Optional Gemini API base URL override |
| `CURSOR_API_KEY` | Credentials for the Cursor backend |

## Troubleshooting and common pitfalls

### `sessions` or `messages` return fewer results than expected

By default those commands scope to the current working directory when project auto-discovery
succeeds. Use one of these forms to widen the search:

```bash
cargo run -- sessions --all
cargo run -- messages --all
cargo run -- messages --project /path/to/project
```

### `messages --session <id>` prints a hint on stderr

That hint means `mmr` searched all sources for the session ID because no explicit `--source`
was provided. Add `--source codex`, `--source claude`, or `--source cursor` to narrow the
lookup.

### `cargo` fails before building

The crate uses Rust edition 2024. If your installed Cargo or Rust toolchain is older, update
Rust before running the verification commands.

### A scripted call cannot find the project you passed

When calling `mmr` from another process, pass `--project` and the project value as separate
arguments. Avoid wrapping the value into a single shell-quoted token such as
`--project=\"/path/to/project\"`, which can pass the quotes literally and break matching.

### A source appears empty

`mmr` only reads the local transcript directories listed above. If a tool has no history in
those locations, that source returns no records rather than failing the whole query.

## Repository docs map

- [`AGENTS.md`](AGENTS.md) - maintainer and contributor reference, including command coverage
  and CLI contract notes
- [`docs/references/session-lookup-invariants.md`](docs/references/session-lookup-invariants.md)
  - exact `messages --session` scoping behavior
- [`docs/references/schemas/`](docs/references/schemas/) - source-specific transcript schema
  notes
- [`adrs/`](adrs/) - architecture decisions and behavior rationale
