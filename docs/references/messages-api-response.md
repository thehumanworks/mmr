# Messages API Response

This document defines the `ApiMessagesResponse` envelope returned by `mmr messages` and `mmr export`.

## Response shape

`ApiMessagesResponse` is defined in `src/types/api.rs`:

```rust
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

## Field semantics

| Field | Meaning |
| --- | --- |
| `messages` | The current page of `ApiMessage` records. |
| `total_messages` | Total number of matching messages before pagination is applied. |
| `next_page` | `true` when another page is available. This is only `true` when a `limit` is in effect and more matching records remain. |
| `next_offset` | The offset to use for the next page. Computed as `offset + page_size`, including the last page. |
| `next_command` | Present only for `mmr messages` when `next_page` is `true`. It contains a ready-to-run CLI command with the current filters and the next offset. |

## Pagination contract

For the default `mmr messages` ordering (`--sort-by timestamp --order asc`), pagination works in three steps:

1. Sort all matching messages chronologically.
2. Take the requested page from the **newest** side of that sorted list.
3. Reverse the page before returning it so the output stays chronological.

This preserves the historical contract that callers see the latest window of activity without losing chronological order inside the page.

### Example

If a session has messages with timestamps `[t1, t2, t3, t4, t5]` and you run:

```bash
mmr messages --session sess-123 --limit 2 --offset 0
```

the response returns `[t4, t5]` in chronological order, with:

```json
{
  "total_messages": 5,
  "next_page": true,
  "next_offset": 2
}
```

Running the suggested next command (or `--offset 2`) returns `[t2, t3]`.

## `next_command` behavior

`next_command` is assembled in `src/cli.rs` after `QueryService::messages()` reports that another page exists. The generated command preserves:

- `--pretty` when present
- `--source`
- `--session`
- `--project`
- `--all`
- `--limit`
- `--sort-by`
- `--order`

This makes the field safe to surface directly in automation or shell workflows.

## `messages` vs `export`

Both `mmr messages` and `mmr export` serialize `ApiMessagesResponse`, but they do not paginate the same way:

- `mmr messages` applies filters, sorting, and optional pagination.
- `mmr export` returns the full chronological message stream for the inferred or explicit project.

As a result, `mmr export` always returns:

- `next_page: false`
- `next_offset: total_messages`
- no `next_command` field in the serialized JSON

## Session lookup interaction

`mmr messages --session <id>` without `--project` bypasses cwd project auto-discovery and searches all projects. When `--source` is omitted, the CLI prints a stderr hint suggesting `--source` to narrow the lookup. See `docs/references/session-lookup-invariants.md`.
