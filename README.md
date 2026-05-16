# mmr

`mmr` is a Rust CLI for reading local AI coding history from Claude, Codex, Cursor, and Pi and exposing it through a small JSON-first command surface.

## What it reads

`mmr` loads transcript data from the current home directory, or from `SIMPLEMMR_HOME` when that override is set.

- Codex: `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl`
- Claude: `~/.claude/projects/<encoded-project>/*.jsonl` plus nested `subagents/*.jsonl`
- Cursor: `~/.cursor/projects/<encoded-project>/agent-transcripts/<session>/*.jsonl`
- Pi: `~/.pi/agent/sessions/**/*.jsonl`

The tool keeps everything in memory. There is no database, cache layer, or index to build.

## Quick start

`Cargo.toml` uses Rust edition 2024. If your default toolchain is older, update stable and run the commands below with `cargo +stable`.

```bash
cargo +stable build
cargo +stable run -- projects --limit 5
cargo +stable run -- sessions
cargo +stable run -- messages --all --limit 10
```

Most commands write JSON to stdout. `remember` is the exception: it defaults to markdown output unless you pass `-O json`.

## Core commands

### `projects`

List known projects across one or more sources.

```bash
cargo +stable run -- projects
cargo +stable run -- --source codex projects --sort-by message-count --limit 20
```

### `sessions`

List sessions. When cwd auto-discovery succeeds, `sessions` scopes to the current project by default.

```bash
cargo +stable run -- sessions
cargo +stable run -- sessions --all
cargo +stable run -- --source codex sessions --project /Users/test/proj
```

### `messages`

List messages with pagination, latest-session selection, and message-index slicing.

```bash
cargo +stable run -- messages
cargo +stable run -- messages --latest
cargo +stable run -- messages --latest 5
cargo +stable run -- messages --session sess-123 --source claude
cargo +stable run -- --source codex messages --project /Users/test/proj --from-message-index 10 --to-message-index 20
```

### `export`

Return all messages for one project in ascending timestamp order.

```bash
cargo +stable run -- export
cargo +stable run -- export --project /Users/test/proj
cargo +stable run -- --source codex export --project /Users/test/proj
```

### `remember`

Generate a continuity brief from prior sessions.

```bash
cargo +stable run -- remember --project /Users/test/proj
cargo +stable run -- remember all --project /Users/test/proj -O json
cargo +stable run -- remember session sess-123 --project /Users/test/proj --agent gemini
```

## Scope resolution and project matching

### `sessions` and `messages`

These commands resolve project scope in this order:

1. `--project <value>` uses the explicit project value.
2. `--all` disables cwd project auto-discovery and searches across projects.
3. Otherwise, the CLI tries to auto-discover the current project from the working directory.

Two edge cases matter:

- If cwd auto-discovery fails, the command falls back to all projects and all matching sources.
- If cwd auto-discovery succeeds but there are no matching records for that project, the command returns an empty result instead of falling back globally.

`MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for `sessions` and `messages`. Unset, empty, or `1` keeps the default behavior.

### `messages --session`

`mmr messages --session <ID>` behaves differently when `--project` is omitted:

- it searches across all projects instead of using cwd auto-discovery
- it still honors `--source`
- when `--source` is omitted, it prints a stderr hint suggesting `--source` for a narrower search

### `export` project matching

`mmr export` without `--project` derives the current project from the working directory:

- Codex and Pi match the canonical filesystem path directly, for example `/Users/mish/proj`
- Claude and Cursor match the same path encoded with slashes replaced by `-` and a leading `-`, for example `-Users-mish-proj`

## Messages pagination and slicing

`messages` defaults to `--sort-by timestamp --order asc`, but the pagination contract is intentionally unusual:

- the query first selects the newest window using `limit` and `offset`
- that window is then returned in chronological order

This preserves the historical behavior expected by scripts and tests.

`ApiMessagesResponse` includes:

- `messages`
- `total_messages`
- `next_page`
- `next_offset`
- `next_command` when another page exists

`next_command` is a copy-pasteable follow-up invocation. It preserves the active scope flags and any non-default sort settings.

### `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns the latest message from that session.

`mmr messages --latest <N>` returns the latest `N` messages from that latest session, still ordered chronologically. `--latest` by itself defaults to `1`.

### `--from-message-index` and `--to-message-index`

These flags apply after source, project, session, sort, and latest-session selection:

- `--from-message-index <N>` is inclusive
- `--to-message-index <N>` is exclusive

`total_messages` still reports the full scoped count before the message-index range is applied.

## Remember backends and output

`remember` supports three backends:

- `--agent cursor`
- `--agent codex`
- `--agent gemini`

If `--agent` is omitted, the CLI uses `MMR_DEFAULT_REMEMBER_AGENT` when it is set to `cursor`, `codex`, or `gemini`. Otherwise it defaults to Cursor with model `composer-2-fast`, unless `--model` overrides that model selection.

Selection works like this:

- `mmr remember` uses the latest matching session
- `mmr remember all` uses all matching sessions
- `mmr remember session <ID>` uses exactly one session

`--source` filters which transcripts are included before the brief is generated.

### Output formats

- `-O md` or `--output-format md` is the default and prints the brief as plain markdown text
- `-O json` returns a JSON object with `agent` and `text`

Markdown output is trimmed and does not include resumability IDs. JSON output also omits interaction or thread IDs.

### `--instructions`

The system prompt used by `remember` has two parts:

1. a base instruction that establishes the Memory Agent identity and the transcript input format
2. an output instruction that defines the default continuity-brief format

Passing `--instructions "<text>"` replaces the entire output-instruction section, but the base instruction remains in place.

### Backend requirements

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL`
- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: Codex CLI auth available to `codex exec`

## Environment variables

- `SIMPLEMMR_HOME` - override the home directory used for transcript discovery
- `MMR_AUTO_DISCOVER_PROJECT` - set to `0` to disable cwd auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE` - set the default source filter to `claude`, `codex`, `cursor`, or `pi`
- `MMR_DEFAULT_REMEMBER_AGENT` - set the default `remember --agent` to `cursor`, `codex`, or `gemini`
- `GOOGLE_API_KEY` / `GEMINI_API_KEY` - Gemini auth
- `GEMINI_API_BASE_URL` - alternate Gemini Interactions API base URL
- `CURSOR_API_KEY` - Cursor agent auth

## Common pitfalls

- `--source all` is not a valid value. Omit `--source` to query all sources.
- `remember` defaults to markdown, not JSON. Use `-O json` for scripting.
- In subprocess calls, pass `--project` and its value as separate arguments instead of embedding quotes in a single argument.
- Legacy `remember` flags such as `--mode`, `--session-id`, `--continue-from`, and `--follow-up` are rejected by the current CLI.

## Related docs

- `specs/messages.md` - canonical `messages` behavior contract
- `specs/remember.md` - canonical `remember` behavior contract
- `docs/references/session-lookup-invariants.md` - `messages --session` scope rules
- `AGENTS.md` - contributor-oriented repository guidance
