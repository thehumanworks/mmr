# mmr Query Contract

This reference summarizes the current query-layer and CLI response contract for `mmr`.

Primary sources:

- `src/types/api.rs`
- `src/messages/service.rs`
- `src/cli.rs`

## Projects response contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Each `ApiProject` includes:

```rust
pub struct ApiProject {
    pub name: String,
    pub source: String,
    pub original_path: String,
    pub session_count: i32,
    pub message_count: i32,
    pub last_activity: String,
}
```

Source: `src/types/api.rs`

## Sessions response contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Each `ApiSession` carries per-item source and project metadata:

```rust
pub struct ApiSession {
    pub session_id: String,
    pub source: String,
    pub project_name: String,
    pub project_path: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub message_count: i32,
    pub user_messages: i32,
    pub assistant_messages: i32,
    pub preview: String,
}
```

Source: `src/types/api.rs`

## Messages response contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

Each `ApiMessage` includes per-message source and project metadata:

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

Notes:

- `next_command` is optional and omitted when there is no next page.
- `QueryService::messages()` sets `next_page` and `next_offset`.
- The CLI injects `next_command` only when `next_page` is true.

Sources: `src/types/api.rs`, `src/messages/service.rs`, `src/cli.rs`

## Sorting and pagination semantics

Supported sort keys are:

- `timestamp`
- `message-count`

Deterministic tie-breakers are required across all surfaces:

- Projects tie-break on the alternate metric, then `name`, `original_path`, and `source`
- Sessions tie-break on the alternate metric, then `session_id`, `project_name`, `project_path`, and `source`
- Messages tie-break on chronological metadata and then `session_id`

The key historical contract for `messages` is preserved:

- When sorting by `timestamp asc`, pagination is applied from the newest end of the full result set
- The selected page is then reversed back into chronological order before returning it

That means `--limit 50 --offset 0` yields the latest 50 messages, ordered oldest-to-newest within that window.

Source: `src/messages/service.rs`

## Project resolution rules

### Cross-source lookup

When `--project` is provided without `--source`, project resolution searches all supported sources.

### Codex normalization

Codex project filters accept either the canonical path or the same path without a leading slash.

### Claude matching

Claude can match either the encoded project directory name or the stored original cwd path.

### Cursor matching

Cursor currently matches the stored project directory name directly. The current implementation does not decode Cursor project names back into canonical filesystem paths before storing `project_name` or `project_path`.

Practical consequence: `--source cursor --project /Users/me/proj` may not match unless that literal string is what Cursor stored.

Source: `src/messages/service.rs`, `src/source/cursor.rs`, `src/source/mod.rs`

## Session lookup invariant

When `messages --session <id>` is called without an explicit `--project`, the CLI bypasses cwd project auto-discovery and searches all projects instead. If `--source` is also omitted, it prints a narrowing hint to `stderr`.

See `docs/references/session-lookup-invariants.md`.
