# mmr

Browse local AI coding history from Claude, Codex, and Cursor as JSON.

`mmr` is a Rust CLI that reads transcript files already stored under your home directory, normalizes them into a common in-memory model, and exposes project, session, message, export, and continuity-brief workflows.

## What it covers

- `projects` - list projects discovered across supported sources
- `sessions` - list sessions, scoped to the current project by default
- `messages` - inspect message history with stable sorting and pagination metadata
- `export` - emit all messages for one project in chronological order
- `remember` - generate a stateless continuity brief from prior sessions

## Data sources

`mmr` reads local files only. It does not add a database or cache layer.

- **Codex:** `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl`
- **Claude:** `~/.claude/projects/<project>/*.jsonl` plus nested `subagents/*.jsonl`
- **Cursor:** `~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl`

For tests or scripted runs, set `SIMPLEMMR_HOME` to point discovery at a different home directory.

## Requirements

- A Rust/Cargo toolchain new enough to build a crate with `edition = "2024"`

If your default Cargo is older than Rust 2024 support, run the same commands with `cargo +stable ...`.

## Build and run

```bash
cargo run -- projects
cargo run -- sessions
cargo run -- messages --all --limit 20
```

`mmr` writes machine-readable output to `stdout`. Errors and hints are printed to `stderr`.

## Common workflows

### List projects

```bash
cargo run -- projects
cargo run -- --source cursor projects --limit 25
```

### Inspect sessions for the current project

When cwd auto-discovery succeeds, `sessions` defaults to the current project.

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- sessions --project /path/to/proj
```

### Inspect messages

```bash
cargo run -- messages
cargo run -- messages --session sess-123
cargo run -- --source claude messages --project /path/to/proj --limit 100
```

`messages` returns pagination metadata:

```json
{
  "messages": [
    {
      "session_id": "sess-123",
      "source": "cursor",
      "project_name": "-Users-test-proj",
      "role": "user",
      "content": "Summarize the last change",
      "model": "",
      "timestamp": "2025-01-07T00:01:00",
      "is_subagent": false,
      "msg_type": "user",
      "input_tokens": 0,
      "output_tokens": 0
    }
  ],
  "total_messages": 42,
  "next_page": true,
  "next_offset": 20,
  "next_command": "mmr messages --limit 20 --offset 20"
}
```

`next_command` is added by the CLI only when another page exists, so you can keep paging with the same filter and sort shape.

### Export all messages for one project

```bash
cargo run -- export
cargo run -- export --project /path/to/proj
cargo run -- --source codex export --project /path/to/proj
```

`export` always emits chronological messages. Without `--project`, it infers the current project from cwd and merges the matching Codex, Claude, and Cursor results into one `ApiMessagesResponse`.

### Generate a continuity brief

```bash
cargo run -- remember --project /path/to/proj
cargo run -- remember all --project /path/to/proj
cargo run -- remember session sess-123 --project /path/to/proj
cargo run -- remember --project /path/to/proj --agent gemini -O json
```

Backend defaults and auth:

- **Cursor** is the default backend when `--agent` is omitted (`composer-2-fast` unless `--model` is set). Requires `CURSOR_API_KEY` and the `agent` CLI on `PATH`.
- **Codex** uses existing Codex CLI auth as configured for `codex exec`.
- **Gemini** requires `GOOGLE_API_KEY` or `GEMINI_API_KEY`. `GEMINI_API_BASE_URL` is optional.

If you pass `--instructions`, `remember` preserves the base "Memory Agent" identity and input-format instructions, but replaces the default output-format section with your custom text.

## Defaults and constraints

### Source filtering

- `--source` accepts `claude`, `codex`, or `cursor`
- Omitting `--source` means all sources unless `MMR_DEFAULT_SOURCE` is set
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` can supply a default source filter

### Project scoping

- `sessions` and `messages` auto-discover the cwd project by default
- `--all` disables cwd project auto-discovery
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery globally
- If cwd discovery fails, `sessions` and `messages` fall back to all projects
- If cwd discovery succeeds but no records match, the command returns the empty result instead of widening scope

### Session lookup behavior

`mmr messages --session <ID>` behaves differently from plain `messages`:

- without `--project`, it searches across all projects
- without `--source`, it prints a stderr hint suggesting `--source`
- with `--project`, the explicit project scope still applies

### Export project resolution

When `export` runs without `--project`:

- Codex matches the canonical cwd path as-is
- Claude and Cursor match the same path with `/` replaced by `-` and a leading `-`

### Message ordering

For `messages --sort-by timestamp --order asc`, pagination is applied from the newest window and then reversed so the returned page remains chronological.

### Cursor project matching caveat

Cursor transcript ingestion currently preserves the stored project directory name under `~/.cursor/projects/` as both `project_name` and `project_path`. In practice that means direct Cursor project filters match the encoded directory name (for example `-Users-mish-proj`), not a decoded filesystem path.

## Troubleshooting and common pitfalls

- **`sessions` or `messages` returned fewer results than expected:** you may be seeing cwd scoping. Retry with `--all` or an explicit `--project`.
- **`messages --session` did not find a session quickly:** add `--source` to avoid searching every source.
- **Codex project filtering seems inconsistent:** Codex project lookup accepts either `/path/to/proj` or `path/to/proj`.
- **Cursor `--project` filtering did not match a filesystem path:** use the stored Cursor project directory name (for example `-Users-mish-proj`) or rely on `export` without `--project` to derive it from cwd.
- **A script passed `--project=\"value\"` and matching broke:** pass `--project` and the value as separate arguments.
- **Build commands fail before compilation starts:** confirm your Rust/Cargo toolchain supports Rust 2024 edition, or retry with `cargo +stable`.

## Repository guideposts

- `AGENTS.md` - contributor-oriented module map, commands, env vars, and verification workflow
- `adrs/002-cwd-scoped-defaults.md` - rationale for cwd-scoped `sessions` and `messages`
- `docs/references/session-lookup-invariants.md` - `messages --session` lookup contract
- `docs/references/schemas/cursor/message_schema.md` - Cursor transcript layout and field extraction rules
