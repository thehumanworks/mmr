# mmr

`mmr` is a local Rust CLI for browsing and reusing AI coding history from Claude and Codex.

It reads conversation logs from your home directory, normalizes them into a single in-memory model, and emits machine-readable JSON on stdout for downstream tools and scripts.

## What it covers

- `projects`: aggregate projects across local Claude and Codex history
- `sessions`: inspect sessions for one project or across everything
- `messages`: page through normalized messages with source metadata
- `export`: dump all messages for a project in chronological order
- `remember`: generate a continuity brief from prior sessions and continue a follow-up thread

## Data sources

`mmr` reads local history directly from disk; it does not use a database or cache layer.

- Codex history:
  - `~/.codex/sessions/**/*.jsonl`
  - `~/.codex/archived_sessions/**/*.jsonl`
- Claude history:
  - `~/.claude/projects/*/*.jsonl`
  - `~/.claude/projects/*/**/subagents/*.jsonl`

Malformed JSONL lines are skipped so one bad entry does not stop ingestion.

By default the CLI reads from your real home directory. For tests or fixture-driven debugging, set `SIMPLEMMR_HOME` to point `mmr` at an alternate home tree.

## Prerequisites

- Rust toolchain with Edition 2024 support
- Local Claude and/or Codex history files under your home directory
- For `remember --agent gemini`:
  - `GOOGLE_API_KEY` or `GEMINI_API_KEY`
  - optional `GEMINI_API_BASE_URL` override for tests or proxies

## Build and run

```bash
cargo run -- projects
cargo run -- sessions --project /Users/test/proj
cargo run -- export --project /Users/test/proj
```

If Cargo reports that Edition 2024 is unsupported, upgrade to a newer Rust toolchain before building.

## Command guide

All commands print JSON to stdout by default. Add `--pretty` to pretty-print the JSON.

### `projects`

List aggregated projects.

```bash
mmr projects
mmr --source codex projects --limit 25 --offset 0
mmr projects --sort-by message-count --order desc
```

Returns objects with:

- `name`
- `source`
- `original_path`
- `session_count`
- `message_count`
- `last_activity`

### `sessions`

List sessions, optionally filtered by project and/or source.

```bash
mmr sessions
mmr sessions --project /Users/test/proj
mmr --source claude sessions --project -Users-test-proj
```

Returns per-session metadata including `source`, `project_name`, `project_path`, message counts, timestamps, and a preview from the first user message.

### `messages`

List messages, optionally filtered by session, project, and/or source.

```bash
mmr messages --session sess-123
mmr messages --project /Users/test/proj --limit 100
mmr --source codex messages --project /Users/test/proj
```

Each message is self-describing and includes:

- `session_id`
- `source`
- `project_name`
- `role`
- `content`
- `model`
- `timestamp`
- `is_subagent`
- `msg_type`
- `input_tokens`
- `output_tokens`

Pagination is applied from the newest window of matching messages, then the returned page is re-ordered chronologically.

### `export`

Export all messages for one project as a single chronological `ApiMessagesResponse`.

```bash
mmr export --project /Users/test/proj
mmr --source codex export --project /Users/test/proj
```

If `--project` is omitted, `mmr export` infers the project from the current working directory:

- Codex lookup uses the canonical path as-is
- Claude lookup uses the same path with `/` replaced by `-` and a leading `-`

Example:

- cwd: `/Users/test/proj`
- Codex project key: `/Users/test/proj`
- Claude project key: `-Users-test-proj`

This makes `mmr export` the easiest way to dump the full transcript for the repo you are currently in:

```bash
cd /Users/test/proj
mmr export | jq '.messages'
```

### `remember`

Generate a continuity brief from prior sessions, or continue an existing agent interaction/thread.

Default behavior:

- `--agent codex`
- `--mode latest`
- output format: JSON

Basic examples:

```bash
mmr remember --project /Users/test/proj
mmr remember --project /Users/test/proj --mode all
mmr --source codex remember --project /Users/test/proj --mode all
```

Continue a prior interaction or thread:

```bash
mmr remember \
  --continue-from interaction-or-thread-id \
  --follow-up "What should I do first?"
```

Override the default output instructions:

```bash
mmr remember \
  --project /Users/test/proj \
  --instructions "Return only a short checklist."
```

Return markdown instead of JSON:

```bash
mmr remember --project /Users/test/proj -O md
```

Use Gemini instead of the default Codex agent:

```bash
mmr remember \
  --agent gemini \
  --model gemini-3.1-flash-lite-preview \
  --project /Users/test/proj
```

Important constraints:

- `--follow-up` requires `--continue-from`
- `--model` only applies to `--agent gemini`
- without `--instructions`, `remember` uses the built-in continuity brief format
- with `--instructions`, your text replaces the default output-format/rules section while preserving the base "Memory Agent" identity and transcript input format

JSON responses include:

- `agent`
- `text`
- `thread_or_interaction_id`

When using `-O md`, the same response is rendered as markdown with a footer showing either a Codex thread ID or a Gemini interaction ID.

## Source and filter semantics

These rules matter when scripting against the CLI:

- `--source` accepts only `claude` or `codex`
- omitting `--source` means "query both sources"
- `--source all` is not a valid value
- `sessions` and `messages` accept optional filters; you can progressively drill down instead of specifying everything up front
- `projects` without `--source` returns projects from both sources
- `export --project <path>` reuses the same `messages` response schema instead of introducing a separate export-only type

## Output and scripting notes

- JSON is always written to stdout
- human-readable errors are written to stderr
- when scripting, pass `--project` and its value as separate arguments instead of embedding quotes inside one argument

Example:

```bash
# Good
python script.py --project /Users/test/proj

# Avoid passing the quotes literally to the CLI
python script.py '--project="/Users/test/proj"'
```

## Troubleshooting

### `No sessions found for project ...`

Check that you are using the right project key for the source:

- Codex usually matches the real path
- Claude project names are path-derived and hyphenated

If you are already inside the repo you care about, `mmr export` or `mmr remember` without `--project` can avoid a mismatch.

### `--follow-up requires --continue-from`

Start a new `remember` call first, capture the returned `thread_or_interaction_id`, then pass that value back to `--continue-from`.

### Build fails before compilation starts

This repo uses Rust Edition 2024. If Cargo cannot parse the manifest, upgrade your Rust toolchain.

## Further reading

- `adrs/001-optional-source-and-project-filters.md` - filter and response-shape contract
- `docs/references/schemas/claude/message_schema.md` - Claude raw message format notes
- `docs/references/schemas/codex/message_schema.md` - Codex raw message format notes
- `AGENTS.md` - contributor-oriented repo workflow guidance
