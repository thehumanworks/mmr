# Messages Command

## Scope Resolution

`messages` accepts the global `--source` filter plus the command-local scope flags `--project`, `--all`, and `--session`.

- If `--project` is provided, `mmr` searches that project. When `--source` is omitted, the lookup spans all supported sources.
- If `--all` is provided, `mmr` searches across all projects and disables cwd project auto-discovery.
- If neither `--project` nor `--all` is provided, `mmr` tries to auto-discover the current working directory as the project scope.
  - If auto-discovery succeeds, the discovered project becomes the scope.
  - If auto-discovery fails, `mmr` falls back to all projects and all sources.
  - If auto-discovery succeeds but the project has no matching messages, `mmr` returns an empty result instead of falling back.
- If `--session <id>` is provided without `--project`, `mmr` bypasses cwd project auto-discovery and searches all projects for that session ID. When `--source` is also omitted, the CLI prints a stderr hint noting that all sources are being searched.

## Default Query Behavior

Without extra flags, `mmr messages` uses:

- `--sort-by timestamp`
- `--order asc`
- `--limit 50`
- `--offset 0`

For the default timestamp-ascending view, pagination preserves the historical contract:

1. Sort the scoped messages chronologically.
2. Page from the newest end of that ordered list.
3. Return the selected page in chronological order.

That means `--limit 2 --offset 0` returns the newest two messages, but still prints them oldest-to-newest within that two-message window. `--offset 2` then returns the next older two-message window, again in chronological order.

When `--sort-by message-count` or `--order desc` is used, pagination applies directly to the selected sort order instead of using the special newest-window behavior.

## Message Index Range

`--from-message-index` and `--to-message-index` slice the already filtered and sorted message stream before `--limit` and `--offset` are applied.

- `--from-message-index <N>` is inclusive.
- `--to-message-index <N>` is exclusive.
- `--from-message-index > --to-message-index` is rejected.
- `total_messages` still reports the full scoped message count before the message-index slice is applied.

Examples:

- `mmr messages --from-message-index 5` keeps messages at indexes `5..`.
- `mmr messages --to-message-index 10` keeps messages at indexes `0..10`.
- `mmr messages --from-message-index 5 --to-message-index 10` keeps messages at indexes `5..10`.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the newest `N` messages from that session.

Rules:

- The same scope filters still apply: `--source`, `--project`, `--all`, and `--session`.
- The selected session is the session containing the most recent message in scope.
- The returned window is always ordered chronologically, even though the newest tail is selected.
- `--from-message-index` and `--to-message-index` apply within the selected latest session before the newest tail is taken.
- `total_messages` reports the full message count of the selected latest session before the index slice and latest-window truncation.
- `next_page` is always `false` for `--latest` queries, and `next_command` is not included in the serialized response.

If `--latest` is present without a value, it defaults to `1`.

## Response Shape

`messages` returns `ApiMessagesResponse`:

```json
{
  "messages": [],
  "total_messages": 0,
  "next_page": false,
  "next_offset": 0
}
```

### `messages`

Each item in `messages` is an `ApiMessage` with these fields:

| Field | Meaning |
| --- | --- |
| `session_id` | Session identifier for the message |
| `source` | Source name: `claude`, `codex`, `cursor`, `grok`, or `pi` |
| `project_name` | Source-specific project identifier retained by `mmr` |
| `role` | Message role |
| `content` | Extracted text content |
| `model` | Source-specific model identifier, if available |
| `timestamp` | Message timestamp as stored or normalized during ingest |
| `is_subagent` | Whether the message came from a subagent transcript |
| `msg_type` | Source-specific normalized message type |
| `input_tokens` | Input token count when the source provides it |
| `output_tokens` | Output token count when the source provides it |

### Pagination Metadata

- `total_messages` is the full scoped message count before pagination. For `--latest`, it is the full count of the selected latest session before the latest-window truncation.
- `next_offset` is `offset + messages.len()` for the current query result.
- `next_page` is `true` only when:
  - the query is not using `--latest`,
  - a finite `--limit` is in effect, and
  - more results remain after the current page within the selected post-range window.
- `next_command` is present only when `next_page` is `true` for a non-`--latest` query.
- When `next_command` is absent, the field is omitted from the serialized JSON response.

When present, `next_command` preserves the active query shape, including:

- `--pretty`
- `--source`
- `--session`
- `--project`
- `--all`
- `--from-message-index`
- `--to-message-index`
- `--limit`
- `--offset` (advanced to `next_offset`)
- `--sort-by`
- `--order`

## Examples

Newest window, returned chronologically:

```bash
mmr --source codex messages --project /Users/test/codex-proj --limit 2
```

Latest session tail:

```bash
mmr --source codex messages --project /Users/test/codex-proj --latest 5
```

Message-index slice before pagination:

```bash
mmr --source codex messages --project /Users/test/codex-proj --from-message-index 1 --to-message-index 4
```
