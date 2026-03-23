# mmr Query Contract

## Table of Contents

- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Normalization](#project-normalization)

## Projects Response Contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

## Sessions Response Contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

Each `ApiSession` carries per-item identity and summary metadata:

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

Source: `src/types/api.rs`

`next_command` is omitted from JSON when it is `None`. The CLI fills it in for paged `messages` responses when another page exists.

## Sorting and Pagination Semantics

- `SortBy` supports `timestamp` and `message-count` (`src/types/query.rs`).
- For `projects`, `timestamp` means `last_activity`; for `sessions`, it means `last_timestamp`.
- `messages` paginates from the newest window, then reverses that page back into chronological order before returning it.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

Source: `src/messages/service.rs`

Deterministic tie-breakers are part of the contract:

- Projects: selected metric, then the secondary metric, then `name`, `original_path`, and `source`.
- Sessions: selected metric, then the secondary metric, then `session_id`, `project_name`, `project_path`, and `source`.
- Messages: selected metric, then chronological order, then `session_id`.

The service computes `next_page` / `next_offset` in `src/messages/service.rs`, and `src/cli.rs` builds `next_command` so callers can request the next page with the same flags.

## Project Normalization

Project filters accept either a leading-slash or no-leading-slash form for Codex-style paths:

```rust
if trimmed.starts_with('/') {
    let without_leading = trimmed.trim_start_matches('/');
    if !without_leading.is_empty() {
        candidates.push(without_leading.to_string());
    }
} else {
    candidates.push(format!("/{trimmed}"));
}
```

Source: `src/messages/service.rs`
