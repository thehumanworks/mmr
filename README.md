# mmr

`mmr` browses local Claude, Codex, and Cursor conversation history as machine-readable JSON, and can generate stateless continuity briefs from prior sessions with `remember`.

Examples below assume `mmr` is on your `PATH`. During local development, replace `mmr ...` with `cargo run -- ...`.

## What `mmr` reads

`mmr` reads transcript files from your home directory on demand:

- Codex: `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl`
- Claude: `~/.claude/projects/<project>/.../*.jsonl` (including subagent transcripts)
- Cursor: `~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl`

For tests or isolated scripting, set `SIMPLEMMR_HOME` to point at an alternate home directory.

## Build and install

This crate uses Rust edition 2024. Use a Rust/Cargo toolchain that supports edition 2024 before building.

```bash
cargo build --release
cargo install --path .
target/release/mmr --help
```

## Output model

- `stdout`: JSON only (`--pretty` pretty-prints it)
- `stderr`: hints and human-facing errors only

That split makes the CLI safe to pipe into tools such as `jq`.

## Quickstart

```bash
mmr projects
mmr sessions
mmr messages
mmr export | jq '.messages'
mmr remember
```

## Command guide

### `projects`

List known projects across the local history sources in scope.

```bash
mmr projects
mmr --source codex projects
mmr projects --limit 25 --offset 25
```

If `--source` is omitted, `mmr` searches all sources unless `MMR_DEFAULT_SOURCE` supplies a default.

### `sessions`

List sessions for a project.

```bash
mmr sessions
mmr sessions --all
mmr sessions --project /path/to/proj
mmr --source cursor sessions --project /path/to/proj
```

Key scope rules:

- With no `--project` and no `--all`, `sessions` tries to auto-discover the current project from the current working directory.
- If cwd auto-discovery fails, `sessions` falls back to searching all projects.
- If cwd auto-discovery succeeds but the discovered project has no matching history, `sessions` returns an empty result instead of widening scope.
- `--all` disables the cwd default and searches globally.

### `messages`

Query messages for a session or project.

```bash
mmr messages
mmr messages --all
mmr messages --project /path/to/proj
mmr messages --session sess-123
mmr messages --latest
mmr messages --latest 5
mmr messages --from-message-index 10 --to-message-index 20
```

Important behavior:

- `messages` uses the same cwd auto-discovery and `--all` rules as `sessions`.
- `mmr messages --session <id>` searches all projects when `--project` is omitted, even if cwd auto-discovery would otherwise scope the query. If `--source` is also omitted, the CLI prints a hint on `stderr` suggesting `--source` to narrow the search.
- `--latest` selects the latest session in the current scope and returns the newest message from that session.
- `--latest <N>` selects the latest session in the current scope and returns the newest `N` messages from that session in chronological order.
- `--from-message-index` and `--to-message-index` slice the filtered, sorted message list using zero-based indexes. `--to-message-index` is exclusive.
- Paginated responses include `next_page`, `next_offset`, and, when another page exists, `next_command` so callers can continue with the same filters.

### `export`

Export all messages for a project as the same `ApiMessagesResponse` schema returned by `messages`.

```bash
mmr export
mmr export --project /path/to/proj
mmr --source claude export --project /path/to/proj
```

When `--project` is omitted, `export` infers the current project from the current working directory:

- Codex uses the canonical cwd path as the project identifier.
- Claude and Cursor use the slash-to-hyphen form with a leading `-`.

The merged export is chronological.

### `remember`

Generate a stateless continuity brief from prior sessions.

```bash
mmr remember
mmr remember all --project /path/to/proj
mmr remember session sess-123 --project /path/to/proj
mmr remember --agent gemini --project /path/to/proj -O json
mmr remember --instructions "Return only unresolved follow-ups."
```

Behavior and backend notes:

- With no selector, `remember` uses the latest matching session.
- `remember all` includes all matching sessions.
- `remember session <id>` restricts the brief to one session.
- `--source` limits which source histories are included before the transcript is sent to the selected backend.
- `-O md` is the default output format; `-O json` returns the structured `RememberResponse`.
- When `--agent` is omitted, `MMR_DEFAULT_REMEMBER_AGENT` applies if set; otherwise the default backend is Cursor.
- Cursor defaults to model `composer-2-fast` unless `--model` overrides it.
- Gemini defaults to its built-in model unless `--model` overrides it.
- The current Codex backend uses its built-in model configuration rather than a CLI model override.
- `--instructions` replaces the default output-format-and-rules portion of the memory-agent system prompt while preserving the base agent identity and transcript input format.

## Environment variables

### Query defaults

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for `sessions` and `messages`.
- `MMR_AUTO_DISCOVER_PROJECT=1` or an unset value keeps cwd auto-discovery enabled.
- `MMR_DEFAULT_SOURCE=claude|codex|cursor` supplies the default source when `--source` is omitted.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default backend for `remember`.

Empty or invalid `MMR_DEFAULT_SOURCE` / `MMR_DEFAULT_REMEMBER_AGENT` values are treated as unset.

### Backend credentials

- Gemini: set `GOOGLE_API_KEY` or `GEMINI_API_KEY`; `GEMINI_API_BASE_URL` is optional.
- Cursor: set `CURSOR_API_KEY` and ensure the `agent` CLI is available on `PATH`.
- Codex: authenticate the Codex CLI / SDK in the environment where you run `mmr`.

## Troubleshooting

### `mmr sessions` or `mmr messages` returned nothing

- Run the same command with `--all` to bypass cwd auto-discovery.
- Pass `--project /path/to/proj` explicitly if you know the project you want.
- If you are testing against fixture data or an alternate home directory, set `SIMPLEMMR_HOME`.

### `remember` failed before contacting a backend

- Gemini errors usually mean `GOOGLE_API_KEY` / `GEMINI_API_KEY` is missing.
- Cursor errors usually mean `CURSOR_API_KEY` is missing or the `agent` executable is not on `PATH`.
- If you want to limit the transcript set before sending it to a backend, add `--source`.

### Cargo cannot parse `edition = "2024"`

Upgrade to a Rust/Cargo toolchain with edition 2024 support, then rebuild.

### Scripting note for `--project`

When invoking `mmr` from another program, pass `--project` and the path as two argv tokens:

```text
["mmr", "messages", "--project", "/path/to/proj"]
```

Do not include literal shell quotes in the project value.

## Further reading

- [Command spec: `messages`](specs/messages.md)
- [ADR-002: cwd-scoped defaults](adrs/002-cwd-scoped-defaults.md)
