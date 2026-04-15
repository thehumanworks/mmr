# mmr Query Contract

## Table of Contents
- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Resolution and Scope Rules](#project-resolution-and-scope-rules)

Primary sources: `src/types/api.rs`, `src/types/domain.rs`, `src/messages/service.rs`, and `src/cli.rs`.

## Projects Response Contract

`projects` returns:

```rust
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Each `ApiProject` includes `name`, `source`, `original_path`, `session_count`, `message_count`, and `last_activity`.

## Sessions Response Contract

`sessions` returns:

```rust
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Each `ApiSession` includes per-item source and project metadata:

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

## Messages Response Contract

`messages` and `export` both return:

```rust
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

Each `ApiMessage` includes `session_id`, `source`, `project_name`, `role`, `content`, `model`, `timestamp`, `is_subagent`, `msg_type`, `input_tokens`, and `output_tokens`.

## Sorting and Pagination Semantics

- `projects` and `sessions` sort with deterministic tie-breakers so output is stable even when the primary metric matches.
- `messages` default to `timestamp asc`.
- For `messages` with `timestamp asc`, pagination is applied from the newest window first and the returned page is then reversed back into chronological order.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

- When a page is partial and more results remain, the CLI populates `next_page`, `next_offset`, and `next_command`.
- If sorting by `message-count`, messages use per-session message totals as the primary sort key and chronological order as the secondary key.

## Project Resolution and Scope Rules

- Omitting `--source` means all sources unless `MMR_DEFAULT_SOURCE` supplies a default.
- `sessions` and `messages` accept optional `--project` and `--all`.
- Without `--project` and without `--all`, `sessions` and `messages` try to auto-discover the cwd project. If discovery fails, they fall back to all projects and sources.
- If cwd auto-discovery succeeds but there are no matching results, return the empty result instead of falling back.
- When `--project` is provided without `--source`, project resolution searches all sources.
- Codex project lookups accept either a canonical path or the same path without a leading slash.
- `messages --session <id>` bypasses cwd project auto-discovery when `--project` is omitted and searches all projects instead. If `--source` is also omitted, the CLI prints a stderr hint suggesting `--source`.
