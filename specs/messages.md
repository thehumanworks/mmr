# Messages Command

`mmr messages` returns an `ApiMessagesResponse` with:

- `messages`: the returned message slice
- `total_messages`: the full filtered result count before windowing or pagination
- `next_page`: whether another page is available for the same query
- `next_offset`: the offset to use for the next page
- `next_command`: a ready-to-run follow-up command when `next_page` is `true`

Each returned message includes per-item `source` and `project_name` metadata so mixed-source results remain self-describing.

## Scope resolution

`messages` accepts optional `--project`, optional `--all`, and optional `--session`.

- If `--project` is provided, `messages` uses that explicit project scope.
- If `--project` is omitted and `--all` is omitted, `messages` auto-discovers the current working directory as the default project scope.
- If cwd auto-discovery fails, `messages` falls back to searching all projects.
- If cwd auto-discovery succeeds but there are no matching records, `messages` returns an empty result instead of widening the scope.
- `--all` disables cwd project auto-discovery.

### `--session` without `--project`

`mmr messages --session <id>` intentionally bypasses cwd project auto-discovery when `--project` is not also provided.

- This lets a session lookup search all projects by default.
- If `--source` is also omitted, the CLI prints a `stderr` hint suggesting `--source` to narrow the search.
- If `--project` is supplied alongside `--session`, the explicit project filter applies normally.

### Source defaults

- `--source` accepts `codex`, `claude`, or `cursor`.
- Omitting `--source` uses `MMR_DEFAULT_SOURCE` when it is set to one of those values.
- Empty or invalid `MMR_DEFAULT_SOURCE` values are treated as unset.
- `--source all` is not a valid value.

## Ordering and pagination

By default, `messages` sorts by `timestamp` ascending.

For the default `timestamp asc` mode, pagination preserves the historical contract:

1. select the newest window using `limit` and `offset`
2. return that window in chronological order

This means the returned page is still oldest-to-newest within the page, even though pagination advances from the newest end of the result set.

For non-default sort modes, pagination is applied directly to the sorted result set.

When another page exists:

- `next_page` is `true`
- `next_offset` is the value to pass to `--offset`
- `next_command` preserves the original query shape, including `--source`, `--project`, `--all`, `--session`, message index range flags, `--limit`, `--sort-by`, and `--order`

When all results fit in the current page, `next_page` is `false` and `next_command` is `null`.

## Message index ranges

`messages` supports filtering the sorted result set with zero-based message indices:

- `--from-message-index <N>` includes messages starting at index `N`
- `--to-message-index <N>` stops before index `N`

Constraints:

- both indices are applied after filtering and sorting
- `--from-message-index` is inclusive
- `--to-message-index` is exclusive
- `--from-message-index` must be less than or equal to `--to-message-index`

Index-range filtering happens before `limit` and `offset` pagination are applied.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Latest-session behavior:

- the latest session is chosen after applying `--source`, `--project`, `--all`, and `--session`
- the selected session is ordered chronologically before the tail window is taken
- any message index range is applied within that selected session before the latest window is taken
- the returned latest-session window is ordered chronologically
- `next_page` is always `false` and `next_command` is always `null`

## Examples

```bash
# Search the auto-discovered cwd project
mmr messages

# Search all projects instead of the cwd project
mmr messages --all

# Search a specific session across all projects
mmr messages --session sess-123

# Search a specific session within one project only
mmr messages --session sess-123 --project /Users/test/proj

# Return the newest two messages from the latest matching session
mmr messages --project /Users/test/proj --latest 2

# Slice the sorted result set before paginating it
mmr messages --project /Users/test/proj --from-message-index 10 --to-message-index 20
```
