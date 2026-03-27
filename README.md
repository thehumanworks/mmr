# mmr

`mmr` is a local Rust CLI for browsing AI coding history from Claude, Codex, and Cursor.

It loads transcript data from your local machine, normalizes it into one query surface, and emits machine-readable JSON on `stdout`.

## What it covers

- `projects` - aggregate project-level history across supported sources
- `sessions` - inspect sessions for a project, or across all projects
- `messages` - inspect normalized message history with pagination metadata
- `export` - emit all messages for the current project or an explicit `--project`
- `remember` - build a continuity brief from prior sessions using Cursor, Codex, or Gemini

## Local data sources

`mmr` reads local history rooted at:

- `SIMPLEMMR_HOME`, when set
- otherwise your normal `HOME` directory

Under that root, it loads:

- Codex history from `.codex/`
- Claude history from `.claude/`
- Cursor history from Cursor-exported transcript locations handled by `src/source/cursor.rs`

This tool is storage-free: it reads source files directly and builds responses in memory.

## Build and run

```bash
cargo build
cargo run -- projects
```

Install locally:

```bash
cargo install --path .
```

Pretty-print JSON when reading responses manually:

```bash
mmr --pretty projects
```

## Common commands

List projects across all sources:

```bash
mmr projects
```

Restrict to one source:

```bash
mmr --source cursor projects
```

List sessions for the current working directory's project:

```bash
mmr sessions
```

Bypass cwd auto-discovery and search globally:

```bash
mmr sessions --all
mmr messages --all
```

Fetch one session directly:

```bash
mmr --source claude messages --session sess-123
```

Export all messages for the current project:

```bash
mmr export
```

Generate a continuity brief from the latest session:

```bash
mmr remember --project /path/to/proj
```

Use all sessions and markdown output:

```bash
mmr remember all --project /path/to/proj -O md
```

## Query behavior and constraints

### Source selection

- `--source` accepts `codex`, `claude`, or `cursor`
- omitting `--source` searches all sources unless `MMR_DEFAULT_SOURCE` supplies a default
- `--source all` is not valid

### Project auto-discovery

`mmr sessions` and `mmr messages` try to infer the current project from the current working directory when:

- `--project` is omitted
- `--all` is omitted
- `MMR_AUTO_DISCOVER_PROJECT` is not `0`

If cwd discovery fails, the CLI falls back to the historical global search behavior.

If cwd discovery succeeds but there are no matching records, the command returns an empty result instead of widening scope.

### Session lookup special case

`mmr messages --session <ID>` skips cwd project auto-discovery when `--project` is not provided. Session lookup searches all projects by default, and prints this hint when `--source` is also omitted:

```text
hint: searching all sources for session; pass --source to narrow the search
```

### Message pagination metadata

`messages` responses include:

- `next_page`
- `next_offset`
- `next_command`

`next_command` is a ready-to-run CLI invocation for the next page when more results are available.

## Environment variables

### CLI defaults

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` sets the default source when `--source` is omitted
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` sets the default remember backend when `--agent` is omitted

Empty or invalid values for the defaulting variables are treated as unset.

### Backend credentials

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- optional Gemini override: `GEMINI_API_BASE_URL`
- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: use your configured Codex CLI authentication

## Troubleshooting

### `mmr` does not see my history

Check which home directory `mmr` is reading:

```bash
echo "${SIMPLEMMR_HOME:-$HOME}"
```

If your test harness or shell sets `SIMPLEMMR_HOME`, that value takes precedence over `HOME`.

### `sessions` or `messages` returned fewer results than expected

You may be in a project directory that auto-scoped the query. Retry with:

```bash
mmr sessions --all
mmr messages --all
```

Or disable auto-discovery temporarily:

```bash
MMR_AUTO_DISCOVER_PROJECT=0 mmr messages
```

### `messages --session` is slow

Pass `--source` to avoid searching every source:

```bash
mmr --source claude messages --session sess-123
```

## Developer references

- `AGENTS.md` - repository workflow, command semantics, and contributor guidance
- `adrs/002-cwd-scoped-defaults.md` - accepted decision for cwd-scoped defaults and env-driven CLI defaults
- `docs/references/session-lookup-invariants.md` - `messages --session` lookup rules
