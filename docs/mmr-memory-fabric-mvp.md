# mmr Memory Fabric MVP Contract

Status: accepted for NHL-268
Date: 2026-05-24
Linear project: `mmr Memory Fabric MVP`

This document locks the product, storage, CLI, and verification contract for the
lean mmr Memory Fabric MVP. Downstream implementation tickets should treat it as
the source of truth unless a later ADR or spec explicitly supersedes it.

## Product Thesis

mmr is a source-neutral memory fabric for local work. It should not care whether
a memory came from Codex, Claude Code, Cursor, a human note, terminal capture,
imported logs, or a future agent. Source is provenance, not the product
abstraction.

The central flow is:

```text
raw history -> retrieval/search/summary -> dream assimilation with evidence
```

## Goals

- Preserve useful raw transcript retrieval through `projects`, `sessions`,
  `messages`, and `export`.
- Add a canonical local SQLite/libSQL-shaped store for normalized memory events.
- Make first-run setup one command: `mmr link`.
- Make repeat reconciliation one command: `mmr sync`.
- Make state inspection one command: `mmr status`.
- Add human-authored observations with `mmr note`.
- Add exact and structured discovery with `mmr rg` and `mmr search`.
- Rename the stateless continuity brief command from `remember` to `summary`,
  keeping `remember` as a compatibility alias unless explicitly removed later.
- Add `mmr dream` as the only public stateful assimilation command.
- Redact before remote sync by default.
- Keep learned memory evidence-linked and reversible.

## Non-Goals

- No `mmr init`.
- No `store` command namespace.
- No public `learn`, `context`, `candidates`, `knowledge`, `promote`, or
  `reject` commands.
- No GitHub organization support in the happy path.
- No explicit GitHub repository argument in the happy path.
- No destructive cleanup during `link` or `sync`.
- No semantic/vector search requirement for the MVP. Exact and structured search
  ship first.

## Terminology

- Project: a local working directory known to mmr.
- Project alias: a source-specific project identifier for the same local
  project, such as a canonical path or Claude/Cursor hyphenated path.
- Source: provenance adapter, such as `codex`, `claude`, `cursor`, `note`, or
  a future source.
- Session: a source-provided conversation or interaction grouping.
- Event: the normalized unit of memory. Events are append-only and reference raw
  local evidence when available.
- Blob: large raw or derived content stored out of the hot event row.
- Source cursor: adapter progress state used to make import and watcher passes
  idempotent.
- Evidence ref: a stable pointer from a summary, search result, dream output, or
  learned memory record back to source events.
- Learned memory: an assimilated durable claim or preference derived by
  `mmr dream` from valid evidence refs.
- Sync manifest: replayable remote export metadata for hydration on a fresh
  host.

## CLI Contract

All machine-readable command output must remain JSON on stdout. Human diagnostics
and colored output belong on stderr only.

### Existing Raw Retrieval

The current raw commands remain useful and keep their existing response contracts:

- `mmr projects`
- `mmr sessions`
- `mmr messages`
- `mmr export`

The existing default source/project rules still apply:

- Omitting `--source` means all supported sources unless `MMR_DEFAULT_SOURCE`
  supplies a single source.
- `--source all` remains invalid.
- `sessions` and `messages` default to the cwd project when discovery succeeds,
  unless `--all` is set or `MMR_AUTO_DISCOVER_PROJECT=0`.
- `export` with no `--project` infers the project from cwd and emits the same
  `ApiMessagesResponse` shape as `messages`.

### `mmr link`

First-run setup for the current project.

Responsibilities:

- Ensure the local mmr store exists.
- Resolve the cwd to a project id and aliases.
- Detect or create the default remote descriptor `github:<user>/mmr-store`.
- Import/reconcile known local source history.
- Run redaction for syncable data.
- Sync redacted replayable state when credentials and network are available.
- Rebuild derived state such as search documents.
- Print the same state shape as `mmr status`.

Default behavior:

- Safe and idempotent.
- Works from a non-Git directory.
- Does not delete local history or raw evidence.
- If GitHub credentials are unavailable, still links locally and reports remote
  setup as pending.

### `mmr sync`

Repeat reconciliation for an already-linked project or store.

Responsibilities:

- Re-run source reconciliation.
- Re-run redaction for changed syncable data.
- Export or import remote replay records where available.
- Detect conflicts without destructive resolution.
- Print sync/status JSON.

Default behavior:

- Idempotent.
- No destructive cleanup.
- Raw local evidence is not uploaded unless a future explicit opt-in allows it.

### `mmr status`

Inspect local and remote state.

Minimum JSON fields:

