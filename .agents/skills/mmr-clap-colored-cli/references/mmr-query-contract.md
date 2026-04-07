# mmr Query Contract

## Table of Contents
- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Codex Project Normalization](#codex-project-normalization)
- [Session Lookup Invariant](#session-lookup-invariant)

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

Each `ApiSession` item carries its own `source`, `project_name`, and `project_path` metadata.

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

Each `ApiMessage` item carries its own `session_id`, `source`, and `project_name` metadata.

`next_command` is populated in `src/cli.rs` only when another page is available.

Source: `src/types/api.rs`, `src/cli.rs`

## Sorting and Pagination Semantics

CLI defaults:

- `projects`: `--sort-by timestamp --order desc`
- `sessions`: `--sort-by timestamp --order desc`
- `messages`: `--sort-by timestamp --order asc`

For `messages`, the default timestamp-ascending mode preserves transcript readability by paginating from the newest results first, then reversing the selected page back into chronological order.

```rust
let descending = chronological.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

`next_offset` is computed as `offset + page_size`, and `next_page` is true only when `limit` is set and more filtered results remain after the current page.

Source: `src/messages/service.rs`, `src/cli.rs`

Projects, sessions, and messages all use deterministic tie-breakers so repeated queries remain stable across identical timestamps or counts.

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

Without `--source`, project resolution searches all supported sources. For cwd-derived export lookups, Claude and Cursor use the same hyphenated project name format.

Source: `src/messages/service.rs`, `src/cli.rs`

## Session Lookup Invariant

When `messages --session <ID>` is called without an explicit `--project`, the CLI bypasses cwd project auto-discovery and searches across all projects instead.

- If `--source` is also omitted, the CLI prints a hint on `stderr` suggesting `--source` for a narrower lookup.
- If `--project` is provided, the explicit project still scopes the query.

See `docs/references/session-lookup-invariants.md` for the detailed behavior contract and test coverage.
