# Session Lookup Invariants

This document specifies the invariant behavior when looking up messages by session ID.

## Invariant: `--session` bypasses cwd project auto-discovery

When `mmr messages --session <ID>` is called **without** an explicit `--project`, the command must search across **all projects** — the default cwd-based project auto-discovery is not applied.

### Rationale

A session ID is globally unique. The caller already knows which session they want; scoping the search to the cwd project would silently return zero results when the session belongs to a different project. This is confusing and defeats the purpose of the lookup.

### Rules

| Flags provided               | Project scope          | Hint printed |
| ----------------------------- | ---------------------- | ------------ |
| `--session`                   | All projects (no cwd)  | Yes (`--source`) |
| `--session --source X`        | All projects (no cwd)  | No           |
| `--session --all`             | All projects (no cwd)  | Yes (`--source`) |
| `--session --project P`       | Explicit project `P`   | No           |
| (no `--session`)              | cwd auto-discovery     | No           |

`--all` does not change session-lookup behavior once `--session` is present without `--project`; the lookup is already widened to all projects.

### Hint message

When `--session` is provided without `--source`, a hint is printed to stderr:

```
hint: searching all sources for session; pass --source to narrow the search
```

This nudges the caller toward a faster, more targeted lookup without blocking the operation.

### Pagination follow-up

`messages --session` returns the same pagination metadata as any other `messages` query:

- `next_page`: whether more matching messages remain
- `next_offset`: the offset to use for the next page
- `next_command`: the exact follow-up command to rerun

When `next_page` is `true`, prefer the emitted `next_command` instead of reconstructing the flags manually. This preserves the original source filter, offset, sort options, and session selector.

### Contract tests

Covered by integration tests in `tests/cli_contract.rs`:

- `messages_session_without_project_searches_all_projects` — session found even when cwd points to a different project
- `messages_session_without_project_or_source_prints_hint` — hint appears on stderr
- `messages_session_with_source_does_not_print_hint` — hint suppressed when `--source` provided
- `messages_session_with_explicit_project_uses_project_scope` — explicit `--project` still applies project scoping
- `messages_pagination_includes_next_page_and_next_command` — paginated responses include a reusable follow-up command
