---
goal_id: "2026-07-01-sync-project-prefix-isolation"
title: "Isolate sync project prefixes"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: Sync and hydration never read or write a different project through the single-remote-project fallback.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Scope is frozen after user confirms DoD + Tasks.
- Do not weaken redaction, privacy blocking, or fake-remote manifest contracts to
  make the test pass.
- Preserve the intended fresh-host hydration path, but require project identity
  or path-alias evidence before using any fallback.
- Full `cargo test` is expected before DONE; if the known memory-fabric gate is
  still broken, coordinate with `goals/2026-07-01-memory-fabric-test-gate-stability.md`
  instead of waiving the suite.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — review finding source and verification evidence.
- `src/sync.rs:276` — `sync_project` chooses the remote project prefix for writes.
- `src/sync.rs:691` — `project_prefix_for_read` fallback currently affects hydration/count paths.
- `src/sync.rs:699` — `project_prefix_for_write` currently falls back to `single_remote_project_prefix`.
- `src/sync.rs:719` — `single_remote_project_prefix` returns the only remote project without identity matching.
- `tests/memory_fabric_contract.rs` — sync/hydration contract fixtures and fake remote setup.
- `AGENTS.md` and `.cursor/rules/verification-loop.mdc` — repo verification and blocked-test policy.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — Syncing two different local projects to the same fake remote creates two distinct remote project prefixes — *verify by:* `cargo test --test memory_fabric_contract sync_two_projects_keep_distinct_remote_prefixes -- --exact --nocapture`
- [ ] **DoD-2** — Rehydrating project A after project B sync does not import or find project B content — *verify by:* `cargo test --test memory_fabric_contract sync_hydration_does_not_cross_project_boundaries -- --exact --nocapture`
- [ ] **DoD-3** — Fresh-host hydration fallback still works only when the remote project identity/path aliases match the requested local project — *verify by:* `cargo test --test memory_fabric_contract sync_single_remote_project_fallback_requires_identity_match -- --exact --nocapture`
- [ ] **DoD-4** — Existing sync/redaction contracts still pass — *verify by:* `cargo test --test memory_fabric_contract sync_ -- --nocapture`
- [ ] **DoD-5** — Repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Cargo/Rust or the fake-remote temp fixture harness is unavailable after one retry.
- **`BLOCKED-TEST-GATE`** — only the separately tracked memory-fabric gate prevents full `cargo test` after targeted sync tests pass.
- **`SCOPE-CHANGE`** — preserving fresh-host hydration requires a new remote identity model or user-visible command/config change.
- **`CONFIDENCE-STALL`** — project matching remains ambiguous after 3 focused design/test attempts.
- **`BUDGET`** — more than 2 full verification-loop attempts after targeted sync tests are green.

---

## 5. Tasks · INVARIANT

### T1 · Characterize the current fallback and write failing regressions · [ ]

**Steps**
- [ ] Re-read the sync prefix helpers and existing sync tests.
- [ ] Add a two-project fake-remote regression that proves writes do not reuse the first project prefix.
- [ ] Add a hydration regression that searches for a project-B-only marker from project A.
- [ ] Add a fresh-host fallback regression with explicit identity/path-match expectations.

**Verification Contract**
- *Check:* new tests fail on the reviewed bug and name the project-isolation contract.
- *Method:* `cargo test --test memory_fabric_contract sync_two_projects_keep_distinct_remote_prefixes sync_hydration_does_not_cross_project_boundaries sync_single_remote_project_fallback_requires_identity_match -- --nocapture`
- *Expected:* before the fix, at least one new regression fails for cross-project mixing; after the fix, all pass.
- *BDD scenarios covered:* Given two local projects sharing one fake remote, when project B syncs after project A, then B writes to B's prefix; given project A hydration, then B-only content is absent.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Remove unsafe write fallback and constrain read fallback · [ ]

**Steps**
- [ ] Change write-prefix selection so a missing current prefix uses the current project prefix, not the only existing remote prefix.
- [ ] Add identity/path alias checks before any read fallback uses the single remote project.
- [ ] Keep remote payload conflict handling and redaction behavior unchanged.

**Verification Contract**
- *Check:* sync writes and hydration reads use only matching project prefixes.
- *Method:* `cargo test --test memory_fabric_contract sync_two_projects_keep_distinct_remote_prefixes sync_hydration_does_not_cross_project_boundaries sync_single_remote_project_fallback_requires_identity_match -- --nocapture`
- *Expected:* all named tests pass.
- *BDD scenarios covered:* Given an unrelated single remote project, when syncing a new local project, then a new prefix is created; given a matching fresh host, hydration still succeeds.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Run targeted and full verification · [ ]

**Steps**
- [ ] Run all sync-focused memory fabric tests.
- [ ] Run the repo verification loop.
- [ ] If the full suite is blocked only by the known memory-fabric test gate, record `BLOCKED-TEST-GATE` instead of claiming DONE.

**Verification Contract**
- *Check:* sync changes do not regress existing sync, redaction, CLI, benchmark, clippy, or release build gates.
- *Method:* `cargo test --test memory_fabric_contract sync_ -- --nocapture && cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands, or explicit `BLOCKED-TEST-GATE` if only the separately tracked test-gate issue remains.
- *BDD scenarios covered:* Given existing sync workflows, when the isolation fix lands, then existing fake-remote and redaction contracts still pass.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-4, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: the risky ambiguity is fresh-host hydration. The goal explicitly preserves it only with identity/path matching and forbids silent write fallback. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
