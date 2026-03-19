# mmr

`mmr` is a Rust CLI for browsing and reshaping local AI coding history from Codex and Claude.

It loads local JSONL history into an in-memory query service, then exposes script-friendly commands for:

- listing projects, sessions, and messages
- exporting a project's full transcript history
- generating a continuity brief with `remember`
- copying history between Codex and Claude with `merge`

Command results are written to stdout. Human-facing errors are written to stderr.

## Architecture at a glance

- `src/source/` loads Codex and Claude JSONL history from disk in parallel.
- `src/messages/service.rs` builds the in-memory project, session, and message indexes.
- `src/cli.rs` owns CLI parsing, default resolution, and command dispatch.
- `src/agent/` powers `remember` by sending selected transcripts to the configured agent.
- `src/merge/` is the only mutating subsystem; it writes merged history back to local source files.

## History sources

`mmr` reads from the current home directory unless `SIMPLEMMR_HOME` is set.

| Source | Paths read | Notes |
| --- | --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl`, `~/.codex/archived_sessions/**/*.jsonl` | Project identity is the recorded `cwd` path. |
| Claude | `~/.claude/projects/*/*.jsonl` and nested `subagents/*.jsonl` files | Project directories are named with Claude's encoded project name, but `mmr` also tracks the recorded `cwd` path when present. |

Malformed JSONL lines are skipped so valid history can still be ingested.

## Quick start

Run from the repository root during development:

```bash
cargo run -- projects
cargo run -- sessions
cargo run -- messages
cargo run -- export
```

Once installed, the same commands are available as `mmr ...`.

## Command guide

### Global flags

- `--pretty`: pretty-print JSON output
- `--source claude|codex`: restrict queries to one source
  - if omitted, both sources are searched unless `MMR_DEFAULT_SOURCE` supplies a default

### `projects`

List known projects across the loaded history.

```bash
cargo run -- projects
cargo run -- --source codex projects --limit 25 --offset 25
```

Use this when you need the canonical project identifier before querying sessions or messages.

### `sessions`

List sessions for a project.

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- sessions --project /Users/test/codex-proj
```

Important defaults:

- Without `--project` and without `--all`, `sessions` tries to auto-discover the current working directory as the project scope.
- If auto-discovery fails, it falls back to the historical global search.
- If auto-discovery succeeds but that project has no history, the result is empty rather than widened.

### `messages`

List messages for a session or project.

```bash
cargo run -- messages
cargo run -- messages --all
cargo run -- messages --session sess-123
cargo run -- --source claude messages --project /Users/test/proj
```

Important defaults:

- `messages` uses the same current-directory project auto-discovery behavior as `sessions`.
- When sorting by ascending timestamp, pagination still follows the historical contract: `mmr` selects the newest window first, then returns that window in chronological order.

### `export`

Return all messages for one project as `ApiMessagesResponse`.

```bash
cargo run -- export
cargo run -- export --project /Users/test/proj
cargo run -- --source codex export --project /Users/test/proj
```

If `--project` is omitted, `export` infers the project from the current working directory:

- Codex uses the canonical cwd path as-is.
- Claude uses the same path encoded with slashes replaced by `-` and a leading `-`.

### `remember`

Generate a stateless continuity brief from prior sessions.

```bash
cargo run -- remember --project /Users/test/proj
cargo run -- remember all --project /Users/test/proj
cargo run -- remember session sess-123 --project /Users/test/proj
cargo run -- remember --instructions "Return only three bullets."
cargo run -- remember -O json
```

Current behavior:

- The default output format is Markdown (`-O md`).
- The default agent is `codex` unless `MMR_DEFAULT_REMEMBER_AGENT` sets `gemini`.
- If `--project` is omitted, `remember` uses the current working directory path.
- `--instructions` replaces the default output-format/rules section of the system prompt, but preserves the base "Memory Agent" identity and input-format description.

Gemini-specific setup:

- set `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- optionally set `GEMINI_API_BASE_URL` to target a non-default API endpoint

### `merge`

Copy history between sessions or between sources.

```bash
cargo run -- merge --from-session sess-claude-1 --to-session sess-codex-1
cargo run -- merge --from-session sess-claude-1 --from-agent claude --to-session sess-codex-1 --to-agent codex
cargo run -- merge --from-agent codex --to-agent claude --project /Users/test/proj
cargo run -- merge --from-agent claude --to-agent codex --session sess-123 --project /Users/test/proj
```

`merge` has two modes:

1. **Session-to-session merge**
   - requires `--from-session` and `--to-session`
   - appends copied messages into an existing destination session
   - may shift copied timestamps forward so the imported block stays after the destination session's final message

2. **Agent-to-agent merge**
   - requires `--from-agent` and `--to-agent`
   - creates new destination sessions instead of appending into an existing one
   - can be narrowed with `--project` and/or `--session`

Operational constraints:

- The global `--source` flag does not apply to `merge`; use `--from-agent` and `--to-agent` instead.
- If a session id is ambiguous across sources, add `--from-agent` or `--to-agent` to disambiguate it.
- Session-to-session merges reject merging a session into itself.

## Merge runbook and caveats

`merge` is the only command that modifies local history files.

### Where writes go

- Agent-to-agent merges that target Codex create new files under `~/.codex/sessions/<generated-session-id>.jsonl`.
- Agent-to-agent merges that target Claude create new files under `~/.claude/projects/<encoded-project>/<generated-session-id>.jsonl`.
- Session-to-session merges append JSONL lines to the existing destination session file.

### Metadata transformations

- Codex stores model metadata at session scope (`session_meta.payload.model_provider`), so importing Claude messages into Codex collapses per-message model values to a single provider string.
- Claude stores model metadata on assistant messages, so importing Codex into Claude expands the Codex provider onto assistant messages.
- Claude subagent ancestry is flattened during agent-to-agent merges into Claude; imported sessions are written as top-level project sessions.

The merge response reports these decisions through:

- `timestamp_strategy`
- `model_strategy`
- `schema_considerations`
- `target_file`

Use those fields as the authoritative record of what happened during a merge.

## Environment variables

| Variable | Effect |
| --- | --- |
| `SIMPLEMMR_HOME` | Override the home directory used for reading history and writing merge output. |
| `MMR_AUTO_DISCOVER_PROJECT=0` | Disable current-directory project auto-discovery for `sessions` and `messages`. |
| `MMR_DEFAULT_SOURCE=codex|claude` | Supply the default `--source` when the flag is omitted. |
| `MMR_DEFAULT_REMEMBER_AGENT=codex|gemini` | Supply the default `remember --agent` when the flag is omitted. |
| `GOOGLE_API_KEY` / `GEMINI_API_KEY` | Credentials for `remember --agent gemini`. |
| `GEMINI_API_BASE_URL` | Override the Gemini Interactions API base URL. |

Empty or invalid values for `MMR_DEFAULT_SOURCE` and `MMR_DEFAULT_REMEMBER_AGENT` are treated as unset.

## Troubleshooting and common pitfalls

### `sessions` or `messages` returned less than expected

You are probably inside a directory that `mmr` auto-resolved to a single project. To search globally again:

```bash
cargo run -- sessions --all
cargo run -- messages --all
MMR_AUTO_DISCOVER_PROJECT=0 cargo run -- messages
```

### `remember --agent gemini` failed immediately

Make sure one of these is set:

```bash
export GOOGLE_API_KEY=...
# or
export GEMINI_API_KEY=...
```

### `merge` could not find or uniquely identify a session

Add source and project hints:

```bash
cargo run -- merge --from-session sess-1 --from-agent claude --to-session sess-2 --to-agent codex
cargo run -- merge --from-agent codex --to-agent claude --session sess-1 --project /Users/test/proj
```

### I only want read-only output

Use `projects`, `sessions`, `messages`, `export`, or `remember`. Only `merge` writes back to disk.

## Additional references

- `AGENTS.md`: contributor-oriented repository guidance
- `adrs/002-cwd-scoped-defaults.md`: rationale for cwd-based default scoping and env-driven defaults
- `docs/references/schemas/`: source-format notes for Codex and Claude history files
