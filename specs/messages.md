# Messages Command

`mmr messages` returns an `ApiMessagesResponse` for the current message scope:

- `messages`: the selected message window
- `total_messages`: total matching messages before pagination
- `next_page`: whether another page is available
- `next_offset`: the offset for the next page
- `next_command`: a ready-to-run continuation command when another page exists

## Scope and default project behavior

`messages` accepts optional `--session`, `--project`, `--all`, and `--source`.

- Without `--project` and without `--all`, the command tries to auto-discover the current working directory as the project scope.
- If cwd auto-discovery fails, the command falls back to the historical global behavior for the current source scope.
- If cwd auto-discovery succeeds but no messages match that project, the command returns an empty result instead of widening the scope.
- `--all` disables cwd project auto-discovery.
- `--project` remains the explicit way to scope to one project.

### `--session` special case

When `--session <ID>` is provided **without** an explicit `--project`, the command bypasses cwd project auto-discovery and searches across all projects in the selected source scope.

- This keeps session lookup global by default, because a session ID is already a precise target.
- If `--source` is also omitted, the CLI prints a stderr hint:

```text
hint: searching all sources for session; pass --source to narrow the search
```

See `docs/references/session-lookup-invariants.md` for the full session-lookup contract.

## Ordering and pagination contract

The default ordering is `--sort-by timestamp --order asc`, but pagination keeps the historical "newest window, then chronological output" behavior:

1. Match messages in scope.
2. Sort them chronologically.
3. Apply any message-index range.
4. Select the page from the newest end using `--limit` and `--offset`.
5. Return that selected page in chronological order.

Example:

```bash
mmr messages --session sess-123 --limit 2 --offset 1
```

This skips the newest matching message, selects the next two newest messages, and prints those two messages oldest-to-newest.

For non-default orderings (for example `--order desc` or `--sort-by message-count`), the selected page is returned directly in the requested sort order.

`next_page`, `next_offset`, and `next_command` are populated only for regular paginated `messages` queries when another page exists. `--latest` responses never advertise another page.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Additional rules:

- `--latest` without a value defaults to `1`.
- The "latest session" is the scoped session containing the newest message.
- Existing scope filters still apply, including `--source`, `--project`, `--all`, and `--session`.
- The returned latest-session tail is always ordered chronologically.
- `total_messages` reports the full message count for the selected latest session before any message-index range or tail window is applied.
- `next_page` is always `false` and `next_command` is always `null`.

Example:

```bash
mmr --source codex messages --project /Users/test/codex-proj --latest 5
```

This returns up to the last five messages from the newest Codex session in that project, ordered oldest-to-newest within the returned tail.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after filtering and sorting.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied:

- `--from-message-index` is inclusive
- `--to-message-index` is exclusive
- open-ended ranges are allowed
- `--from-message-index > --to-message-index` is rejected

Range application order:

- For regular `messages` queries, the message-index range is applied before `--limit` and `--offset`.
- For `--latest`, the message-index range is applied to the selected latest session before taking the latest `N` messages.

`total_messages` continues to report the full scoped message count before applying the message-index range.

Example:

```bash
mmr messages \
  --project /Users/test/codex-proj \
  --from-message-index 10 \
  --to-message-index 20 \
  --limit 5
```

This selects messages in index range `[10, 20)` from the filtered, sorted message stream, then paginates within that slice.
