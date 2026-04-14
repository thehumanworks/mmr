# mmr Query Contract

This reference covers the public response envelopes and the query semantics implemented by:

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
#[derive(Debug, Serialize)]
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

Each `ApiSession` includes per-item source and project metadata:

```rust
#[derive(Debug, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
}
```

Each `ApiMessage` includes per-item source and project metadata:

```rust
#[derive(Debug, Serialize)]
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

Source: `src/types/api.rs`

## Sorting and pagination semantics

### Projects

- Default sort is `timestamp desc` at the CLI layer.
- Project ordering uses deterministic tie-breakers: secondary metric, then `name`, `original_path`, and `source`.

### Sessions

- Default sort is `timestamp desc` at the CLI layer.
- Session ordering uses deterministic tie-breakers: secondary metric, then `session_id`, `project_name`, `project_path`, and `source`.

### Messages

- Default sort is `timestamp asc` at the CLI layer.
- For `timestamp + asc`, pagination preserves the historical contract: page from the newest window, then reverse that window back into chronological order before returning it.
- For other sort combinations, pagination is applied directly to the sorted list.
- `next_page` is true only when a limited page leaves additional results.
- `next_offset` equals `offset + page_size`.
- `next_command` is populated by `src/cli.rs` when another page exists and preserves active filters and formatting flags.

Source: `src/messages/service.rs`, `src/cli.rs`

## Project resolution semantics

### Source matching

- `--source` accepts only `claude`, `codex`, or `cursor`.
- Omitting `--source` means all sources unless `MMR_DEFAULT_SOURCE` supplies a default.

### Explicit project filters

`sessions --project` and `messages --project` resolve the provided project identifier against known projects:

- Codex accepts either the canonical path or the same path without a leading slash.
- Claude and Cursor match the stored project name or original decoded path.
- Without a source filter, project lookup searches Codex, Claude, and Cursor.

### CWD defaults

- `sessions` and `messages` auto-discover the cwd project unless `--project` is given, `--all` is set, or `MMR_AUTO_DISCOVER_PROJECT=0`.
- If cwd auto-discovery fails, the commands fall back to the global view.
- If cwd auto-discovery succeeds but no matching records exist, they return the empty result instead of widening scope.
- `messages --session <id>` without `--project` bypasses cwd auto-discovery and searches all projects.

### Export

- `export --project <path>` reuses `ApiMessagesResponse` and queries without a page limit.
- `export` without `--project` resolves cwd to:
  - Codex canonical path
  - Claude hyphenated project name
  - Cursor hyphenated project name
- The CLI merges per-source results, sorts by timestamp ascending, and returns `next_page = false` with `next_command = None`.

Source: `src/messages/service.rs`, `src/cli.rs`
