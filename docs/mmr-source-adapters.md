# mmr Source Adapter Framework

Status: implemented for NHL-270
Date: 2026-05-24

The source adapter framework is the provider-neutral capture layer for normalized
memory events. Existing raw retrieval loaders remain storage-free; adapters feed
the Memory Fabric store introduced in NHL-269.

## Layers

- Adapter discovery finds source session files or future source handles.
- Watcher deltas read append-only file changes without assuming Git.
- Reconciler scans known roots and backfills missed events idempotently.

Hooks may be added later as lifecycle markers, but they are not the only
persistence mechanism.

## Core Types

Implemented in `src/capture.rs`:

- `SourceAdapter`
- `SourceDiscoveryRoot`
- `SourceSessionRef`
- `SourceImportBatch`
- `NormalizedEvent`
- `SourceCursorUpdate`
- `SourceWarning`
- `FileWatcher`
- `Reconciler`

## Event Boundaries

Adapters normalize provider data into these boundaries:

- `session_start`
- `user_turn`
- `assistant_turn`
- `tool_call`
- `tool_result`
- `compaction`
- `session_end`
- `unknown_raw_event`

Every normalized event must preserve:

- source name
- source session id
- parser version
- timestamp
- role or boundary-derived role
- raw local ref
- optional source event id
- optional parent hash

## Watcher Semantics

The file watcher reads byte deltas from a tracked offset.

- Partial trailing JSONL rows are not emitted until a newline completes the row.
- When a partial trailing row follows complete rows, the offset advances through
  the complete rows so already-emitted rows are not replayed.
- Truncation or rotation is detected when the current file length is shorter
  than the stored offset.
- Same-size or larger replacement is detected with the file fingerprint carried
  by `WatchState`/`WatchDelta`.
- Watcher output is bytes-only; parsing remains an adapter responsibility.

## Reconciler Semantics

The reconciler:

- calls `discover`
- imports each discovered session
- writes normalized events through `Store::insert_event`
- writes source cursor updates with parser version and last event hash
- returns a report with discovered count, imported count, warnings, and stable
  event ids

Store insertion is idempotent, so watcher simulation and reconciler replay should
converge on the same final event ids when adapters preserve stable source event
ids for partial and full parses.

## Fixture Adapter

`FixtureAdapter` is the conformance adapter used by tests and later CLI smoke
paths. It parses generic JSONL fixtures, skips malformed rows with warnings,
preserves raw local refs, and maps known fixture shapes into normalized boundary
events.

Provider-specific importers in NHL-274 through NHL-276 should reuse the same
traits and report structures rather than adding parallel ingestion contracts.

## Adding A Provider

1. Implement `SourceAdapter`.
2. Discover provider session refs without requiring Git metadata.
3. Parse defensively and skip malformed records with warnings.
4. Preserve unknown records as `unknown_raw_event` where possible.
5. Fill parser version and raw local ref on every event.
6. Emit cursor updates keyed by source file/path or provider cursor id.
7. Add adapter conformance tests using Memory Fabric fixtures.
8. Add watcher/reconciler tests for partial writes, rotation, duplicate events,
   and missed-event backfill.
