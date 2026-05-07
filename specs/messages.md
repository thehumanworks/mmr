# Messages Command

`mmr messages` returns an `ApiMessagesResponse` for the selected message scope. It supports three related workflows:

- browsing a paginated message stream
- inspecting the newest tail of the latest matching session
- slicing a filtered/sorted message stream by zero-based message index

## Scope resolution

`messages` accepts optional `--source`, `--project`, `--all`, and `--session` filters.

Default scope behavior:

- If `--project` is omitted, `--all` is absent, and `--session` is absent, the command auto-discovers the current working directory's project.
- If cwd auto-discovery fails, the command falls back to searching all projects and sources.
- If cwd auto-discovery succeeds but there are no matching records, the command returns an empty result instead of widening scope.
- If `--session` is provided without `--project`, cwd auto-discovery is skipped and the lookup searches across all projects.

When `--session` is provided without `--source`, the CLI prints this `stderr` hint:

```text
hint: searching all sources for session; pass --source to narrow the search
```

## Response contract

`messages` returns `ApiMessagesResponse`:

```rust
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

Field semantics:

- `messages`: the returned message window
- `total_messages`: full scoped message count before pagination and before any message-index range is applied
- `next_page`: `true` when another page exists within the selected message window
- `next_offset`: the offset to use for the next page
- `next_command`: a follow-up CLI command when another page exists on the normal paginated path

## Ordering and pagination

For the normal paginated path (without `--latest`):

- filtering happens first
- sorting happens next
- message-index range slicing happens after sorting
- pagination happens last

For the default `--sort-by timestamp --order asc` case, the command preserves the historical behavior of paging from the newest results while still returning each page in chronological order.

For other sort/order combinations, pagination follows the explicitly requested order.

`next_command` is populated only when:

- the query is using the normal paginated path
- `next_page` is `true`

The generated command preserves the active `--source`, `--pretty`, `--session`, `--project`, `--all`, message-index range, `--limit`, `--sort-by`, and `--order` flags.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

Latest-session rules:

- `--latest` defaults to `1` when the value is omitted.
- Latest-session selection happens after source/project/session filtering.
- The selected session is sorted chronologically before taking the newest tail window.
- The returned latest-session window is always ordered chronologically.
- `total_messages` reports the full message count for the selected latest session before any message-index range is applied.
- Latest-session responses always set `next_page = false` and `next_command = None`.

Existing scope filters still apply, including `--source`, `--project`, `--all`, and `--session`.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N`.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

Range rules:

- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- The range is applied after filtering and sorting.
- On the normal paginated path, the range is applied before pagination.
- On the latest-session path, the range is applied before taking the newest `--latest` tail window.
- Out-of-range bounds are clamped to the available message count.
- `--from-message-index` must be less than or equal to `--to-message-index`; otherwise the CLI returns an error.

`total_messages` continues to report the full scoped message count before applying the message-index window.

## Examples

Browse a project with pagination metadata:

```bash
mmr --source codex messages --project /Users/test/codex-proj --limit 2
```

Get the newest message from the latest session in scope:

```bash
mmr messages --latest
```

Get the newest five messages from the latest session in one project:

```bash
mmr --source codex messages --project /Users/test/codex-proj --latest 5
```

Slice a sorted message stream by message index:

```bash
mmr --source codex messages --project /Users/test/codex-proj \
  --from-message-index 10 \
  --to-message-index 20
```
