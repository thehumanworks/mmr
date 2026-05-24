# mmr Memory Fabric Store

Status: implemented for NHL-269
Date: 2026-05-24

This document records the concrete local store shape that implements the MVP
contract in `docs/mmr-memory-fabric-mvp.md`.

## Location

Default DB path:

```text
${XDG_DATA_HOME:-<dirs::data_dir()>}/mmr/mmr.db
```

Tests and scripts should set `XDG_DATA_HOME` when they need an isolated store.

## Implementation

- Module: `src/store.rs`
- SQLite client: `rusqlite` with bundled SQLite
- Schema version: `1`
- Migration runner table: `schema_migrations`
- Checksum drift: opening a store fails if an already-applied migration version
  has a different checksum
- Hidden dev smoke command: `mmr __db-info`

`mmr __db-info` prints JSON with at least:

```json
{
  "db_path": "/tmp/data/mmr/mmr.db",
  "schema_version": 1
}
```

With `--project <path> --smoke-event`, it creates or reuses a project link for a
non-Git directory, inserts a deterministic synthetic event idempotently, reads it
back, and includes `project_id` plus `event_count`.

## Table Map

### `schema_migrations`

Tracks applied migrations.

Key fields:

- `version` primary key
- `name`
- `applied_at`
- `checksum`

### `projects`

Canonical project records.

Key fields:

- `id` primary key, deterministic `proj:v1:<sha256>`
- `canonical_path` unique
- `display_name`
- timestamps

### `project_aliases`

Source-specific aliases for project lookup.

Key fields:

- `id` primary key
- `project_id` foreign key
- `source`
- `alias`
- `alias_kind`
- unique `(source, alias)`

### `project_links`

Local link metadata for cwd/project path setup, including non-Git directories.

Key fields:

- `id` primary key
- `project_id` foreign key
- `canonical_path` unique
- timestamps

### `sources`

Adapter registry.

Key fields:

- `id` primary key
- `name` unique
- `adapter_version`
- `enabled`
- timestamps

### `sessions`

Normalized source sessions.

Key fields:

- `id` primary key, deterministic from project/source/source session id
- `project_id` foreign key
- `source`
- `source_session_id`
- `started_at`
- optional `ended_at`, `title`, `raw_local_ref`
- unique `(project_id, source, source_session_id)`

### `blobs`

Local references for large or raw content. The table stores references and hashes,
not raw sensitive blob bytes by default.

Key fields:

- `id` primary key
- `kind`
- `media_type`
- `content_hash` supplied by the caller from the referenced bytes or stable source
  payload, never derived from the local path alone
- `storage_ref`
- `byte_len`
- unique `(content_hash, storage_ref)`

### `events`

Append-only normalized memory events.

Key fields:

- `id` primary key, deterministic `evt:v1:<sha256>`
- `project_id`, `session_id`
- source provenance: `source`, `source_session_id`, optional `source_event_id`
- event fields: `event_type`, `role`, `timestamp`, `content_text`
- identity/version fields: `content_hash`, `parent_hash`, `parser_version`
- local-only evidence: `raw_local_ref`, optional `blob_id`
- sync safety: `redaction_policy_id`, `sync_status`

Idempotency:

- event insertion uses `ON CONFLICT(id) DO NOTHING`
- replaying the same normalized event does not duplicate rows

### `source_cursors`

Idempotent importer/watcher progress.

Key fields:

- `id` primary key
- `source`, `project_id`, `cursor_key`
- `cursor_value`
- `parser_version`
- optional `last_event_hash`
- unique `(source, project_id, cursor_key)`

### `redaction_policies`

Redaction policy registry.

Key fields:

- `id` primary key
- `version` unique
- `description`
- `created_at`

The initial migration inserts `redaction-policy:v1:default`.

### `redaction_runs`

Per-event redaction results.

Key fields:

- `id` primary key
- `policy_id`
- `event_id`
- `status`: `pending`, `passed`, `blocked`, or `failed`
- `blocking_findings`

