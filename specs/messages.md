# Messages Command

`mmr messages` returns normalized chat messages from the configured source scope.

## CLI Surface

```text
mmr [--source <claude|codex|cursor|grok|pi>] messages \
  [--session <id>] \
  [--project <name-or-path> | --all] \
  [--latest[=<n>]] \
  [--from-message-index <n>] \
  [--to-message-index <n>] \
  [--limit <n>] \
  [--offset <n>] \
  [--sort-by <timestamp|message-count>] \
  [--order <asc|desc>]
```

Defaults from `src/cli.rs`:

- omitting `--source` uses `MMR_DEFAULT_SOURCE` when set, otherwise all sources
- `--limit 50`
- `--offset 0`
- `--sort-by timestamp`
- `--order asc`
- `--latest` without a value means `--latest 1`

## Scope Resolution

The command resolves scope in this order:

1. `--project <value>` uses the explicit project scope.
2. `--session <id>` without `--project` bypasses cwd auto-discovery and searches all projects.
3. `--all` disables cwd auto-discovery and searches all projects.
4. Otherwise, cwd auto-discovery is used when enabled.

Auto-discovery rules:

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery.
- Unset, empty, or `1` keeps cwd auto-discovery enabled.
- If cwd discovery fails, `messages` falls back to all projects instead of erroring.
- If cwd discovery succeeds but there are no matching records, the command returns an empty result instead of widening scope.

Session lookup rule:

- `mmr messages --session <id>` searches all projects when `--project` is omitted, even if cwd auto-discovery would otherwise apply.
- When `--session` is used without `--source`, the CLI prints this hint to `stderr`:

```text
hint: searching all sources for session; pass --source to narrow the search
```

## Response Contract

The command returns `ApiMessagesResponse` from `src/types/api.rs`:

```rust
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

Each `ApiMessage` contains:

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

Envelope semantics:

- `total_messages` is the full scoped message count before pagination.
- When a message-index range is supplied, `total_messages` still reports the pre-range scoped count.
- `next_page` is `true` only when another page exists for a non-`--latest` response.
- `next_offset` is the offset to pass to the next page. For `--latest`, it is the number of returned messages.
- `next_command` is populated only when `next_page` is `true` and `--latest` is not in use.

## Ordering and Pagination

### Default timestamp ordering

With the default `--sort-by timestamp --order asc`, the command preserves the historical contract:

1. Sort the scoped messages chronologically.
2. Page from the newest end of that ordered list.
3. Return the selected page in chronological order.

That means `--offset` counts from the newest window, not from the oldest message. For example, with six chronological messages:

- `--limit 2 --offset 0` returns the latest two messages, still ordered oldest-to-newest within that two-message page.
- `--limit 2 --offset 2` returns the previous two messages, again in chronological order.

Tie-breaking remains deterministic through source-file and line metadata before the final session-id tie-breaker.

### Non-default sorts

When sorting by something other than `timestamp asc` (for example `--sort-by message-count -o desc`), pagination is applied directly to the sorted result set without the newest-window reversal.

### `next_command`

When another page exists, `next_command` preserves the active query shape:

- `--pretty`
- `--source`
- `--session`
- `--project`
- `--all`
- `--from-message-index` / `--to-message-index`
- `--limit`
- updated `--offset`
- non-default `--sort-by`
- non-default `--order`

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the newest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the newest `N` messages from that session.

Rules:

- The latest session is chosen after applying source, project, and optional session filters.
- The returned window is always chronological.
- `total_messages` reports the full message count of that selected latest session before the tail window is taken.
- `next_page` is always `false` and `next_command` is always `null`.
- Message-index range filtering runs before the tail window is selected.

Examples:

```bash
mmr --source codex messages --all --latest
mmr --source codex messages --project /Users/test/codex-proj --latest 5
```

## `--from-message-index` and `--to-message-index`

`--from-message-index` and `--to-message-index` slice the filtered-and-sorted message list by zero-based index before pagination (or before the `--latest` tail window is taken).

- `--from-message-index <N>` is inclusive.
- `--to-message-index <N>` is exclusive.
- Either flag may be used on its own.
- If `to < from`, the CLI rejects the request before querying.

Examples:

```bash
mmr --source codex messages --project /Users/test/codex-proj --from-message-index 1 --to-message-index 4
mmr messages --session sess-123 --from-message-index 10
```

## Related Verification

The current contract is covered by integration tests in `tests/cli_contract.rs`, including:

- latest-session selection and default `--latest`
- chronological newest-window pagination
- `next_page`, `next_offset`, and `next_command`
- session lookup without project scope
- message-index range validation and slicing