```json
{
  "command": "status",
  "store": {
    "db_path": "...",
    "exists": true,
    "existed_before_command": true,
    "schema_version": 2,
    "expected_schema_version": 2,
    "schema_status": "ok"
  },
  "project": {
    "id": "...",
    "display_name": "...",
    "path_hash": "..."
  },
  "remote": {
    "descriptor": "github:<user>/mmr-store",
    "backend": "file-github",
    "available": true,
    "auth_status": "ok",
    "created": false
  },
  "status": {
    "linked": true,
    "sync_status": "synced",
    "events_total": 1,
    "source_counts": {
      "note": 1
    },
    "sync_status_counts": {
      "synced": 1
    },
    "redaction": {
      "policy_id": "redaction-policy:v1:default",
      "redacted_or_synced": 1,
      "blocked": 0,
      "pending": 0
    },
    "sync": {
      "remote_events": 1,
      "local_manifests": 1,
      "latest_manifest_id": "manifest:v1:...",
      "blocked_events": 0,
      "unsynced_events": 0
    }
  },
  "diagnostics": {
    "schema": {
      "status": "ok",
      "current_version": 2,
      "expected_version": 2,
      "action": null
    },
    "remote": {
      "status": "available",
      "descriptor": "github:<user>/mmr-store",
      "backend": "file-github",
      "available": true,
      "auth_status": "ok",
      "action": null
    },
    "sources": [],
    "privacy_filter": {
      "status": "degraded",
      "detector": "openai/privacy-filter",
      "reason": "...",
      "action": "..."
    },
    "summary_runner": {
      "agent": "cursor",
      "status": "missing_api_key",
      "command": "agent",
      "api_key_env": ["CURSOR_API_KEY"],
      "action": "..."
    },
    "dream_runner": {
      "runner": "mock",
      "status": "available",
      "command_configured": false,
      "command_env": "MMR_DREAM_COMMAND",
      "action": null
    },
    "actions": []
  }
}
```

`diagnostics.actions` is the human recovery checklist for common failure modes:
unlinked cwd, missing source roots, remote auth failure, privacy-filter
degradation, blocked sync, schema mismatch, or missing dream runner command.

### `mmr note`

Adds human-authored observations as first-class events.

Behavior:

- `mmr note <text>` records one note scoped to the linked or cwd project.
- `mmr note` reads stdin for multiline terminal workflows.
- Notes flow through the same store, search documents, redaction, sync, summary,
  and dream contracts as imported source events.
- Note events use source `note` and role `user`.

### `mmr rg`

POSIX-friendly exact search.

Behavior:

- Accepts a pattern argument and emits JSON on stdout by default, preserving the
  repo-wide machine-readable stdout contract.
- May add an explicit `--line` or equivalent line-oriented mode for shell
  pipelines, but that mode must be opt-in and documented as the exception.
- Searches generated search documents, not raw provider files.
- Every result includes an evidence ref or citation that can be resolved back to
  a normalized event.

### `mmr search`

Structured exact search over normalized memory.

Minimum filters:

- `--project`
- `--session`
- `--source`
- `--role`
- `--event-type`
- time range once the store layer has a stable timestamp contract

Minimum JSON result fields:

```json
{
  "query": "literal text",
  "results": [
    {
      "event_id": "evt:v1:...",
      "source": "codex",
      "project_id": "proj:v1:...",
      "session_id": "sess-codex-1",
      "role": "assistant",
      "snippet": "...",
      "citation": "mmr://event/evt:v1:..."
    }
  ],
  "total_results": 1
}
```

### `mmr summary`

Stateless continuity brief generation from prior sessions.

Behavior:

- Reuses the current `remember` selection model: latest, all, or one session.
- Uses the same agent/provider options that `remember` currently supports unless
  a downstream ticket narrows the surface intentionally.
- Keeps `--instructions` semantics: custom text replaces the default output
  instruction while preserving the base identity/input-format instruction.
- The user prompt stays neutral so the system instruction owns output behavior.
- `remember` remains an alias for `summary` during the MVP compatibility window.

### `mmr dream`

Stateful assimilation into learned memory.

Behavior:

- Analyzes the current project by default.
- Calls a configurable provider/model through the dream runner interface.
- Produces structured candidate records with evidence refs.
- Validates evidence refs before writing learned memory.
- Writes learned memory only through the local store.
- Does not expose public `learn`, `context`, `candidates`, `knowledge`,
  `promote`, or `reject` commands in the MVP.

## Store Location

The default local store path is:

```text
${XDG_DATA_HOME:-<dirs::data_dir()>}/mmr/mmr.db
```

