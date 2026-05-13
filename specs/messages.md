# Messages Command

`mmr messages` returns `ApiMessagesResponse`:

- `messages`: the selected window of `ApiMessage` items
- `total_messages`: total messages in scope before any message-index window is applied
- `next_page`: whether more results remain for normal paginated `messages` queries
- `next_offset`: the next offset to use for the same query shape
- `next_command`: a ready-to-run follow-up command when another page exists; omitted otherwise

This command supports two related workflows:

1. Standard scoped message queries, which can page through a filtered/sorted result set.
2. Latest-session queries via `--latest`, which first pick one session and then return a tail window from that session.

## Scope resolution

`messages` composes the same scope filters as the CLI:

- `--source` narrows to one source (`claude`, `codex`, or `cursor`).
- `--project` narrows to one project.
- `--session` narrows to one session.
- `--all` disables cwd project auto-discovery.

Without `--project` and without `--all`, `messages` auto-discovers the cwd project by default. If cwd discovery fails, the command falls back to all projects. If cwd discovery succeeds but that project has no matching history, the command returns an empty result instead of widening scope.

When `--session` is provided without an explicit `--project`, `messages` bypasses cwd project auto-discovery and searches all projects instead. If `--source` is also omitted, the CLI prints this hint to `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

See `docs/references/session-lookup-invariants.md` for the session-lookup contract.

## Standard query ordering and pagination

By default, `mmr messages` sorts by timestamp ascending. For this default sort, pagination keeps the historical behavior: it selects the requested page from the newest messages first, then returns that page in chronological order.

That means:

- `--limit 50 --offset 0` returns the newest 50 messages in scope, ordered oldest-to-newest within that 50-message window.
- `--limit 50 --offset 50` returns the next-oldest 50 messages, again ordered chronologically within that page.
- `total_messages` reports the full scoped count before `--from-message-index` / `--to-message-index`.

For non-default sorting (`--sort-by message-count` and/or `--order desc`), pagination applies directly to the selected sorted order.

When a standard `messages` query has more results, `next_command` is populated with a follow-up invocation that preserves the active query shape:

- `--source`
- `--pretty`
- `--session`
- `--project`
- `--all`
- `--from-message-index`
- `--to-message-index`
- `--limit`
- `--sort-by`
- `--order`

Example:

```bash
mmr --source codex messages --project /Users/test/codex-proj --limit 2
```

returns a `next_command` shaped like:

```bash
mmr --source codex messages --project /Users/test/codex-proj --limit 2 --offset 2
```

If all selected results fit in the current page, `next_page` is `false` and `next_command` is omitted.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Important details:

- The latest session is chosen after applying any `--source`, `--project`, `--all`, and `--session` filters.
- The returned window is always chronological, even though it is taken from the tail of the latest session.
- Omitting the value defaults `--latest` to `1`.
- `total_messages` reports the full size of the selected latest session before any message-index range is applied.
- `next_page` is always `false` and `next_command` is omitted for `--latest` queries.

Example:

```bash
mmr --source codex messages --project /Users/test/codex-proj --latest 5
```

If the latest matching session only has two messages, both are returned in chronological order and `total_messages` is `2`.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after filtering and sorting.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied:

- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- The range is applied before pagination.
- The range is also applied before the tail window is taken for `--latest`.

The CLI rejects inverted ranges:

```text
--from-message-index must be less than or equal to --to-message-index
```

Out-of-bounds values are clamped to the available result length rather than causing an error.

Example:

```bash
mmr --source codex messages --project /Users/test/codex-proj --from-message-index 1 --to-message-index 4
```

This selects the second, third, and fourth messages from the already filtered and sorted result set.
