# mmr Query Contract

## Table of Contents

- [Projects response contract](#projects-response-contract)
- [Sessions response contract](#sessions-response-contract)
- [Messages response contract](#messages-response-contract)
- [Sorting and pagination semantics](#sorting-and-pagination-semantics)
- [Project resolution semantics](#project-resolution-semantics)

## Projects response contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Source of truth: `src/types/api.rs`

Notes:

- `projects` omits envelope-level source or project metadata because each `ApiProject` is self-describing.
- Default CLI behavior with no `--source` is "all sources" unless `MMR_DEFAULT_SOURCE` supplies a default.

## Sessions response contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Source of truth: `src/types/api.rs`

Each `ApiSession` currently includes per-item:

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

## Messages response contract

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

Source of truth: `src/types/api.rs`

Each `ApiMessage` currently includes per-item:

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

`next_command` is populated by the CLI layer in `src/cli.rs`, not by `QueryService` itself.

## Sorting and pagination semantics

Source of truth: `src/messages/service.rs`

- Project and session sorting use deterministic tie-breakers so output remains stable when primary sort keys match.
- `messages` default sorting is `timestamp asc`.
- For that default ascending-timestamp view, pagination preserves the historical "newest window, then chronological output" contract:

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

- `next_page` is `true` only when a `limit` was provided and more results remain.
- `next_offset` is computed as `offset + returned page size`.

## Project resolution semantics

Source of truth: `src/messages/service.rs`

When `--project` is provided:

- Codex lookup accepts either `/Users/test/proj` or `Users/test/proj`.
- With no `--source`, project resolution searches all supported sources.
- Claude and Cursor project matching use the same resolved project-name machinery as the CLI-facing commands.

Current supported sources are:

- `claude`
- `codex`
- `cursor`
