# mmr

`mmr` is a Rust CLI for browsing local Claude, Codex, and Cursor conversation history as machine-readable JSON.

It is designed for scripting and automation: command results go to `stdout` as JSON, while hints and human-facing diagnostics go to `stderr`.

## What `mmr` reads

`mmr` loads local history from your home directory:

- Codex data under `~/.codex`
- Claude projects under `~/.claude/projects`
- Cursor projects under `~/.cursor/projects`

Set `SIMPLEMMR_HOME` to point the loader at a different home directory root, which is useful for tests and fixture-based automation.

## Requirements

- Rust stable with edition 2024 support
- A recent Cargo toolchain (`cargo +stable ...` works in this repository)

Common verification commands:

```bash
cargo +stable fmt
cargo +stable test
cargo +stable test --test cli_benchmark -- --ignored --nocapture
cargo +stable clippy --all-targets --all-features -- -D warnings
cargo +stable build --release
```

## Quick start

List projects:

```bash
cargo +stable run -- projects
```

List sessions for the current working directory's project when auto-discovery succeeds:

```bash
cargo +stable run -- sessions
```

List messages for the current working directory's project:

```bash
cargo +stable run -- messages
```

List messages across all projects:

```bash
cargo +stable run -- messages --all
```

Export all messages for the current working directory's project in chronological order:

```bash
cargo +stable run -- export
```

Generate a continuity brief for the current project:

```bash
cargo +stable run -- remember
```

## Messages command workflows

`mmr messages` supports three main workflows: paged browsing, latest-session inspection, and explicit session lookup.

### Browse messages with pagination metadata

```bash
cargo +stable run -- --source codex messages --project /Users/test/codex-proj --limit 2
```

`ApiMessagesResponse` includes:

- `messages`: returned message window
- `total_messages`: full scoped count before pagination
- `next_page`: whether another page exists
- `next_offset`: offset to use for the next page
- `next_command`: a ready-to-run follow-up command when another page exists

For the default `timestamp asc` sort, `messages` preserves the historical behavior of paging from the newest results while still returning each page in chronological order.

### Inspect the latest session in scope

Return the newest message from the latest matching session:

```bash
cargo +stable run -- messages --latest
```

Return the newest five messages from the latest matching session:

```bash
cargo +stable run -- --source codex messages --project /Users/test/codex-proj --latest 5
```

`--latest` defaults to `1` when you omit the value. The returned window is always chronological. Unlike normal pagination, latest-session queries do not emit a follow-up `next_command`.

### Slice a sorted message stream by message index

```bash
cargo +stable run -- --source codex messages \
  --project /Users/test/codex-proj \
  --from-message-index 10 \
  --to-message-index 20
```

- `--from-message-index` is inclusive
- `--to-message-index` is exclusive
- The index window is applied after source/project/session filtering and sorting

This is useful when you want a stable subrange without changing the main `--offset` pagination cursor.

### Look up a session directly

```bash
cargo +stable run -- messages --session sess-123
```

When `--session` is provided without `--project`, `mmr` searches across all projects instead of applying cwd project auto-discovery. If you also omit `--source`, the CLI prints a `stderr` hint suggesting `--source` to narrow the lookup.

## Default scoping and environment variables

`sessions` and `messages` auto-discover the current working directory's project unless:

- you pass `--project`
- you pass `--all`
- `MMR_AUTO_DISCOVER_PROJECT=0`

If cwd project discovery fails, the commands fall back to the global cross-project view. If discovery succeeds but there are no matching records, the command returns an empty result instead of widening scope.

Other defaults:

- `MMR_DEFAULT_SOURCE=codex|claude|cursor`
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini`

## Scripting and troubleshooting

- Pass `--project` and its value as separate arguments in scripts. Avoid a single token like `--project=\"/tmp/proj\"`, which can pass the quotes literally and break matching.
- Use `--all` when you want the old global `sessions`/`messages` behavior instead of cwd-local behavior.
- Use `--source` with `--session` to avoid broad cross-source session lookups.
- `remember` defaults to Cursor as the backend, and returns markdown unless you request JSON with `-O json`.

## Canonical specs

Behavior specs live under [`specs/`](specs/). The current canonical spec for the `messages` command is [`specs/messages.md`](specs/messages.md).
