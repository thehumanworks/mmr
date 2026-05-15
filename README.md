# mmr

`mmr` is a Rust CLI for browsing local AI conversation history from Claude, Codex, Cursor, and Pi.

- Machine-readable output stays on `stdout` as JSON.
- Human-facing diagnostics and hints go to `stderr`.
- Query commands can search one source or merge results across all supported sources.

For contributor workflow details, see [AGENTS.md](AGENTS.md). For the canonical `messages` behavior contract, see [specs/messages.md](specs/messages.md).

## What `mmr` reads

`mmr` loads history from the current user's home directory by default:

- Codex: `~/.codex/sessions/*.jsonl`
- Claude: `~/.claude/projects/<project>/*.jsonl`
- Cursor: `~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl`
- Pi: `~/.pi/agent/sessions/<project>/*.jsonl`

Set `SIMPLEMMR_HOME=/path/to/home` to point the loaders at a different home root. This is useful for tests, fixtures, or debugging a copied history tree.

## Build and run

`mmr` targets Rust edition 2024. Use a Rust toolchain that supports edition 2024; current stable works.

```bash
cargo run -- projects
cargo run -- sessions
cargo run -- messages
```

If your default Cargo is older than edition 2024, run commands with `cargo +stable`.

## Common workflows

### Inspect projects

```bash
cargo run -- projects
cargo run -- --source pi projects
```

### Inspect sessions

`sessions` defaults to the current project when cwd auto-discovery succeeds. Use `--all` to search across projects.

```bash
cargo run -- sessions
cargo run -- sessions --all
cargo run -- --source codex sessions --project /Users/test/codex-proj
```

### Inspect messages

`messages` uses the same default cwd project scoping as `sessions`.

```bash
cargo run -- messages
cargo run -- messages --all --limit 100
cargo run -- messages --latest 10
cargo run -- messages --from-message-index 20 --to-message-index 40
cargo run -- --source claude messages --project my-proj
```

Looking up a specific session is broader by default:

```bash
cargo run -- messages --session sess-123
```

When `--session` is provided without `--project`, `mmr` bypasses cwd project auto-discovery and searches all projects. If `--source` is also omitted, it prints a hint on `stderr` so you can narrow the lookup.

### Export one project's merged transcript

```bash
cargo run -- export
cargo run -- export --project /Users/test/proj
cargo run -- --source cursor export --project /Users/test/cursor-proj
```

`export` returns the same `ApiMessagesResponse` envelope as `messages`, but with all matching messages sorted chronologically.

### Generate a continuity brief

`remember` defaults to the current working directory as its project and returns markdown unless you ask for JSON.

```bash
cargo run -- remember --project /Users/test/proj
cargo run -- remember all --project /Users/test/proj
cargo run -- remember session sess-123 --project /Users/test/proj
cargo run -- remember --project /Users/test/proj -O json
```

## Scope and source rules

- `--source` accepts `claude`, `codex`, `cursor`, or `pi`.
- Omitting `--source` searches all sources unless `MMR_DEFAULT_SOURCE` provides a default.
- `sessions` and `messages` auto-discover the cwd project unless you pass `--project`, pass `--all`, or use `messages --session <id>` without `--project`.
- `export` without `--project` derives project IDs from the current working directory:
  - Codex and Pi use the canonical filesystem path.
  - Claude and Cursor use the same path with `/` replaced by `-` and a leading `-`.

## Response shape notes

- `projects` returns `ApiProjectsResponse`.
- `sessions` returns `ApiSessionsResponse`.
- `messages` and `export` return `ApiMessagesResponse`.
- `messages` includes `next_page`, `next_offset`, and `next_command` for paginated browsing.
- Standard `messages` pagination selects the newest matching window first, then returns that window in chronological order.
- `messages --latest [N]` selects the latest session in scope and returns the chronological tail of that session.

## Environment variables

### Query defaults

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery for `sessions` and `messages`.
- `MMR_DEFAULT_SOURCE=codex|claude|cursor|pi` sets the default `--source` when the flag is omitted.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` sets the default backend for `remember`.

### Loader root

- `SIMPLEMMR_HOME=/path/to/home` overrides the home directory used for source discovery.

### `remember` backends

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL`
- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: authenticated `codex exec` environment

## Troubleshooting and common pitfalls

- `messages --session <id>` intentionally searches across projects when `--project` is omitted. Add `--source` or `--project` to narrow the lookup.
- When scripting, pass `--project` and the project value as separate arguments. Avoid a single quoted argument like `--project=\"/path\"`.
- `remember` returns markdown by default. Use `-O json` if another program needs machine-readable output.
- If `sessions` or `messages` returns no records from a project directory, that may mean cwd auto-discovery succeeded but the project has no matching history. Use `--all` to widen the query explicitly.
