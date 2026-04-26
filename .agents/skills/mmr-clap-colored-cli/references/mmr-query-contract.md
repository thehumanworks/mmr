# mmr Query Contract

## Table of Contents
- [Projects response contract](#projects-response-contract)
- [Sessions response contract](#sessions-response-contract)
- [Messages response contract](#messages-response-contract)
- [Default query scope and source behavior](#default-query-scope-and-source-behavior)
- [Sorting and pagination semantics](#sorting-and-pagination-semantics)
- [Project normalization and source-specific matching](#project-normalization-and-source-specific-matching)

## Projects response contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

`ApiProject` items include:

```rust
pub struct ApiProject {
    pub name: String,
    pub source: String,
    pub original_path: String,
    pub session_count: i32,
    pub message_count: i32,
    pub last_activity: String,
}
```

## Sessions response contract

```rust
#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}
```

Source: `src/types/api.rs`

Each `ApiSession` carries the source and project identity per row:

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

Each `ApiMessage` includes source and project metadata on every row:

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

`QueryService::messages` always leaves `next_command` as `None`; the CLI fills it only when another page exists so the suggested command preserves the current `messages` query shape.

## Default query scope and source behavior

- `--source` accepts `claude`, `codex`, or `cursor`. Omitting it means all sources unless `MMR_DEFAULT_SOURCE` supplies a default.
- `sessions` and `messages` accept optional `--project` and optional `--all`.
- Without `--project` and without `--all`, `sessions` and `messages` auto-discover the current project from cwd when possible.
- If cwd auto-discovery fails, the CLI falls back to the historical all-projects query.
- If cwd auto-discovery succeeds but there are no matching records, the CLI returns an empty result instead of widening the scope.
- `messages --session <id>` without `--project` is a special case: it bypasses cwd auto-discovery and searches all projects so globally unique session IDs remain discoverable. When `--source` is omitted in that mode, the CLI prints a stderr hint suggesting `--source` to narrow the lookup.
- `remember` participates in source filtering, but its default stdout format is markdown. Use `-O json` to request `RememberResponse` JSON.

Primary implementation sources: `src/cli.rs`, `src/messages/service.rs`, `tests/cli_contract.rs`, and `docs/references/session-lookup-invariants.md`.

## Sorting and pagination semantics

Messages keep the historical pagination contract for chronological output: when sorting by timestamp ascending, page from the newest window first, then reverse that page back into chronological order before returning it.

```rust
let descending = filtered.into_iter().rev().collect::<Vec<_>>();
let mut paged = apply_pagination(descending, limit, offset);
paged.reverse();
```

Source: `src/messages/service.rs`

Projects, sessions, and messages all use deterministic tie-breakers so repeated queries remain stable:

- Projects: primary metric, secondary metric, then `name`, `original_path`, `source`
- Sessions: primary metric, secondary metric, then `session_id`, `project_name`, `project_path`, `source`
- Messages: selected sort metric, then chronological order when needed, then `session_id`

## Project normalization and source-specific matching

Project filtering resolves against known project names and original paths for all three sources.

- Codex project matching accepts either `/Users/test/proj` or `Users/test/proj`.
- Claude and Cursor project matching use the encoded project directory name (slashes replaced by hyphens with a leading hyphen) when filtering by cwd-derived project identity.
- `export` uses the canonical cwd path for Codex and the encoded cwd-derived name for both Claude and Cursor, then merges per-source results into one chronological `ApiMessagesResponse`.

Normalization logic lives in `src/messages/service.rs` and cwd inference lives in `src/cli.rs`.
