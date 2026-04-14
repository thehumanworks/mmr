# mmr

`mmr` is a Rust CLI for browsing local Claude, Codex, and Cursor coding history and generating continuity briefs from prior sessions.

## What it reads

`mmr` reads local JSONL history directly from the tools' on-disk data stores:

| Source | Locations |
| --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl` |
| Claude | `~/.claude/projects/<encoded-project>/` session files, plus nested `subagents/*.jsonl` files |
| Cursor | `~/.cursor/projects/<encoded-project>/agent-transcripts/<session-id>/*.jsonl` |

For Claude and Cursor, `<encoded-project>` is the project path with `/` replaced by `-` and a leading `-` added. For example, `/Users/alex/app` becomes `-Users-alex-app`.

## Quickstart

Build and run with Cargo:

```bash
cargo run -- projects
cargo run -- sessions
cargo run -- messages --session sess-123 --source codex
cargo run -- export --project /Users/test/proj
cargo run -- remember --project /Users/test/proj
```

Add `--pretty` to pretty-print JSON output:

```bash
cargo run -- --pretty messages --all --limit 5
```

`projects`, `sessions`, `messages`, and `export` write JSON to `stdout`. `remember` writes markdown by default; use `-O json` when you need machine-readable output.

## Command overview

- `projects` - list projects across the selected source set.
- `sessions` - list sessions, defaulting to the current project when cwd auto-discovery succeeds.
- `messages` - list messages with pagination metadata.
- `export` - return all messages for a project in chronological order.
- `remember` - generate a continuity brief from the latest session, all sessions, or one selected session.

## Project scoping and source matching

### Source selection

- `--source` accepts `claude`, `codex`, or `cursor`.
- If `--source` is omitted, `mmr` searches all sources unless `MMR_DEFAULT_SOURCE` supplies a default.
- `MMR_DEFAULT_SOURCE` accepts `codex`, `claude`, or `cursor`. Empty or invalid values are treated as unset.

### Current-directory defaults

- `sessions` and `messages` auto-discover the current working directory as the default project scope.
- `--all` disables that default and searches across all projects.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery. Any other value, including unset, keeps it enabled.
- If cwd auto-discovery fails, `sessions` and `messages` fall back to the global view.
- If cwd auto-discovery succeeds but the project has no matching history, the command returns an empty result instead of widening scope.

### Project identifiers by source

- Codex projects match the canonical filesystem path.
- Claude and Cursor projects match the hyphenated form of that same path.
- `export` without `--project` resolves all three forms from the current directory, queries each source separately, merges the results, and sorts them chronologically.
- `remember` without `--project` uses the raw `current_dir()` string instead of canonicalizing first. When symlinks or differing path spellings matter, prefer an explicit `--project`.

### Session lookup behavior

`messages --session <id>` without `--project` skips cwd project auto-discovery and searches all projects instead. If `--source` is also omitted, the CLI prints a `stderr` hint recommending `--source` to narrow the search.

## Messages pagination contract

`messages` returns this envelope:

```json
{
  "messages": [],
  "total_messages": 0,
  "next_page": false,
  "next_offset": 0,
  "next_command": null
}
```

Pagination is based on the sorted message list. For the default `timestamp` + `asc` mode, `mmr` preserves the historical behavior of paging from the newest window and then returning that window in chronological order. When more results are available, `next_command` includes a ready-to-run continuation command that preserves the active filters and formatting flags.

`export` also returns `ApiMessagesResponse`, but it is intentionally unpaginated: `next_page` is always `false` and `next_command` is always omitted.

## `remember` backends

`remember` supports three backends:

| Agent | Default when `--agent` omitted | Auth / requirements | Model behavior |
| --- | --- | --- | --- |
| Cursor | Yes, unless `MMR_DEFAULT_REMEMBER_AGENT` overrides it | `CURSOR_API_KEY` and `agent` on `PATH` | Defaults to `composer-2-fast`; `--model` overrides it |
| Codex | No | Codex CLI authentication available to `codex exec` | Ignores `--model` |
| Gemini | No | `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL` | Defaults to `gemini-3.1-flash-lite-preview`; `--model` overrides it |

Examples:

```bash
cargo run -- remember --project /Users/test/proj
cargo run -- remember all --project /Users/test/proj --agent gemini -O json
cargo run -- remember session sess-123 --project /Users/test/proj --agent cursor --model composer-2-fast
cargo run -- remember --project /Users/test/proj --instructions "Return only a terse checklist."
```

`--instructions` replaces the default output-formatting section of the system prompt while keeping the base "Memory Agent" identity and transcript input description intact.

## Troubleshooting

- No results from `sessions` or `messages` in a project directory: retry with `--all` to confirm whether cwd auto-discovery is narrowing the scope.
- `messages --session` finds the wrong session or is slow: add `--source` to avoid searching every source.
- `remember` fails immediately with Cursor: verify `CURSOR_API_KEY` is set and the `agent` CLI is available.
- `remember` fails immediately with Gemini: verify `GOOGLE_API_KEY` or `GEMINI_API_KEY`.
- Scripted invocations should pass `--project` and the path as separate arguments, not as a quoted `--project="value"` token.

## Developer workflow

The repository uses Rust 2024 and keeps JSON output machine-readable on `stdout`.

Mandatory verification for code changes:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

See `AGENTS.md` for repository structure, contributor guidance, and deeper implementation notes.
