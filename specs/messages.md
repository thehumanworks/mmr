# Messages command

`mmr messages` returns message records as machine-readable JSON. The command supports
four common workflows:

- browse the newest messages in the current project
- look up one specific session by ID
- tail the latest session in scope
- slice a stable index range out of the filtered result set

## Scope resolution

The effective project scope is resolved in this order:

1. `--project <value>` uses the explicit project filter.
2. `--session <id>` **without** `--project` searches all projects instead of applying
   cwd auto-discovery.
3. Otherwise, if `--all` is absent and cwd auto-discovery is enabled, the command
   uses the current working directory as the default project scope.
4. If cwd auto-discovery is disabled (`MMR_AUTO_DISCOVER_PROJECT=0`) or discovery
   fails, the command falls back to all projects.

Additional scope rules:

- `--source` narrows the search to `codex`, `claude`, or `cursor`.
- When `--session` is provided without `--source`, the command prints this hint to
  `stderr` while still performing the lookup:

  ```text
  hint: searching all sources for session; pass --source to narrow the search
  ```

- When cwd auto-discovery succeeds but the discovered project has no matching
  messages, the command returns an empty result instead of widening the search.

See `docs/references/session-lookup-invariants.md` for the session lookup contract.

## Default ordering and pagination

By default, `mmr messages` sorts by timestamp ascending, but pagination is applied
from the newest end of that ordered list. This preserves the historical behavior of
"show me the latest window" while still returning the selected page in chronological
order.

Example:

```bash
mmr messages --project /Users/test/codex-proj --limit 2
```

The response includes pagination metadata:

- `total_messages`: total messages in scope before pagination and before any
  message-index window is applied
- `next_page`: whether another page exists within the selected result set
- `next_offset`: offset to use for the next page
- `next_command`: convenience CLI command for the next page when pagination is
  active; omitted when all results fit on one page

`next_command` preserves active filters and ordering, including `--source`,
`--project`, `--session`, `--all`, `--from-message-index`, `--to-message-index`,
`--sort-by`, and `--order`.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns
only the newest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and
returns the latest `N` messages from that session.

Latest-session mode has these rules:

- the latest session is chosen after applying `--source`, `--project`, `--all`, and
  optional `--session`
- messages are returned in chronological order
- `total_messages` reports the full size of the selected latest session before any
  message-index range is applied
- `next_page` is always `false`
- `next_command` is always omitted

Examples:

```bash
mmr messages --source codex --all --latest
mmr messages --project /Users/test/codex-proj --latest 5
```

## `--from-message-index` and `--to-message-index`

`--from-message-index` and `--to-message-index` slice the already-filtered,
already-sorted result set by zero-based message position.

- `--from-message-index <N>` starts at index `N` (inclusive)
- `--to-message-index <N>` stops before index `N` (exclusive)

The index range is applied after source, project, session, sort, and latest-session
selection, but before pagination.

Examples:

```bash
mmr messages --project /Users/test/codex-proj --from-message-index 1 --to-message-index 4
mmr messages --source codex --all --latest 10 --from-message-index 2
```

Constraints:

- `--from-message-index` must be less than or equal to `--to-message-index`
- `total_messages` continues to report the full scoped count before the
  message-index window
- `next_page` and `next_offset` are computed against the index-windowed result set

## Output shape

Each `messages` item includes:

- `session_id`
- `source`
- `project_name`
- `role`
- `content`
- `model`
- `timestamp`
- `is_subagent`
- `msg_type`
- `input_tokens`
- `output_tokens`
