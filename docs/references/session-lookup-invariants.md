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
| `--session --project P`       | Explicit project `P`   | No           |
| `--session-back` / `--session-range` | cwd auto-discovery (recency computed within scope) | No |
| `--session-back --all`        | All projects (recency computed across all) | No |
| (no `--session`)              | cwd auto-discovery     | No           |

## Invariant: recency selectors stay cwd-scoped; literal `--session <id>` stays global

A literal `--session <id>` is an identity lookup: the caller already knows which
session they want, so it bypasses cwd auto-discovery and searches all projects.

The recency selectors `--session-back` and `--session-range` are **not** identity
lookups — "age 1" only has meaning relative to a scope. They therefore keep the
default cwd-project scope (ADR-002): a bare `mmr messages --session-back 1` means
"the previous session in this cwd project". Widen the recency scope explicitly
with `--all` or `--project`. See ADR-004 for the full rationale, including why
age 0 (the assumed-live newest session) is held back by default.

### Hint message

When `--session` is provided without `--source`, a hint is printed to stderr:

```
hint: searching all sources for session; pass --source to narrow the search
```

This nudges the caller toward a faster, more targeted lookup without blocking the operation.

### Contract tests

Covered by integration tests in `tests/cli_contract.rs`:

- `messages_session_without_project_searches_all_projects` — session found even when cwd points to a different project
- `messages_session_without_project_or_source_prints_hint` — hint appears on stderr
- `messages_session_with_source_does_not_print_hint` — hint suppressed when `--source` provided
- `messages_session_with_explicit_project_uses_project_scope` — explicit `--project` still applies project scoping
