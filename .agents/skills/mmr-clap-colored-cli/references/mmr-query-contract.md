# mmr Query Contract

## Table of Contents

- [Response Shapes](#response-shapes)
- [Per-item Metadata](#per-item-metadata)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Resolution Semantics](#project-resolution-semantics)

## Response Shapes

All public query response structs live in `src/types/api.rs`.

### Projects

```rust
#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

### Sessions

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

### Messages

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

`next_command` is optional. `QueryService::messages` returns it as `None`; the CLI layer populates it when another page exists and it can suggest a follow-up `mmr messages ...` invocation.

## Per-item Metadata

The response envelopes stay small; source and project metadata live on each returned session/message item.

### `ApiSession`

`ApiSession` includes:

- `session_id`
- `source`
- `project_name`
- `project_path`
- `first_timestamp`
- `last_timestamp`
- `message_count`
- `user_messages`
- `assistant_messages`
- `preview`

### `ApiMessage`

`ApiMessage` includes:

- `session_id`
- `source`
- `project_name`
- `role`
- `content`
- `model`
- `timestamp`
- `is_subagent`
- `msg_type`
- `input_tokens`
- `output_tokens`

## Sorting and Pagination Semantics

Sort enums live in `src/types/domain.rs`:

- `SortBy::Timestamp` (default)
- `SortBy::MessageCount`
- `SortOrder::Asc`
- `SortOrder::Desc`

Key rules from `src/messages/service.rs`:

- `projects` and `sessions` use deterministic tie-breakers so results stay stable when primary sort fields match.
- `messages` sorts chronologically by timestamp, then `source_file`, then `line_index`, and finally `session_id`.
- When sorting messages by `timestamp asc`, pagination preserves the historical CLI contract: select the newest window first, then reverse that page back into chronological order before returning it.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

That means a default `messages` query still returns each page oldest-to-newest, even though pagination advances through the newest messages first.

## Project Resolution Semantics

Project resolution lives in `src/messages/service.rs` and is intentionally source-aware:

- With no `--source` filter, project matching searches across Codex, Claude, and Cursor names.
- Codex project lookups accept either `/path/to/proj` or `path/to/proj`.
- Claude and Cursor project matching uses the stored project identifier from their transcript directories.

Codex normalization accepts both leading-slash forms:

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

Do not move `source`, `project_name`, or `project_path` back onto the top-level response envelopes; downstream callers depend on the current per-item metadata layout.
