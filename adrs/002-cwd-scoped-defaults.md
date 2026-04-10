# ADR-002: CWD-Scoped Defaults for Sessions and Messages

## Status

Accepted

## Date

2026-03-18

## Context

ADR-001 made `sessions` and `messages` progressively explorable by allowing all filters to be omitted. That improved discovery, but it also made the most common workflow noisier: users often run `mmr` from inside a project directory because they want the history for that project, not the combined history for every project on disk.

The repository already had one cwd-aware behavior in `export`, which infers the project from the current working directory using the canonical path for Codex and the slash-to-hyphen form for Claude. `remember` also already defaults its project from cwd, but it uses the raw `current_dir()` string rather than `export`'s canonicalized project-resolution path. `sessions` and `messages` were the remaining commands that still required an explicit `--project` to feel project-local.

The change must preserve two edge cases explicitly:

- If cwd auto-discovery fails, the CLI should not error; it should fall back to the previous global behavior.
- If cwd auto-discovery succeeds but that project has no messages, the CLI must return the empty result for that project instead of silently widening scope.

The change also introduces env-driven defaults for source selection and remember agent selection, so the command-line defaults can be tuned without changing every invocation.

## Decision

### `sessions` and `messages` auto-discover the cwd project by default

When the user omits `--project`, `sessions` and `messages` resolve the current working directory to a project identifier and scope the query to that project by default.

- Codex matching uses the canonical cwd path.
- Claude matching uses the same project resolution machinery already used by `export` and project filtering.
- If cwd discovery fails, the command falls back to the previous global behavior.
- If cwd discovery succeeds but no history matches, the command returns an empty result.

### `--all` disables the new default project scoping

Both `sessions` and `messages` gain `--all`.

- `--all` bypasses cwd project auto-discovery.
- `--all` does not change source behavior; `--source` and source defaults still apply.
- `--project` remains the explicit way to scope to a chosen project.

### New environment variables can supply defaults

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery for `sessions` and `messages`.
- `MMR_AUTO_DISCOVER_PROJECT=1` or unset keeps cwd project auto-discovery enabled.
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` supplies the default source filter when `--source` is omitted.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default `remember --agent` value when `--agent` is omitted. When unset, the default backend is Cursor (`composer-2-fast` unless `--model` overrides).

Empty or invalid values for `MMR_DEFAULT_SOURCE` and `MMR_DEFAULT_REMEMBER_AGENT` are treated as unset so the CLI remains usable.

## Consequences

- Running `mmr sessions` or `mmr messages` from inside a project directory is now project-local by default instead of global.
- Users can recover the historical global behavior with `--all` or `MMR_AUTO_DISCOVER_PROJECT=0`.
- The existing JSON response shapes do not change; only the default filters do.
- Repository guidance and contract tests must be updated to prevent stale assumptions that unfiltered `sessions` or `messages` are always global.
