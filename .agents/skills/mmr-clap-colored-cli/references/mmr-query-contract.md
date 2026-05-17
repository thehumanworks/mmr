# mmr Query Contract

## Table of Contents
- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Scope and Source Resolution](#scope-and-source-resolution)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Normalization and Matching](#project-normalization-and-matching)

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

## Scope and Source Resolution

- `--source` accepts only `claude`, `codex`, `cursor`, `grok`, or `pi`.
- Omitting `--source` means all sources unless `MMR_DEFAULT_SOURCE` supplies a default.
- `sessions` and `messages` default to the cwd project when auto-discovery succeeds; `--all` disables that default.
- `messages --session <id>` without `--project` bypasses cwd auto-discovery and searches all projects. When `--source` is omitted, the CLI prints a narrowing hint on `stderr`.
- `export` with no `--project` uses the cwd project, querying Codex/Grok/Pi with the canonical path and Claude/Cursor with the slash-to-hyphen form.

Primary sources: `src/cli.rs`, `src/messages/service.rs`

## Sorting and Pagination Semantics

- `projects`, `sessions`, and `messages` all use deterministic tie-breakers after the primary sort key.
- When `messages` sorts by `timestamp asc`, it preserves the historical contract of selecting the newest window first and then returning that window in chronological order.
- `total_messages` reports the fully scoped count before limit/offset pagination.
- `next_page` and `next_offset` reflect pagination over the selected window, and the CLI populates `next_command` when it can generate the follow-up `mmr messages ...` invocation.
- `latest_session_messages` always returns `next_page: false`; it selects the latest session in scope, applies any message-index range, then returns the newest `N` messages from that session in chronological order.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, options.limit, options.offset);
paged.reverse();
```

Primary source: `src/messages/service.rs`

## Project Normalization and Matching

- Project matching compares the requested value against both `name` and `original_path`.
- The resolver tries both slash-preserving and leading-slash-normalized variants, so `/Users/test/proj` and `Users/test/proj` can resolve to the same project when the source data stores one form or the other.
- This lets a canonical cwd path match Codex/Grok/Pi directly and still resolve Claude/Cursor projects through their `original_path`.

```rust
let mut candidates = vec![trimmed.to_string()];
if trimmed.starts_with('/') {
    let without_leading = trimmed.trim_start_matches('/');
    if !without_leading.is_empty() {
        candidates.push(without_leading.to_string());
    }
} else {
    candidates.push(format!("/{trimmed}"));
}
```

Primary source: `src/messages/service.rs`
