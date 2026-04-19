# mmr

`mmr` is a Rust CLI for browsing local Claude, Codex, and Cursor conversation history as machine-readable JSON.

## What it covers

- Lists projects, sessions, and messages across supported local history sources.
- Auto-discovers the current project for `sessions`, `messages`, `export`, and `remember`.
- Exports chronological message history for scripts and downstream tooling.
- Generates stateless continuity briefs from prior sessions via Cursor, Codex, or Gemini backends.

## Supported local history sources

`mmr` reads local transcript files directly from the current user's home directory:

- **Codex**: `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl`
- **Claude**: `~/.claude/projects/<project>/**/*.jsonl`
- **Cursor**: `~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl`

The CLI keeps JSON output on `stdout`. Human-facing diagnostics and hints go to `stderr`.

## Build and run

```bash
cargo run -- --help
cargo run -- projects
```

If your default Cargo is too old to understand `edition = "2024"`, use the stable toolchain explicitly:

```bash
cargo +stable run -- --help
```

## Common workflows

### Inspect available projects

```bash
cargo run -- projects
cargo run -- --source cursor projects
```

### List sessions for the current project

When you run `mmr` from inside a project directory, `sessions` and `messages` auto-discover that cwd as the default project scope.

```bash
cargo run -- sessions
cargo run -- messages --limit 20
```

Use `--all` to bypass cwd scoping:

```bash
cargo run -- sessions --all
cargo run -- messages --all --limit 100
```

### Look up one session directly

```bash
cargo run -- messages --session sess-123
cargo run -- --source codex messages --session sess-123
```

When `--session` is provided without `--project`, `mmr` searches all projects instead of applying cwd auto-discovery. If `--source` is also omitted, the CLI prints a narrowing hint on `stderr`.

### Export messages for scripts

`export` always returns `ApiMessagesResponse`, sorted chronologically.

```bash
cargo run -- export
cargo run -- --source claude export --project /Users/test/proj
```

If you need only the message array:

```bash
cargo run -- export | jq '.messages'
```

### Generate a continuity brief

```bash
cargo run -- remember --project /Users/test/proj
cargo run -- remember all --project /Users/test/proj
cargo run -- remember session sess-123 --project /Users/test/proj
```

By default, `remember` uses the Cursor backend with model `composer-2-fast` unless `--agent` or `--model` changes that behavior.

## Output shape

The primary read commands return stable JSON response envelopes from `src/types/api.rs`:

- `projects` -> `ApiProjectsResponse`
- `sessions` -> `ApiSessionsResponse`
- `messages` and `export` -> `ApiMessagesResponse`
- `remember -O json` -> `RememberResponse`

`ApiMessage` and `ApiSession` items always include per-item `source` and `project_name` metadata.

`messages` pagination is intentionally agent-friendly:

- the query window is selected from newest messages first
- the returned `messages` array remains chronological
- when another page exists, `next_page`, `next_offset`, and `next_command` are populated

## Project resolution and source caveats

`mmr export` resolves the current working directory differently per source:

- **Codex** matches the canonical filesystem path directly, such as `/Users/mish/proj`
- **Claude** and **Cursor** use the same path encoded as a directory name, such as `-Users-mish-proj`

Direct `--project` filters compare against both stored `project_name` and `project_path`, but source data is not normalized identically:

- **Codex** reliably matches canonical paths
- **Claude** can match canonical paths when transcript records include `cwd`
- **Cursor** currently matches the encoded project directory name stored under `~/.cursor/projects/`

That means cwd-based `export`, `sessions`, and `messages` are usually the most reliable cross-source workflow, while explicit `--project /path/to/proj` filters can behave differently for Cursor history.

## Environment variables

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` supplies the default `--source`
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default `remember --agent`
- `GOOGLE_API_KEY` or `GEMINI_API_KEY` authenticates Gemini
- `GEMINI_API_BASE_URL` overrides the Gemini API base URL
- `CURSOR_API_KEY` authenticates Cursor backend requests

The Codex backend uses the existing Codex CLI authentication available to `codex-app-server-sdk`.

## Troubleshooting

### `sessions` or `messages` unexpectedly return nothing

- Check whether cwd auto-discovery scoped the command to the current directory.
- Retry with `--all` to search across every project.
- For direct session lookups, add `--source` to avoid searching all sources unnecessarily.

### `--project /path/to/proj` works for one source but not another

Project identifiers differ by source. Codex stores canonical paths, while Claude and Cursor are often keyed by encoded directory names. If you are investigating Cursor data, compare against the encoded directory under `~/.cursor/projects/`.

### `remember` fails immediately

Verify the selected backend's auth and environment:

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- Cursor: `CURSOR_API_KEY` and `agent` on `PATH`
- Codex: working Codex CLI auth for the local environment

## Deeper references

- `AGENTS.md` - contributor and maintenance guide
- `docs/references/session-lookup-invariants.md` - `messages --session` lookup contract
- `docs/references/schemas/codex/message_schema.md` - Codex transcript schema
- `docs/references/schemas/claude/message_schema.md` - Claude transcript schema
- `docs/references/schemas/cursor/message_schema.md` - Cursor transcript schema
