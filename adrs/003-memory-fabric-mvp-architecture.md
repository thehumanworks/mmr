# ADR-003: Memory Fabric MVP Architecture Contract

## Status

Accepted

## Date

2026-05-24

## Context

mmr is expanding from raw local history parsing into a local-first memory fabric.
The MVP needs a stable architecture contract before storage, capture, search,
sync, summary, and dreaming work fan out across separate tickets.

The project direction in Linear is source-neutral: Codex, Claude Code, Cursor,
human notes, terminal capture, and future agents are provenance sources, not
separate product abstractions.

## Decision

- Use a local SQLite/libSQL-shaped relational store as the canonical working
  store.
- Keep existing raw retrieval commands useful: `projects`, `sessions`,
  `messages`, and `export`.
- Add lean public commands only: `link`, `sync`, `status`, `note`, `rg`,
  `search`, `summary`, and `dream`.
- Rename `remember` to `summary` while preserving compatibility intentionally.
- Use `github:<authenticated-user>/mmr-store` as the first remote/export adapter,
  not as the hot event database.
- Redact before sync by default.
- Require learned memory to be evidence-linked and written only through
  `mmr dream`.
- Treat `docs/mmr-memory-fabric-mvp.md` and
  `tests/memory_fabric_contract.rs` as the initial implementation contract for
  downstream tickets.

## Consequences

- Downstream tickets should implement against the contract instead of reopening
  product direction.
- `link` and `sync` must be idempotent and non-destructive.
- Remote hydration must be replayable from redacted data.
- No `init`, `store`, `learn`, `context`, `candidates`, `knowledge`, `promote`,
  or `reject` command is part of the MVP.
- Pending contract tests may compile and remain ignored until the owning ticket
  implements the referenced module.
