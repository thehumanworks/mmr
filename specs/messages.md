# Messages Command

`mmr messages` returns a machine-readable `ApiMessagesResponse` for the selected scope.

This spec covers scope resolution, response fields, pagination metadata, `--latest`, and message-index slicing.

## Scope resolution

The command applies filters in this order:

1. Source filtering from `--source` or `MMR_DEFAULT_SOURCE`.
2. Project/session scoping from explicit flags.
3. Default cwd project auto-discovery when `--project` is omitted and `--all` is not set.
4. Sorting, message-index slicing, and pagination/windowing.

### Default project scope

Without `--project` and without `--all`, `mmr messages` auto-discovers the current working directory as the default project scope.

- If cwd auto-discovery fails, the command falls back to all projects.
- If cwd auto-discovery succeeds but no messages match, the command returns an empty result instead of widening the scope.
- `--all` disables cwd project auto-discovery.

### `--session` lookup invariant

When `--session <ID>` is provided without `--project`, the command searches across all projects instead of applying cwd auto-discovery.

- If `--source` is omitted in that mode, `mmr` prints this hint to `stderr`:

  ```text
  hint: searching all sources for session; pass --source to narrow the search
  ```

- Adding `--project` restores explicit project scoping.
- Adding `--source` suppresses the hint.

Examples:

```bash
mmr messages
mmr messages --all
mmr messages --session sess-claude-1
mmr messages --session sess-claude-1 --source claude
mmr messages --session sess-claude-1 --project /Users/test/proj
```

## Response envelope

`mmr messages` and `mmr export` both serialize `ApiMessagesResponse`:

```json
{
  "messages": [],
  "total_messages": 0,
  "next_page": false,
  "next_offset": 0
}
```

Fields:

- `messages`: the returned page or latest-session window.
- `total_messages`: the number of messages in scope before pagination. When message-index slicing is used, this remains the full scoped count before the index window is applied.
- `next_page`: `true` when another page is available for the same filtered query.
- `next_offset`: the offset to pass to the next paginated request.
- `next_command`: an optional convenience command for fetching the next page. This field is omitted when no follow-up command applies.

Each item in `messages` includes per-message `source` and `project_name` metadata in addition to message content, timing, token counts, and session identifiers.

## Pagination semantics

Pagination metadata applies to the normal `messages` query path (that is, when `--latest` is not used).

- `--limit` and `--offset` page within the already filtered and sorted result set.
- `next_page` becomes `true` only when `--limit` is set and more results remain.
- `next_command` is emitted only when `next_page` is `true`.
- `next_command` preserves the active filter shape, including `--source`, `--project`, `--session`, `--all`, `--from-message-index`, `--to-message-index`, `--sort-by`, `--order`, and `--pretty`.

### Chronological paging contract

When sorting by ascending timestamp, paging preserves the historical contract:

- select the newest window from the filtered chronological result set
- then return that page in chronological order

This means the output stays chronological even though pagination advances from the newest remaining messages.

Example:

```bash
mmr --source codex messages --project /Users/test/codex-proj --limit 2
```

The response may include:

```json
{
  "next_page": true,
  "next_offset": 2,
  "next_command": "mmr --source codex messages --project /Users/test/codex-proj --limit 2 --offset 2"
}
```

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Rules:

- Omitting the numeric value defaults to `1`.
- The latest-session window is always returned in chronological order.
- `total_messages` reports the total number of messages in the selected latest session before the final `N`-message tail window is taken.
- `next_page` is always `false` for `--latest`.
- `next_command` is omitted for `--latest`.

Example:

```bash
mmr --source codex messages --all --latest
mmr --source codex messages --project /Users/test/codex-proj --latest 5
```

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after source, project, session, and sort filtering.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

Rules:

- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- The returned window remains ordered according to the selected message ordering.
- If `--to-message-index` is smaller than `--from-message-index`, the result window is empty.
- On the normal query path, `total_messages` continues to report the full scoped message count before applying the message-index window.
- On the `--latest` path, the index range is applied after the latest session is chosen and before the tail window is taken.

Example:

```bash
mmr --source codex messages --project /Users/test/codex-proj --from-message-index 1 --to-message-index 4
```
