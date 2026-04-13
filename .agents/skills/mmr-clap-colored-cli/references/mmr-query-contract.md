# mmr Query Contract

## Table of Contents
- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Codex Project Normalization](#codex-project-normalization)

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

## Sorting and Pagination Semantics

Projects default to the timestamp/last-activity sort; timestamp-ascending `messages` pagination takes the newest window first and then reverses that page back to chronological output.

```rust
let descending = chronological.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

Source: `src/messages/service.rs`

Projects and sessions sort tie-breakers preserve deterministic ordering:
- Projects: `last_activity/message_count/session_count` then `name`, `original_path`, and `source`
- Sessions: selected metric then `session_id`, `project_name`, `project_path`, and `source`

Additional pagination metadata:
- `total_messages` is the full match count before pagination.
- `next_page` is `true` only when `limit` is set and more records remain.
- `next_offset` is `offset + page_size`, even on the last page.
- `next_command` is populated by `src/cli.rs` for `mmr messages` when `next_page` is `true`; `mmr export` reuses `ApiMessagesResponse` but always returns `next_page: false` with `next_command: None`.

Source: `src/messages/service.rs`, `src/cli.rs`

## Codex Project Normalization

Codex `sessions --project` accepts either with or without leading slash:

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

When no source filter is provided, project resolution searches Codex, Claude, and Cursor project identifiers.

Source: `src/messages/service.rs`
