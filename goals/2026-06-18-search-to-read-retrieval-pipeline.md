---
goal_id: "search-to-read-retrieval-pipeline"
title: "Implement search-to-read retrieval pipeline"
description: "Implement the PRD's search-to-read retrieval pipeline as a first-class mmr CLI contract that turns a literal clue into ranked, bounded session context with citations."
date: "2026-06-18"
status: "in-progress"
confidence_floor: 90
created: "2026-06-18"
updated: "2026-06-18"
---

# Goal: `mmr retrieve <query>` uses literal search to return ranked, cited, bounded session-read packets.

## 1. Invariants · the rules that must not break

This file is the only state — if it isn't written here, it didn't happen. The full
procedure (boot loop, confidence rubric, logging cadence) lives in the
**goal-driven-development** skill; these rules hold even if that skill isn't loaded:

- **Scope is frozen after user confirms DoD + Tasks.** Until then, §3 and §5 may be
  edited freely. After confirm, the only permitted edits are: tick checkboxes (Task
  **and** DoD), update Confidence, append Evidence, append to the live sections
  (§6/§7/§8), and update frontmatter `status`/`updated` — never add, remove, reword,
  split, or merge a DoD item or Task, and never rewrite or delete a live-section entry.
- **Never tick below the floor.** A task is ticked done only at Confidence >=
  `confidence_floor`. If you cannot reach it, leave it unticked and fire `CONFIDENCE-STALL`.
- **Scope change is an exit, not a decision.** If scope must change, record the
  proposal in §6 and fire `SCOPE-CHANGE` — stop and surface it to the user.
- **Live sections are append-only.** Log each decision (§6) and learning (§7) at
  the moment it happens — before ticking the task it came from. Never delete entries.

---

## 2. References

Everything the agent needs before/while working. Each entry is `path-or-url — why it matters`.

- `goals/2026-06-18-feature-prd-opportunities.md` — PRD 1 defines the Search-to-read retrieval pipeline, target user, non-goals, risks, and DoD.
- `AGENTS.md` — repo workflow, command taxonomy, verification loop, commit/push conditions, and mmr-specific learned constraints.
- `.cursor/rules/verification-loop.mdc` — required Rust verification loop before claiming implementation complete.
- `.cursor/rules/cli-contract.mdc` — stdout/stderr, source/project filtering, message ordering, and response-shape constraints for CLI changes.
- `.cursor/rules/test-discipline.mdc` — fixture-driven integration test expectations.
- `src/cli.rs` — clap command surface, current `find` implementation, search response structs, `read` routing, and pagination command builders.
- `src/messages/service.rs` — session/message aggregation, source/project filtering, message paging, and session-axis selection helpers.
- `src/types/api.rs` — public API response structs for read/message payloads.
- `src/mcp.rs` — MCP tools/prompts, including the existing `mmr_find_then_read` prompt that composes this workflow manually.
- `tests/memory_fabric_contract.rs` — existing `find` fixture tests, `mmr://event/...` citation checks, and raw-local-ref leakage assertions.
- `tests/cli_contract.rs` — CLI integration patterns for read/session pagination, JSON contract, and current command parser tests.
- `tests/mcp_contract.rs` — MCP tool/prompt registration contract tests.
- `specs/messages.md` — current recall/read behavior, pagination stability, and session-selection documentation style.

### 2.1 Draft Retrieval Contract

Freeze this contract before implementation. If implementation proves any item
wrong, fire `SCOPE-CHANGE` instead of silently changing §3 or §5 after user
confirmation.

**Command surface**

