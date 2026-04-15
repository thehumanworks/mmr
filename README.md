# mmr

`mmr` is a Rust CLI for browsing local Claude, Codex, and Cursor conversation history as stable JSON.

It loads history files from disk, builds in-memory project/session/message aggregates, and exposes read-oriented commands for querying or exporting them. The CLI keeps machine-readable results on stdout and reserves human-facing diagnostics for stderr.

## What it covers

- `projects`: summarize known projects across sources
- `sessions`: inspect sessions for a project or across all projects
- `messages`: page through message history with stable ordering metadata
- `export`: emit all messages for one project as chronological JSON
- `remember`: turn one or more past sessions into a stateless continuity brief

## Architecture at a glance

- `src/source/`: loads Claude, Codex, and Cursor JSONL history in parallel
- `src/messages/service.rs`: resolves project filters, aggregates sessions/projects, sorts, and paginates
- `src/cli.rs`: clap-based command surface, cwd project auto-discovery, export behavior, and `remember` wiring
- `src/agent/`: backend clients and prompt assembly for `remember`

The tool is storage-free: there is no database or cache layer. Every invocation reads local history, then answers queries from in-memory aggregates.

## Setup

### Requirements

- Rust toolchain with Edition 2024 support
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
- `MMR_DEFAULT_SOURCE` supplies the default `--source` when omitted.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd-based default project scoping for `sessions` and `messages`.
- `MMR_DEFAULT_REMEMBER_AGENT` supplies the default `remember --agent` when omitted.

## Command quick reference

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

By default, `sessions` tries to auto-discover the current project from the working directory. If discovery succeeds, results stay scoped to that project; if discovery fails, the command falls back to all projects and sources.

### Read messages

```bash
cargo run -- messages
cargo run -- messages --session sess-123
cargo run -- --source claude messages --project my-proj --limit 100 --offset 0
```

Important message lookup rules:

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

- Codex project lookup uses the canonical filesystem path
- Claude and Cursor project lookup use the same path transformed into a leading-hyphen name with `/` replaced by `-`

For cwd-based export, the CLI queries each matching source separately, merges the messages, and sorts them chronologically.

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
- `-O json` returns the structured `RememberResponse`
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

This avoids the cross-source session lookup hint and speeds up matching when you already know where the session lives.

### Export only the message array for scripting

```bash
cargo run -- export --project /path/to/proj | jq '.messages'
```

When invoking `mmr` from scripts, pass `--project` and the project value as separate arguments instead of embedding quotes into a single argument such as `--project="/path/to/proj"`.

## Troubleshooting and pitfalls

### I got zero sessions or messages

Check these in order:

1. Confirm the right source with `--source claude|codex|cursor`
2. If you expected cross-project results, add `--all` or an explicit `--project`
3. For `messages --session`, remember that the command already searches all projects when `--project` is omitted
4. If you changed `SIMPLEMMR_HOME`, verify that it points at the home directory containing the history files

### Export from the wrong directory returned the wrong project

`export` without `--project` is intentionally cwd-sensitive. Run it from the repository you want to inspect, or pass `--project` explicitly.

### JSON output contains no colors

That is expected. `mmr` keeps stdout machine-readable and writes colored diagnostics only to stderr.

## Verification commands

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
