# mmr

`mmr` is a Rust CLI for inspecting local AI coding history from Claude, Codex, and Cursor, then turning that history into machine-readable exports or continuity briefs.

## What it covers

- **Projects** aggregated from local transcript stores
- **Sessions** scoped by project, source, or explicit session ID
- **Messages** returned as JSON, with stable pagination metadata
- **Continuity briefs** generated from prior sessions through Cursor, Codex, or Gemini

## Transcript locations

`mmr` reads local files under your home directory and does not maintain its own database.

| Source | Location |
| --- | --- |
| Codex | `~/.codex/sessions/**.jsonl`, `~/.codex/archived_sessions/**.jsonl` |
| Claude | `~/.claude/projects/<project>/**/*.jsonl` |
| Cursor | `~/.cursor/projects/<project>/agent-transcripts/<session>/**/*.jsonl` |

Set `SIMPLEMMR_HOME` to point `mmr` at a different home directory, which is useful in tests or when inspecting another transcript tree.

## Quick start

```bash
cargo run -- projects
cargo run -- sessions
cargo run -- messages --limit 20
cargo run -- export
```

All read commands write machine-readable output to `stdout`.

## Command guide

### `projects`

List known projects across all loaded transcripts.

```bash
cargo run -- projects
cargo run -- --source cursor projects --limit 5
```

Response shape: `ApiProjectsResponse`

- `projects[]`: name, source, original path, session count, message count, last activity
- `total_messages`
- `total_sessions`

### `sessions`

List sessions for a project. When `--project` is omitted, `mmr` tries to auto-discover the project from the current working directory.

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- sessions --project /Users/test/codex-proj
cargo run -- --source claude sessions --project -Users-test-codex-proj
```

Important behavior:

- If cwd auto-discovery succeeds, the query is scoped to that project.
- If cwd auto-discovery fails, the command falls back to all projects.
- If cwd auto-discovery succeeds but there are no matching sessions, the result stays empty; it does not widen scope.
- `--all` disables cwd project auto-discovery.

Response shape: `ApiSessionsResponse`

- `sessions[]`: session metadata plus per-item `source`, `project_name`, `project_path`, and preview text
- `total_sessions`

### `messages`

Return messages for a project or session.

```bash
cargo run -- messages
cargo run -- messages --session sess-123
cargo run -- messages --project /Users/test/codex-proj --limit 100
cargo run -- --source cursor messages --project -Users-test-my-proj
```

Important behavior:

- Without `--project` and without `--all`, the command uses the same cwd project auto-discovery as `sessions`.
- `messages --session <id>` intentionally **bypasses** cwd auto-discovery unless `--project` is also provided.
- When `--session` is used without `--source`, `mmr` prints this hint to `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

#### Pagination contract

The `messages` command paginates from the **newest** matching messages, then returns the selected page in **chronological order**. This preserves historical CLI behavior while still making incremental fetches practical.

The response includes:

- `messages[]`
- `total_messages`
- `next_page`
- `next_offset`
- `next_command` when another page exists

Example:

```bash
cargo run -- messages --limit 50
```

Sample pagination fields:

```json
{
  "total_messages": 137,
  "next_page": true,
  "next_offset": 50,
  "next_command": "mmr messages --limit 50 --offset 50"
}
```

### `export`

Export all messages for one project as a single `ApiMessagesResponse`.

```bash
cargo run -- export
cargo run -- export --project /Users/test/proj
cargo run -- --source codex export --project /Users/test/proj
```

Behavior:

- `export --project <path>` queries directly against the provided project value.
- Bare `export` resolves the project from the current working directory:
  - **Codex** matches the canonical path, such as `/Users/test/proj`
  - **Claude** and **Cursor** match a hyphenated project name, such as `-Users-test-proj`
- Bare `export` queries each applicable source separately, merges the results, then sorts them chronologically.

### `remember`

Generate a stateless continuity brief from prior sessions for a project.

```bash
cargo run -- remember --project /Users/test/proj
cargo run -- remember all --project /Users/test/proj
cargo run -- remember session sess-123 --project /Users/test/proj
```

