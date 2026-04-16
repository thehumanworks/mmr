# mmr

`mmr` is a Rust CLI for browsing local Claude, Codex, and Cursor conversation history as stable JSON.

Each invocation reads local history files, builds in-memory aggregates, and returns machine-readable results on stdout. Human-facing diagnostics stay on stderr.

## What it covers

- `projects` - summarize known projects across sources
- `sessions` - inspect sessions for one project or across all projects
- `messages` - page through message history with stable ordering metadata
- `export` - emit all messages for one project as chronological JSON
- `remember` - turn one or more past sessions into a stateless continuity brief

## Architecture at a glance

- `src/source/` - loads Claude, Codex, and Cursor JSONL history in parallel
- `src/messages/service.rs` - resolves project filters, aggregates sessions/projects, sorts, and paginates
- `src/cli.rs` - clap-based command surface, cwd project auto-discovery, export behavior, and `remember` wiring
- `src/agent/` - backend clients and prompt assembly for `remember`

The tool is storage-free: there is no database or cache layer.

## History locations and project identity

| Source | History location | Project identity used by mmr |
| --- | --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl` | Canonical filesystem path from `session_meta.payload.cwd` |
| Claude | `~/.claude/projects/<project_name>/**/*.jsonl` | Directory name under `projects/`; when `cwd` exists in the file, mmr also preserves that filesystem path as `project_path` |
| Cursor | `~/.cursor/projects/<project_name>/agent-transcripts/<session_id>/*.jsonl` | Directory name under `projects/` (for example `-Users-me-proj`) |

That last row matters for filtering: Cursor project matching is currently based on the stored project directory name, not a decoded filesystem path.

## Setup

### Requirements

- A recent Rust toolchain with Edition 2024 support
- Access to the local history files you want to inspect

### Build

```bash
cargo build
```

### Optional environment variables

```bash
export SIMPLEMMR_HOME="$HOME"
export MMR_DEFAULT_SOURCE=cursor
export MMR_AUTO_DISCOVER_PROJECT=1
export MMR_DEFAULT_REMEMBER_AGENT=gemini
```

- `SIMPLEMMR_HOME` overrides the home directory used to discover Claude, Codex, and Cursor history.
- `MMR_DEFAULT_SOURCE=claude|codex|cursor` supplies the default `--source` when omitted.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd-based default project scoping for `sessions` and `messages`.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default `remember --agent` when omitted.

Empty or invalid values for the `MMR_DEFAULT_*` settings are treated as unset.

## Command quick reference

Across the read-oriented commands, `--source` accepts only `claude`, `codex`, or `cursor`. Omitting it means all sources unless `MMR_DEFAULT_SOURCE` sets a default.

### List projects

```bash
cargo run -- projects
cargo run -- --source cursor projects --limit 25 --offset 0
```

Returns `ApiProjectsResponse` JSON with project totals and last activity.

### List sessions

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- --source codex sessions --project /Users/test/codex-proj
```

Default behavior:

- Without `--project` and without `--all`, `sessions` auto-discovers the current project from the working directory.
- If cwd discovery succeeds, the command stays scoped to that project.
- If cwd discovery fails, the command falls back to all projects and sources.
- `--all` disables only the cwd project default; source filtering still applies.

### Read messages

```bash
cargo run -- messages
cargo run -- messages --session sess-123
cargo run -- --source claude messages --project my-proj --limit 100 --offset 0
```

Important lookup and pagination rules:

- Default sort is `--sort-by timestamp --order asc`.
- Pagination is based on the newest window first, then the returned page is reordered into chronological output.
- `messages --session <id>` skips cwd project auto-discovery when `--project` is omitted and searches all projects instead.
- When `--session` is used without `--source`, the CLI prints a hint on stderr suggesting `--source` to narrow the search.
- When more results are available, the response includes `next_page`, `next_offset`, and a ready-to-run `next_command`.

### Export a project transcript

```bash
cargo run -- export
cargo run -- export --project /path/to/proj
cargo run -- --source cursor export --project /path/to/proj
```

`export` always returns `ApiMessagesResponse` JSON.

Without `--project`, `mmr` infers the current project from the working directory:

- Codex lookup uses the canonical filesystem path
- Claude and Cursor lookup use the same path transformed into a leading-hyphen name with `/` replaced by `-`

For cwd-based export, the CLI queries each matching source separately, merges the results, and sorts them chronologically.

### Generate a continuity brief

```bash
cargo run -- remember --project /path/to/proj
cargo run -- remember all --project /path/to/proj
cargo run -- remember session sess-123 --project /path/to/proj
cargo run -- remember --project /path/to/proj --agent gemini -O json
cargo run -- remember --project /path/to/proj --instructions "Return only three bullets."
```

`remember` supports `cursor`, `codex`, and `gemini` backends.

- Default output format is Markdown (`-O md`)
- If `--agent` is omitted, the default is `MMR_DEFAULT_REMEMBER_AGENT` or Cursor (`composer-2-fast`)
- `-O json` returns a structured `RememberResponse`
- `--instructions` replaces the default output-formatting and rules section of the system prompt while preserving the base Memory Agent identity and transcript input format

Backend requirements:

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL`
- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: working Codex CLI authentication for `codex exec`

## Usage examples

### Inspect the current project across all sources

```bash
cargo run -- sessions
cargo run -- messages --limit 20
```

### Narrow a known session to one source

```bash
cargo run -- --source cursor messages --session sess-123
```

This avoids the cross-source session lookup hint and makes the lookup more targeted.

### Export only the message array for scripting

```bash
cargo run -- export --project /path/to/proj | jq '.messages'
```

## Troubleshooting and common pitfalls

### I got zero sessions or messages

Check these in order:

1. Confirm the right source with `--source claude|codex|cursor`
2. If you expected cross-project results, add `--all` or an explicit `--project`
3. For `messages --session`, remember that the command already searches all projects when `--project` is omitted
4. If you changed `SIMPLEMMR_HOME`, verify that it points at the home directory containing the history files

### Cursor project filters do not match my filesystem path

Current Cursor project matching uses the stored directory name under `~/.cursor/projects/`, such as `-Users-me-proj`.

- `export` without `--project` handles that encoding automatically from the current directory.
- For direct filtering such as `--source cursor sessions --project ...`, pass the encoded project name that exists on disk under `~/.cursor/projects/`.

### My script is not matching `--project` correctly

When invoking `mmr` from scripts, pass `--project` and the project value as separate arguments. Avoid embedding quotes into a single argument such as `--project=\"/path/to/proj\"`.

### JSON output contains no colors

That is expected. `mmr` keeps stdout machine-readable and writes colored diagnostics only to stderr.

## Additional references

- `docs/references/session-lookup-invariants.md` - `messages --session` scoping rules
- `docs/references/schemas/codex/message_schema.md` - Codex JSONL layout
- `docs/references/schemas/claude/message_schema.md` - Claude JSONL layout
- `docs/references/schemas/cursor/message_schema.md` - Cursor JSONL layout

## Verification commands

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
