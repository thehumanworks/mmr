# Messages Command

`mmr messages` returns normalized chat messages across Claude, Codex, Cursor, Grok, and Pi.

This spec defines scope resolution, response fields, pagination behavior, `--latest`, and message-index windowing.

## Scope Resolution

`messages` accepts these filters:

- `--source <claude|codex|cursor|grok|pi>`
- `--project <name-or-path>`
- `--all`
- `--session <session-id>`

Default scope behavior:

1. If `--project` is provided, query that project.
2. Else if `--all` is provided, query all projects.
3. Else if `MMR_AUTO_DISCOVER_PROJECT` is unset or `1`, attempt cwd auto-discovery and scope to that project.
4. If cwd auto-discovery fails, fall back to all projects.
5. If cwd auto-discovery succeeds but no messages match, return an empty result instead of widening scope.

### `--session` Bypasses CWD Auto-Discovery

When `--session` is provided **without** `--project`, `messages` searches all projects instead of the auto-discovered cwd project. This lets users fetch a known session ID even when they are currently inside a different project directory.

If that widened search also omits `--source`, the CLI prints a stderr hint encouraging the caller to pass `--source` to narrow the search. Stdout remains machine-readable JSON.

## Response Contract

`messages` and `export` both serialize `ApiMessagesResponse` from `src/types/api.rs`:

```rust
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

Each entry in `messages` is an `ApiMessage`:

```rust
pub struct ApiMessage {
    pub session_id: String,
    pub source: String,
    pub project_name: String,
    pub role: String,
    pub content: String,
    pub model: String,
    pub timestamp: String,
    pub is_subagent: bool,
    pub msg_type: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}
```

Response field semantics:

- `messages`: the returned page or latest-session window.
- `total_messages`: the total number of messages in the resolved scope **before** `--from-message-index` / `--to-message-index` are applied.
- `next_page`: `true` when another page exists inside the currently selected scope.
- `next_offset`: the offset to use for the next page within the selected scope.
- `next_command`: a copy-pasteable shell command for the next page. Omitted from JSON when there is no next page.

## Sorting and Pagination

Defaults:

- `--sort-by timestamp`
- `--order asc`
- `--limit 50`
- `--offset 0`

### Default Timestamp Pagination

When sorting by `timestamp` in ascending order, `messages` preserves the historical CLI contract:

1. Filter and sort the full scoped message list chronologically.
2. Apply any message-index range.
3. Page from the **newest** end of that list.
4. Reverse the selected page back into chronological order before returning it.

This means the default command does **not** page from the oldest messages first, even though returned messages remain chronological within each page.

### Non-Default Sort/Order

When `--sort-by message-count` or `--order desc` is used, pagination follows the already sorted list directly without the special newest-window reversal.

### `next_command`

When `next_page` is `true` and `--latest` is **not** active, the CLI populates `next_command` with a shell command that preserves:

- `--pretty`
- `--source`
- `--session`
- `--project`
- `--all`
- `--from-message-index`
- `--to-message-index`
- `--limit`
- the computed next `--offset`
- non-default `--sort-by`
- non-default `--order`

Example:

```bash
mmr --source codex messages --project /Users/test/codex-proj --limit 2 --offset 2
```

`next_command` is omitted when:

- all results fit in the current page, or
- `--latest` is used.

## `--latest`

`mmr messages --latest` selects the latest session in the resolved scope and returns the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the resolved scope and returns the latest `N` messages from that session.

Rules:

- The latest session is chosen from the fully filtered scope (`--source`, `--project`, `--all`, and optional `--session` still apply).
- The returned `messages` window is chronological even though the command chooses the newest `N` messages.
- `total_messages` reports the total number of messages in that latest session before message-index windowing.
- `next_page` is always `false`.
- `next_command` is always omitted.

## `--from-message-index` and `--to-message-index`

`--from-message-index <N>` starts the selected message window at zero-based index `N`.

`--to-message-index <N>` stops the selected message window before zero-based index `N`.

Rules:

- The index range is applied **after** source/project/session filtering and sorting.
- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- If both are supplied and `from > to`, the CLI errors.
- The returned page or latest-session window keeps its normal output ordering after the range is applied.
- `total_messages` remains the full scoped count before range application.

Examples:

```bash
mmr messages --project /Users/test/proj --from-message-index 10 --to-message-index 20
mmr messages --latest 5 --from-message-index 1 --to-message-index 4
```