Tests should set `HOME` and, where needed, `XDG_DATA_HOME` so they never read or
write the user's real mmr state.

## Initial Schema Boundary

The first store implementation should use additive migrations and a single
integer schema version table. Names below are logical table boundaries; exact SQL
types are owned by NHL-269.

NHL-269 must lock SQL details without changing the product contract:

- primary keys and foreign keys for every relationship below
- unique keys for event identity, project aliases, source cursors, and sync
  manifest entries
- not-null requirements for ids, timestamps, provenance, hashes, and status
  fields
- enum/status domains for sources, event types, redaction status, sync status,
  learned-memory status, and dream-run status
- indexes for project/session/source/timestamp lookup, event citation lookup,
  source cursor reconciliation, redaction blocking checks, and sync manifest
  replay
- deterministic ID formats for project, session, blob, summary, dream-run,
  learned-memory, and sync-manifest records

### `schema_migrations`

- `version`
- `name`
- `applied_at`
- `checksum`

### `projects`

- `id`
- `canonical_path`
- `display_name`
- `created_at`
- `updated_at`

### `project_aliases`

- `id`
- `project_id`
- `source`
- `alias`
- `alias_kind`
- `created_at`

### `project_links`

- `id`
- `project_id`
- `canonical_path`
- `created_at`
- `updated_at`

### `sources`

- `id`
- `name`
- `adapter_version`
- `enabled`
- `created_at`
- `updated_at`

### `sessions`

- `id`
- `project_id`
- `source`
- `source_session_id`
- `started_at`
- `ended_at`
- `title`
- `raw_local_ref`
- `created_at`
- `updated_at`

### `events`

- `id`
- `project_id`
- `session_id`
- `source`
- `source_event_id`
- `event_type`
- `role`
- `timestamp`
- `content_text`
- `content_hash`
- `parent_hash`
- `parser_version`
- `raw_local_ref`
- `blob_id`
- `redaction_policy_id`
- `sync_status`
- `created_at`

### `blobs`

- `id`
- `kind`
- `media_type`
- `content_hash`
- `storage_ref`
- `byte_len`
- `created_at`

### `source_cursors`

- `id`
- `source`
- `project_id`
- `cursor_key`
- `cursor_value`
- `parser_version`
- `last_event_hash`
- `updated_at`

### `redaction_policies`

- `id`
- `version`
- `description`
- `created_at`

### `redaction_runs`

- `id`
- `policy_id`
- `event_id`
- `status`
- `blocking_findings`
- `created_at`

### `redaction_spans`

- `id`
- `run_id`
- `event_id`
- `kind`
- `start_byte`
- `end_byte`
- `replacement`
- `confidence`
- `blocks_sync`

### `search_documents`

- `id`
- `event_id`
- `project_id`
- `session_id`
- `source`
- `document_text`
- `citation`
- `updated_at`

### `summaries`

- `id`
- `project_id`
- `selection_kind`
- `selection_ref`
- `agent`
- `model`
- `instructions_hash`
- `output_text`
- `created_at`

### `dream_runs`

- `id`
- `project_id`
- `provider`
- `model`
- `status`
- `input_evidence_hash`
- `output_hash`
- `created_at`
- `completed_at`

### `dream_candidates`

- `id`
- `dream_run_id`
- `project_id`
- `kind`
- `claim`
- `confidence`
- `evidence_refs_json`
- `status`
- `created_at`

### `learned_memory`

- `id`
- `project_id`
- `kind`
- `claim`
- `confidence`
- `status`
- `evidence_refs_json`
- `dream_run_id`
- `created_at`
- `superseded_by`

### `sync_manifests`

- `id`
- `remote`
- `project_id`
- `manifest_version`
- `root_hash`
- `redaction_policy_id`
- `created_at`

### `sync_manifest_entries`

- `id`
- `manifest_id`
- `entry_kind`
- `entry_ref`
- `content_hash`
- `sync_path`
- `created_at`

## Event Identity and Versioning

Event ids use content-addressed identity:

```text
evt:v1:<hex_sha256(canonical_event_identity_json)>
```

`canonical_event_identity_json` includes:

- `source`
- `source_session_id`
- `source_event_id` when available
- `event_type`
- `role`
- `timestamp`
- `content_hash`
- `parent_hash`
- `parser_version`

It excludes:

- local filesystem paths
- ingestion time
- sync status
- redacted text
- mutable search snippets

This keeps event identity stable across machines while allowing local raw refs to
remain private and host-specific.

Parser versions are part of event identity. A parser that changes normalization
semantics must bump its version so the reconciler can keep or supersede prior
events deterministically.

## Source Adapter Interface

Implementation tickets should converge on this logical interface:

