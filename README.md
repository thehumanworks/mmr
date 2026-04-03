# mmr

`mmr` is a Rust CLI for browsing local AI coding history from Claude, Codex, and Cursor.

It loads transcript files from your local machine, normalizes them into shared JSON response shapes, and lets you inspect projects, sessions, messages, exports, and continuity briefs from the command line.

## What it covers

- **Projects**: list projects seen across supported sources
- **Sessions**: inspect conversation sessions, usually scoped to the current working directory
- **Messages**: inspect message history for a project or a specific session
- **Export**: emit a project's full chronological message history
- **Remember**: generate a resumable continuity brief from prior sessions

## Supported local sources

`mmr` reads local transcript files under `HOME`:

- **Codex**: `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl`
- **Claude**: `~/.claude/projects/**/*.jsonl`
- **Cursor**: `~/.cursor/projects/*/agent-transcripts/*/*.jsonl`

The CLI parses these files defensively: malformed lines are skipped so one bad record does not block the rest of the ingest.

## Build and run

This crate uses Rust edition 2024, so use a toolchain new enough to support it.

```bash
cargo build
cargo run -- projects
```

For development verification:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

## Quick start

List all detected projects:

```bash
cargo run -- projects
```

List only Cursor-backed projects:

```bash
cargo run -- --source cursor projects
```

List sessions for the current working directory's project:

```bash
cargo run -- sessions
```

List sessions across every project instead of the auto-discovered cwd project:

```bash
cargo run -- sessions --all
```

List messages for one session ID:

```bash
cargo run -- messages --session sess-123
```

Export a project's full history:

```bash
cargo run -- export --project /path/to/project
```

Generate a continuity brief from the latest matching session:

```bash
cargo run -- remember --project /path/to/project
```

Generate a continuity brief from all matching sessions as JSON:

```bash
cargo run -- remember all --project /path/to/project -O json
```

## Common workflows

### Explore the current project's history

When `--project` is omitted, `sessions` and `messages` try to auto-discover the project from the current working directory.

```bash
cargo run -- sessions
cargo run -- messages
```

Rules:

- If cwd auto-discovery succeeds, the query is scoped to that project.
- If cwd auto-discovery fails, the CLI falls back to all projects and sources.
- If cwd auto-discovery succeeds but the project has no matching history, the CLI returns an empty result instead of widening scope.
- `--all` disables cwd auto-discovery for `sessions` and `messages`.

### Look up a session directly

`messages --session <ID>` behaves differently from the default cwd-scoped flow: when you pass `--session` without `--project`, `mmr` searches across all projects instead of applying cwd project auto-discovery.

```bash
cargo run -- messages --session sess-123
```

If you also omit `--source`, `mmr` prints this hint on stderr:

```text
hint: searching all sources for session; pass --source to narrow the search
```

Use `--source` when you already know where the session came from:

```bash
cargo run -- --source codex messages --session sess-123
```

### Export for scripting

`export` returns the same message response shape as `messages`, but emits the full project history in chronological order.

```bash
cargo run -- export --project /path/to/project
```

If you run `export` without `--project`, the CLI infers the project from the current working directory:

- Codex matches the canonical filesystem path
- Claude and Cursor match the same path encoded with `/` replaced by `-` and a leading `-`

If you only need the message array:

```bash
cargo run -- export --project /path/to/project | jq '.messages'
```

### Generate a continuity brief

`remember` sends session transcripts to one backend agent and returns either Markdown or JSON.

Supported agents:

- `cursor`
- `codex`
- `gemini`

Examples:

```bash
cargo run -- remember --project /path/to/project
cargo run -- remember all --project /path/to/project
cargo run -- remember session sess-123 --project /path/to/project
cargo run -- remember --project /path/to/project --agent gemini --model gemini-2.5-pro
```

By default:

- output format is Markdown (`-O md`)
- `MMR_DEFAULT_REMEMBER_AGENT` can supply the default backend
- if no remember agent is configured, the default backend is Cursor

`--instructions` replaces the default output-formatting rules while preserving the base "Memory Agent" identity and transcript-input description.

## Environment variables

These defaults change CLI behavior without changing each invocation:

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` sets the default source filter when `--source` is omitted
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` sets the default remember backend when `--agent` is omitted

Empty or invalid values are treated as unset.

## Output and interface notes

- Successful command output goes to **stdout** as JSON, except `remember` in Markdown mode.
- Human-facing hints and errors go to **stderr**.
- `--source` accepts `codex`, `claude`, or `cursor`.
- `--source all` is not a valid value.
- `messages` paginates from the newest matching window, then returns the selected page in chronological order.

## Troubleshooting

### No results from `sessions` or `messages`

Check whether you are inside the expected project directory. By default, these commands scope to the current working directory's project when that project can be resolved.

To bypass that behavior:

```bash
cargo run -- sessions --all
cargo run -- messages --all
```

Or target a specific project directly:

```bash
cargo run -- messages --project /path/to/project
```

### `messages --session` is slower than expected

Without `--source`, session lookup searches all supported sources. Narrow it when possible:

```bash
cargo run -- --source cursor messages --session sess-123
```

### Scripts fail to match a project path

Pass `--project` and the value as separate arguments in subprocess calls. Avoid embedding shell quotes into a single argument value.

### Build fails on an older Rust toolchain

This repository uses Rust edition 2024. Upgrade to a toolchain that supports edition 2024 and rerun the build.

## Related docs

- `AGENTS.md` - contributor-oriented repository guide and command reference
- `adrs/002-cwd-scoped-defaults.md` - cwd-scoped defaults for `sessions` and `messages`
- `docs/references/session-lookup-invariants.md` - `messages --session` lookup rules
