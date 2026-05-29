---
title: "Reverse session selection for mmr messages"
description: "Add --session-back, --session-range, --include-newest, and the `mmr prev` subcommand so an agent can pull a previous session's messages by recency without knowing a session ID."
date: 2026-05-29
status: done
---

# GOAL: Reverse session selection for `mmr messages`

## Outcome

An AI agent (or human) running inside a session can pull the messages of a
**previous** session by recency, without knowing any session ID, using a
mental model that is impossible to misread:

```
mmr prev                         # the previous session, in the cwd project
mmr messages --session-back 1    # identical to `mmr prev`
mmr messages --session-back 2    # two sessions back
mmr messages --session-range 2..1   # the two sessions before the newest
```

You are **done** when those commands work, every check in
[Validation](#validation-run-this-exact-loop) is green, and the behavior below
is locked by tests. Treat the task as incomplete until then or until a step is
marked `[blocked]` with the smallest missing fact.

## Why this shape (read before coding)

`mmr` has **no notion of a "current session."** It has no session-ID
environment variable and no runtime signal for which session the caller is in
(verified: grep `src/` finds no `CLAUDE_SESSION_ID`/`CODEX_SESSION`; the only
`current_session_id` symbols in `src/capture.rs` are local ingest parsers).
`mmr messages` with no flags scopes to the cwd **project**, not a session.

Therefore "previous session" can only be defined by **recency**, and there is a
race: `mmr` reads provider JSONL at invocation time and lags un-flushed writes,
so the newest session it can see may be the caller's own half-written live
session. Returning that as "previous" would feed an agent fragments of its own
transcript — the worst failure for a context tool. The design below makes the
newest session unaddressable by default specifically to defend against this.

This supersedes the original strawman (`--from-index -1 --to-index -1`) in three
ways, each deliberate: an **unsigned ordinal** instead of a negative axis (no
clap hyphen-parsing trap, no signed-type ripple into `build_next_messages_command`,
no "is the current session counted?" ambiguity); the names **`--session-back` /
`--session-range`** instead of `--from-index/--to-index` (which collide visually
with the existing `--from-message-index/--to-message-index` message axis); and a
**count plus an inclusive `..` range** instead of inclusive-both-ends `-1 -1`
(whose endpoints contradict the existing exclusive `--to-message-index`).

## Decisions (locked defaults — flip only on maintainer instruction)

1. **Indexing:** one-based, unsigned, counted from the newest session in scope.
   Recency-age 0 = newest visible session; age 1 = previous; etc.
2. **Age 0 is not selectable by default.** `--include-newest` opts in.
3. **Out-of-range / age-0-without-opt-in:** hard structured error on `stderr`
   with non-zero exit, naming the available count. Never clamp, never silently
   return empty. A legitimately empty scope is distinct: empty response with
   `total_sessions_in_scope: 0`.
4. **`--all` (cross-project) and cross-source spans are allowed** and made
   self-explaining via `source` + `project_name` on every selected entry.
5. **The session axis honors `--source` / `MMR_DEFAULT_SOURCE`** exactly as
   `--latest` does today.

## Non-goals

- No new "current session" detection mechanism. Recency is the only definition.
- No change to the existing message axis (`--from-message-index` inclusive,
  `--to-message-index` exclusive, `usize`) — names, types, and contract stay.
- No change to existing default `mmr messages` output bytes (new response field
  is `Option` + skip-if-none).
- The strawman spelling `--from-index/--to-index` is **not** shipped; a
  copy-paste of it should fail with clap's normal "unexpected argument" error.

## Behavior spec (the contract to test against)

### Session-axis surface (new)

| Surface | Type | Behavior |
|---|---|---|
| `--session-back <N>` | `u32`, N ≥ 1 | The single session at recency-age N (counting back from, and excluding, the newest). `0` rejected unless `--include-newest`. |
| `--session-range <FROM..TO>` | two `u32` ages, FROM ≥ TO ≥ 1 | Contiguous span by age, **both ends inclusive**, written older-bound `..` newer-bound (`2..1` = ages 1 and 2). |
| `--include-newest` | bool | Makes age 0 addressable. Off by default. |
| `mmr prev [N]` | subcommand, default N = 1 | Sugar for `mmr messages --session-back N`; accepts scope flags `--project/--all/--source/--limit/--pretty`. |

### Composition & conflicts

- `--session-back`, `--session-range`, `--session <id>`, `--latest` are mutually
  exclusive **session selectors**: any two together is a usage error on
  `stderr` (mirror the teleport "pass either --session or --latest, not both"
  guard), non-zero exit.
- `--from-message-index/--to-message-index/--limit/--offset/--sort-by/--order`
  **compose on top of** the selected session(s).
- `--all/--project/--source` define the scope the ages are computed over
  **before** age assignment. Bare `--session-back 1` = "previous session in this
  cwd project" (ADR-002 cwd defaults preserved).
- Ranking reuses the existing `sort_sessions(Timestamp, Desc)` comparator and its
  full tie-break chain so age assignment is deterministic.
- Messages from a multi-session range are merged and returned chronologically
  (preserve the existing "newest window, then chronological" pagination trick).

### Acceptance examples (turn these into tests)

| Command | Expected |
|---|---|
| `mmr prev` | Previous session (age 1) in cwd project, chronological, capped at `--limit`. Strawman `--from-index -1 --to-index -1` maps here. |
| `mmr messages --session-back 1 --pretty` | Same session; `session_selection.selected[0].age == 1`. |
| `mmr messages --session-range 2..1` | The two sessions before the newest (ages 1 & 2) merged chronologically. Strawman `--from-index -2 --to-index -1` maps here. |
| `mmr messages --session-range 2..1 --all` | Same span over all projects/sources, ranked by `sort_sessions`; each `selected` entry carries `source` + `project_name`. |
| `mmr messages --session-back 0` | Error `age_zero_not_selectable`, non-zero exit. |
| `mmr messages --session-back 0 --include-newest` | Newest session (age 0). |
| `mmr messages --session-back 5` (3 older sessions) | Error `session_back_out_of_range`, names `total_sessions_in_scope` and `max_selectable_age`, non-zero exit. |
| `mmr messages --session-back 1 --latest 5` | Usage error: pick one selector. |

### JSON response change

Add exactly one optional field to `ApiMessagesResponse`
(`src/types/api.rs`), serialized with
`#[serde(skip_serializing_if = "Option::is_none")]` so every existing response
is byte-identical:

```jsonc
"session_selection": {
  "scope":  { "project": "mmr" | null, "all": false, "source": "codex" | null },
  "axis":   "session-back" | "session-range",
  "total_sessions_in_scope": 12,
  "selected": [
    { "age": 1, "session_id": "…", "source": "codex", "project_name": "mmr",
      "first_timestamp": "…", "last_timestamp": "…", "message_count": 87,
      "equivalent_command": "mmr messages --session <id>" }
  ],
  "skipped_newest": { "age": 0, "session_id": "…", "last_timestamp": "…", "assumed_live": true }
}
```

`total_messages` keeps its current meaning (scoped count before the
message-index window). `next_page/next_offset/next_command` stay driven by
`--limit/--offset`. The field is present only when a session-axis flag is used.

## Working agreements (how to build it)

- **TDD, strictly.** For every behavior, write the failing test first, watch it
  fail for the right reason, then make the smallest change to pass. A test that
  passes immediately is suspicious — tighten the assertion. Update tests before
  implementation (`test-discipline.mdc`).
- **Composable code.** Keep pure logic (range parsing, age assignment,
  validation) in small functions independent of clap and I/O, mirroring the
  existing `validate_message_index_range` / `apply_message_index_range` split.
  Reuse `sort_sessions`, `sort_messages`, `apply_message_index_range`, and the
  pagination block rather than duplicating them. Inject `now`/clock where a test
  needs determinism.
- **Don't mix refactors with behavior changes** in the same commit; if a refactor
  is needed to keep things composable, land it separately first.
- **Preserve the contract** (`cli-contract.mdc`): stable response shapes,
  machine-readable JSON on `stdout`, diagnostics/colored errors on `stderr`,
  newest-window-then-chronological pagination, per-item `source`/`project_name`.
- **Comments:** only where the *why* is non-obvious (e.g. why age 0 is excluded).
  No narration of *what* the code does.

## Phased plan (each phase = red → green → refactor → verify)

1. **Parsers/validators** (`src/cli.rs`, pure fns). `parse_session_range("FROM..TO")
   -> RangeInclusive<u32>` and a `--session-back` validator. Table-tests:
   `1`→age 1; `2..1`→{1,2}; `0` rejected; `-1` rejected; reversed range (`1..2`)
   rejected; non-numeric rejected. No production wiring yet.
2. **Service selection** (`src/messages/service.rs`).
   `messages_by_session_age(scope, source_filter, ages, include_newest, options)`:
   rank in-scope `SessionAggregate`s via `sort_sessions(Timestamp, Desc)`, assign
   ages (0 = newest), enforce decisions #2/#3, resolve to concrete session keys,
   then reuse the existing per-session filter + `sort_messages` +
   `apply_message_index_range` + pagination. Pin exact `session_id`s for ages 0,
   1, and a 2-wide range against a fixture (as `latest_session_messages` tests do).
3. **Wiring + response** (`src/cli.rs`, `src/types/api.rs`). Clap flags + the
   `mmr prev` subcommand + the mutual-exclusion guard (beside the existing
   `validate_message_index_range` call); add `SessionSelection`/`SelectedSession`/
   `SkippedNewest` and the optional field on `ApiMessagesResponse`.
4. **Pagination stability (riskiest — write the failing test first).** Recency
   ages are unstable across time. `next_command` for a paged session-axis query
   must **not** echo `--session-back`; it must pin to the resolved concrete
   session id(s) (the `equivalent_command` form) so a session landing between
   calls cannot shift the window. Test: page 1, inject a new session, page 2,
   assert the same session set.
5. **Docs.** `adrs/003-reverse-session-selection.md` (recency-not-identity,
   age-0-excluded default, the three strawman overrides); a `--session-back` /
   `mmr prev` section in `specs/messages.md` with the two-axis table and the
   live-session-lag caveat; a row in
   `docs/references/session-lookup-invariants.md` (session-axis selectors stay
   cwd-scoped; literal `--session <id>` stays global).

## Validation (run this exact loop; report real output)

Per `verification-loop.mdc`, after meaningful changes:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Plus a manual smoke against fixtures or real history:

```bash
cargo run -- prev --pretty
cargo run -- messages --session-back 1 --pretty
cargo run -- messages --session-range 2..1 --pretty
cargo run -- messages --session-back 0            # expect structured error, non-zero exit
```

Report failures with concrete command output and fix before finalizing. Never
claim success on red, skipped, or partial checks. Existing `tests/cli_contract.rs`
snapshots of default `mmr messages` output must stay green (proof the new field
is invisible when unused).

## Definition of Done

- [ ] All acceptance examples pass as fixture-driven tests (temp `HOME`, `run_cli_in_dir`).
- [ ] `--session-back`, `--session-range`, `--include-newest`, `mmr prev` implemented; selectors mutually exclusive.
- [ ] `session_selection` field added as `Option` + skip-if-none; existing output unchanged.
- [ ] Out-of-range and age-0 paths error loudly with non-zero exit and named counts.
- [ ] Pagination pins to concrete session ids (stability test green).
- [ ] Full verification loop green; benchmark contract run explicitly.
- [ ] ADR-003, `specs/messages.md`, and session-lookup-invariants updated.
