# ADR 0001: Persisted CLI Cache With Explicit `ingest`

Date: 2026-02-06  
Status: Accepted

## Context

The `memory` CLI is frequently invoked in scripts/agents, but each invocation currently:

1. Creates a new **in-memory** DuckDB (`Connection::open_in_memory()`).
2. Scans the filesystem for Claude/Codex JSONL logs (`~/.claude/projects/`, `~/.codex/sessions/`).
3. Re-ingests all messages + rebuilds FTS.
4. Runs the query command and exits.

This makes command latency scale with *total history size*, even for simple reads like `memory projects`.

### Root Cause

`run_cli()` always calls `ingest_all()` + `create_fts_index()` on a fresh in-memory DB for every CLI subcommand invocation.

## Decision

Introduce an on-disk DuckDB **cache** for CLI queries, and make ingestion an explicit operation:

- Add `memory ingest` (alias: `memory refresh`) to (re)build the cache.
- Make the read-only CLI subcommands (`projects`, `sessions`, `messages`, `search`, `stats`) open and query the cache DB instead of re-ingesting.
- If the cache is missing or not initialized, the CLI fails with a clear message instructing the user to run `memory ingest`.

Implementation details:

- Cache DB location defaults to an OS cache directory (override via `MEMORY_DB_PATH`).
- `memory ingest` writes to a temporary DB file, then swaps it into place via rename to avoid partially-written caches.
- A `cache_meta` table stores `schema_version` and refresh metadata for validation.

## Flows

### Old Flow (CLI query commands)

```text
memory <query-cmd>
  -> parse args
  -> open DuckDB (in-memory)
  -> scan ~/.claude + ~/.codex
  -> ingest_all()
  -> create_fts_index()
  -> run cmd_* query
  -> print JSON
```

### New Flow

```text
memory ingest
  -> parse args
  -> open DuckDB (temp file)
  -> ingest_all()
  -> create_fts_index()
  -> write cache_meta
  -> rename(temp -> cache path)

memory <query-cmd>
  -> parse args
  -> open DuckDB (cache file)
  -> validate cache_meta schema_version
  -> run cmd_* query
  -> print JSON
```

### Server Flow (Unchanged)

```text
memory (or memory serve)
  -> open DuckDB (in-memory)
  -> ingest_all()
  -> create_fts_index()
  -> start Axum server
```

## Consequences

Benefits:

- CLI query commands become fast and predictable (no repeated filesystem scanning/ingestion).
- Better suited for scripting and repeated calls.
- Clear separation of concerns: “refresh data” vs “query data”.

Tradeoffs:

- Cache can be stale until `memory ingest` is run.
- Small amount of disk usage for the cache DB.

## Alternatives Considered

- Incremental ingestion based on file mtimes: faster than full ingest, but more complex (needs per-file state + merge/delete handling).
- Keep a daemon/server and have the CLI proxy to it: great latency, but adds lifecycle management and operational complexity.