- Add `mmr retrieve <query>` as a new intent-first top-level command.
- Reuse existing literal `find` semantics over normalized search documents.
- Supported retrieval flags:
  - `--project <path>`: explicit project scope.
  - Global `--source claude|codex|cursor|grok|pi` and `MMR_DEFAULT_SOURCE`: source filter.
  - `--session <id>`: restrict matching to one session id within the project/source scope.
  - `--role <role>`, `--event-type <type>`, `--ignore-case`, `-C/--context <n>`: same meaning as `find`.
  - `--max-sessions <n>`: selected-session cap; default `3`.
  - `--before-messages <n>`: messages before each matched event; default `3`.
  - `--after-messages <n>`: messages after each matched event; default `12`.
  - `--max-messages-per-session <n>`: hard cap after window merge; default `24`.
  - `--limit <n>` and `--offset <n>`: page the flattened selected-session message windows after dedupe; default `limit = max_sessions * max_messages_per_session` (`72` with defaults), and `limits.limit` must serialize that concrete derived value.
  - `--pinned-session <json>`: concrete continuation selector used by `next_command`; repeatable JSON object with `source`, `project_name`, and public provider `source_session_id`; bypasses fuzzy re-selection.
- `--all`, `--remote`, semantic/vector search, automatic summarization, and legacy `search`/`rg` aliases are out of scope for this goal.
- A new first-class MCP tool is out of scope for v1, and the existing `mmr_find_then_read` MCP prompt may remain as the manual fallback for MCP clients. This is an accepted v1 limitation unless the user expands scope.

**Scope and matching**

- Without `--project`, use the same current-directory linked-project behavior as `find`; `next_command` must materialize the resolved project name/path so continuation does not depend on ambient cwd.
- Global `--source` and `MMR_DEFAULT_SOURCE` filter both matching and selected session reads; `next_command` must materialize the effective source filter when one is active.
- Match groups and continuation must use the public provider `source_session_id` that `mmr read session <id>` can read, not the internal Store `session_id`.
- Store-to-read mapping must join `Store::EventRecord.source_session_id` to `ApiMessage.session_id` for the same source and resolved provider `project_name`; fixtures must cover Codex plus at least one encoded-name provider such as Claude or Cursor.
- Event-backed groups select a session only when provider messages are readable from `QueryService` for the same `(source, project_name, source_session_id)`.
- Learned-memory hits (`mmr://learned-memory/...`) and event matches without readable provider messages are reported under `unreadable_matches[]` with a `reason`, and must not create a selected session packet.

**Ranking**

- Group event-backed readable matches by `(source, project_name, source_session_id)`.
- Rank groups by `match_count desc`, then `latest_match_timestamp desc`, then `source asc`, `project_name asc`, `source_session_id asc`; fixtures must cover equal-count and equal-timestamp tie-breaks.
- Select the top `--max-sessions` groups.

**Message windows**

- For each selected session, collect chronological messages around each matched event using `before_messages` and `after_messages`.
- Merge overlapping windows within the same session, dedupe messages, preserve chronological message order, and cap at `max_messages_per_session`.
- When merged windows exceed the cap, keep matched anchor messages first, then retain nearest surrounding context in chronological order until the cap is reached; set `message_window.truncated = true`.
- If matched anchor messages alone exceed `max_messages_per_session`, retain anchors in chronological order up to the cap and set `message_window.truncated = true`.
- If a matched event cannot be mapped directly to a message, anchor the window at the nearest message timestamp in the same session; if no provider message exists for the session, move the match to `unreadable_matches[]` with reason `provider_messages_unavailable`.

**Pagination**

- Flatten order is selected-session rank ascending, then each session's messages chronological.
- `--offset` and `--limit` slice the flattened message list only.
- `selected_sessions[]` remains present for every pinned selected session on every page; `matches[]` remain complete; `messages[]` contains only messages present on that page for that session.
- `next_offset = offset + number_of_returned_messages`.

**Response shape**

JSON stdout must include at least:

- `query`
- `limits` with `max_sessions`, `before_messages`, `after_messages`, `max_messages_per_session`, `limit`, and `offset`
- `total_matches`
- `total_selected_sessions`
- `selected_sessions[]`
  - `rank`
  - `source`
  - `project_name`
  - `source_session_id`
  - `rank_reason` containing `match_count`, `latest_match_timestamp`, and tie-break fields
  - `match_count`
  - `first_match_citation`
  - `matches[]` with `citation`, `event_id`, `event_type`, `role`, `timestamp`, `line_number`, `snippet`, `before`, and `after`
  - `message_window` with `before_messages`, `after_messages`, `max_messages_per_session`, and `truncated`
  - `messages[]` using the existing `ApiMessage` shape
