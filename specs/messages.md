# Messages Command

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

The returned latest-session window is ordered chronologically. Existing scope filters still apply, including `--source`, `--project`, `--all`, and `--session`.
