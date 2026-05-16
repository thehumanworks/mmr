# Messages Command

This spec is the canonical contract for `mmr messages`.

## Scope resolution

`messages` resolves scope in this order:

1. `--project <value>` uses the explicit project filter.
2. `--all` disables cwd project auto-discovery.
3. Otherwise, the CLI tries to auto-discover the current project from the working directory.

### Auto-discovery edge cases

- If cwd auto-discovery fails, `messages` falls back to all matching projects and sources.
- If cwd auto-discovery succeeds but no records match that discovered project, `messages` returns an empty result instead of falling back globally.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery. Unset, empty, or `1` keeps it enabled.

## `--session` without `--project`

When `--session <ID>` is provided without `--project`, `messages` searches across all projects instead of applying cwd auto-discovery.

Additional rules:

- `--source` still narrows the lookup when provided.
- If `--source` is omitted, the CLI prints this hint to stderr:

```text
hint: searching all sources for session; pass --source to narrow the search
```

## Ordering and pagination

### Default ordering

The default sort is:

- `--sort-by timestamp`
- `--order asc`

### Historical pagination contract

When sorting by ascending timestamp, pagination does **not** page from the oldest messages. Instead:

1. the command selects the newest window using `limit` and `offset`
2. that selected window is returned in chronological order

This behavior is intentional and must remain stable for scripts and tests that rely on it.

When sorting by anything other than ascending timestamp, normal pagination is applied to the already sorted list.

## Response shape

`messages` returns `ApiMessagesResponse`:

- `messages`: the current page of `ApiMessage` items
- `total_messages`: total count before pagination and before any message-index range is applied
- `next_page`: whether another page exists inside the selected result set
- `next_offset`: offset to use for the next page
- `next_command`: a follow-up `mmr messages ...` command when `next_page` is true

`next_command` must preserve:

- `--source`
- `--session`
- `--project`
- `--all`
- `--from-message-index`
- `--to-message-index`
- `--limit`
- `--offset`
- any non-default `--sort-by` or `--order`

`next_command` is omitted when there is no next page.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Additional rules:

- `--latest` without a value defaults to `1`
- the returned window is ordered chronologically
- `total_messages` reports the full message count for the selected latest session before the latest-window truncation
- `next_page` is always `false`
- `next_command` is always omitted

Existing scope filters still apply, including `--source`, `--project`, `--all`, and `--session`.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after source, project, session, sort, and latest-session filtering.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied:

- `--from-message-index` is inclusive
- `--to-message-index` is exclusive

Additional rules:

- the returned window remains ordered according to the selected message ordering
- `total_messages` continues to report the full scoped message count before applying the message-index range
- pagination metadata (`next_page` and `next_offset`) applies to the ranged selection, not to the full pre-range message set