- `unreadable_matches[]` always present as an array; entries include `citation`, `reason`, and enough match metadata to explain why no readable session packet was produced
- `next_page`
- `next_offset`
- `next_command`
- `suggested_next_action`

**Continuation**

- When `next_page` is true, `next_command` must pin the original selected session set with repeated `--pinned-session '{"source":"...","project_name":"...","source_session_id":"..."}'` flags, retain the same resolved project/source/window/limit options, and advance `--offset`.
- `next_command` must not rely on re-running the fuzzy query to choose sessions.
- Continuation stability freezes selected session identities across newer sessions landing later; it is not a snapshot guarantee if provider files mutate inside a pinned session between page reads.

**Pinned-session behavior**

- `--pinned-session` JSON must contain exactly `source`, `project_name`, and `source_session_id`; invalid JSON or missing fields is a usage error.
- When pins are present, `query` is still required for match evidence, but session selection is restricted to the pinned identities and must not re-rank against unpinned sessions.
- A stale or nonexistent pinned identity fails with structured `pinned_session_not_found` instead of silently falling back to fuzzy selection.
- `next_command` must be executable as printed by zsh/bash on macOS, preserving query, `--pretty`, resolved `--project`, effective `--source`, limits, windows, pins, and `--offset`.

---

## 3. Definition of Done · INVARIANT

Each item is **atomic** (one verifiable assertion per checkbox), tagged with a
stable id that Tasks reference via **Closes:**, and carries a concrete `verify by:`.

Tick a `DoD-N` box only when its own `verify by:` has been run and passed (not merely
because a closing Task is ticked). Log the command and its outcome as an Evidence bullet
under the Task that **Closes:** it. DONE requires every DoD box ticked.

