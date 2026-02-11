# Architecture

## Data Model

### Entity Relationship

```
source (claude | codex)
  └── 1:N ── project
                └── 1:N ── session
                              └── 1:N ── message
```

A **source** is one of `claude` or `codex`. Each source has many **projects** (identified by working directory). Each project has many **sessions** (individual conversations). Each session has many **messages**.

Session IDs are globally unique:
- Claude: UUID derived from filename (`{UUID}.jsonl`)
- Codex: `rollout-{timestamp}-{uuid}` from `session_meta.payload.id`

Because session IDs are globally unique, a `session_id` alone is sufficient to identify a session and retrieve its messages. The `project` and `source` can be derived from the session.

### Tables

#### `projects`
| Column | Type | Notes |
|---|---|---|
| name | VARCHAR | Dash-encoded project dir name (e.g. `-Users-mish-memory`) |
| source | VARCHAR | `'claude'` or `'codex'` |
| original_path | VARCHAR | Decoded filesystem path (e.g. `/Users/mish/memory`) |
| session_count | INTEGER | Aggregate count |
| message_count | INTEGER | Aggregate count |

**PK**: `(name, source)`

#### `sessions`
| Column | Type | Notes |
|---|---|---|
| session_id | VARCHAR | Globally unique identifier |
| project | VARCHAR | FK to projects.name |
| source | VARCHAR | FK to projects.source |
| first_timestamp | VARCHAR | Earliest message timestamp |
| last_timestamp | VARCHAR | Latest message timestamp |
| message_count | INTEGER | Total messages in session |
| user_messages | INTEGER | Count of role='user' |
| assistant_messages | INTEGER | Count of role='assistant' |

**PK**: `(session_id, project, source)` — composite, though `session_id` alone is unique in practice.

#### `messages`
| Column | Type | Notes |
|---|---|---|
| id | INTEGER | Auto-incrementing PK |
| source | VARCHAR | `'claude'` or `'codex'` |
| project | VARCHAR | Dash-encoded project name |
| project_path | VARCHAR | Decoded filesystem path |
| session_id | VARCHAR | FK to sessions.session_id |
| is_subagent | BOOLEAN | True for Claude subagent messages |
| message_uuid | VARCHAR | Claude message UUID (empty for codex) |
| parent_uuid | VARCHAR | Claude parent message UUID |
| msg_type | VARCHAR | JSONL line type (e.g. `user`, `assistant`) |
| role | VARCHAR | `'user'` or `'assistant'` |
| content_text | VARCHAR | Extracted text content |
| model | VARCHAR | Model name/ID |
| timestamp | VARCHAR | ISO timestamp |
| cwd | VARCHAR | Working directory at time of message |
| git_branch | VARCHAR | Git branch (codex only) |
| slug | VARCHAR | Claude slug identifier |
| version | VARCHAR | CLI version |
| input_tokens | BIGINT | Token usage |
| output_tokens | BIGINT | Token usage |

**PK**: `id`

### Full-Text Search

DuckDB FTS extension indexes `messages` on: `content_text`, `role`, `project`, `msg_type`, `source`.

Search uses `fts_main_messages.match_bm25()` for ranked results, with LIKE fallback.

## Ingestion

1. `ingest_claude()` — walks `~/.claude/projects/{project-dir}/{uuid}.jsonl`, plus subagent files in `{uuid}/subagents/`
2. `ingest_codex()` — walks `~/.codex/sessions/` and `~/.codex/archived_sessions/` recursively
3. `sessions` table populated via aggregate INSERT from `messages` grouped by `(session_id, project, source)`

Server mode uses an in-memory DuckDB (full re-ingestion on every startup). CLI query commands use an on-disk DuckDB cache built/refreshed via `mmr ingest`.

## API

| Endpoint | Params | Description |
|---|---|---|
| `GET /api/projects` | — | List all projects (all sources) |
| `GET /api/sessions` | `name, source?` | List sessions for a project |
| `GET /api/messages` | `session` | Get all messages in a session |
| `GET /api/search` | `q, project?, source?, page?` | Full-text search |
| `GET /api/analytics` | — | Aggregate stats |
| `GET /openapi.json` | — | OpenAPI 3.1.0 spec |
