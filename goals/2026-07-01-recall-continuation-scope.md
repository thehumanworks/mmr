---
goal_id: "2026-07-01-recall-continuation-scope"
title: "Preserve recall continuation scope"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: `recall` pagination continuations pin the resolved project and source scope that produced page 1.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Do not restore legacy `--session-back` or `--session-range` user-facing flags.
- Keep `recall` age selection stable by converting continuation to concrete `read session` selectors.
- Preserve chronological message ordering and newest-first pagination semantics.
- Full `cargo test` is expected before DONE; coordinate with the test-gate goal if it is still blocked.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — review finding source and reproduction summary.
- `src/cli.rs:7190` — `run_session_axis` builds paged recall responses.
- `src/cli.rs:7214` — `build_next_messages_command` is called with `project=None` and `all=false`.
- `src/messages/service.rs:686` — `SelectedSession.equivalent_command` is bare `mmr read session shared-id`.
- `tests/cli_contract.rs:3771` — existing pagination test pins session id but not duplicate-id project scope.
- `specs/messages.md` — recall/read session behavior documentation.
- `docs/references/session-lookup-invariants.md` — stale legacy wording to check if docs are touched.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — When two projects contain the same provider session id, `recall --project /tmp/project-a --limit 1` emits a `next_command` that keeps page 2 scoped to project A — *verify by:* `cargo test --test cli_contract recall_next_command_preserves_project_scope_with_duplicate_session_ids -- --exact --nocapture`
- [ ] **DoD-2** — When an effective source filter came from `--source` or `MMR_DEFAULT_SOURCE`, the recall `next_command` preserves that source filter — *verify by:* `cargo test --test cli_contract recall_next_command_preserves_effective_source_scope -- --exact --nocapture`
- [ ] **DoD-3** — `session_selection.selected[].equivalent_command` is executable and scoped consistently with the selected session identity — *verify by:* `cargo test --test cli_contract recall_equivalent_command_preserves_scope -- --exact --nocapture`
- [ ] **DoD-4** — Existing recall pagination and session lookup contracts still pass — *verify by:* `cargo test --test cli_contract recall_ messages_session_ session_axis_pagination_pins_to_concrete_session_not_recency_age -- --nocapture`
- [ ] **DoD-5** — Repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — fixture harness or Rust/Cargo is unavailable after one retry.
- **`BLOCKED-TEST-GATE`** — only the separately tracked memory-fabric gate prevents full `cargo test` after targeted recall tests pass.
- **`SCOPE-CHANGE`** — fixing continuation scope requires changing public `recall` response shape or reintroducing legacy flags.
- **`CONFIDENCE-STALL`** — continuation scope remains ambiguous after 3 focused attempts with duplicate session ids.
- **`BUDGET`** — more than 2 full verification-loop attempts after targeted recall tests are green.

---

## 5. Tasks · INVARIANT

### T1 · Add duplicate-session continuation regressions · [ ]

**Steps**
- [ ] Create fixtures with duplicate `source_session_id` across two projects.
- [ ] Assert page 1's `next_command` includes the resolved project when needed.
- [ ] Execute the printed `next_command` and assert page 2 stays in the original project.
- [ ] Add source-filter/env coverage for the continuation.

**Verification Contract**
- *Check:* recall continuation remains scoped across duplicate session ids and source filters.
- *Method:* `cargo test --test cli_contract recall_next_command_preserves_project_scope_with_duplicate_session_ids recall_next_command_preserves_effective_source_scope -- --nocapture`
- *Expected:* all named tests pass after implementation.
- *BDD scenarios covered:* Given project A and project B both have session `shared`, when page 1 came from A, then page 2 reads A's `shared` only.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Thread resolved scope into continuation builders · [ ]

**Steps**
- [ ] Pass the resolved project scope into `build_next_messages_command` from `run_session_axis`.
- [ ] Preserve effective source filter from CLI/env precedence.
- [ ] Update `SelectedSession.equivalent_command` generation or response assembly so it is scoped.
- [ ] Keep recency-age selectors out of `next_command`.

**Verification Contract**
- *Check:* generated continuation/equivalent commands are scoped and executable.
- *Method:* `cargo test --test cli_contract recall_next_command_preserves_project_scope_with_duplicate_session_ids recall_next_command_preserves_effective_source_scope recall_equivalent_command_preserves_scope -- --nocapture`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given a paged recall response, when a user runs the printed command, then it continues the same concrete session set.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Verify recall contracts and repo gates · [ ]

**Steps**
- [ ] Run recall/session-axis contract tests.
- [ ] Update docs only if response examples or invariant docs mention stale behavior.
- [ ] Run full verification loop or exit on the separate test-gate blocker.

**Verification Contract**
- *Check:* fix preserves existing recall/read session contracts.
- *Method:* `cargo test --test cli_contract recall_ messages_session_ session_axis_pagination_pins_to_concrete_session_not_recency_age -- --nocapture && cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands, or explicit `BLOCKED-TEST-GATE` if only that known suite blocker remains.
- *BDD scenarios covered:* Given existing recall flows, they still return previous stable sessions, reject invalid zero age, and pin pagination to concrete sessions.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-4, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: the bug is not just `next_command`; `equivalent_command` can mislead users too. Added DoD-3. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
