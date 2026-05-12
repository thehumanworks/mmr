# Messages Command

`mmr messages` is the canonical read API for message history. This spec defines the
public response shape, scope resolution rules, pagination semantics, and the
`--latest` / message-index windowing behavior.

## Response contract

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

pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

- `messages` is returned in the command's final output order.
- `total_messages` reports the full scoped message count before `--from-message-index`,
  `--to-message-index`, `--limit`, or `--offset` are applied.
- `next_page` and `next_offset` describe whether another page exists within the
  selected window.
- `next_command` is only populated when ordinary paginated `messages` output has a
  follow-up page. It is omitted (or observed as `null` by JSON consumers) when no
  follow-up command is available.

## Scope resolution

`messages` accepts optional `--source`, `--project`, `--all`, and `--session`
filters.

- If `--project` is provided, that explicit project scope is used.
- If `--all` is provided, cwd project auto-discovery is disabled and the query runs
  across all projects.
- If neither `--project` nor `--all` is provided, the CLI auto-discovers the
  current working directory as the default project scope.
- If cwd auto-discovery fails, `messages` falls back to the historical global
  cross-project behavior.
- If cwd auto-discovery succeeds but no history matches that discovered project,
  `messages` returns an empty result instead of widening scope.
- If `--session <ID>` is provided without `--project`, cwd auto-discovery is
  bypassed and the query searches all projects for that session ID.
- When `--session` is provided without `--project` and without `--source`, the CLI
  prints this hint to `stderr`:

  ```text
  hint: searching all sources for session; pass --source to narrow the search
  ```

When `--source` is omitted, the effective source filter is "all sources" unless
`MMR_DEFAULT_SOURCE` supplies `claude`, `codex`, or `cursor`.

## Ordering and pagination

The default sort is `--sort-by timestamp --order asc`.

For the default chronological sort, paging follows the historical "newest window,
then chronological output" contract:

1. Scope and filter the matching messages.
2. Apply sorting.
3. Apply any message-index range.
4. Select the page from the newest end of the result set using `--limit` and
   `--offset`.
5. Reverse that page back into chronological order before returning it.

This means the response still reads chronologically, but `--offset` advances from
the newest messages rather than the oldest ones.

For non-default sort combinations (for example `--sort-by message-count` or
`--order desc`), pagination happens directly in the selected sort order.

When `next_page` is `true`, `next_command` preserves the user-visible query shape:

- `--pretty`
- `--source`
- `--session`
- `--project`
- `--all`
- `--from-message-index`
- `--to-message-index`
- `--limit`
- `--offset`
- `--sort-by`
- `--order`

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns
only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and
returns the newest `N` messages from that session.

`--latest` without a value defaults to `1`.

The "latest session" is the scoped session whose newest message sorts last by
timestamp, with deterministic tie-breakers on session ID, project identity, and
source identity.

Additional rules:

- Existing filters still apply, including `--source`, `--project`, `--all`, and
  `--session`.
- The returned latest-session window is always ordered chronologically.
- If `N` is larger than the number of messages in the latest session, the entire
  session is returned.
- `total_messages` reports the full size of the selected latest session before
  applying any message-index range or tail window.
- `next_page` is always `false` and `next_command` is absent for `--latest`
  queries.

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based
message index `N` after scope filtering and sorting.

`mmr messages --to-message-index <N>` stops the result window before zero-based
message index `N`.

Rules:

- `--from-message-index` is inclusive.
- `--to-message-index` is exclusive.
- Open-ended ranges are allowed.
- The range is applied before pagination for ordinary `messages` queries.
- The range is also applied before the tail window for `--latest` queries.
- `--from-message-index` must be less than or equal to `--to-message-index`; the
  CLI rejects inverted ranges.
- `total_messages` continues to report the full scoped message count before applying
  the message-index range.

## Examples

```bash
# Default: current cwd project when auto-discovery succeeds
mmr messages

# Search all projects for one session ID
mmr messages --session sess-123

# Return the newest five messages from the latest scoped session
mmr --source codex messages --project /Users/test/codex-proj --latest 5

# Slice a chronological message window before pagination
mmr --source codex messages --project /Users/test/codex-proj --from-message-index 10 --to-message-index 20

# Continue an existing paginated query using next_command / next_offset
mmr --source codex messages --project /Users/test/codex-proj --limit 50 --offset 50
```
