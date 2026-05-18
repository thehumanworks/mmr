# mmr

`mmr` is a Rust CLI for querying local AI coding history from Claude, Codex, Cursor, Grok, and Pi.

It exposes a small read-only command surface:

- `projects` for project-level summaries
- `sessions` for session discovery
- `messages` for paginated transcript windows
- `export` for a full project transcript across matching sources
- `remember` for a resume brief generated from prior sessions

## Toolchain and setup

This repository uses Rust edition `2024`. In this cloud environment, the preinstalled `cargo 1.83.0` is too old to parse `edition = "2024"`, so prefer the current stable toolchain:

```bash
cargo +stable --version
cargo +stable build
```

`mmr` reads history from the current user's home directory by default. Set `SIMPLEMMR_HOME` to point at a fixture tree or alternate history root.

## Quick start

```bash
# List projects across all sources
cargo +stable run -- projects

# Scope sessions to the current working directory when auto-discovery succeeds
cargo +stable run -- sessions

# Search all projects instead of the cwd-derived project
cargo +stable run -- sessions --all

# Return the newest 20 messages from the latest session in scope
cargo +stable run -- messages --latest 20

# Export all messages for a specific project as JSON
cargo +stable run -- export --project /path/to/proj

# Generate a continuity brief as JSON instead of the default markdown
cargo +stable run -- remember --project /path/to/proj -O json
```

## Project scoping and source matching

`sessions` and `messages` share the same default project-resolution behavior:

- If `--project` is omitted and `--all` is not set, the CLI tries to auto-discover the current working directory as the project scope.
- If cwd auto-discovery fails, the command falls back to the historical global behavior across all projects.
- If cwd auto-discovery succeeds but that project has no matching records, the command returns an empty result instead of widening scope silently.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery. Unset or `1` keeps it enabled.

`messages --session <id>` has one special rule: when `--project` is omitted, it searches all projects instead of applying cwd auto-discovery. If `--source` is also omitted, the CLI prints this hint on `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

`export` without `--project` resolves the current directory differently per source:

- Codex, Grok, and Pi match the canonical path directly (for example `/Users/me/proj`).
- Claude and Cursor match the same path encoded with `/` replaced by `-` and a leading hyphen (for example `-Users-me-proj`).

Explicit `--project` queries search across all sources unless `--source` is provided. One current constraint is worth knowing: Cursor transcripts are stored under encoded project directory names and the generic project resolver only matches that encoded Cursor name, so direct Cursor-only `--project` lookups should use values such as `-Users-me-proj`. `export` without `--project` and cwd auto-discovery already perform that translation for you.

## Messages contract highlights

The canonical behavior spec lives in [`specs/messages.md`](specs/messages.md). The most important operational details are:

- Default query options are `--limit 50 --offset 0 --sort-by timestamp --order asc`.
- `total_messages` reports the full number of scoped messages before message-index slicing and pagination.
- `next_page` and `next_offset` are computed against the selected window after any `--from-message-index` / `--to-message-index` range is applied.
- `next_command` is included only when another page exists and `--latest` is not in use.

Pagination is intentionally asymmetric:

- With the default `timestamp asc` ordering, `messages` pages from the newest end of the chronological list and then returns that window in chronological order.
- With any other sort/order pair, `messages` sorts first and then applies `--offset` / `--limit` directly.

`--latest` uses a different path:

- It selects the latest session in the current scope.
- It returns the newest `N` messages from that one session in chronological order.
- It always returns `next_page: false` and omits `next_command`.

## Remember command

`remember` can call three backends:

- `cursor` (default when `--agent` and `MMR_DEFAULT_REMEMBER_AGENT` are both omitted)
- `codex`
- `gemini`

Environment requirements:

- Cursor: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex: Codex CLI authentication as configured for `codex exec`
- Gemini: `GOOGLE_API_KEY` or `GEMINI_API_KEY`, plus optional `GEMINI_API_BASE_URL`

Two behavior details matter for automation:

- The default output format is markdown (`-O md`). Use `-O json` for machine-readable output.
- `--instructions` replaces the default output-format section of the system prompt, but preserves the Memory Agent identity and transcript-input description.

## Environment variables

- `SIMPLEMMR_HOME`: override the history root instead of reading from the current user's home directory
- `MMR_AUTO_DISCOVER_PROJECT=0|1`: disable or enable cwd project auto-discovery for `sessions` and `messages`
- `MMR_DEFAULT_SOURCE=claude|codex|cursor|grok|pi`: supply the default `--source` when the flag is omitted
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini`: supply the default `remember --agent`

Invalid values for `MMR_DEFAULT_SOURCE` and `MMR_DEFAULT_REMEMBER_AGENT` are treated as unset.

## Deeper references

- [`specs/messages.md`](specs/messages.md): canonical `messages` behavior, scope resolution, and pagination contract
- [`docs/references/session-lookup-invariants.md`](docs/references/session-lookup-invariants.md): `messages --session` scope rules and stderr hint contract
- [`docs/references/schemas/codex/message_schema.md`](docs/references/schemas/codex/message_schema.md): Codex raw transcript schema
- [`docs/references/schemas/claude/message_schema.md`](docs/references/schemas/claude/message_schema.md): Claude raw transcript schema
- [`docs/references/schemas/grok/message_schema.md`](docs/references/schemas/grok/message_schema.md): Grok raw transcript schema
- [`docs/references/schemas/pi/message_schema.md`](docs/references/schemas/pi/message_schema.md): Pi raw transcript schema
- [`AGENTS.md`](AGENTS.md): contributor workflow, module map, and verification guidance

## Common pitfalls

- If `cargo` fails before compilation starts, check the toolchain first; Cargo 1.83.0 is too old for this repo's Rust 2024 edition.
- When invoking `mmr` from another program, pass `--project` and the project value as separate argv items. Avoid a single quoted argument such as `--project=\"/path\"`.
- If `messages --session` feels broader than expected, add `--source` to avoid an all-sources lookup.
