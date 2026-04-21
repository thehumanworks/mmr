# mmr Query Contract

This reference describes the current CLI JSON/query contracts used by `mmr`.
It should stay aligned with `src/types/api.rs`, `src/messages/service.rs`, and
`src/cli.rs`.

## Table of Contents

- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Matching and Normalization](#project-matching-and-normalization)

## Projects Response Contract

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

#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

Notes:

- `projects` is the paginated/sorted window.
- `total_messages` and `total_sessions` reflect the filtered source scope, not
  just the current page.
- Each project item carries its own `source`.

## Sessions Response Contract

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

#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

Notes:

- There is no envelope-level `project_name`, `project_path`, or `source`.
- Per-item metadata is required because a single response may mix sources and
  projects when filters are broad.

## Messages Response Contract

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

Notes:

- There is no envelope-level `session_id`, `project_name`, `project_path`, or
  `source`.
- `next_command` is omitted from JSON when it is `None`.
- `messages` and `export` share this response shape.

## Sorting and Pagination Semantics

### Projects

Projects are sorted in `src/messages/service.rs` with deterministic tie-breakers:

- Primary sort: selected metric (`timestamp` or `message-count`)
- Secondary sort: the other metric
- Tie-breakers: `name`, then `original_path`, then `source`

### Sessions

Sessions are sorted with deterministic tie-breakers:

- Primary sort: selected metric (`timestamp` or `message-count`)
- Secondary sort: the other metric
- Tie-breakers: `session_id`, then `project_name`, then `project_path`, then
  `source`

### Messages

`messages` keeps the historical CLI contract for the default timestamp-ascending
view: it pages from the newest window first, then reverses that page back into
chronological order before returning it.

```rust
if sort.by == SortBy::Timestamp && sort.order == SortOrder::Asc {
    let descending = filtered.into_iter().rev().collect::<Vec<_>>();
    let mut paged = apply_pagination(descending, limit, offset);
    paged.reverse();
    paged
} else {
    apply_pagination(filtered, limit, offset)
}
```

Source: `src/messages/service.rs`

Pagination metadata rules:

- `next_offset` is `offset + page_size`
- `next_page` is true only when a limit is present and more results remain
- `next_command` is filled by the CLI layer when another page exists

## Project Matching and Normalization

### Cwd-derived project identifiers

When `--project` is omitted, CLI project auto-discovery uses:

- Codex: canonical cwd path, e.g. `/Users/mish/proj`
- Claude: encoded form with `/` replaced by `-` and a leading `-`,
  e.g. `-Users-mish-proj`
- Cursor: the same encoded form as Claude

### Explicit Codex project filters

Codex project filters are normalized so users can pass a path either with or
without the leading slash:

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

### Session lookup invariant

`messages --session <id>` without `--project` bypasses cwd auto-discovery and
searches all projects. When `--source` is also omitted, the CLI prints this
stderr hint:

```text
hint: searching all sources for session; pass --source to narrow the search
```
