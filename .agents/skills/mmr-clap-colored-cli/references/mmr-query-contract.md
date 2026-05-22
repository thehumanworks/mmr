# mmr Query Contract

## Table of Contents

- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Resolution Rules](#project-resolution-rules)

## Projects Response Contract

```rust
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

## Sessions Response Contract

```rust
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Each `ApiSession` carries per-item source/project metadata:

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

## Messages Response Contract

```rust
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

Each `ApiMessage` carries per-item source/project metadata:

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

Source: `src/types/api.rs`

## Sorting and Pagination Semantics

Projects and sessions use deterministic tie-breakers so ordering stays stable when primary metrics match.

Messages have one special historical behavior:

- With `SortBy::Timestamp` and `SortOrder::Asc`, the query pages from the newest window, then reverses that page back into chronological order before returning it.
- With any other sort/order combination, pagination is applied directly to the sorted list.

```rust
let paged = if options.sort.by == SortBy::Timestamp && options.sort.order == SortOrder::Asc {
    let descending = filtered.into_iter().rev().collect::<Vec<_>>();
    let mut paged = apply_pagination(descending, options.limit, options.offset);
    paged.reverse();
    paged
} else {
    apply_pagination(filtered, options.limit, options.offset)
};
```

Additional `messages` rules:

- `total_messages` is computed before `--from-message-index` / `--to-message-index` slicing.
- `next_page` and `next_offset` are computed against the sliced result set.
- `next_command` is populated by `src/cli.rs` only when another page exists and `--latest` is not active.
- `next_command` preserves active filters and non-default sort/order flags.

Primary sources: `src/messages/service.rs`, `src/cli.rs`

## Project Resolution Rules

Project resolution happens in `resolve_project()` in `src/messages/service.rs`.

Key rules:

- Project filtering accepts either a raw project name or a project path.
- Slash-prefixed and non-slash-prefixed Codex-style project paths are both considered during matching.
- When no `--source` is supplied, project resolution searches across all supported sources.
- `export` with no `--project` uses per-source cwd mapping:
  - Codex, Grok, Pi: canonical cwd path
  - Claude, Cursor: slash-to-hyphen encoding with a leading hyphen

The CLI contract therefore depends on both `src/messages/service.rs` and `src/cli.rs`, not on a standalone query module.