- [ ] **DoD-1** — `mmr retrieve <query>` parses as a top-level command with the flags named in §2.1 — *verify by:* `cargo test --test cli_contract retrieve_command_parses_contract -- --exact`
- [ ] **DoD-2** — Store event matches map to readable provider messages through public `source_session_id` and provider `project_name` for Codex plus one encoded-name provider — *verify by:* `cargo test --test memory_fabric_contract retrieve_maps_store_events_to_provider_sessions -- --exact`
- [ ] **DoD-3** — a fixture query matching at least two readable provider sessions returns `selected_sessions[]` ranked by the §2.1 tie-break rules, including equal-count and equal-timestamp ties — *verify by:* `cargo test --test memory_fabric_contract retrieve_ranks_matching_sessions -- --exact`
- [ ] **DoD-4** — each selected session includes event-backed `matches[]` with stable `mmr://event/...` citations — *verify by:* `cargo test --test memory_fabric_contract retrieve_preserves_event_citations -- --exact`
- [ ] **DoD-5** — each selected session includes bounded chronological `messages[]` windows using the existing `ApiMessage` shape — *verify by:* `cargo test --test memory_fabric_contract retrieve_returns_bounded_message_windows -- --exact`
- [ ] **DoD-6** — default caps are exactly `max_sessions=3`, `before_messages=3`, `after_messages=12`, `max_messages_per_session=24`, and derived `limit=72` unless explicit flags override them — *verify by:* `cargo test --test memory_fabric_contract retrieve_default_limits_are_conservative -- --exact`
- [ ] **DoD-7** — retrieval surfaces learned-memory-only hits and event matches without readable provider messages under `unreadable_matches[]` and never turns them into selected sessions — *verify by:* `cargo test --test memory_fabric_contract retrieve_unreadable_matches_are_reported -- --exact`
- [ ] **DoD-8** — paged retrieval `next_command` uses repeated JSON `--pinned-session` selectors with source, resolved project, and public `source_session_id`, and never depends on fuzzy re-selection — *verify by:* `cargo test --test cli_contract retrieve_next_command_pins_concrete_sessions -- --exact`
- [ ] **DoD-9** — following the emitted `next_command` as printed after adding a newer matching session returns the next contiguous page from the same pinned selected sessions with correct `next_offset` and complete `matches[]` — *verify by:* `cargo test --test cli_contract retrieve_next_command_is_executable_and_stable -- --exact`
- [ ] **DoD-10** — empty-match retrieval succeeds with JSON containing zero selected sessions, zero readable messages, and a concrete `suggested_next_action` — *verify by:* `cargo test --test memory_fabric_contract retrieve_empty_match_is_successful_json -- --exact`
- [ ] **DoD-11** — retrieval preserves machine-readable stdout and `--pretty` formatting behavior — *verify by:* `cargo test --test memory_fabric_contract retrieve_json_and_pretty_contract -- --exact`
- [ ] **DoD-12** — retrieval applies `--project`, global `--source`, and `MMR_DEFAULT_SOURCE` consistently across search matches and selected session reads — *verify by:* `cargo test --test memory_fabric_contract retrieve_project_and_source_filters_are_consistent -- --exact`
- [ ] **DoD-13** — retrieval applies `--session`, `--role`, `--event-type`, `--ignore-case`, and `-C/--context` matching filters consistently with `find` — *verify by:* `cargo test --test memory_fabric_contract retrieve_match_filters_follow_find_semantics -- --exact`
- [ ] **DoD-14** — retrieval output does not expose `raw_local_ref` or other private local raw refs — *verify by:* `cargo test --test memory_fabric_contract retrieve_does_not_leak_raw_local_refs -- --exact`
- [ ] **DoD-15** — existing `find` JSON and line-format behavior remains unchanged — *verify by:* `cargo test --test memory_fabric_contract rg_cli_contract_is_implemented -- --exact && cargo test --test memory_fabric_contract search_cli_contract_is_implemented -- --exact`
- [ ] **DoD-16** — `specs/retrieval.md` documents every supported retrieval flag and default in §2.1, including short `-C` — *verify by:* `test -f specs/retrieval.md && rg -n "mmr retrieve" specs/retrieval.md && rg -n -- "--project" specs/retrieval.md && rg -n -- "--source" specs/retrieval.md && rg -n "MMR_DEFAULT_SOURCE" specs/retrieval.md && rg -n -- "--session" specs/retrieval.md && rg -n -- "--role" specs/retrieval.md && rg -n -- "--event-type" specs/retrieval.md && rg -n -- "--ignore-case" specs/retrieval.md && rg -n -- "-C" specs/retrieval.md && rg -n -- "--context" specs/retrieval.md && rg -n -- "--max-sessions" specs/retrieval.md && rg -n -- "--before-messages" specs/retrieval.md && rg -n -- "--after-messages" specs/retrieval.md && rg -n -- "--max-messages-per-session" specs/retrieval.md && rg -n -- "--limit" specs/retrieval.md && rg -n -- "--offset" specs/retrieval.md && rg -n -- "--pinned-session" specs/retrieval.md`
- [ ] **DoD-17** — `specs/retrieval.md` documents response fields, `unreadable_matches[]`, empty-result behavior, and non-goals — *verify by:* `rg -n "selected_sessions" specs/retrieval.md && rg -n "source_session_id" specs/retrieval.md && rg -n "rank_reason" specs/retrieval.md && rg -n "unreadable_matches" specs/retrieval.md && rg -n "suggested_next_action" specs/retrieval.md && rg -n "semantic/vector search" specs/retrieval.md && rg -n -- "--remote" specs/retrieval.md && rg -n "first-class MCP" specs/retrieval.md`
- [ ] **DoD-18** — full repo verification loop is green after implementation — *verify by:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- [ ] **DoD-19** — a deterministic fixture-backed smoke asserts retrieval JSON has selected sessions, citations, bounded messages, and no raw-local-ref leakage — *verify by:* `cargo test --test memory_fabric_contract retrieve_fixture_smoke_outputs_inspectable_json -- --exact --nocapture`
- [ ] **DoD-20** — user-provided `--pinned-session` rejects malformed JSON, missing/extra fields, and stale identities with structured errors instead of falling back to fuzzy selection — *verify by:* `cargo test --test cli_contract retrieve_pinned_session_validation_errors_are_structured -- --exact`
- [ ] **DoD-21** — window edge cases preserve anchors through overlapping windows, cap overflow, anchor-overflow truncation, and nearest-timestamp fallback — *verify by:* `cargo test --test memory_fabric_contract retrieve_window_edge_cases_are_deterministic -- --exact`

