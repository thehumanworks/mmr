# mmr

`mmr` is a local Rust CLI for browsing AI coding history from Claude, Codex, and Cursor.
It loads transcript JSONL files from your home directory, normalizes them in memory, and
returns machine-readable output for projects, sessions, messages, exports, and continuity
brief generation.

## What it does

- Reads local history from:
  - `~/.codex/sessions` and `~/.codex/archived_sessions`
  - `~/.claude/projects`
  - `~/.cursor/projects`
- Aggregates those transcripts in memory via `src/messages/service.rs`
- Emits stable JSON for `projects`, `sessions`, `messages`, and `export`
- Sends selected transcripts to Cursor, Codex, or Gemini for `remember`

`stdout` is reserved for JSON or markdown output. Human-facing hints and errors belong on
`stderr`.

## Architecture at a glance

- `src/cli.rs` - clap command surface, cwd scoping rules, and output formatting
- `src/source/` - source-specific JSONL loaders for Codex, Claude, and Cursor
- `src/messages/service.rs` - filtering, sorting, pagination, and project resolution
- `src/types/` - response contracts and sort/source enums
- `src/agent/` - `remember` backends and transcript-to-prompt orchestration

## Quick start

Build and run with a Rust toolchain that supports Edition 2024.
If your default Cargo is too old, use `cargo +stable ...`.

```bash
cargo run -- projects --pretty
cargo run -- sessions
cargo run -- messages --limit 20
cargo run -- export --project /path/to/proj
cargo run -- remember --project /path/to/proj
```

## Common workflows

### Inspect the current project's history

```bash
cargo run -- sessions
cargo run -- messages
```

By default, `sessions` and `messages` scope to the auto-discovered cwd project. If cwd
discovery fails, the CLI falls back to all projects and sources.

### Search everything instead of the cwd project

```bash
cargo run -- sessions --all
cargo run -- messages --all
MMR_AUTO_DISCOVER_PROJECT=0 cargo run -- messages
```

### Look up one session directly

```bash
cargo run -- messages --session sess-123
```

When `--session` is provided without `--project`, `mmr` intentionally searches all
projects instead of using cwd auto-discovery. If `--source` is also omitted, the CLI prints
this narrowing hint to `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

### Export full message history for a project

```bash
cargo run -- export
cargo run -- export --project /path/to/proj
```

`export` always returns `ApiMessagesResponse`. Without `--project`, it derives the project
from cwd: Codex uses the canonical path, while Claude and Cursor use the slash-to-hyphen
project identifier.

### Generate a continuity brief

```bash
cargo run -- remember --project /path/to/proj
cargo run -- remember all --project /path/to/proj
cargo run -- remember session sess-123 --project /path/to/proj
cargo run -- remember -O json --project /path/to/proj
```

`remember` defaults to Cursor and returns markdown by default. Use `-O json` for
machine-readable output.

## Environment variables

- `MMR_AUTO_DISCOVER_PROJECT=0` - disable cwd project auto-discovery for `sessions` and
  `messages`
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` - supply the default source when `--source` is
  omitted
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` - supply the default `remember --agent`
- `SIMPLEMMR_HOME=/tmp/mmr-home` - override the home directory used for transcript discovery

## Troubleshooting and common pitfalls

- **Expected global history but got an empty result:** the cwd project may have been
  auto-discovered successfully. Retry with `--all`.
- **Script needs JSON from `remember`:** pass `-O json`; markdown is the default output
  format.
- **Need the next page of `messages`:** consume `next_page` / `next_offset`, or run the
  suggested `next_command` when it is present.
- **Passing `--project` from another program:** provide `--project` and the value as separate
  argv entries so the value is not quoted literally.
- **No history is being found:** confirm the local transcript directories exist, or point
  `SIMPLEMMR_HOME` at a fixture/home directory that contains `.codex`, `.claude`, or
  `.cursor` data.
- **`remember` auth/setup issues:** Cursor needs `CURSOR_API_KEY` and the `agent` CLI on
  `PATH`; Gemini needs `GOOGLE_API_KEY` or `GEMINI_API_KEY`; Codex uses the local Codex CLI
  authentication flow.

## More documentation

- `AGENTS.md` - detailed developer guide and CLI contract notes
- `docs/references/session-lookup-invariants.md` - `messages --session` behavior contract
- `docs/references/schemas/` - raw transcript schema notes for Codex and Claude
- `adrs/002-cwd-scoped-defaults.md` - cwd scoping and env-defaults decision record
