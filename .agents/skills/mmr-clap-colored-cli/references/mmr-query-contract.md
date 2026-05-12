# mmr Query Contract

## Table of Contents

- [Projects response contract](#projects-response-contract)
- [Sessions response contract](#sessions-response-contract)
- [Messages response contract](#messages-response-contract)
- [Messages scope, sorting, and pagination](#messages-scope-sorting-and-pagination)
- [Project normalization and resolution](#project-normalization-and-resolution)

## Projects response contract

```rust
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

## Sessions response contract

```rust
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Each `ApiSession` carries its own `source`, `project_name`, and `project_path`
metadata.

Source: `src/types/api.rs`

## Messages response contract

```rust
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    pub next_command: Option<String>,
}
```

Each `ApiMessage` carries its own `session_id`, `source`, and `project_name`
metadata.

Source: `src/types/api.rs`

## Messages scope, sorting, and pagination

- `messages` accepts optional `--session`, `--project`, `--all`, and `--source`.
- Without `--project` and without `--all`, cwd project auto-discovery supplies the
  default scope.
- If `--session` is provided without `--project`, cwd auto-discovery is bypassed and
  the query searches all projects for that session.
- Default `messages` sorting is `timestamp asc`.
- For that default chronological sort, pagination still works from the newest end of
  the result set and then reverses the returned page back into chronological order.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, options.limit, options.offset);
paged.reverse();
```

- `--latest` selects the latest session in scope and returns a chronological tail of
  that session.
- `--from-message-index` / `--to-message-index` apply after scope filtering and
  sorting, before pagination or `--latest` tail selection.
- `next_command` is only populated when another page exists for a non-`--latest`
  query, and it preserves the effective query flags.

Sources: `src/cli.rs`, `src/messages/service.rs`, `specs/messages.md`

## Project normalization and resolution

Project resolution accepts explicit project values across sources and still supports
Codex path matching with or without a leading slash.

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

When no `--source` filter is provided, project resolution searches the known project
set across Codex, Claude, and Cursor.

Source: `src/messages/service.rs`
