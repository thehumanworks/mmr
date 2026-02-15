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
| source_file | VARCHAR | Absolute JSONL path used for this row |
| source_offset | BIGINT | Byte offset of the source JSONL line |

**PK**: `id`

#### `ingest_files`
| Column | Type | Notes |
|---|---|---|
| source | VARCHAR | `'claude'` or `'codex'` |
| file_path | VARCHAR | Absolute JSONL file path |
| project | VARCHAR | Project key for this file |
| project_path | VARCHAR | Human path for this file |
| session_id | VARCHAR | Session ID for this file |
| is_subagent | BOOLEAN | Claude subagent file marker |
| last_offset | BIGINT | Last ingested byte offset |
| file_size | BIGINT | Last seen file size |
| file_mtime_unix | BIGINT | Last seen file mtime (unix seconds) |
| last_ingested_unix | BIGINT | Last refresh time |
| last_message_timestamp | VARCHAR | Last ingested message timestamp |
| last_message_key | VARCHAR | Last ingested message watermark key |

**PK**: `(source, file_path)`

#### `ingest_projects`
| Column | Type | Notes |
|---|---|---|
| source | VARCHAR | `'claude'` or `'codex'` |
| project | VARCHAR | Project key |
| project_path | VARCHAR | Project path |
| first_seen_unix | BIGINT | First time project was observed |
| last_seen_unix | BIGINT | Most recent time project was observed |
| last_ingested_unix | BIGINT | Most recent time project was ingested |

**PK**: `(source, project)`

#### `ingest_sessions`
| Column | Type | Notes |
|---|---|---|
| source | VARCHAR | `'claude'` or `'codex'` |
| project | VARCHAR | Project key |
| project_path | VARCHAR | Project path |
| session_id | VARCHAR | Session identifier |
| last_message_timestamp | VARCHAR | Latest ingested message timestamp |
| last_message_key | VARCHAR | Latest ingested message watermark key |
| last_ingested_unix | BIGINT | Most recent ingestion time |

**PK**: `(source, session_id)`

### Full-Text Search

DuckDB FTS extension indexes `messages` on: `content_text`, `role`, `project`, `msg_type`, `source`.

Search uses `fts_main_messages.match_bm25()` for ranked results, with LIKE fallback.

## Ingestion

CLI refresh path is incremental and file-diff based:

1. Discover all Claude/Codex JSONL files.
2. For each file, compare `(file_size, file_mtime_unix)` to `ingest_files`.
3. If unchanged, skip.
4. If append-only, seek to `last_offset` and ingest only new lines.
5. If rewritten/truncated, delete only rows from that `source_file` and reparse that file.
6. If a previously tracked file disappears, remove that file's rows and checkpoint.
7. Rebuild `sessions`, `projects`, and `ingest_sessions` from `messages` when data changed.
8. Rebuild FTS only when data changed.

Server mode still uses an in-memory DuckDB with full ingest at startup.

CLI query commands now auto-refresh the on-disk cache on every invocation (`projects`, `sessions`, `messages`, `search`, `stats`). `mmr ingest` remains available as an explicit full cache rebuild path.

## API

| Endpoint | Params | Description |
|---|---|---|
| `GET /api/projects` | — | List all projects (all sources) |
| `GET /api/sessions` | `name, source?` | List sessions for a project |
| `GET /api/messages` | `session` | Get all messages in a session |
| `GET /api/search` | `q, project?, source?, page?` | Full-text search |
| `GET /api/analytics` | — | Aggregate stats |
| `GET /openapi.json` | — | OpenAPI 3.1.0 spec |
