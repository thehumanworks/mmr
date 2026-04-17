# mmr

Read local AI coding histories from Claude Code, Codex CLI, and Cursor as machine-readable data.

`mmr` scans the transcript files those tools already write on disk, normalizes them into a shared schema, and exposes query-oriented CLI commands for projects, sessions, messages, export, and continuity summaries.

- Query commands write JSON to `stdout`.
- Human-facing hints and errors go to `stderr`.
- `remember` defaults to Markdown output; use `-O json` for machine-readable output.

## What `mmr` reads

| Source | Files read | Project identifier used by `mmr` |
| --- | --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl`, `~/.codex/archived_sessions/**/*.jsonl` | Canonical working-directory path from session metadata |
| Claude | `~/.claude/projects/<project>/**/*.jsonl` | Encoded project directory name such as `-Users-me-proj`; original cwd is retained when present |
| Cursor | `~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl` | Project directory name under `~/.cursor/projects` (current implementation keeps this encoded value) |

Reference docs:

- `docs/references/session-lookup-invariants.md`
- `docs/references/schemas/codex/message_schema.md`
- `docs/references/schemas/claude/message_schema.md`
- `docs/references/schemas/cursor/message_schema.md`

## Build and run

`mmr` requires a Rust toolchain that supports Edition 2024.

```bash
cargo build --release
cargo run -- projects
```

If your default Cargo toolchain reports that Edition 2024 is unsupported, switch to a newer stable Rust toolchain and rerun the command.

## Quick command examples

```bash
# List projects across all supported sources
cargo run -- projects

# Restrict results to one source
cargo run -- --source cursor projects

# List sessions for the current working directory when auto-discovery succeeds
cargo run -- sessions

# Bypass cwd auto-discovery and query everything
cargo run -- sessions --all

# Fetch one session's messages
cargo run -- messages --session sess-123

# Export the current directory's messages across sources in chronological order
cargo run -- export

# Generate a continuity brief from the latest session
cargo run -- remember --project /path/to/proj

# Generate a JSON continuity brief from all matching sessions
cargo run -- remember all --project /path/to/proj -O json
```

## Project scoping rules

`projects` always lists all matching projects for the selected source filter. `sessions` and `messages` have more scoping behavior:

- Omitting `--source` searches all sources unless `MMR_DEFAULT_SOURCE` is set.
- `--source all` is not valid.
- `sessions` and `messages` default to the current working directory when project auto-discovery succeeds.
- Set `MMR_AUTO_DISCOVER_PROJECT=0` to disable cwd auto-discovery.
- `--all` bypasses cwd auto-discovery and searches all projects.
- If cwd auto-discovery fails, `sessions` and `messages` fall back to all projects.
- If cwd auto-discovery succeeds but the discovered project has no matching records, the commands return an empty result instead of falling back.

### `messages --session` special case

When you pass `--session` without `--project`, `mmr` skips cwd auto-discovery and searches all projects instead. This avoids false empty results when the requested session belongs to a different project.

If `--source` is also omitted, `mmr` prints this hint to `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

See `docs/references/session-lookup-invariants.md` for the full contract.

### Project-name normalization caveats

- Codex project filters accept either `/Users/me/proj` or `Users/me/proj`.
- Claude stores an encoded project directory name such as `-Users-me-proj`, but also preserves cwd metadata, so canonical-path lookups still work for normal Claude transcripts.
- Cursor currently keeps the encoded project directory name as both `project_name` and `project_path`. Direct `sessions --project` and `messages --project` lookups therefore match the encoded Cursor directory name, not the decoded filesystem path.

Practical guidance for Cursor history:

- Run `projects --source cursor` first to discover the exact project identifier.
- Prefer `export` without `--project` when you are already inside the target repository; `export` handles the cwd-to-Cursor encoding for you.

## Messages output and pagination

`messages` returns this response shape:

```json
{
  "messages": [],
  "total_messages": 0,
  "next_page": false,
  "next_offset": 0,
  "next_command": "mmr messages --offset 50"
}
```

`next_command` is optional and only appears when another page is available.

Important pagination semantics:

- Default message sorting is `--sort-by timestamp --order asc`.
- For that default sort, pagination is applied from the newest end of the result set, then the selected slice is returned in chronological order.
- In practice, `--limit 50 --offset 0` returns the latest 50 messages, ordered oldest-to-newest within that 50-message window.
- `next_offset` advances by the number of messages returned in the current page.

## `export`

`export` always returns the same `ApiMessagesResponse` shape as `messages`, but without paginating.

- `export --project <value>` queries the explicit project across the selected source filter.
- `export` without `--project` infers the project from the current working directory:
  - Codex: canonical path
  - Claude: encoded path with `/` replaced by `-` and a leading `-`
  - Cursor: the same encoded project name as Claude

The returned `messages` array is sorted in ascending timestamp order.

## `remember`

`remember` builds a stateless continuity brief from prior sessions for a project.

### Selection modes

- `remember` -> latest matching session
- `remember all` -> all matching sessions
- `remember session <session-id>` -> one specific session

### Output modes

- `-O md` (default): plain Markdown text
- `-O json`: structured JSON with `agent` and `text`

### Agent backends

- `cursor`
  - Default backend when `--agent` and `MMR_DEFAULT_REMEMBER_AGENT` are both unset
  - Requires `CURSOR_API_KEY`
  - Uses the `agent` CLI on `PATH`
  - Defaults to model `composer-2-fast` unless `--model` is set
- `gemini`
  - Requires `GOOGLE_API_KEY` or `GEMINI_API_KEY`
  - Honors optional `GEMINI_API_BASE_URL`
  - Defaults to model `gemini-3.1-flash-lite-preview` unless `--model` is set
- `codex`
  - Uses local Codex CLI auth/configuration
  - Uses the built-in default model configured in `src/agent/codex.rs`

### Custom instructions

`--instructions "<text>"` replaces the default output-format and rules section of the system prompt, while preserving the Memory Agent identity and input-format section.

## Useful environment variables

| Variable | Effect |
| --- | --- |
| `MMR_AUTO_DISCOVER_PROJECT=0` | Disable cwd auto-discovery for `sessions` and `messages` |
| `MMR_DEFAULT_SOURCE=codex|claude|cursor` | Apply a default source filter when `--source` is omitted |
| `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` | Apply a default `remember --agent` value when omitted |
| `SIMPLEMMR_HOME=/tmp/fixture-home` | Override the home directory used for history discovery (mainly useful for tests and fixtures) |

## Troubleshooting and pitfalls

- If `messages --session <id>` seems slow, add `--source` to avoid searching every source.
- If Cursor sessions do not appear for an explicit filesystem path, use `projects --source cursor` to find the encoded project name that Cursor actually stored.
- When invoking `mmr` from a subprocess API, pass `--project` and its value as separate arguments. Avoid a single literal argument such as `--project=\"/path/to/proj\"`, which can pass the quotes through and break matching.
