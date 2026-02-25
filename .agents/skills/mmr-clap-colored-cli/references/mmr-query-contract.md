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

Source: `src/model.rs:81-86`

## Sessions Response Contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub project_name: String,
    pub project_path: String,
    pub source: String,
    pub sessions: Vec<ApiSession>,
}
```

Source: `src/model.rs:101-107`

## Messages Response Contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiMessagesResponse {
    pub session_id: String,
    pub project_name: String,
    pub project_path: String,
    pub source: String,
    pub messages: Vec<ApiMessage>,
}
```

Source: `src/model.rs:121-127`

## Sorting and Pagination Semantics

Projects sort defaults to `last-activity`; messages paginate from newest then reverse to chronological output.

```rust
let descending = chronological.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

Source: `src/query.rs:317-319`

Projects and sessions sort tie-breakers preserve deterministic ordering:
- Projects: `last_activity/message_count/session_count` then name
- Sessions: selected metric then `session_id`

Source: `src/query.rs:416-463`

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

Source: `src/query.rs:384-391`
