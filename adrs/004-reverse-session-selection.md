# ADR-004: Reverse Session Selection for `messages`

## Status

Accepted

## Date

2026-05-29

> Numbering note: the goal that drove this work referred to it as "ADR-003", but
> `003-memory-fabric-mvp-architecture.md` already claimed that number, so this
> record is filed as 004.

## Context

Agents and humans frequently want the messages of the **previous** session
without knowing any session ID — for example, to recover context after a
restart. `mmr` has no notion of a "current session": there is no session-ID
environment variable and no runtime signal for which session the caller is in
(`src/capture.rs` only parses local ingest state; nothing reads
`CLAUDE_SESSION_ID`/`CODEX_SESSION`). `mmr messages` with no flags already scopes
to the cwd **project**, not a session.

Therefore "previous session" can only be defined by **recency**. That introduces
a race: `mmr` reads provider JSONL at invocation time and lags un-flushed writes,
so the newest session it can see is often the caller's own half-written live
session. Returning that as "previous" would feed an agent fragments of its own
transcript — the worst failure mode for a context tool.

An earlier strawman proposed `--from-index -1 --to-index -1` (a signed axis with
inclusive endpoints on both ends). That spelling has three problems: signed
values trip clap's hyphen parsing and ripple a signed type through the
pagination plumbing; `--from-index`/`--to-index` collide visually with the
existing `--from-message-index`/`--to-message-index` message axis; and
inclusive-both-ends endpoints contradict the existing exclusive
`--to-message-index` contract.

## Decision

### Recency, not identity

Sessions in scope are ranked newest-first with the existing
`sort_sessions(Timestamp, Desc)` comparator (full tie-break chain), then assigned
**one-based, unsigned ages**: age 0 is the newest visible session, age 1 the
previous, and so on. There is no new "current session" detection mechanism;
recency is the only definition.

### Age 0 is not selectable by default

The newest visible session is assumed-live and is held back unless the caller
opts in with `--include-newest`. This is the core defence against the
live-session-lag race. When a session axis is used, the response documents the
held-back session under `session_selection.skipped_newest` so the exclusion is
visible rather than silent.

### Two selectors, plus sugar

- `--session-back <N>` (`u32`, `N ≥ 1`): the single session at recency-age `N`.
  `0` is rejected unless `--include-newest`.
- `--session-range <FROM..TO>` (two `u32` ages, `FROM ≥ TO ≥ 1`): a contiguous
  span by age, both ends inclusive, written older-bound `..` newer-bound, so
  `2..1` selects ages 1 and 2. The newest (age 0) is never range-addressable.
- `mmr prev [N]` (default `N = 1`): sugar for `mmr messages --session-back N`,
  accepting the scope flags `--project/--all/--source/--limit/--pretty`.

`--session-back`, `--session-range`, `--session <id>`, and `--latest` are
mutually exclusive session selectors; passing two together is a usage error on
stderr with a non-zero exit, mirroring the existing teleport selector guard.

### Errors are loud and named; empty scope is not an error

Out-of-range ages and age-0-without-opt-in produce a hard structured error on
stderr with a non-zero exit (machine JSON on stdout carrying `error_kind` plus
`total_sessions_in_scope` / `max_selectable_age`). The axis never clamps and
never silently returns empty. A legitimately empty scope is distinct: it returns
an empty success with `session_selection.total_sessions_in_scope: 0`.

### Pagination pins to concrete session ids

Because recency ages are unstable across time, the `next_command` for a paged
session-axis query pins to the **resolved concrete session id(s)** (the
`mmr messages --session <id>` form) rather than echoing `--session-back` /
`--session-range`. A session landing between page reads therefore cannot shift
the window. To make a multi-session range's continuation runnable, `--session`
is now repeatable.

### Self-describing cross-scope results

`--all` (cross-project) and cross-source spans are allowed. Every selected entry
carries `source` and `project_name` so a span that crosses projects or sources
explains itself without the caller re-deriving the ranking.

### Response shape

`ApiMessagesResponse` gains one optional `session_selection` field, serialized
with `skip_serializing_if = "Option::is_none"` and present only when a
session-axis selector is used, so every existing `messages` response stays
byte-identical. `total_messages` keeps its meaning (scoped count before the
message-index window); `next_page`/`next_offset`/`next_command` stay driven by
`--limit`/`--offset`.

## Consequences

- Agents can recover the previous session's context with `mmr prev` or
  `mmr messages --session-back 1` without knowing any session ID, and without
  risking contamination from their own live transcript.
- The strawman spelling `--from-index/--to-index` is not shipped; a copy-paste
  of it fails with clap's normal "unexpected argument" error.
- `--session` is now repeatable; single-value usage is unchanged.
- `--latest` no longer composes with `--session`; they are now mutually
  exclusive selectors (see `specs/messages.md`).
