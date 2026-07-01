---
goal_id: "2026-07-01-retrieve-window-ranking-performance"
title: "Avoid unused retrieve windows"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: `mmr retrieve` ranks candidate sessions before materializing provider message windows for only the selected sessions.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Preserve the current concise default retrieve JSON contract.
- Preserve `--full-message-history`, `--debug`, pinned-session, ranking, pagination, and unreadable-match semantics.
- Do not reintroduce full provider message history in default output.
- Full `cargo test` is expected before DONE; coordinate with the test-gate goal if it is still blocked.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — review finding source.
- `goals/2026-06-28-retrieve-debug-and-snippet-output.md` — current concise/default retrieve contract.
- `specs/retrieval.md` — retrieval behavior and response shape.
- `src/cli.rs:5313` — retrieve currently builds candidates for every grouped matched session.
- `src/cli.rs:5325` — provider messages are loaded before ranking/truncation.
- `src/cli.rs:5363` — candidates are truncated after message windows were built.
- `src/cli.rs:5805` — `retrieve_provider_messages` calls `service.messages`.
- `src/messages/service.rs:389` — `service.messages` scans/clones messages.
- `tests/cli_contract.rs` and `tests/memory_fabric_contract.rs` — retrieve contract and fixture tests.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — Retrieve ranks matched session identities from match metadata before loading provider message windows — *verify by:* `cargo test --test memory_fabric_contract retrieve_ranks_before_provider_window_loading -- --exact --nocapture`
- [ ] **DoD-2** — With default concise output and `--max-sessions 3`, provider windows are materialized only for the selected sessions, not every matched group — *verify by:* `cargo test --test memory_fabric_contract retrieve_default_output_loads_windows_only_for_selected_sessions -- --exact --nocapture`
- [ ] **DoD-3** — `--full-message-history` pagination, pinned-session continuation, unreadable matches, and ranking tie-breaks remain unchanged — *verify by:* `cargo test --test cli_contract retrieve_flattened_pagination_across_selected_sessions retrieve_pinned_next_command_executes_as_printed_and_freezes_sessions retrieve_next_command_preserves_debug_and_full_message_history -- --nocapture && cargo test --test memory_fabric_contract retrieve_ranking_ties_use_documented_order retrieve_unreadable_matches_include_learned_memory_and_db_only_events -- --nocapture`
- [ ] **DoD-4** — Retrieve docs/specs still describe the concise default and `--full-message-history` behavior accurately — *verify by:* `rg -n "concise|full-message-history|selected_sessions|messages" specs/retrieval.md docs/site/retrieval-human.md docs/site/retrieval-agent.md`
- [ ] **DoD-5** — Repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Rust/Cargo or fixture instrumentation is unavailable after one retry.
- **`BLOCKED-TEST-GATE`** — only the separately tracked memory-fabric gate prevents full `cargo test` after targeted retrieve tests pass.
- **`SCOPE-CHANGE`** — avoiding unused windows requires changing public retrieve ranking, pagination, or response schema.
- **`CONFIDENCE-STALL`** — no reliable test instrumentation can distinguish selected-session window loading from all-match loading after 3 attempts.
- **`BUDGET`** — more than 2 full verification-loop attempts after targeted retrieve tests are green.

---

## 5. Tasks · INVARIANT

### T1 · Add retrieve performance guardrails · [ ]

**Steps**
- [ ] Add a fixture with more matched sessions than `--max-sessions`.
- [ ] Add instrumentation or test seams that count provider-window materialization without changing public JSON.
- [ ] Assert default concise output does not load windows for discarded groups.
- [ ] Assert selected ranking remains based on documented metadata.

**Verification Contract**
- *Check:* tests can fail if retrieve loads provider windows before truncating to `--max-sessions`.
- *Method:* `cargo test --test memory_fabric_contract retrieve_ranks_before_provider_window_loading retrieve_default_output_loads_windows_only_for_selected_sessions -- --nocapture`
- *Expected:* all named tests pass after implementation.
- *BDD scenarios covered:* Given many matched sessions and `--max-sessions 3`, only the three selected sessions need provider windows.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Refactor retrieve selection before window loading · [ ]

**Steps**
- [ ] Split retrieve candidate ranking metadata from provider message-window materialization.
- [ ] Sort and truncate identities before calling `retrieve_provider_messages`.
- [ ] Preserve unreadable-match handling for unsupported or missing provider transcripts.
- [ ] Keep pinned-session behavior stable.

**Verification Contract**
- *Check:* retrieve output stays contract-compatible while avoiding discarded window loads.
- *Method:* `cargo test --test memory_fabric_contract retrieve_ranks_before_provider_window_loading retrieve_default_output_loads_windows_only_for_selected_sessions retrieve_ranking_ties_use_documented_order retrieve_unreadable_matches_include_learned_memory_and_db_only_events -- --nocapture`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given normal and pinned retrieve flows, selected sessions/ranks are identical but unused groups do not incur provider-window work.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Verify retrieve contract and docs · [ ]

**Steps**
- [ ] Run retrieve CLI and memory-fabric contract tests.
- [ ] Update specs/docs only if implementation terms changed.
- [ ] Run full verification loop or exit on the separate test-gate blocker.

**Verification Contract**
- *Check:* performance refactor preserves retrieve behavior and documentation.
- *Method:* `cargo test --test cli_contract retrieve_ -- --nocapture && cargo test --test memory_fabric_contract retrieve_ -- --nocapture && rg -n "concise|full-message-history|selected_sessions|messages" specs/retrieval.md docs/site/retrieval-human.md docs/site/retrieval-agent.md && cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands, or explicit `BLOCKED-TEST-GATE` if only that known suite blocker remains.
- *BDD scenarios covered:* Existing retrieve parser, default output, debug, full-history, pagination, pinned-session, and docs contracts remain valid.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-3, DoD-4, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: pure wall-clock benchmark assertions would be brittle, so the goal requires a behavior-level guardrail that proves discarded sessions do not trigger provider window materialization. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
