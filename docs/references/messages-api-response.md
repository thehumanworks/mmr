# Messages API response contract

`mmr messages` and `mmr export` both serialize `ApiMessagesResponse` from `src/types/api.rs`.
This reference documents the public JSON envelope and the pagination behavior that scripts and operators can rely on.

## Commands covered

- `mmr messages`
- `mmr export`

Both commands emit the same top-level shape:

```json
{
  "messages": [],
  "total_messages": 0,
  "next_page": false,
  "next_offset": 0
}
```

`next_command` is present only when another `messages` page is available.

## Envelope fields

| Field | Type | Meaning |
| --- | --- | --- |
| `messages` | `ApiMessage[]` | The returned page of messages. |
| `total_messages` | `i64` | Count of all matching messages before pagination. |
| `next_page` | `bool` | `true` when `--limit` was set and another page exists. |
| `next_offset` | `i64` | Offset to pass to the next request. Computed as `current offset + returned message count`. |
| `next_command` | `string \| null` | CLI-generated continuation command for `mmr messages`. Omitted when there is no next page. |

Each `ApiMessage` currently includes:

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

These fields are defined in `src/types/api.rs`.

## `mmr messages` pagination semantics

`QueryService::messages` always sorts the full filtered result set before pagination.

### Default behavior: newest window, chronological output

The default command is:

```bash
mmr messages
```

That means:

- `--sort-by timestamp`
- `--order asc`

For this specific sort/order pair, paging intentionally preserves the historical contract described in `src/messages/service.rs`:

1. Sort all matching messages chronologically.
2. Page from the newest end of the result set.
3. Reverse the selected page back into chronological order before returning it.

Example with timestamps `[t1, t2, t3, t4, t5, t6]`:

- `--limit 2 --offset 0` returns `[t5, t6]`
- `--limit 2 --offset 2` returns `[t3, t4]`
- `--limit 2 --offset 4` returns `[t1, t2]`

This is why `offset` is best understood as "skip N messages from the newest side of the sorted result set" when using the default ascending timestamp view.

### Non-default sorts

When either of these is true:

- `--order desc`
- `--sort-by message-count`

pagination walks the already sorted list directly. There is no reverse-after-pagination step.

## `next_command` contract

`next_command` is assembled in `src/cli.rs`, not in `QueryService`.

When `next_page` is `true`, the CLI preserves the relevant invocation flags in the continuation command:

- `--pretty`
- `--source`
- `--session`
- `--project`
- `--all`
- `--limit`
- `--offset`
- `--sort-by` when non-default
- `--order desc` when non-default

Example:

```json
{
  "next_page": true,
  "next_offset": 2,
  "next_command": "mmr --source codex messages --project /Users/test/codex-proj --limit 2 --offset 2"
}
```

If all results fit in the current page, `next_page` is `false` and `next_command` is omitted.

## `mmr export` differences

`mmr export` reuses `ApiMessagesResponse`, but its behavior is intentionally different from `mmr messages`:

- It returns all matching messages for the chosen project.
- Output is always chronological by timestamp ascending.
- With no explicit `--project`, the CLI infers the project from the current working directory.
  - Codex uses the canonical cwd path.
  - Claude and Cursor use the same path transformed into the hyphenated project form.
- The CLI merges per-source query results before sorting.
- `next_page` is always `false`.
- `next_offset` is set to `total_messages`.
- `next_command` is always omitted.

## Related invariants

- `docs/references/session-lookup-invariants.md` documents the special `messages --session` behavior that bypasses cwd auto-discovery when `--project` is omitted.
- `tests/cli_contract.rs` contains the integration tests that lock down pagination metadata and continuation-command behavior.
