# Query Scoping and Source Selection

This document captures the default scoping, source-selection, and source-specific project matching rules for the `mmr` CLI.

## Supported sources

`mmr` supports four history backends:

- `claude`
- `codex`
- `cursor`
- `pi`

`--source` accepts only those values.

- Omitting `--source` means "all supported sources" unless `MMR_DEFAULT_SOURCE` is set.
- `MMR_DEFAULT_SOURCE=claude|codex|cursor|pi` supplies the default source filter when the flag is omitted.
- An explicit `--source` flag always overrides `MMR_DEFAULT_SOURCE`.
- `--source all` is not valid. Omit the flag instead.

Examples:

```bash
mmr projects
mmr --source pi sessions --all
MMR_DEFAULT_SOURCE=codex mmr messages --all
```

## Project identifiers by source

The query layer works across source-specific project naming schemes, but the underlying identifiers differ by backend:

- **Codex** stores and matches projects by canonical filesystem path.
- **Pi** stores `project_path` from the session `cwd` and follows Codex-style path matching for cwd-based export.
- **Claude** stores a slash-to-hyphen project name with a leading `-`.
- **Cursor** uses the same project-name form as Claude for cwd-based export.

When `--project` is provided without `--source`, the query service attempts source-aware matching using each source's stored project names and original paths.

### Pi-specific notes

Pi sessions are loaded from `~/.pi/agent/sessions/**/*.jsonl`.

- `project_name` comes from the session file's parent directory name.
- `project_path` comes from the session's `cwd`.

This means Pi `projects` output can show a storage-oriented name while `original_path` and session `project_path` still point at the real filesystem path.

## Default project scoping for `sessions` and `messages`

`mmr sessions` and `mmr messages` default to the current project when cwd auto-discovery succeeds.

- If `--project` is provided, that explicit project wins.
- If `--all` is provided, cwd auto-discovery is skipped.
- If `MMR_AUTO_DISCOVER_PROJECT=0`, cwd auto-discovery is disabled.
- If cwd auto-discovery fails, the commands fall back to the global all-projects behavior.
- If cwd auto-discovery succeeds but the project has no matching history, the command returns an empty result instead of widening scope.

Examples:

```bash
mmr sessions
mmr messages
mmr messages --all
MMR_AUTO_DISCOVER_PROJECT=0 mmr sessions
mmr messages --project /Users/test/proj
```

## `messages --session` invariant

When `mmr messages --session <ID>` is called without an explicit `--project`, the command searches across all projects instead of applying cwd auto-discovery.

- This preserves direct session lookup even when the current directory points at a different project.
- If `--source` is also omitted, the command searches all sources and prints a stderr hint:

```text
hint: searching all sources for session; pass --source to narrow the search
```

`--project` still re-enables explicit project scoping for a session lookup.

## `export`

`mmr export` returns the same `ApiMessagesResponse` shape as `messages`, but always emits the full scoped message set in chronological order.

- `mmr export --project <path>` queries that project directly.
- `mmr export` without `--project` infers the project from the current working directory.
- In cwd mode:
  - Codex and Pi use the canonical cwd path.
  - Claude and Cursor use the slash-to-hyphen name derived from that path.
- `--source` filters which source-specific query paths are used.

Examples:

```bash
mmr export
mmr export --project /Users/test/proj
mmr --source cursor export
mmr --source pi export --project /Users/test/pi-proj
```

## `remember`

`mmr remember` generates a continuity brief from prior sessions in the selected project scope.

- `--project` is optional; omitting it uses the current directory path.
- `--agent` accepts `cursor`, `codex`, or `gemini`.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default agent when `--agent` is omitted.
- If neither the flag nor the env var is set, the default agent is `cursor`.
- Selectors:
  - no selector: latest matching session
  - `all`: all matching sessions
  - `session <id>`: one specific session
- `--source` narrows which histories are included before transcript formatting.
- `--instructions` replaces the default output/rules portion of the memory-agent system instruction while preserving the base agent identity and transcript-input description.

Examples:

```bash
mmr remember --project /Users/test/proj
mmr remember all --project /Users/test/proj --agent gemini -O json
mmr --source codex remember session sess-123 --project /Users/test/proj
MMR_DEFAULT_REMEMBER_AGENT=gemini mmr remember --project /Users/test/proj -O json
```

## Common pitfalls

- Do not pass `--source all`; omit `--source` to query every source.
- Prefer `--project` in scripts when the working directory may be ambiguous or different from the target project.
- For Pi history, prefer `original_path` or session `project_path` when you need the real filesystem path; `project_name` may reflect the Pi storage directory name instead.
- `messages --session <ID>` is intentionally broader than default `messages`; add `--source` or `--project` if you want to narrow the lookup.
