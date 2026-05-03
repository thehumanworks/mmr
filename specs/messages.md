# Messages Command

`mmr messages` returns `ApiMessagesResponse` and is the canonical read path for message-level history queries.

## Scope resolution

By default, `mmr messages` uses the auto-discovered current working directory as the project scope.

- `--project <value>` uses the explicit project and disables cwd auto-discovery.
- `--all` disables cwd auto-discovery and searches across all projects.
- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for the command.
- If cwd auto-discovery fails, the command falls back to the global cross-project search.
- If cwd auto-discovery succeeds but the discovered project has no matching records, the command returns an empty result instead of widening the search.

When `--session <ID>` is provided without `--project`, cwd auto-discovery is bypassed and the command searches across all projects for that session ID. If `--source` is also omitted, the CLI prints a stderr hint:

```text
hint: searching all sources for session; pass --source to narrow the search
```

The JSON response on stdout is unchanged by that hint.

## Default ordering and pagination contract

The default invocation is equivalent to:

```text
mmr messages --limit 50 --offset 0 --sort-by timestamp --order asc
```

For the default timestamp-ascending order, pagination is intentionally applied from the newest end of the scoped result set, and the selected page is then returned in chronological order. This preserves the historical CLI contract:

- `--limit` and `--offset` select a window from newest to oldest.
- The returned `messages` array is still oldest-to-newest within that selected window.

For non-default sort modes (for example `--sort-by message-count --order desc`), pagination is applied directly to the sorted result order.

`ApiMessagesResponse` pagination fields have these semantics:

- `total_messages`: total number of scoped messages before any message-index range is applied.
- `next_page`: `true` when another page exists within the current scoped and ranged result set.
- `next_offset`: offset to use for the next page in the current query shape.
- `next_command`: included only when `next_page` is `true`; contains a ready-to-run `mmr messages ...` command that preserves the current filters, sort, and range flags.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Latest-session behavior is distinct from normal pagination:

- The latest session is chosen first, after applying source, project, `--all`, and `--session` filters.
- The returned latest-session window is always chronological.
- `next_page` is always `false`.
- `next_command` is omitted.
- `total_messages` reports the full message count for the selected latest session before any message-index range is applied.

Examples:

```text
mmr messages --latest
mmr messages --latest 5 --project /path/to/proj
mmr --source claude messages --session sess-123 --latest 10
```

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after source, project, session, sort, and latest-session filtering.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied:

- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- The range is validated before execution; `from > to` is an error.

The range is applied before pagination. That means:

- In normal `messages` queries, `--limit` and `--offset` page within the ranged result set.
- In `--latest` queries, the range is applied to the selected latest session before the trailing latest-message window is chosen.

Examples:

```text
mmr messages --project /path/to/proj --from-message-index 10 --to-message-index 20
mmr messages --latest 5 --from-message-index 2
```

## Common operator pitfalls

- Empty result from the current directory: pass `--all` to search globally, or `--project <value>` to force a specific project scope.
- Session lookup is slower than necessary: add `--source claude|codex|cursor` when the session source is known.
- Automation wants the next page command: use the emitted `next_command` when present rather than rebuilding flags manually.

See also: `docs/references/session-lookup-invariants.md`.
