# mmr Query Contract

## Table of Contents
- [Codepaths and source of truth](#codepaths-and-source-of-truth)
- [Projects response contract](#projects-response-contract)
- [Sessions response contract](#sessions-response-contract)
- [Messages response contract](#messages-response-contract)
- [Sorting and pagination semantics](#sorting-and-pagination-semantics)
- [Project resolution across sources](#project-resolution-across-sources)

## Codepaths and source of truth

- Public JSON response types live in `src/types/api.rs`.
- Query aggregates and helper state live in `src/types/query.rs`.
- Filtering, sorting, pagination, and response assembly live in `src/messages/service.rs`.
- CLI defaults for cwd scoping, env-based source selection, and `next_command` generation live in `src/cli.rs`.

When this reference and the code disagree, treat `src/types/api.rs`, `src/messages/service.rs`, and `.cursor/rules/cli-contract.mdc` as authoritative.

## Projects Response Contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Each `ApiProject` item includes:

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

`ApiSessionsResponse` is a flat envelope. Project and source metadata live on each `ApiSession`, not at the top level:

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
}
```

`ApiMessagesResponse` is also a flat envelope. Session and project metadata live on each `ApiMessage`:

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

Notes:

- `next_command` is populated by the CLI for `messages` when another page exists.
- `export` reuses `ApiMessagesResponse`; when exporting from cwd, the CLI merges source-specific queries and returns `next_page: false`, `next_offset: total_messages`, and `next_command: null`.

Source: `src/types/api.rs`, `src/cli.rs`

## Sorting and Pagination Semantics

### Projects

Projects are sorted with deterministic tie-breakers:

- primary: selected metric (`timestamp` or `message-count`)
- secondary: the other metric
- then: `name`, `original_path`, `source`

Source: `sort_projects()` in `src/messages/service.rs`

### Sessions

Sessions are sorted with deterministic tie-breakers:

- primary: selected metric (`timestamp` or `message-count`)
- secondary: the other metric
- then: `session_id`, `project_name`, `project_path`, `source`

Source: `sort_sessions()` in `src/messages/service.rs`

### Messages

For `messages` with the default `timestamp` ascending sort, pagination preserves the historical contract: select the newest window first, then return that window in chronological order.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

That means:

- page selection happens from newest to oldest
- returned message order is oldest to newest within the selected page
- `next_offset` advances by the number of returned items
- `next_page` is true only when `limit` is set and more matching messages remain

For non-default message sorts (for example `--sort-by message-count` or `--order desc`), the service applies normal pagination over the sorted list without the chronological-window reversal.

Source: `messages()` and `sort_messages()` in `src/messages/service.rs`

## Project Resolution Across Sources

Project filtering resolves one user-supplied identifier against known project names and paths across the selected sources.

Codex path matching is forgiving about a leading slash:

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

Cross-source behavior:

- when `--source` is omitted, project resolution checks Codex, Claude, and Cursor
- matches can come from either the stored project name or original path
- if no known project matches, the raw user input is preserved as the filter value

Source: `resolve_project()` in `src/messages/service.rs`