---

## 4. Exit Conditions

The goal terminates when **any** condition holds. On exit, state which fired —
explicitly — in the response to the user.

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Cargo, the Rust toolchain, or the local test binary fixture harness is unavailable after one direct retry. Exit without the blocked step; name it explicitly.
- **`SCOPE-CHANGE`** — work cannot complete without changing §2.1 command, response schema, source/project/default-source filters, learned-memory/unreadable-match behavior, window, ranking, continuation, docs, or MCP scope. Record the proposal in §6 and exit to the user.
- **`DESIGN-BLOCKED-CONTINUATION`** — T1 cannot define a stable multi-session continuation that avoids fuzzy re-selection after two focused design attempts. Exit before adding broad implementation.
- **`CONFIDENCE-STALL`** — a task cannot reach the floor after 3 honest check/fix attempts. Exit, report the task and the gap.
- **`BUDGET`** — implementation exceeds 2 full verification-loop attempts after retrieval contract tests are green, or T1/T2 consume more than 6 focused check/fix cycles without a frozen testable contract. Exit and report progress.

---

## 5. Tasks · INVARIANT

Ordered, dependency-aware units of work that together satisfy the DoD. Tick the
trailing `[ ]` only when the Verification Contract passes and Confidence >= floor.

---

### T1 · Freeze retrieval contract and implementation map · [ ]

**Steps**
- [ ] Re-read PRD 1 and the current `find`, `read`, message service, API type, MCP, and test surfaces listed in §2.
- [ ] Confirm §2.1 against live code and update it only during Author mode or with user-approved scope change.
- [ ] Confirm learned-memory-only and DB-only event matches are surfaced as `unreadable_matches[]`; keep them out of selected session reads unless a concrete transcript mapping exists.
- [ ] Preserve the non-goals: no semantic/vector search, no legacy `search`/`rg` alias restoration, no `--remote`/`--all` retrieval in v1, and no automatic model summarization.

**Verification Contract**
- *Check:* The frozen contract is explicit enough that all tests can be written before production code, and omitted flags/features are named as non-goals or scope-change triggers.
- *Method:* `rg -n "mmr retrieve|--pinned-session|source_session_id|max_messages_per_session|DESIGN-BLOCKED-CONTINUATION|semantic/vector search" goals/2026-06-18-search-to-read-retrieval-pipeline.md`
- *Expected:* Output shows command surface, continuation format, default limits, design-blocked exit, and non-goals.
- *BDD scenarios covered:* Given a literal clue, when a user asks `mmr retrieve`, then the CLI uses existing literal search and returns bounded cited context; given semantic search or remote retrieval pressure, then it remains out of scope.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** none

**Evidence (required before tick; append-only)**
- *(none yet — when setting Confidence >= floor, append a bullet with all three: date + command/check run + outcome (exit code / test counts / artifact path))*

---

### T2 · Add red retrieval contract tests · [ ]

**Steps**
- [ ] Extend isolated fixture data so one literal query matches at least two sessions and another query matches nothing.
- [ ] Add parser/help, identity mapping, ranking, citations, message windows, window-edge cases, default caps, unreadable-match, continuation, pinned-session validation, empty-match, stdout/pretty, filter, leakage, existing-find, docs, and fixture-smoke tests named by §3.
- [ ] Run the new retrieve-focused test filters before implementation and verify they fail for the expected missing command/schema, not because fixtures or harness setup are broken.

