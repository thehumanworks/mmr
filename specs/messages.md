# Messages Command

`mmr messages` returns normalized message records in the stable `ApiMessagesResponse` shape.

## Default invocation

Without extra flags, the command behaves as:

```bash
mmr messages --limit 50 --sort-by timestamp --order asc
```

The command accepts optional `--source`, `--project`, `--all`, `--session`, `--latest`, `--from-message-index`, `--to-message-index`, `--limit`, `--offset`, `--sort-by`, and `--order`.

## Scope resolution

The effective scope is resolved in this order:

1. If `--project` is provided, use that project.
2. Else if `--session` is provided without `--project`, skip cwd auto-discovery and search all matching projects in scope.
3. Else if `--all` is provided, search all matching projects.
4. Else if cwd auto-discovery is enabled and succeeds, scope to the discovered cwd project.
5. Else, fall back to all matching projects.

Additional rules:

- `MMR_AUTO_DISCOVER_PROJECT=0` disables step 4.
- Unset `MMR_AUTO_DISCOVER_PROJECT`, `MMR_AUTO_DISCOVER_PROJECT=1`, or invalid values keep step 4 enabled.
- If cwd auto-discovery succeeds but there are no matching messages, the command returns an empty result instead of widening scope.
- When `--session` is provided without `--project` and `--source` is omitted, the CLI writes a human-facing hint to `stderr` noting that the lookup is searching all sources.

## Response contract

`mmr messages` always returns:

```json
{
  "messages": [],
  "total_messages": 0,
  "next_page": false,
  "next_offset": 0
}
```

with `messages` populated by normalized `ApiMessage` items:

| Field | Description |
| --- | --- |
| `session_id` | Session identifier from the source transcript |
| `source` | `claude`, `codex`, `cursor`, `grok`, or `pi` |
| `project_name` | Source-native project identifier |
| `role` | Normalized chat role (`user` or `assistant`) |
| `content` | Extracted text content |
| `model` | Source-specific model identifier when available |
| `timestamp` | Source timestamp string |
| `is_subagent` | Source-specific subagent marker |
| `msg_type` | Original normalized message type |
| `input_tokens` | Input token count when available, else `0` |
| `output_tokens` | Output token count when available, else `0` |

`total_messages` reports the number of messages after scope and source/session/project filtering, but before message-index slicing and before pagination.

`next_command` is optional and is omitted when there is no follow-up page to suggest.

## Ordering and pagination

### Sort behavior

- `--sort-by timestamp` orders by message chronology with deterministic tie-breakers.
- `--sort-by message-count` groups messages by their containing session's message count, with chronology as the secondary key.

### Pagination behavior

For the default `--sort-by timestamp --order asc` case, pagination is applied from the newest matching window and then returned in chronological order. This preserves the historical contract that the default command shows the most recent conversation slice while keeping that slice readable oldest-to-newest.

For all other sort/order combinations, pagination is applied directly in the selected order.

### Pagination metadata

- `next_page` is `true` only when a limited paginated result has additional messages remaining after the message-index range is applied.
- `next_offset` advances by the number of messages returned in the current page.
- `next_command` is emitted only for ordinary paginated `messages` results when another page exists. It preserves active flags such as `--source`, `--project`, `--all`, `--session`, `--from-message-index`, `--to-message-index`, `--limit`, `--offset`, `--sort-by`, and `--order`.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Semantics:

- The latest session is chosen by the most recent message in scope, with deterministic tie-breakers across session, project, and source identity.
- The selected session is then ordered chronologically.
- If `--from-message-index` / `--to-message-index` are present, the index range is applied to the selected session before taking the latest `N` messages.
- The returned window is chronological.
- `total_messages` reports the full size of the selected latest session before the message-index range and before the `N`-message tail window.
- `next_page` is always `false` and `next_command` is always omitted for `--latest`.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the selected message window at zero-based message index `N` after scope resolution, source/session/project filtering, and sorting.

`mmr messages --to-message-index <N>` stops the selected message window before zero-based message index `N`.

Rules:

- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- Omitting either side creates an open range.
- `--from-message-index` must be less than or equal to `--to-message-index`; otherwise the CLI returns an error.
- The returned window keeps the current message ordering contract.
- `total_messages` still reports the full scoped message count before applying the message-index range.
- `next_page` and `next_offset` are computed against the post-range window for ordinary paginated results.

## Examples

Latest message from the latest session in scope:

```bash
mmr messages --latest
```

Latest five messages from the latest session for one project:

```bash
mmr messages --project /Users/test/codex-proj --latest 5
```

Chronological paginated view over the newest messages:

```bash
mmr messages --project /Users/test/codex-proj --limit 20 --offset 20
```

Slice a zero-based message window before pagination:

```bash
mmr messages --session sess-123 --from-message-index 10 --to-message-index 30
```
