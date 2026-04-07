# mmr

`mmr` is a Rust CLI for browsing local AI coding history from Claude, Codex, and Cursor.

It is designed for local analysis and scripting: command output stays machine-readable on `stdout`, while human hints and diagnostics go to `stderr`.

## Build and run

```bash
cargo build --release
./target/release/mmr --help
```

For development, run commands through Cargo:

```bash
cargo run -- projects
```

## Common workflows

List projects across all detected sources:

```bash
cargo run -- projects
```

List sessions for the current working directory's project:

```bash
cargo run -- sessions
```

List messages for the current working directory's project:

```bash
cargo run -- messages
```

Bypass cwd auto-discovery and search all projects:

```bash
cargo run -- messages --all
```

Look up one session directly:

```bash
cargo run -- messages --session sess-123 --source codex
```

Export a project's merged message history:

```bash
cargo run -- export --project /path/to/proj
```

Generate a continuity brief from previous sessions:

```bash
cargo run -- remember --project /path/to/proj
```

Pretty-print JSON output:

```bash
cargo run -- --pretty messages --all --limit 5
```

## Sources and project scoping

### Source selection

- `--source` accepts `claude`, `codex`, or `cursor`.
- Omitting `--source` searches all sources unless `MMR_DEFAULT_SOURCE` is set.
- `--source all` is not valid.

### CWD auto-discovery

- `sessions` and `messages` auto-discover the current working directory's project when `--project` is omitted.
- Use `--all` to disable that default and search across all projects.
- Set `MMR_AUTO_DISCOVER_PROJECT=0` to disable cwd auto-discovery by default.
- If cwd auto-discovery fails, the CLI falls back to a global search.
- If cwd auto-discovery succeeds but the project has no matching history, the CLI returns an empty result instead of widening scope.

### Session lookups

`messages --session <ID>` is the main exception to cwd scoping:

- Without `--project`, it searches across all projects.
- Without `--source`, it also searches across all sources and prints a narrowing hint on `stderr`.
- With `--project`, the explicit project still applies.

This keeps direct session lookup useful even when you run the command from an unrelated repository.

### Export and remember

- `export` uses the current working directory as the project when `--project` is omitted.
- `remember` also defaults its project from the current working directory.

For cwd-derived export lookups:

- Codex matches the canonical path directly, such as `/Users/alex/proj`.
- Claude and Cursor use the hyphenated form with a leading dash, such as `-Users-alex-proj`.

## Messages pagination

`messages` returns an `ApiMessagesResponse` envelope with:

- `messages`
- `total_messages`
- `next_page`
- `next_offset`
- `next_command` when another page is available

With the default timestamp sort, pagination is applied from the newest messages first, then the selected page is returned in chronological order. That makes transcript-style output readable while still letting callers page backward through recent history.

Example shape:

```json
{
  "messages": [],
  "total_messages": 42,
  "next_page": true,
  "next_offset": 10,
  "next_command": "mmr messages --limit 10 --offset 10"
}
```

## Environment variables

- `MMR_AUTO_DISCOVER_PROJECT=0|1` controls cwd auto-discovery for `sessions` and `messages`.
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` supplies a default `--source`.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies a default backend for `remember`.

## Troubleshooting and scripting notes

- If `sessions` or `messages` unexpectedly return no records, confirm whether cwd auto-discovery scoped the query to the current repository. Use `--all` to widen the search.
- If you already know the session ID, prefer `messages --session <ID>` and pass `--source` when possible to narrow the lookup.
- When scripting `mmr`, pass `--project` and the project value as separate arguments. Avoid a single argument like `--project=\"/path/to/proj\"`, which can pass the quotes literally and break matching.

## More documentation

- `AGENTS.md` - contributor-oriented command and architecture reference
- `adrs/002-cwd-scoped-defaults.md` - accepted design notes for cwd-scoped defaults
- `docs/references/session-lookup-invariants.md` - rules for `messages --session`
- `docs/references/schemas/claude/message_schema.md` - Claude history schema reference
- `docs/references/schemas/codex/message_schema.md` - Codex history schema reference
