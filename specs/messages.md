# Messages Command

This document is the canonical contract for `mmr messages`.

## Scope resolution

The command accepts four independent scope controls:

- `--source`
- `--project`
- `--all`
- `--session`

When `--project` is omitted, scope resolution follows these rules:

| Flags provided | Effective project scope | Notes |
| --- | --- | --- |
| `--session` | all projects | cwd auto-discovery is bypassed |
| `--session --source X` | all projects | same lookup, no stderr hint |
| `--project P` | explicit project `P` | searches all sources unless `--source` narrows it |
| `--all` | all projects | disables cwd auto-discovery |
| none of the above | auto-discovered cwd project | falls back to all projects only if cwd discovery itself fails |

Additional rules:

- If cwd auto-discovery succeeds but the discovered project has no matching records, the command returns an empty result instead of widening scope.
- When `--session` is provided without `--project` and without `--source`, the CLI prints this hint on `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

See [`docs/references/session-lookup-invariants.md`](../docs/references/session-lookup-invariants.md) for the focused `--session` lookup contract.

## Default query options

Unless flags override them, `messages` uses:

- `--limit 50`
- `--offset 0`
- `--sort-by timestamp`
- `--order asc`

## Response shape

`messages` returns `ApiMessagesResponse`:

| Field | Type | Meaning |
| --- | --- | --- |
| `messages` | array | Returned message window |
| `total_messages` | integer | Full count after source / project / session filters, before message-index slicing and pagination |
| `next_page` | boolean | Whether another page exists for the current selection |
| `next_offset` | integer | Offset to use for the next page |
| `next_command` | string? | CLI command that reproduces the same query with the next offset; omitted when unavailable |

Each `messages[*]` item includes `session_id`, `source`, `project_name`, `role`, `content`, `model`, `timestamp`, `is_subagent`, `msg_type`, `input_tokens`, and `output_tokens`.

## Pagination semantics

Pagination depends on the selected sort order.

### Default path: `timestamp asc`

With the default sort (`--sort-by timestamp --order asc`), `messages` preserves the historical contract:

1. Filter to the scoped message set.
2. Sort chronologically.
3. Apply any message-index range.
4. Page from the newest end of that chronological list.
5. Return the selected page in chronological order.

Practical effect: `--offset 0 --limit 50` returns the newest 50 scoped messages, but the returned array is still oldest-to-newest within that window.

### Other sort and order combinations

For every other sort/order pair, `messages` sorts first and then applies `--offset` / `--limit` directly. There is no reverse-page-reverse step.

## `next_page`, `next_offset`, and `next_command`

- `next_offset` is `offset + page_size`.
- `next_page` is true only when a bounded page size was requested and the selected window has more data remaining.
- When `--from-message-index` / `--to-message-index` are active, `next_page` is computed against the post-range selection, not against `total_messages`.
- `next_command` is added by the CLI only when `next_page` is true and `--latest` is not in use. It preserves the current filters and advances only `--offset`.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Rules:

- The current scope still honors `--source`, `--project`, `--all`, and `--session`.
- The latest-session window is sorted chronologically before the trailing `N` messages are selected.
- Any message-index range is applied to the selected session before the trailing `N` window is taken.
- `total_messages` reports the full message count for the chosen latest session before message-index slicing.
- `next_page` is always `false`, `next_command` is omitted, and `next_offset` equals the number of returned messages.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after source, project, session, and sorting rules are applied.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied:

- `--from-message-index` is inclusive
- `--to-message-index` is exclusive
- `--from-message-index` must be less than or equal to `--to-message-index`

The returned window remains ordered according to the selected message ordering path. `total_messages` continues to report the full scoped count before applying the message-index window.
