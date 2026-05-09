# Messages Command

## Scope resolution

`mmr messages` resolves project scope in this order:

1. `--project <PATH>` or `--project <NAME>` uses the explicit project filter.
2. `--all` disables project auto-discovery and searches across all projects.
3. Otherwise, the command auto-discovers the current working directory as the project scope.

When cwd auto-discovery fails, the command falls back to the global search behavior. When cwd auto-discovery succeeds but there are no matching records, the command returns the empty result for that project instead of widening the search.

## `--session`

`mmr messages --session <ID>` treats the session ID as a global lookup key.

- Without an explicit `--project`, the command bypasses cwd project auto-discovery and searches all projects.
- With `--project`, the explicit project filter still applies.
- Without `--source`, the command prints this stderr hint before searching all sources:

```text
hint: searching all sources for session; pass --source to narrow the search
```

This prevents false negatives when the caller knows the session ID but is running the command from a different project directory.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

The returned latest-session window is ordered chronologically. Existing scope filters still apply, including `--source`, `--project`, `--all`, and `--session`.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after source, project, session, sort, and latest-session filtering.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied, `--from-message-index` is inclusive and `--to-message-index` is exclusive. The returned window remains ordered according to the selected message ordering. `total_messages` continues to report the full scoped message count before applying the message-index window.

## Examples

```bash
# Search by session across all projects because --project is omitted.
mmr messages --session sess-123

# Search by session within one known project.
mmr messages --session sess-123 --project /Users/test/proj

# Keep the lookup narrow and suppress the stderr hint.
mmr --source claude messages --session sess-123

# Return messages 10..20 from the latest session in the current scope.
mmr messages --latest 100 --from-message-index 10 --to-message-index 20
```
