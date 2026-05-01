# Messages Command

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

The returned latest-session window is ordered chronologically. Existing scope filters still apply, including `--source`, `--project`, `--all`, and `--session`.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after source, project, session, sort, and latest-session filtering.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied, `--from-message-index` is inclusive and `--to-message-index` is exclusive. The returned window remains ordered according to the selected message ordering. `total_messages` continues to report the full scoped message count before applying the message-index window.