**Verification Contract**
- *Check:* New tests exist and initially fail for the intended missing retrieval behavior; no production implementation is hidden in the test task.
- *Method:* Run separately and log both outputs: `cargo test --test memory_fabric_contract retrieve_ -- --nocapture` and `cargo test --test cli_contract retrieve_ -- --nocapture`
- *Expected:* Before T3, both commands discover retrieve-focused tests and exit non-zero only because `retrieve` command/schema behavior is missing; after T3/T5 the same filters pass.
- *BDD scenarios covered:* Given matches across two sessions, retrieval should rank and return bounded packets; given no matches, retrieval should return successful empty JSON; given filters and paging, retrieval should preserve scope and stable continuation.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** none

**Evidence (required before tick; append-only)**
- *(none yet)*

---

### T3 · Implement retrieval query, ranking, and window service · [ ]

**Steps**
- [ ] Reuse/refactor existing search-document logic so retrieval and `find` share literal matching semantics.
- [ ] Bridge Store search events to QueryService messages through public `source_session_id`, not internal Store `session_id`.
- [ ] Group event-backed readable matches by `(source, project_name, source_session_id)` and rank by the §2.1 formula.
- [ ] Select and dedupe bounded message windows around matched events, preserving chronological order and the existing `ApiMessage` shape.
- [ ] Enforce default caps in service-level logic, not only in clap parsing.
- [ ] Surface learned-memory-only hits and event-backed matches without readable provider messages as `unreadable_matches[]`.

**Verification Contract**
- *Check:* Retrieval service returns ranked selected sessions, event citations, bounded windows, and default caps for fixture data.
- *Method:* `cargo test --test memory_fabric_contract retrieve_ -- --nocapture`
- *Expected:* Retrieve-focused memory fabric tests pass and fail if ranking, source-session-id mapping, citation preservation, message bounds, unreadable-match handling, or default caps are removed.
- *BDD scenarios covered:* Given a literal query with matches in multiple sessions, when retrieval runs, then top sessions are selected deterministically and only bounded windows are returned.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-2, DoD-3, DoD-4, DoD-5, DoD-6, DoD-7, DoD-21

**Evidence (required before tick; append-only)**
- *(none yet)*

---

### T4 · Wire CLI, filters, continuation, and contract preservation · [ ]

**Steps**
- [ ] Add the `retrieve` clap command and parser tests for all supported flags in §2.1.
- [ ] Serialize the retrieval response without leaking private local refs.
- [ ] Implement JSON `--pinned-session` continuation and `next_command` generation without fuzzy re-selection, including resolved project and public provider session identity.
- [ ] Apply project/source/default-source/session/role/event-type/ignore-case filters consistently across matching and reads.
- [ ] Keep existing `find` behavior unchanged.

**Verification Contract**
- *Check:* CLI surface, continuation, empty output, stdout/pretty, filters, leakage protection, and existing-find contracts pass.
- *Method:* `cargo test --test cli_contract retrieve_ -- --nocapture && cargo test --test memory_fabric_contract retrieve_ -- --nocapture && cargo test --test memory_fabric_contract rg_cli_contract_is_implemented -- --exact && cargo test --test memory_fabric_contract search_cli_contract_is_implemented -- --exact`
- *Expected:* All listed tests pass; `next_command` contains `--pinned-session`; existing find tests remain green.
- *BDD scenarios covered:* Given filtered retrieval, only allowed matches and sessions appear; given a paged result, following `next_command` reads the same selected sessions; given existing `find`, output remains unchanged.

**Confidence:** 0 / 90 · **Depends on:** T3 · **Closes:** DoD-1, DoD-8, DoD-9, DoD-10, DoD-11, DoD-12, DoD-13, DoD-14, DoD-15, DoD-20