The NHL-272 implementation uses a deterministic run id for each event/policy
pair. Rerunning redaction updates the run and replaces its spans, which keeps
scan output idempotent while allowing policy behavior to evolve with a future
policy version.

### `redaction_spans`

Concrete redaction spans for event content.

Key fields:

- `run_id`, `event_id`
- `kind`
- byte range
- `replacement`
- `confidence`
- `blocks_sync`

Blocking spans are never printed raw in `mmr sync --dry-run`; command responses
use redacted projections derived from spans when showing payload previews. Under
the default degraded PII coverage, dry-run omits payload previews entirely and
reports the degraded policy as the block reason.

### `search_documents`

Generated exact-search documents and citations.

Key fields:

- `event_id` unique
- `project_id`, `session_id`, `source`
- `document_text`
- `citation`

These rows are local-only source material until a redacted sync projection is
explicitly generated. Sync code must not export `document_text` directly.

NHL-273 search commands rebuild missing rows from normalized events on demand.
`mmr rg` and `mmr search` read these documents for lexical matches and return
`mmr://event/<event-id>` citations.

### `summaries`

Stateless summary audit/cache records.

Key fields:

- `project_id`
- `selection_kind`: `latest`, `all`, or `session`
- optional `selection_ref`
- `agent`, optional `model`
- `instructions_hash`
- `output_text`

### `dream_runs`

Provider-backed dream execution records.

Key fields:

- `project_id`
- `provider`
- `model`
- `status`: `running`, `completed`, or `failed`
- `input_evidence_hash`
- optional `output_hash`

### `dream_candidates`

Structured dream observations before learned-memory write decisions.

Key fields:

- `dream_run_id`
- `project_id`
- `kind`
- `claim`
- `confidence`
- `evidence_refs_json`
- `status`: `pending`, `accepted`, or `rejected`

### `learned_memory`

Evidence-linked durable memory claims.

Key fields:

- `project_id`
- `kind`
- `claim`
- `confidence`
- `status`: `active`, `pending`, `superseded`, or `rejected`
- `evidence_refs_json`
- optional `dream_run_id`
- optional `superseded_by`

### `sync_manifests`

Replayable remote export manifests.

Key fields:

- `remote`
- `project_id`
- `manifest_version`
- `root_hash`
- `redaction_policy_id`

NHL-277 records one row per unique remote/project/root-hash projection. The
root hash is derived from redacted event/search payload entries, so repeated
syncs record the same manifest id instead of appending duplicate state.
Remote payload files are append-only for a given path: if a file already exists,
sync compares its JSON against the expected projection and fails on mismatch
before marking local events synced.

### `sync_manifest_entries`

Manifest entry map.

Key fields:

- `manifest_id`
- `entry_kind`
- `entry_ref`
- `content_hash`
- `sync_path`
- unique `(manifest_id, entry_kind, entry_ref)`

Entry kinds currently generated by full sync:

- `event`: redacted event payload under
  `projects/<project-id>/sessions/<source-session-id>/events/<event-id>.json`
- `search_document`: redacted search projection under
  `projects/<project-id>/search/<event-id>.json`

## Store API Coverage

Implemented and covered by tests:

- migration replay from empty DB
- migration idempotency
- checksum drift rejection
- default DB path resolution
- project link creation and lookup from non-Git directories
- source/session upsert
- append-only event insertion with deterministic ids/content hashes
- idempotent parser replay
- parent hash and parser version identity semantics
- source cursor update/read
- source cursor parser-version and last-event-hash metadata reads
- raw blob reference insertion/read
- blob refs require an explicit content hash separate from local storage path
- sync manifest insertion/read and manifest-entry replay metadata
- event sync-status marking after remote replay
- transaction rollback on failure
- CLI smoke path for schema/path inspection plus synthetic insert/read

## Deferred

- Dream runner and learned-memory validation: NHL-278 and NHL-279.
