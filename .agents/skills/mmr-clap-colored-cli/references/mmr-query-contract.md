# mmr Query Contract

## Table of Contents
- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Messages Pagination Metadata](#messages-pagination-metadata)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Resolution Semantics](#project-resolution-semantics)

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

`projects` returns per-project source metadata and aggregate counts across the filtered result set.

Source: `src/types/api.rs`

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

Each session carries its own `source`, `project_name`, and `project_path`, so mixed-source results do not rely on envelope-level metadata.

Source: `src/types/api.rs`

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
}
```

`messages` and `export` both serialize `ApiMessagesResponse`.

Source: `src/types/api.rs`

## Messages Pagination Metadata

- `total_messages` is the number of matching messages before pagination.
- `next_offset` is the offset to pass on the next request; it is computed as the current `offset + page_size`.
- `next_page` is `true` only when a `limit` was applied and more results remain.
- `next_command` is optional and is omitted from JSON when there is no follow-up page.
- `QueryService::messages()` sets `next_page` and `next_offset`, but leaves `next_command` as `None`.
- The CLI populates `next_command` only when `next_page` is `true`, preserving the active `--source`, `--session`, `--project`, `--all`, `--limit`, `--offset`, `--sort-by`, and `--order` flags.
- `export` returns the same envelope shape in a single page (`next_page: false`, `next_offset: total_messages`, no `next_command`).

Sources: `src/messages/service.rs`, `src/cli.rs`, `tests/cli_contract.rs`

## Sorting and Pagination Semantics

Projects and sessions use deterministic tie-breakers so repeated runs stay stable:

- Projects: selected metric, then the other metric, then `name`, `original_path`, and `source`
- Sessions: selected metric, then the other metric, then `session_id`, `project_name`, `project_path`, and `source`

Messages support two sorting modes:

- `timestamp`: compare by chronological order, then source file position, then `session_id`
- `message-count`: compare by the session's total message count, then message chronology, then `session_id`

For `messages --sort-by timestamp --order asc`, pagination preserves the historical contract:

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

That means offsets count from the newest matching messages, but each returned page is still chronological. For all other message sort/order combinations, pagination applies directly to the sorted list.

Source: `src/messages/service.rs`

## Project Resolution Semantics

Codex project filters accept a path with or without a leading slash:

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

When `--source` is omitted, project resolution searches Codex, Claude, and Cursor identifiers before falling back to the raw project string.

Source: `src/messages/service.rs`