**Evidence (required before tick; append-only)**
- *(none yet)*

---

### T5 · Update retrieval docs · [ ]

**Steps**
- [ ] Add `specs/retrieval.md` with `mmr retrieve` examples, supported flags, defaults, response shape, empty-result behavior, unreadable matches, and non-goals.
- [ ] Document that a first-class MCP `mmr_retrieve` tool is out of scope for v1, while CLI retrieval can be used by agents through shell execution.

**Verification Contract**
- *Check:* Docs describe every required retrieval flag/default, response field, empty-result behavior, unreadable-match behavior, and non-goal.
- *Method:* `test -f specs/retrieval.md && rg -n "mmr retrieve" specs/retrieval.md && rg -n -- "--project" specs/retrieval.md && rg -n -- "--source" specs/retrieval.md && rg -n "MMR_DEFAULT_SOURCE" specs/retrieval.md && rg -n -- "--session" specs/retrieval.md && rg -n -- "--role" specs/retrieval.md && rg -n -- "--event-type" specs/retrieval.md && rg -n -- "--ignore-case" specs/retrieval.md && rg -n -- "-C" specs/retrieval.md && rg -n -- "--context" specs/retrieval.md && rg -n -- "--max-sessions" specs/retrieval.md && rg -n -- "--before-messages" specs/retrieval.md && rg -n -- "--after-messages" specs/retrieval.md && rg -n -- "--max-messages-per-session" specs/retrieval.md && rg -n -- "--limit" specs/retrieval.md && rg -n -- "--offset" specs/retrieval.md && rg -n -- "--pinned-session" specs/retrieval.md && rg -n "selected_sessions" specs/retrieval.md && rg -n "unreadable_matches" specs/retrieval.md && rg -n "suggested_next_action" specs/retrieval.md && rg -n "semantic/vector search" specs/retrieval.md && rg -n -- "--remote" specs/retrieval.md && rg -n "first-class MCP" specs/retrieval.md`
- *Expected:* Every command exits 0; docs mention all required flags/defaults, response fields, empty-result guidance, and non-goals.
- *BDD scenarios covered:* Given a human reading docs, retrieval is a first-class CLI workflow with clear limits and exclusions.

**Confidence:** 0 / 90 · **Depends on:** T4 · **Closes:** DoD-16, DoD-17

**Evidence (required before tick; append-only)**
- *(none yet)*

---

### T6 · Prove full verification loop and fixture retrieval smoke · [ ]

**Steps**
- [ ] Run every DoD `verify by:` command that has not already been logged as passing.
- [ ] Run the full repo verification loop from `.cursor/rules/verification-loop.mdc`.
- [ ] Run and inspect the fixture-backed retrieval smoke output; record whether selected sessions, citations, bounded messages, and private-ref safety are visible.
- [ ] Optionally run `cargo run -- retrieve <query> --pretty` against local history only as extra evidence, not as required DoD proof.
- [ ] Get independent completion-claim review before final DONE unless the user explicitly narrows this goal to authoring only.

**Verification Contract**
- *Check:* All retrieval DoD checks, fixture smoke, and the full Rust verification loop pass without weakening existing contracts.
- *Method:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release && cargo test --test memory_fabric_contract retrieve_fixture_smoke_outputs_inspectable_json -- --exact --nocapture`
- *Expected:* Exit 0 for every command; fixture smoke JSON contains selected sessions, `mmr://event/...` citations, bounded messages, and no `raw_local_ref`; any failure routes through VER-8 check/fix/rerun.
- *BDD scenarios covered:* Given the complete repo, when verification runs, then retrieval and all existing mmr contracts remain green; given a real CLI invocation, the response is inspectable and bounded.

**Confidence:** 0 / 90 · **Depends on:** T5 · **Closes:** DoD-18, DoD-19

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

Meaningful choices/concessions needing visibility. Scope impact must be `none`.