Backends:

- `--agent cursor`
- `--agent codex`
- `--agent gemini`

Default behavior:

- If `--agent` is omitted, `MMR_DEFAULT_REMEMBER_AGENT` is used when set.
- Otherwise the default backend is **Cursor** with model `composer-2-fast`, unless `--model` overrides it.
- `remember` outputs Markdown by default; use `-O json` for JSON output.

Examples:

```bash
cargo run -- remember --project /Users/test/proj -O json
cargo run -- remember --project /Users/test/proj --agent gemini --model gemini-3.1-flash-lite-preview
cargo run -- remember --project /Users/test/proj --instructions "Return only a bullet list of open tasks."
```

#### `--instructions` contract

The remember prompt is assembled in two parts:

1. A **base instruction** that always remains present and only establishes the Memory Agent role plus transcript input format
2. An **output instruction** that defines the expected summary format

When `--instructions <text>` is supplied, it replaces the default output instruction completely, but the base instruction stays intact. The user prompt stays neutral, so the system instruction remains the authoritative output contract.

## Source and project resolution

Project identifiers vary by source:

- **Codex** stores the project as a filesystem path
- **Claude** stores the project directory name from `~/.claude/projects`
- **Cursor** stores the project directory name from `~/.cursor/projects`

When you pass `--project` without `--source`, `mmr` resolves that value against all sources:

- For Codex, both `/Users/test/proj` and `Users/test/proj` can resolve to the same project
- For Claude and Cursor, matching is done against known project names and original paths

This is why a single `--project` argument can work across mixed-source transcript stores.

## Environment variables

| Variable | Effect |
| --- | --- |
| `SIMPLEMMR_HOME` | Read transcript data from a different home directory |
| `MMR_AUTO_DISCOVER_PROJECT=0` | Disable cwd project auto-discovery for `sessions` and `messages` |
| `MMR_DEFAULT_SOURCE=codex|claude|cursor` | Supply the default source filter when `--source` is omitted |
| `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` | Supply the default remember backend when `--agent` is omitted |
| `CURSOR_API_KEY` | Required for `remember --agent cursor` |
| `GOOGLE_API_KEY` or `GEMINI_API_KEY` | Required for `remember --agent gemini` |
| `GEMINI_API_BASE_URL` | Optional override for the Gemini Interactions API base URL |

Notes:

- Empty or invalid values for `MMR_DEFAULT_SOURCE` and `MMR_DEFAULT_REMEMBER_AGENT` are treated as unset.
- Codex authentication is handled by the local Codex CLI environment.
- Cursor remember calls require the `agent` CLI on `PATH`.

## Output contracts

`mmr` keeps `stdout` machine-readable:

- `projects` -> `ApiProjectsResponse`
- `sessions` -> `ApiSessionsResponse`
- `messages` and `export` -> `ApiMessagesResponse`
- `remember -O json` -> `RememberResponse`
- `remember` without `-O json` -> Markdown text only

Human-oriented hints and errors are written to `stderr`.

## Developer workflow

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Relevant code paths:

- `src/cli.rs` - command surface and runtime defaults
- `src/messages/service.rs` - query filtering, sorting, and pagination
- `src/source/` - source-specific transcript ingestion
- `src/agent/ai.rs` - remember prompt construction and orchestration

## Troubleshooting

### `sessions` or `messages` unexpectedly return empty output

You may be inside a directory that auto-resolves to a project with no matching history. Try one of:

```bash
cargo run -- sessions --all
cargo run -- messages --all
cargo run -- messages --project /explicit/project/path
```

### `messages --session` is slower than expected

By default it searches all projects and, if `--source` is omitted, all sources too. Add `--source` when you already know the origin:

```bash
cargo run -- --source codex messages --session sess-123
```

### A script passes `--project` but nothing matches

Pass the flag and value as separate arguments. Avoid embedding literal shell quotes inside one argument value.

Good:

```bash
mmr messages --project /Users/test/proj
```

Bad:

```bash
mmr messages '--project="/Users/test/proj"'
```
