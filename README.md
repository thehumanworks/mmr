# mmr

`mmr` is a Rust CLI for browsing local AI coding history from Claude, Codex, and Cursor.

It loads local transcript files, builds an in-memory index, and returns machine-readable JSON on
stdout for scripting and automation.

## What it reads

`mmr` reads local history under your `HOME` directory:

- Codex: `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl`
- Claude: `~/.claude/projects/*/**/*.jsonl`
- Cursor: `~/.cursor/projects/*/agent-transcripts/*/*.jsonl`

Malformed JSONL lines are skipped so valid records from the same file can still be ingested.

## Build and install

`mmr` uses Rust Edition 2024, so use a Rust toolchain that supports it.

```bash
cargo build --release
cargo install --path .
```

Common local commands:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
```

## Output contract

- JSON responses are printed to stdout.
- Human-facing diagnostics and hints are printed to stderr.
- Errors are colored on stderr only; JSON output is never colored.

This makes the CLI safe to pipe into tools like `jq`.

## Quick start

List known projects across all sources:

```bash
cargo run -- projects
```

Limit to one source:

```bash
cargo run -- --source cursor projects
```

List sessions for the current project:

```bash
cargo run -- sessions
```

List messages for one session:

```bash
cargo run -- messages --session sess-123
```

Export all messages for the current project:

```bash
cargo run -- export | jq '.messages'
```

Generate a continuity brief from the latest session:

```bash
cargo run -- remember --project /path/to/proj
```

## Command behavior

### `projects`

`projects` lists aggregated projects and includes per-project message and session counts.

Examples:

```bash
cargo run -- projects --limit 20
cargo run -- --source codex projects --sort-by timestamp --order desc
```

### `sessions`

`sessions` returns aggregated session metadata, including:

- `session_id`
- `source`
- `project_name`
- `project_path`
- first and last timestamps
- message counts
- a preview from the earliest user message in the session

Default scoping rules:

- If `--project` is provided, that project is used.
- If `--project` is omitted and `--all` is not set, `mmr` tries to auto-discover the current
  working directory as the project.
- If cwd auto-discovery fails, `sessions` falls back to all projects.
- If cwd auto-discovery succeeds but the project has no matching history, `sessions` returns an
  empty result instead of widening the scope.

Examples:

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- sessions --project /path/to/proj
```

### `messages`

`messages` returns individual messages with per-item metadata:

- `session_id`
- `source`
- `project_name`
- `role`
- `content`
- `model`
- `timestamp`
- `is_subagent`
- `msg_type`
- token counts when available

Default scoping follows the same cwd auto-discovery rules as `sessions`, with one important
exception:

- `messages --session <id>` without `--project` skips cwd auto-discovery and searches all projects.
- If `--source` is also omitted, `mmr` prints a stderr hint suggesting `--source` to narrow the
  lookup.

Examples:

```bash
cargo run -- messages
cargo run -- messages --all --limit 100
cargo run -- messages --project /path/to/proj
cargo run -- messages --session sess-123
```

#### Pagination semantics

For the default sort (`--sort-by timestamp --order asc`), pagination uses the historical contract:

1. Select the newest matching window.
2. Return that page in chronological order.

Responses include:

- `next_page`
- `next_offset`
- `next_command` when another page is available

That allows shell workflows like:

```bash
eval "$(cargo run -- messages --limit 50 | jq -r '.next_command')"
```

When you change sort or order, `next_command` preserves those flags.

### `export`

`export` returns all messages for one project in chronological order as `ApiMessagesResponse`.

Examples:

```bash
cargo run -- export
cargo run -- export --project /path/to/proj
cargo run -- --source claude export --project my-proj
```

With no `--project`, `export` infers the current directory as the project and queries each source
using that source's project naming convention:

- Codex uses the canonical path directly.
- Claude uses the slash-to-hyphen project name with a leading `-`.
- Cursor uses the same slash-to-hyphen form as Claude.

### `remember`

`remember` sends prior session transcripts to an AI backend and returns a stateless continuity
brief.

Selectors:

- latest session: `remember`
- all matching sessions: `remember all`
- one specific session: `remember session <session-id>`

Backends:

- `cursor` - default backend when `--agent` is omitted; default model is `composer-2-fast`
- `codex` - fixed to `gpt-5.4-mini` with medium reasoning effort
- `gemini` - default model is `gemini-3.1-flash-lite-preview`

Examples:

```bash
cargo run -- remember --project /path/to/proj
cargo run -- remember all --project /path/to/proj
cargo run -- remember session sess-123 --project /path/to/proj
cargo run -- remember --agent gemini --model gemini-3.1-flash-lite-preview --project /path/to/proj
cargo run -- remember --instructions "Return only three bullet points." --project /path/to/proj
```

Output behavior:

- Default output format is Markdown.
- Use `-O json` or `--output-format json` for a JSON envelope.
- `--instructions` replaces the default output-format section of the system prompt but keeps the
  base "Memory Agent" identity and transcript input-format instructions.

## Environment variables

`mmr` supports a few opt-in defaults:

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` sets the default source when `--source` is omitted
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` sets the default backend for `remember`
- `GOOGLE_API_KEY` or `GEMINI_API_KEY` configures Gemini
- `GEMINI_API_BASE_URL` overrides the Gemini API base URL
- `CURSOR_API_KEY` configures the Cursor backend

Empty or invalid values for `MMR_DEFAULT_SOURCE` and `MMR_DEFAULT_REMEMBER_AGENT` are treated as
unset.

## Troubleshooting

### `sessions` or `messages` only show the current project

That is the default behavior when cwd auto-discovery succeeds.

Use one of:

```bash
cargo run -- sessions --all
cargo run -- messages --all
MMR_AUTO_DISCOVER_PROJECT=0 cargo run -- messages
```

### `messages --session` returns nothing

The session may exist in a different source than you expect.

Try:

```bash
cargo run -- messages --session sess-123
cargo run -- --source codex messages --session sess-123
cargo run -- --source cursor messages --session sess-123
```

### `remember` fails immediately

Check backend-specific credentials:

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: working Codex CLI authentication for `codex exec`

### A script cannot match a project path

When calling `mmr` from scripts, pass `--project` and the path as separate arguments. Avoid a
single argument such as `--project="/path/to/proj"` because the quotes can be passed literally and
break matching.

## Related docs

- `AGENTS.md` - repository workflow and maintenance notes
- `adrs/002-cwd-scoped-defaults.md` - rationale for cwd-scoped `sessions` and `messages`
- `docs/references/session-lookup-invariants.md` - `messages --session` lookup behavior