```rust
pub trait SourceAdapter {
    fn source_name(&self) -> SourceName;
    fn adapter_version(&self) -> AdapterVersion;
    fn discover(&self, root: &SourceDiscoveryRoot) -> anyhow::Result<Vec<SourceSessionRef>>;
    fn import_session(&self, session: &SourceSessionRef) -> anyhow::Result<SourceImportBatch>;
    fn reconcile(&self, cursor: Option<&SourceCursor>) -> anyhow::Result<SourceImportBatch>;
}
```

`SourceImportBatch` must include:

- normalized events
- updated cursor proposals
- parser version
- raw local refs
- warnings for skipped malformed records

Adapter rules:

- Skip malformed source records and keep importing valid records.
- Preserve raw local refs locally.
- Preserve unknown source events as typed events or blobs when possible.
- Keep import and watcher paths idempotent.
- Use stable source file and line/offset metadata for deterministic ordering.

## Redaction Contract

Redaction runs before sync by default.

The MVP policy has two layers:

- Deterministic secret checks for API keys, tokens, private keys, env files, and
  common credential formats.
- Optional model-assisted PII detection when the dependency is available.

Sync is blocked when:

- any deterministic secret finding is unresolved
- a redaction run fails
- an event has never been evaluated by the active policy
- a learned memory record references invalid evidence

Remote payloads should carry redacted content and enough metadata to hydrate a
usable store without raw secrets.

## Summary Runner Contract

`summary` is stateless. It may call an agent/provider, but it does not write
learned memory. It may write a `summaries` row only as an audit/cache record once
the store exists.

The output instruction replacement behavior from `remember --instructions` is
part of the contract and must be preserved.

## Dream Runner Contract

Dreaming is stateful assimilation, not summarization.

NHL-278 implements the provider-neutral runner layer in `src/dream.rs`.
NHL-279 adds the public `mmr dream` command and durable learned-memory writes.

Runner selection resolves from an explicit runner override, project default,
user default (`MMR_DEFAULT_DREAM_RUNNER`), then the built-in `mock` runner. The
`command` runner is a local command adapter configured by `MMR_DREAM_COMMAND`;
it reads a JSON evidence request on stdin and emits structured dream output JSON
on stdout. `MMR_DREAM_COMMAND` is parsed as a program plus arguments, for
example `MMR_DREAM_COMMAND="python runner.py"`.

Shared-safe evidence bundles redact deterministic local PII, omit events blocked
by deterministic secret findings, and preserve normalized metadata plus
`mmr://event/<id>` refs.

`mmr dream --dry-run` validates proposed learned-memory changes without writing
state. `mmr dream --review` returns the same non-mutating proposal shape with a
review status. A normal `mmr dream` records a dream run and writes active
learned memory only after runner output passes schema and evidence-ref
validation.

Provider output must be structured before it can update learned memory:

```json
{
  "observations": [
    {
      "kind": "preference",
      "claim": "Use fixture-driven integration tests for cwd behavior.",
      "confidence": 0.82,
      "evidence_refs": ["mmr://event/evt:v1:..."]
    }
  ]
}
```

Validation rules:

- Every evidence ref must resolve to an event in the submitted dream evidence
  bundle.
- Claims without evidence are rejected.
- Output with unknown schema fields is rejected.
- Runner requests use shared-safe redacted evidence by default; raw local
  evidence requires explicit local-only opt-in and is blocked for command/API
  style runners.
- Confidence below `0.8`, counterevidence, or sensitive/identity-affecting
  content queues or rejects the item instead of applying active learned memory.
- Plain `observations` are candidates/audit material. Active learned memory must
  come from `learned_memory_updates` or explicit `claims`.
- A dream run may not silently overwrite learned memory. Supersession must be
  explicit via `superseded_by` or a later equivalent field.
- Learned memory sync remaps local evidence refs to redacted remote event refs,
  and hydration restores learned-memory rows after replaying remote events.
- Active learned memory is inspectable through existing `search`/`rg` surfaces;
  the MVP does not expose public `learn`, `context`, `candidates`, `knowledge`,
  `promote`, or `reject` commands.

## Sync Manifest Contract

GitHub is the first remote/export adapter and is not the canonical hot database.
The canonical working store is local SQLite/libSQL-shaped storage.

The default remote descriptor is:

```text
github:<authenticated-user>/mmr-store
```

NHL-277 implements the first backend as a file-backed GitHub-layout adapter. In
tests and local E2E runs, `MMR_FAKE_REMOTE_DIR` points at the fake repository
root while the public descriptor remains `github:<user>/mmr-store`. Live GitHub
smoke remains optional and must be explicitly gated by credentials.

