# mmr

`mmr` is a Rust CLI for browsing local AI coding history from Claude, Codex, and Cursor.
It reads transcripts from local tool storage, aggregates them into projects/sessions/messages,
and returns machine-readable JSON on stdout.

## What it is for

- Audit local coding-assistant activity by project, session, or message
- Export a project's full transcript stream as chronological JSON
- Generate a stateless continuity brief from prior sessions with `remember`
- Script against stable JSON responses instead of parsing human-oriented terminal output

## Requirements

- Rust with Edition 2024 support (use a current stable toolchain)
- Access to the local history directories for the sources you want to query

Build or run from the repository root:

```bash
cargo +stable build
cargo +stable run -- projects
```

## Common workflows

### List projects

```bash
cargo +stable run -- projects
cargo +stable run -- --source cursor projects
```

### Browse sessions for the current project

```bash
cargo +stable run -- sessions
```

By default, `sessions` scopes to the current working directory when project auto-discovery
succeeds.

### Browse messages

```bash
cargo +stable run -- messages
cargo +stable run -- messages --all
cargo +stable run -- --source codex messages --project /Users/test/codex-proj
```

### Look up one session directly

```bash
cargo +stable run -- messages --session sess-123
```

When `--session` is provided without `--project`, `mmr` searches across all projects instead of
applying cwd-based project scoping. If `--source` is also omitted, the CLI prints a stderr hint
telling you that `--source` can narrow the lookup.

### Export a project's full transcript stream

```bash
cargo +stable run -- export
cargo +stable run -- export --project /Users/test/codex-proj
```

`export` always returns chronological messages and reuses the same JSON shape as `messages`.

### Generate a continuity brief

```bash
cargo +stable run -- remember --project /Users/test/codex-proj
cargo +stable run -- remember all --project /Users/test/codex-proj
cargo +stable run -- remember session sess-123 --project /Users/test/codex-proj
```

## Key behavior contracts

### Project scoping defaults

- `sessions` and `messages` auto-discover the cwd project unless you pass `--project`,
  pass `--all`, or set `MMR_AUTO_DISCOVER_PROJECT=0`.
- If cwd auto-discovery fails, the CLI falls back to the historical global behavior.
- If cwd auto-discovery succeeds but the discovered project has no history, the CLI returns an
  empty result instead of widening scope.

### Source filtering

- `--source` accepts `claude`, `codex`, or `cursor`.
- Omitting `--source` queries all sources unless `MMR_DEFAULT_SOURCE` supplies a default.
- `--source all` is not a valid value.

### Project identifier matching

When you omit `--project`, different sources derive the project identifier differently:

| Source | Derived identifier |
| --- | --- |
| Codex | Canonical cwd path, e.g. `/Users/mish/proj` |
| Claude | Canonical cwd path encoded as `-Users-mish-proj` |
| Cursor | Same encoded form as Claude |

That matters most for `export`, which queries each source using its own derived project value when
`--project` is omitted.

### Messages pagination

`messages` responses include pagination metadata:

```json
{
  "messages": [],
  "total_messages": 6,
  "next_page": true,
  "next_offset": 2,
  "next_command": "mmr --source codex messages --project /Users/test/codex-proj --limit 2 --offset 2"
}
```

- `next_page` indicates whether another page exists
- `next_offset` is the offset for the next request
- `next_command` is a ready-to-run follow-up command when another page exists

With the default `messages` sort (`timestamp` ascending), pagination still works from the newest
window first and then returns that page in chronological order. This preserves the historical CLI
contract while keeping page-to-page navigation practical.

## `remember` backends

`remember` sends session transcripts to one of three backends:

| Agent | Requirements | Default model behavior |
| --- | --- | --- |
| `cursor` | `CURSOR_API_KEY` and the `agent` CLI on `PATH` | Defaults to `composer-2-fast` unless `--model` is set |
| `codex` | Codex auth configured for the local Codex client | Uses the backend default unless overridden |
| `gemini` | `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL` | Defaults to `gemini-3.1-flash-lite-preview` unless `--model` is set |

When `--agent` is omitted, `MMR_DEFAULT_REMEMBER_AGENT` applies if set; otherwise the default is
`cursor`.

`--instructions` replaces the default output-format section of the Memory Agent system prompt while
preserving the base identity and input-format instruction.

## Environment variables

| Variable | Effect |
| --- | --- |
| `MMR_AUTO_DISCOVER_PROJECT=0` | Disable cwd project auto-discovery for `sessions` and `messages` |
| `MMR_AUTO_DISCOVER_PROJECT=1` or unset | Keep cwd project auto-discovery enabled |
| `MMR_DEFAULT_SOURCE=codex\|claude\|cursor` | Set the default `--source` when the flag is omitted |
| `MMR_DEFAULT_REMEMBER_AGENT=cursor\|codex\|gemini` | Set the default `remember --agent` when the flag is omitted |
| `GEMINI_API_BASE_URL` | Override the Gemini Interactions API base URL |

Invalid or empty `MMR_DEFAULT_SOURCE` / `MMR_DEFAULT_REMEMBER_AGENT` values are treated as unset.

## Troubleshooting and pitfalls

### "I expected project-local results but got global results"

Cwd-based scoping only applies when `mmr` can successfully canonicalize the current directory.
If discovery fails, the CLI falls back to all projects. Use `--project` for an explicit scope.

### "I expected cross-project results but got an empty list"

If cwd auto-discovery succeeds, `sessions` and `messages` stay scoped to that project even when the
project has no history. Pass `--all` to bypass auto-discovery.

### "My explicit project filter does not match"

Project names are source-specific. Codex uses canonical paths; Claude and Cursor use the encoded
hyphenated form internally. When scripting, pass `--project` and its value as separate arguments
instead of a single quoted `--project="..."` token.

For Codex project filters, `mmr` accepts either a leading-slash path or the same path without the
leading slash and normalizes both forms during lookup.

### "The toolchain is too old for this repo"

This repository uses Rust Edition 2024. If the default `cargo` is too old, run commands with a
current stable toolchain, for example:

```bash
cargo +stable test
```

## More reference material

- `AGENTS.md` - contributor-oriented command and contract reference
- `adrs/002-cwd-scoped-defaults.md` - decision record for cwd project scoping and env defaults
- `docs/references/session-lookup-invariants.md` - `messages --session` lookup rules
- `.agents/skills/mmr-clap-colored-cli/references/mmr-query-contract.md` - internal JSON/query contract notes
