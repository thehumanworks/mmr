# mmr Query Contract

## Table of Contents

- [Projects Response Contract](#projects-response-contract)
- [Sessions Response Contract](#sessions-response-contract)
- [Messages Response Contract](#messages-response-contract)
- [Sorting and Pagination Semantics](#sorting-and-pagination-semantics)
- [Project Matching Rules](#project-matching-rules)
- [CLI-Layer Scoping Rules](#cli-layer-scoping-rules)

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

Notes:

- `projects` items are self-describing through per-item `source` and `original_path`
- Omitting `--source` means all sources unless `MMR_DEFAULT_SOURCE` supplies a default

## Sessions Response Contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

Each `ApiSession` includes:

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

Source: `src/types/api.rs`

Each `ApiMessage` includes per-item metadata such as `session_id`, `source`, and `project_name`.

`next_command` is optional and is populated by the CLI only when another page exists.

## Sorting and Pagination Semantics

Relevant sources: `src/messages/service.rs` and `src/cli.rs`

- `projects` and `sessions` support `timestamp` and `message-count` sorting
- Sorts include deterministic tie-breakers so output remains stable when primary keys match
- `messages` defaults to `--sort-by timestamp --order asc`
- For timestamp-ascending message queries, pagination is applied from the newest window first, then the selected page is reversed back into chronological order

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

- `next_offset` is `offset + page_size`
- `next_page` is true only when `limit` is set and more messages remain
- The CLI builds `next_command` so callers can continue pagination without reconstructing flags by hand

## Project Matching Rules

Relevant source: `src/messages/service.rs`

When `--project` is provided:

- Codex matching accepts either `/Users/me/proj` or `Users/me/proj`
- When `--source` is omitted, project resolution searches all supported sources
- Claude matching can succeed on either the stored project directory name or the preserved `cwd`-based `project_path`
- Cursor matching currently uses the stored project directory name under `~/.cursor/projects/`

This means Cursor project filters typically need the encoded name (for example `-Users-me-proj`) unless the CLI branch does its own cwd-to-name translation first, as `export` does.

## CLI-Layer Scoping Rules

Relevant source: `src/cli.rs`

- `sessions` and `messages` auto-discover the cwd project by default unless `--project` is provided, `--all` is set, or `MMR_AUTO_DISCOVER_PROJECT=0`
- If cwd discovery fails, `sessions` and `messages` fall back to all projects
- If cwd discovery succeeds but there are no matching records, the CLI returns an empty result instead of widening the query
- `messages --session <id>` bypasses cwd project auto-discovery when `--project` is omitted and searches all projects instead
- When `messages --session` is used without `--source`, the CLI prints a narrowing hint on stderr
- `export` without `--project` infers the project from cwd and queries each matching source separately before merging results chronologically
