---
goal_id: "2026-10-21-assimilation-activation-guardrails"
title: "Keep model-derived memory proposals pending"
description: "Separate validation of untrusted runner proposals from trusted activation decisions."
date: "2026-10-21"
status: "done"
confidence_floor: 90
created: "2026-10-21"
updated: "2026-10-21"
---

# Goal: Model confidence and valid citation IDs cannot activate learned memory.

## Invariants

- Keep public `assimilate project/source` stateless and provider-free.
- Preserve existing output enums and JSON fields; downgrade model-requested active state to pending, preserve rejected state, and keep validation idempotent.
- Citation validation proves only reference existence, not support or permission.
- Direct store/sync APIs remain trusted persistence primitives; deliberate caller-supplied active fixtures and hydration retain compatibility.
- Imported messages, tool output, cited claims and model recommendations are untrusted data and cannot grant authorization or modify instruction hierarchy.
- No new scheduler, auto-activation, provider requests, private-history ingestion or live credentials.

## Definition of Done

- [x] DoD-1 — High confidence, explicit active/pending, counterevidence and repeated validation cannot activate model proposals — verify by: focused dream activation tests.
- [x] DoD-2 — Model-only runner→validation→store completion persists pending/rejected rows and preserves citations; updates/abstention instructions remain explicit — verify by: integration guardrail contracts.
- [x] DoD-3 — Public assimilation remains stateless and gives explicit trust/activation guardrails — verify by: existing and extended project/source CLI contracts.
- [x] DoD-4 — Existing native sharing, sync/hydration and full verification loop pass — verify by: fixture-only format/test/benchmark/Clippy/release logs.

## Tasks

### T1 · Specify and lock proposal trust contract · [x]

Update assimilation spec/docs before implementation and add failing synthetic cases for high-confidence unrelated or contradictory evidence, requested active status, rejection, missing refs and repeat validation.

**Verification Contract:** affected tests fail against confidence-based baseline for the intended reason. **Confidence:** 97 / 90.

### T2 · Enforce pending proposal boundary · [x]

Normalize all non-rejected model observations/claims/memory updates/counterevidence to pending, document trusted store primitives and strengthen agent-facing handoff rules without adding a broad activation API.

**Verification Contract:** model-only persistence produces no active learned memory; existing trusted direct-store and sync contracts still pass. **Confidence:** 97 / 90.

### T3 · Verify and commit scoped slice · [x]

Run targeted tests, full verification, diff hygiene and manual goal eligibility. Parent arranges independent review and publication; do not push or merge from this worktree.

**Verification Contract:** all declared gates pass with only known host LLVM stripping warning allowed (release exit 0). **Confidence:** 97 / 90.

## Exit Conditions

- DONE — all DoD/tasks verified at confidence >=90, scoped commit delivered for independent review.
- SCOPE-CHANGE — activation API or broad persistence migration becomes necessary.
- BLOCKED-DEP — required tooling unavailable after one diagnostic pass.

## Evidence

- 2026-10-21 — Parent audit reproduced a pending candidate at confidence 0.99 becoming active even when the same event is cited as support and counterevidence.
- 2026-10-21 — `Store::complete_dream_run` and sync hydration accept deliberately supplied statuses; public assimilation does not call a runner or complete a dream run. Preserve those trusted persistence semantics and fix the untrusted validation boundary.

## Decisions

- 2026-10-21 — Do not invent semantic entailment checks or a new activation command. All runner output is a proposal; caller-reviewed activation is outside the model-validation path.

- 2026-10-21 — Regression-first check `cargo test --offline --locked model_activation_is_pending` failed on baseline with requested Active observed where Pending was required (`../audit/evidence/activation-red-test.log`).
- 2026-10-21 — Targeted `cargo test --offline --locked dream` and `cargo test --offline --locked assimilate_project_and_source_handoffs` passed after implementation. The persistence contract runs actual mock runner validation, completes pending/rejected rows, reopens SQLite and replays proposals without duplicates or activation.
- 2026-10-21 — Full `../audit/verify.py --repo . --tag activation` loop passed using the isolated per-worktree target and synthetic HOME: formatting; 356 tests (158 library, 137 CLI, 12 MCP, 49 memory-fabric); four ignored benchmarks; strict all-target/all-feature Clippy; release build. Logs: `../audit/evidence/activation-*.log`, summary `activation-verification.json`. External smoke gates remained disabled; no live histories, credentials or model requests were used.
- 2026-10-21 — Release exit 0 retained the baseline local `rust-objcopy`/missing-libLLVM debug stripping warning; application release build and tests pass.
- 2026-10-21 — Independent read-only reviewer and parent production-diff review reported no findings: all validation entry points return pending proposals, rejection/idempotent warning preserved, persistence/restart proof meaningful, public behavior stateless/provider-free, trusted Store/sync boundary explicit.
- 2026-10-21 — Global goal-driven-development skill and gdd_status.py were unavailable after parent searches in ~/.agents/skills, ~/.codex/skills and plugin cache. Manually checked required frontmatter, frozen scope, all DoD evidence, task verification contracts/confidence and full-loop exit codes; all done-eligible. `git diff --check` passed. Parent handles publication; no push or merge from this worktree.
