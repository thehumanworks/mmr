# mmr

Browse local Claude, Codex, and Cursor conversation history from the command line.

`mmr` reads transcript JSONL files from your machine and prints machine-readable results to `stdout`. It is designed for local exploration, scripting, and continuity workflows.

## What `mmr` reads

By default, `mmr` reads local history under your home directory:

- Codex: `~/.codex/sessions` and `~/.codex/archived_sessions`
- Claude: `~/.claude/projects`
- Cursor: `~/.cursor/projects`

Set `SIMPLEMMR_HOME=/path/to/home` to point `mmr` at an alternate history root for tests, sandboxes, or CI.

## Build and run

```bash
cargo build --release
./target/release/mmr --help
```

For local iteration:

```bash
cargo run -- --help
```

## Quick start

List projects across all discovered sources:

```bash
cargo run -- projects
```

List sessions for the current working directory's project:

```bash
cargo run -- sessions
```

List messages for one known session ID:

```bash
cargo run -- messages --session sess-123
```

Export all messages for a project as chronological JSON:

```bash
cargo run -- export --project /path/to/project
```

Generate a continuity brief as JSON instead of the default Markdown:

```bash
cargo run -- remember --project /path/to/project -O json
```

## Output behavior

- `projects`, `sessions`, `messages`, and `export` write JSON to `stdout`.
- `remember` defaults to Markdown on `stdout`. Use `-O json` for automation.
- Human-facing hints and diagnostics go to `stderr`.
- `--pretty` pretty-prints JSON responses.

## Command behavior

### Source filtering

Use `--source claude`, `--source codex`, or `--source cursor` to narrow queries. If `--source` is omitted, `mmr` searches all sources unless `MMR_DEFAULT_SOURCE` supplies a default.

### Project scoping for `sessions` and `messages`

`sessions` and `messages` auto-discover the current working directory's project when all of the following are true:

- `--project` is omitted
- `--all` is omitted
- `MMR_AUTO_DISCOVER_PROJECT` is unset or `1`

Important constraints:

- If cwd auto-discovery fails, the command falls back to all projects.
- If cwd auto-discovery succeeds but no history matches, the command returns an empty result instead of widening scope.
- `--all` disables cwd-based project scoping.

### Session lookup by ID

`mmr messages --session <ID>` behaves differently from plain `messages`:

- Without `--project`, it searches all projects instead of using cwd auto-discovery.
- Without `--source`, it prints a hint on `stderr` suggesting `--source` for a narrower lookup.
- With `--project`, the explicit project still applies.

See `docs/references/session-lookup-invariants.md` for the full contract.

### `export`

`mmr export` returns an `ApiMessagesResponse` with messages sorted chronologically.

- `mmr export --project /path/to/project` queries that explicit project.
- `mmr export` infers the project from the current working directory and queries all sources unless `--source` narrows the search.

### `messages` pagination

`mmr messages` paginates from the newest history window and returns the selected page in chronological order. Responses include:

- `next_page`
- `next_offset`
- `next_command` when another page is available

`next_command` is a ready-to-run CLI invocation for fetching the next page with the same filters.

### `remember`

`remember` generates a stateless continuity brief from prior sessions:

- Default selection: latest matching session
- `remember all`: use all matching sessions
- `remember session <session-id>`: use one specific session
- Default agent: `cursor` unless `MMR_DEFAULT_REMEMBER_AGENT` overrides it
- Default output format: Markdown (`-O md`)

If you pass `--instructions`, your text replaces the default output-instruction section of the memory-agent system prompt while preserving the base input-format instruction.

## Environment and backend configuration

| Setting | Purpose |
| --- | --- |
| `SIMPLEMMR_HOME` | Override the home directory used to locate Codex, Claude, and Cursor history. |
| `MMR_AUTO_DISCOVER_PROJECT=0` | Disable cwd auto-discovery for `sessions` and `messages`. |
| `MMR_DEFAULT_SOURCE=codex|claude|cursor` | Supply a default source filter when `--source` is omitted. |
| `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` | Supply the default backend for `remember`. |

Backend-specific requirements for `remember`:

- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL`
- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: Codex CLI authentication configured for `codex exec`

## Scripting notes

- When invoking `mmr` from code, pass `--project` and the project value as separate arguments.
- If you only need the message array, you can pipe `messages` or `export` output through `jq '.messages'`.
- Prefer `-O json` for `remember` in scripts so the output shape is explicit.

## Troubleshooting

- `sessions` or `messages` returned fewer results than expected: run again with `--all` to bypass cwd auto-discovery.
- `messages --session` feels slow: add `--source` to avoid searching every source.
- `remember` printed plain text instead of JSON: rerun with `-O json`.
- No history found in CI or a sandbox: verify `SIMPLEMMR_HOME` points at the expected transcript root.

## Further reading

- `AGENTS.md` - contributor workflow, command catalog, and testing guidance
- `adrs/002-cwd-scoped-defaults.md` - cwd-scoped default behavior
- `docs/references/session-lookup-invariants.md` - `messages --session` behavior contract
- `docs/references/schemas/` - source transcript schema references
