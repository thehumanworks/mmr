# mmr Query Contract

## Table of Contents

- [Projects response contract](#projects-response-contract)
- [Sessions response contract](#sessions-response-contract)
- [Messages response contract](#messages-response-contract)
- [Source and scope semantics](#source-and-scope-semantics)
- [Sorting and pagination semantics](#sorting-and-pagination-semantics)
- [Project resolution and export mapping](#project-resolution-and-export-mapping)

## Projects response contract

`projects` returns an aggregate envelope from `src/types/api.rs`:

```rust
#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Each `ApiProject` item carries its own `source` and `original_path`.

## Sessions response contract

`sessions` returns only the item list plus the total count:

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Per-session metadata lives on each `ApiSession` item:

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

## Messages response contract

`messages` and `export` share the same envelope:

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

Each message item is self-describing:

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

`next_command` is present only when another page exists.

## Source and scope semantics

- `--source` is optional on `projects`, `sessions`, `messages`, `export`, and `remember`.
- Valid values are `claude`, `codex`, `cursor`, and `pi`.
- Omitting `--source` means all sources unless `MMR_DEFAULT_SOURCE` provides a default.
- `sessions` and `messages` auto-discover the cwd project unless the caller passes `--project` or `--all`.
- `messages --session <id>` without `--project` bypasses cwd auto-discovery and searches all projects instead.
- When `messages --session <id>` also omits `--source`, the CLI prints the narrowing hint on `stderr`.

The query engine itself lives in `src/messages/service.rs`; CLI policy and default-resolution live in `src/cli.rs`.

## Sorting and pagination semantics

- Projects and sessions use deterministic tie-breakers so output remains stable when primary sort keys match.
- Standard `messages` pagination selects the newest matching window first, then returns that window in chronological order.
- `total_messages` reports the scoped total before pagination.
- `next_page` and `next_offset` describe whether more messages remain in the current query shape.
- `build_next_messages_command()` in `src/cli.rs` preserves the active filters, range flags, sort, and order when generating `next_command`.
- `messages --latest [N]` is separate from offset pagination: it selects the latest session in scope, applies any message-index range, and returns the chronological tail of that session.

## Project resolution and export mapping

Project matching spans multiple sources, so the CLI normalizes identifiers at the boundary:

- Codex and Pi use the canonical filesystem path as the project identifier.
- Claude and Cursor use the same path encoded with `/` replaced by `-` and a leading `-`.
- `export` without `--project` issues one scoped `messages` query per selected source, merges the results, and sorts them chronologically.

`resolve_project()` in `src/messages/service.rs` still handles Codex-style path normalization so explicit `--project` lookups can match with or without a leading slash.
