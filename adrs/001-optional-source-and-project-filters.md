# ADR-001: Optional Source and Project Filters

## Status

Accepted

## Date

2026-02-25

## Context

The original CLI required explicit `--source` for `sessions` (only `codex` or `claude`), required `--project` for `sessions`, and only supported `--session` for `messages`. The `--source` flag also accepted `all` as a value. This forced users to know exactly which source and project they wanted before querying, preventing exploratory workflows like "show me all my recent sessions" or "find messages across both sources for this project".

## Decision

### `--source` accepts only `claude` and `codex`

`--source all` is removed. Omitting `--source` is equivalent to querying both sources. This eliminates a redundant enum variant and makes the absence of a flag the natural "everything" default.

### All filters are optional for `sessions` and `messages`

**`sessions` command:**
- `--project` is optional. Without it, sessions from all projects are returned.
- `--source` is optional. Without it, sessions from both sources are returned.
- When `--project` is given without `--source`, the app searches both sources for the project using best-effort path normalization.

**`messages` command:**
- `--session` is optional. Without it, messages from all sessions are returned.
- `--project` is optional (newly added). Filters messages to the given project.
- `--source` is optional. Filters messages to the given source.
- All three filters compose: you can use any combination.

### Per-item metadata on response objects

`ApiSession` now includes `source`, `project_name`, and `project_path` fields.
`ApiMessage` now includes `session_id`, `source`, and `project_name` fields.

This ensures each item is self-describing regardless of whether envelope-level filters were applied. The `ApiSessionsResponse` envelope was simplified to `{ sessions, total_sessions }` and `ApiMessagesResponse` to `{ messages, total_messages }`.

### `projects` command

`projects` without `--source` now returns projects from both sources (previously defaulted to codex). Adding `--source codex` or `--source claude` filters to a single source.

## Consequences

- Breaking change: `--source all` is no longer valid and will be rejected by clap.
- Breaking change: `projects` without `--source` now returns both sources instead of codex-only.
- Breaking change: `ApiSessionsResponse` and `ApiMessagesResponse` envelopes changed shape. Consumers that relied on `project_name`/`source` at the envelope level should read per-item fields instead.
- New capability: `sessions` and `messages` support progressive drill-down without requiring all filters upfront.
- Project resolution across sources uses the existing Codex path normalization logic, extended to search both sources when no `--source` is specified.
