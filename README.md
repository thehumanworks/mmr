# mmr

`mmr` is a Rust CLI for browsing local AI coding history from Claude, Codex, and Cursor.

It reads transcripts directly from the filesystem, aggregates them in memory, and prints machine-readable results to `stdout`. Human-facing diagnostics go to `stderr`.

## What it covers

- `projects`: summarize known projects across supported sources
- `sessions`: inspect session-level history for a project or across all projects
- `messages`: inspect message-level history with stable pagination
- `export`: emit the full message stream for the current project
- `remember`: build a continuity brief from prior sessions

## Where history comes from

By default `mmr` reads from your home directory. Set `SIMPLEMMR_HOME` to point at a different root when testing or working with fixtures.

| Source | Files read | Notes |
| --- | --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl`, `~/.codex/archived_sessions/**/*.jsonl` | Uses the session `cwd` as the project identifier. |
| Claude | `~/.claude/projects/<project>/*.jsonl` and nested `subagents/*.jsonl` files | Project directories are keyed by the Claude project name. |
| Cursor | `~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl` | Project directories are keyed by Cursor's encoded project name. |

Malformed JSONL lines are skipped so one bad line does not prevent the rest of the history from loading.

## Quick start

```bash
# List recently active projects across all sources
cargo run -- projects

# Show sessions for the current working directory's project
cargo run -- sessions

# Search all projects instead of the auto-discovered cwd project
cargo run -- sessions --all

# Show messages for one session
cargo run -- messages --session sess-123

# Export the current project across matching sources
cargo run -- export

# Generate a continuity brief from the latest session in a project
cargo run -- remember --project /path/to/project
```

## Command behavior that matters in practice

### `sessions` and `messages` default to the current project

When you omit `--project`, both commands try to infer the project from the current working directory.

- If auto-discovery succeeds, results are scoped to that project.
- If auto-discovery fails, the CLI falls back to all projects.
- `--all` disables cwd scoping explicitly.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd scoping globally for these commands.

The inferred scope is a canonical filesystem path. That maps cleanly to Codex history and to Claude sessions that recorded `cwd`, which is the common case in the fixtures and CLI contract tests. Cursor history is keyed by an encoded project name instead, so path-based project scoping is not the reliable way to discover Cursor data.

Examples:

```bash
# Project-scoped by default
cargo run -- messages

# Force cross-project search
cargo run -- messages --all

# Disable cwd scoping through the environment
MMR_AUTO_DISCOVER_PROJECT=0 cargo run -- sessions
```

### `messages --session` searches all projects unless you pin `--project`

When you already know the session ID, `mmr messages --session <id>` skips cwd project auto-discovery and searches across all projects by default. This avoids false negatives when the session belongs to a different project than your current directory.

If you omit `--source`, the CLI prints this hint to `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

Examples:

```bash
# Search all projects and all sources for a session ID
cargo run -- messages --session sess-123

# Narrow the search when you already know the source
cargo run -- --source cursor messages --session sess-123

# Re-apply explicit project scoping
cargo run -- messages --session sess-123 --project /path/to/project
```

### `messages` pagination is newest-window first, but output stays chronological

For timestamp-ascending reads, pagination is applied from the newest end of the result set and then returned in chronological order. That preserves the historical CLI contract while still letting callers walk backward through recent history.

When more results exist, the JSON response includes:

- `next_page`
- `next_offset`
- `next_command`

Example:

```bash
cargo run -- messages --session sess-123 --limit 50
```

If more messages exist, the response will contain a ready-to-run command similar to:

```json
{
  "next_page": true,
  "next_offset": 50,
  "next_command": "mmr messages --session sess-123 --limit 50 --offset 50"
}
```

### `export` uses the current directory when `--project` is omitted

`mmr export` resolves the current directory into source-specific project identifiers and merges matching messages into one chronological stream.

- Codex uses the canonical path directly.
- Claude uses the same path with `/` replaced by `-` and a leading `-`.
- Cursor uses the same encoded project key as Claude for cwd-based export.

Running `export` from inside the target project directory is the reliable way to include Cursor history for that project.

Example:

```bash
# Export the project for the directory you are standing in
cargo run -- export

# Export a specific project path instead
cargo run -- export --project /path/to/project
```

### `remember` defaults and backend behavior

`remember` generates a continuity brief from prior transcripts for one project.

- Default selection: latest matching session
- Other selections: `all` or `session <session-id>`
- Default output format: Markdown
- JSON output: `-O json`
- Default backend when `--agent` is omitted: Cursor
- Default Cursor model when `--model` is omitted: `composer-2-fast`
- Default Gemini model when `--model` is omitted: `gemini-3.1-flash-lite-preview`
- Codex currently uses a fixed backend configuration: `gpt-5.4-mini` with medium reasoning effort

Examples:

```bash
# Latest session, markdown output
cargo run -- remember --project /path/to/project

# All sessions, JSON output
cargo run -- remember all --project /path/to/project -O json

# One session with custom instructions
cargo run -- remember session sess-123 --project /path/to/project --instructions "Return only bullet points."
```

## Environment variables

| Variable | Effect |
| --- | --- |
| `SIMPLEMMR_HOME` | Override the home directory used for transcript discovery. |
| `MMR_AUTO_DISCOVER_PROJECT=0|1` | Disable or enable cwd project auto-discovery for `sessions` and `messages`. Unset behaves like `1`. |
| `MMR_DEFAULT_SOURCE=codex|claude|cursor` | Supply the default `--source` when the flag is omitted. Invalid values are ignored. |
| `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` | Supply the default `remember --agent` when the flag is omitted. Invalid values are ignored. |
| `CURSOR_API_KEY` | Required for `remember --agent cursor`. |
| `GOOGLE_API_KEY` or `GEMINI_API_KEY` | Required for `remember --agent gemini`. |
| `GEMINI_API_BASE_URL` | Optional override for the Gemini Interactions API base URL. |

## Output formats

- `projects`, `sessions`, `messages`, and `export` emit JSON on `stdout`
- `remember` emits Markdown by default
- `remember -O json` emits JSON
- Error messages and hints are written to `stderr`

## Troubleshooting

### `sessions` or `messages` returned fewer results than expected

You may be seeing cwd-based project scoping.

- Use `--all` to search across all projects.
- Use `--project <path-or-name>` to pin the scope explicitly.
- Set `MMR_AUTO_DISCOVER_PROJECT=0` if you prefer global search by default.

If the missing history is from Cursor, prefer `--all --source cursor` for discovery. Cursor project matching uses the encoded project key rather than the canonical path used by cwd auto-discovery.

### `messages --session` is slow

Without `--source`, the command searches all supported sources. Add `--source codex`, `--source claude`, or `--source cursor` when you know where the session lives.

### `remember` failed before making a model request

Check backend-specific auth first:

- Cursor: `CURSOR_API_KEY`
- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- Codex: working Codex CLI authentication in the environment where `mmr` runs

### No history was found

Check that the expected transcript directories exist under your home directory, or point `SIMPLEMMR_HOME` at a fixture tree and retry.

### `--project /path/to/project` did not include Cursor data

For Cursor, the stored project identifier is the encoded project directory name under `~/.cursor/projects`, not the decoded filesystem path. Use one of these workflows instead:

- run `cargo run -- export` from inside the project directory
- search globally with `cargo run -- --source cursor messages --all`
- search globally with `cargo run -- --source cursor sessions --all`

## Developer verification

Repository workflow expects this verification loop after meaningful changes:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

## Additional references

- `AGENTS.md` - repository workflow and contributor guidance
- `adrs/002-cwd-scoped-defaults.md` - rationale for cwd-scoped defaults
- `docs/references/session-lookup-invariants.md` - `messages --session` lookup contract