- 2026-06-18 — Chose a new `mmr retrieve <query>` command as the draft scope because the PRD allows either `retrieve` or `find --read`, current `find` is a literal search contract, and a separate intent-first command avoids changing exact-search behavior. Alternatives rejected: `find --read` as the primary surface because it makes `find` both search and read-composition. Scope impact: none.
- 2026-06-18 — Draft goal keeps semantic/vector search, remote/all-project retrieval, legacy aliases, and automatic summarization out of scope, matching PRD non-goals and limiting implementation to literal search plus bounded read windows. Scope impact: none.
- 2026-06-18 — Used `spawn_agents_on_csv` reviewer subagents (`alignment_verification`, `implementation_risk`; job `a2ae9ba5-523b-4f16-99fc-4358829759f1`) for author-time adversarial review. They flagged compound DoDs, invalid multi-test commands, weak MCP/doc verification, `active` status mismatch, fuzzy/semantic ambiguity, underspecified window/continuation semantics, and T2 incorrectly closing final behavior DoDs. Scope impact: none.
- 2026-06-18 — Revised the draft to use `status: in-progress`, define fuzzy as literal clue matching, freeze command flags/default caps/ranking/window/continuation semantics in §2.1, split DoD into atomic assertions, make red-test T2 close `none`, and add a design-blocked continuation exit. Scope impact: none.
- 2026-06-18 — Re-ran reviewer subagents (`alignment_verification`, `implementation_risk`; job `0e1a70b3-7517-426b-a19a-cead5b036950`) after material revisions. They accepted the direction but required freezing remaining schema/MCP/session-identity choices, fixing invalid multi-filter cargo commands, making smoke/docs deterministic, and addressing Store internal session IDs versus public provider session IDs. Scope impact: none.
- 2026-06-18 — Revised the contract again to require public `source_session_id`, unreadable-match handling for learned memory and DB-only matches, JSON `--pinned-session` selectors containing source/project/session identity, deterministic fixture smoke, split docs DoDs, and single-filter runnable cargo commands. Scope impact: none.
- 2026-06-18 — Re-ran reviewer subagents (`alignment_verification`, `implementation_risk`; job `c2b889bd-e031-44b3-9a26-1dd88bb4a379`) after the second revision. They flagged MCP as PRD scope expansion, missing unreadable-match DoD, weak executable pagination proof, docs coverage gaps, and vague budget wording. Scope impact: none.
- 2026-06-18 — Narrowed v1 back to the PRD's CLI contract by making first-class MCP out of scope, adding Store-to-QueryService mapping and unreadable-match DoDs, requiring executable pinned `next_command` pagination proof, enumerating every documented flag/response field, and replacing `one work session` with 6 focused check/fix cycles. Scope impact: none.
- 2026-06-18 — Re-ran reviewer subagents (`alignment_verification`, `implementation_risk`; job `99fe4db2-35cb-4cfb-a27d-dd967b160dc5`) after CLI-only narrowing. They required pinned-session validation coverage, retrieval-scoped docs checks, explicit MCP limitation, shell-executable next-command proof, narrower pinned-session drift semantics, window-edge coverage, and stronger ranking/identity fixture expectations. Scope impact: none.
- 2026-06-18 — Added DoD coverage for malformed/stale `--pinned-session`, executable next-command behavior with zsh/bash quoting, window edge cases, equal tie-break ranking, retrieval-scoped `specs/retrieval.md` checks, explicit first-class MCP out-of-scope wording, and the limitation that continuation freezes selected session identities but not in-session provider-file mutations. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

Flash cards: trigger → wrong action → revision → correct action, with impact `1-5`.
When an attempt failed and the fix is not yet known, log the **open form** —
trigger → wrong action → *(open: revision/correct not yet found)* → pointer to the raw
failure (log path or commit) — still impact-tagged, so a dead-end is recorded before a
fresh context re-treads it.

*(none yet)*

---

## 8. Skills · LIVE (append-only)

Reusable workflows created via the **skill-creator** skill while working this goal.

*(none yet)*
