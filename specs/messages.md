# Messages Command

`mmr messages` returns `ApiMessagesResponse` JSON for the current message scope.

## Scope selection

- `--project <value>` scopes the query to one project.
- `--all` disables cwd project auto-discovery and searches all projects in scope.
- With no `--project` and no `--all`, the command auto-discovers the current project from the current working directory.
- If cwd auto-discovery fails, the command falls back to all projects.
- If cwd auto-discovery succeeds but the discovered project has no matching records, the command returns an empty result instead of widening scope.

## `--session`

`mmr messages --session <id>` searches for the session across all projects when `--project` is omitted, even if cwd auto-discovery would otherwise scope the query.

- If `--source` is also omitted, the CLI prints a narrowing hint on `stderr`.
- If `--project` is provided alongside `--session`, the explicit project filter still applies.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

The returned latest-session window is ordered chronologically. Existing scope filters still apply, including `--source`, `--project`, `--all`, and `--session`.

## Message index range

`--from-message-index` and `--to-message-index` slice the filtered and sorted message list using zero-based indexes.

- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- If both are provided, `from <= to` is required.

## Pagination

Standard `messages` queries return:

- `messages`
- `total_messages`
- `next_page`
- `next_offset`
- `next_command` when another page exists

Pagination is applied from the newest sorted window, but the returned `messages` array remains in chronological order.