Repository layout:

```text
remote.json
projects/<project-id>/
  project.json
  sessions/<source-session-id>/events/<event-id>.json
  search/<event-id>.json
  manifests/<root-hash>.json
```

Conflict strategy:

- event and search payload files are immutable/content-addressed by event id
- existing remote payloads are compared against the expected JSON before local
  events are marked synced
- hydration rejects remote events whose content hash or event id no longer
  matches the redacted payload
- manifests are root-hash addressed and replayable
- sync never appends to a shared hot file
- repeated sync writes no duplicate event payloads

Sync payload rules:

- full sync builds a redacted projection with deterministic local PII rules
- deterministic secret findings block payload export
- `tool_call`, `tool_result`, and `unknown_raw_event` stay blocked until a
  dedicated safe projection exists
- local raw refs and raw `search_documents.document_text` are not exported
- local raw-derived event ids are not exported; remote ids are derived from the
  redacted projection

Remote data must be replayable enough to hydrate a fresh host:

- schema version
- project records and aliases
- source/session/event metadata
- redacted event content
- redaction policy ids and run summaries
- search document inputs or enough data to rebuild them
- learned memory with evidence refs
- manifest hashes for idempotent sync

## Golden Fixtures

The initial fixture corpus lives under `tests/fixtures/memory_fabric/`:

- `codex_session.jsonl`
- `claude_like_session.jsonl`
- `human_note.jsonl`
- `tool_output_fake_secret.jsonl`
- `pii_heavy_sample.jsonl`
- `malformed_mixed_session.jsonl`

These are intentionally small and stable. Downstream tickets may add richer
fixtures, but should not rewrite the semantic purpose of these records.

## Verification Contract

Normal repo gate after meaningful code changes:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Downstream implementation tickets must also run or explicitly justify skipping:

- the relevant pending contract tests after removing their `#[ignore]`
- migration replay tests for storage changes
- fixture-backed source adapter tests for importer changes
- CLI smoke tests for every changed public command
- redaction adversarial cases before any sync/export change
- an adversarial review, local or delegated, for risky behavior changes
- docs/spec/ADR/progress-file updates when contracts or user workflows change
- a clean commit and push when the ticket is accepted for handoff

MVP contract tests:

```bash
cargo test --test memory_fabric_contract
```

There should be no ignored MVP contract tests at handoff. If a future ticket
adds a pending contract, it must remove the relevant `#[ignore]` once it
satisfies the behavior.

Active contract tests should continue to assert MVP non-goals. In particular,
`init`, `store`, `learn`, `context`, `candidates`, `knowledge`, `promote`, and
`reject` must remain outside the public command surface during the MVP.

Implemented contract ownership:

- NHL-269: schema validation and migration replay.
- NHL-270: source adapter normalization.
- NHL-272: redaction policy application.
- NHL-273: search document generation and citations.
- NHL-278 and NHL-279: dream output validation and learned-memory writes.
- NHL-277: sync manifest generation and hydration.
- NHL-280: summary command plus `remember` compatibility, status diagnostics,
  quickstart/recovery docs, and integrated CLI command-surface tests for
  `note`, `rg`, `search`, `link`, `sync`, `status`, `dream`, and `summary`.

## Dependency Graph

1. NHL-268 architecture/schema/verification contract blocks all work.
2. NHL-269 local store and migrations blocks capture, search, redaction, sync,
   summary audit records, and dreaming.
3. NHL-270 source adapter framework blocks provider importers.
4. NHL-271 notes, NHL-273 search, and NHL-272 redaction can proceed after the
   store contract is available.
5. NHL-274, NHL-275, and NHL-276 source importers proceed after the adapter
   framework.
6. NHL-277 link/sync/status and NHL-278 dream runner proceed after redaction.
7. NHL-279 dream assimilation proceeds after search and dream runner.
8. NHL-280 integrated CLI/docs follows the command surfaces and adds the
   `summary` compatibility path.
9. NHL-281 final release verification follows all implementation tickets.

## Resolved Questions

- Canonical hot store: local SQLite/libSQL-shaped storage.
- First remote adapter: default authenticated-user GitHub repo `mmr-store`.
- Public assimilation surface: `mmr dream` only.
- Raw retrieval: remains a core product surface.
- Redaction: blocks sync by default.

## Deferred Questions

- Exact SQLite crate and optional libSQL/Turso feature flag: NHL-269.
- Exact GitHub transport/auth crate or shell integration: NHL-277.
- Whether semantic/vector search is added after lexical search: post-MVP.
- Whether `remember` is removed after the compatibility window: post-MVP.
