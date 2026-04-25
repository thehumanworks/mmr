# mmr Query Contract

## Table of Contents
- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Default Scoping and Session Lookup](#default-scoping-and-session-lookup)
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

Each `ApiSession` carries per-item source and project metadata:

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

Each `ApiMessage` carries per-item source and project metadata:

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

## Sorting and Pagination Semantics

`messages --sort-by timestamp --order asc` paginates from the newest window, then reverses that page so the returned output stays chronological.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

Source: `src/messages/service.rs`

Projects, sessions, and messages all include deterministic tie-breakers so ordering remains stable even when primary sort fields match.

Source: `src/messages/service.rs`

## Default Scoping and Session Lookup

- Omitting `--source` means all sources unless `MMR_DEFAULT_SOURCE` supplies a default.
- `sessions` and `messages` auto-discover the cwd project by default unless `--all` is passed or `MMR_AUTO_DISCOVER_PROJECT=0`.
- `messages --session <id>` bypasses cwd project auto-discovery when `--project` is omitted and searches all projects instead.
- When `--session` is provided without `--source`, the CLI prints a stderr hint suggesting `--source` to narrow the search.

Source: `src/cli.rs`

## Codex Project Normalization

Codex project lookup accepts either a leading-slash or no-leading-slash form:

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
