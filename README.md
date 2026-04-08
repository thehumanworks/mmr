# mmr

`mmr` is a Rust CLI for browsing local Claude, Codex, and Cursor conversation history.

It is designed for two main workflows:

- inspect normalized projects, sessions, and messages as JSON
- generate a continuity brief from prior sessions with `remember`

## Requirements

- A Rust toolchain with Edition 2024 support
- Local Claude, Codex, and/or Cursor history already present on disk
- For `remember`, authentication for the backend you choose:
  - Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`
  - Codex: Codex CLI auth as configured for `codex exec`
  - Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`

## Build and install

Build a local binary:

```bash
cargo build --release
./target/release/mmr projects
```

Install into your cargo bin directory:

```bash
cargo install --path .
mmr projects
```

## Global flags

- `--source <claude|codex|cursor>` filters commands to one source
- `--pretty` pretty-prints JSON on stdout without changing the schema

If `--source` is omitted, `mmr` searches all sources unless `MMR_DEFAULT_SOURCE` supplies a default.

## Command overview

### `projects`

List projects across the selected source set.

```bash
mmr projects
mmr --source cursor projects --limit 20
mmr projects --sort-by message-count --order desc
```

Defaults:

- `--limit 10`
- `--offset 0`
- `--sort-by timestamp`
- `--order desc`

### `sessions`

List sessions for a project. When `--project` is omitted, `mmr sessions` auto-discovers the current working directory and scopes the query to that project by default.

```bash
mmr sessions
mmr sessions --all
mmr --source codex sessions --project /Users/test/codex-proj
mmr sessions --sort-by message-count --order desc
```

Defaults:

- `--limit 20`
- `--offset 0`
- `--sort-by timestamp`
- `--order desc`

Notes:

- `--all` disables cwd-based project auto-discovery.
- If cwd discovery fails, `sessions` falls back to the historical global search.
- If cwd discovery succeeds but the project has no history, the response is empty instead of silently widening scope.

### `messages`

List normalized messages for a session or project.

```bash
mmr messages
mmr messages --all --limit 100
mmr messages --session sess-123
mmr --pretty messages --project /Users/test/codex-proj --limit 20
mmr messages --all --sort-by message-count --order desc
```

Defaults:

- `--limit 50`
- `--offset 0`
- `--sort-by timestamp`
- `--order asc`

Important behavior:

- Like `sessions`, `messages` auto-discovers the cwd project by default unless `--project` or `--all` is provided.
- `messages --session <id>` without `--project` searches across all projects instead of applying cwd scoping.
- When `--session` is provided without `--source`, `mmr` prints a stderr hint suggesting `--source` for a narrower lookup.
- With the default `--sort-by timestamp --order asc`, pagination is applied from the newest window first, then the returned page is reversed back into chronological order.
- With `--sort-by message-count`, messages are ordered by their parent session's total message count, then by message chronology as a tie-breaker.

### `export`

Export all matching messages in chronological order using the same JSON envelope as `messages`.

```bash
mmr export
mmr export --project /Users/test/codex-proj
mmr --source cursor export
```

Notes:

- `export` without `--project` resolves the current directory into source-specific project identifiers:
  - Codex uses the canonical filesystem path
  - Claude and Cursor use the same path encoded with `/` replaced by `-` and a leading `-`
- `export` always sorts by ascending timestamp and returns the full match set for the selected scope.

### `remember`

Generate a stateless continuity brief from prior sessions.

```bash
mmr remember --project /Users/test/proj
mmr remember all --project /Users/test/proj
mmr remember session sess-123 --project /Users/test/proj
mmr remember --project /Users/test/proj -O json
mmr remember --project /Users/test/proj --agent gemini --model gemini-2.5-pro
mmr remember --project /Users/test/proj --instructions "Return only a keyword."
```

Defaults:

- project: current working directory
- session selection: latest matching session
- agent: `cursor` unless `MMR_DEFAULT_REMEMBER_AGENT` is set
- output format: markdown (`-O json` for structured output)

`--instructions` replaces the default output-format-and-rules portion of the system prompt, while preserving the base Memory Agent identity and input-format instructions.

## JSON output contract

`projects`, `sessions`, `messages`, and `export` write machine-readable JSON to stdout. Human-facing hints and errors go to stderr.

`messages` and `export` return this envelope:

- `messages`: array of normalized messages
- `total_messages`: total count before pagination
- `next_page`: `true` when another page exists
- `next_offset`: offset to pass on the next call
- `next_command`: suggested follow-up command string, present only when another page exists

Example:

```json
{
  "messages": [],
  "total_messages": 0,
  "next_page": false,
  "next_offset": 0,
  "next_command": null
}
```

`next_command` mirrors the active paging, source, and sorting flags so scripts and shell users can continue paging without reconstructing the command manually.

## Environment variables

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` supplies the default source when `--source` is omitted
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default `remember --agent` value
- `GEMINI_API_BASE_URL` optionally overrides the Gemini API base URL

Empty or invalid values for `MMR_DEFAULT_SOURCE` and `MMR_DEFAULT_REMEMBER_AGENT` are treated as unset.

## Troubleshooting and common pitfalls

### `sessions` or `messages` returned fewer results than expected

You may be seeing cwd-based project scoping. Retry with one of:

```bash
mmr sessions --all
mmr messages --all
MMR_AUTO_DISCOVER_PROJECT=0 mmr messages
```

### I know the session ID, but the project is different from my current directory

Use `messages --session <id>`. Without `--project`, it searches all projects by design.

Add `--source` when you know the source to reduce lookup noise:

```bash
mmr --source codex messages --session sess-123
```

### I need machine-readable output from `remember`

`remember` defaults to markdown. Use JSON explicitly:

```bash
mmr remember --project /Users/test/proj -O json
```

### My script passes `--project`, but matching fails

When invoking `mmr` from another program, pass `--project` and the value as separate arguments rather than embedding quotes inside one argument.

Good:

```text
["mmr", "messages", "--project", "/Users/test/proj"]
```

Bad:

```text
["mmr", "messages", "--project=\"/Users/test/proj\""]
```

## Additional contributor reference

See `AGENTS.md` for repository workflow, verification commands, and deeper contributor guidance.
