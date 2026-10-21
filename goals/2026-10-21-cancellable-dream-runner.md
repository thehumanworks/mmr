---
goal_id: "2026-10-21-cancellable-dream-runner"
title: "Cooperatively cancel bounded local dream commands"
description: "Add one-way cancellation with joined subprocess cleanup while keeping the existing runner API compatible."
date: "2026-10-21"
status: "done"
confidence_floor: 90
created: "2026-10-21"
updated: "2026-10-21"
---

# Goal: A caller can request command cancellation without leaving detached pipe readers or skipping cleanup.

## Invariants

- `DreamRunner::run` remains compatible; public assimilation remains stateless.
- Cancellation is an explicit one-way library handle, not an environment or model instruction.
- All subprocess output and argument values stay out of error messages.
- Request, output, execution and cleanup budgets remain enforced.
- No worker jobs, scheduler, daemon, configuration, provider calls or native-history writes.
- Synthetic local fixtures only; no private sessions, credentials, push, merge or deployment.

## Definition of Done

- [x] DoD-1 — Sticky cancellation can prevent spawn and wake running I/O across threads/runtimes — verify by: pre-cancel marker, blocked stdin and nested-Tokio tests.
- [x] DoD-2 — Cancellation returns only after the existing bounded cleanup, preserving cleanup-error precedence — verify by: inherited-descriptor/ordinary-descendant/PID reaping and injected-cleanup-failure tests.
- [x] DoD-3 — Compatibility and limits are documented without claiming SIGKILL/crash containment — verify by: specification, developer guide and existing fixture tests.
- [x] DoD-4 — Independent review and the full repository gate pass — verify by: fmt, all tests, ignored benchmarks, strict Clippy and release build.

## Tasks

### T1 · Freeze contract and add regressions · [x]

Add `specs/runner-cancellation.md` and discriminating synthetic tests before implementing the API.

**Verification Contract:** tests cover pre-spawn prevention, blocked stdin, pipe capacity, inherited descriptors, token reuse/isolation, cross-thread/runtime wakeup and cleanup precedence. **Confidence:** 97 / 90.

### T2 · Add the one-way handle and owned cancellation future · [x]

Use already-enabled Tokio watch support; retain the existing scoped runtime/thread and common cleanup path. No reset and no detached task.

**Verification Contract:** targeted cancellation and old runner tests pass without raw output/arguments in errors. **Confidence:** 97 / 90.

### T3 · Review, measure and verify · [x]

Document limitations and completion races; collect repeated fixture cancellation latency and uncancelled workload measurements; run the full gate and independent review.

**Verification Contract:** source inspection plus full gate and bounded synthetic measurements. **Confidence:** 97 / 90.

## Exit Conditions

- DONE — all DoD/tasks verified with confidence >=90.
- SCOPE-CHANGE — worker state, scheduling or provider configuration becomes necessary.
- BLOCKED-DEP — required Rust/OS fixture tooling unavailable.

## Evidence

- 2026-10-21 — Parent approved this additive cancellation slice on PR17 base `9c72eeb1893e2581e6c5874fbe947c8ad5af79b8`; no worker implementation is in scope.
- 2026-10-21 — Tokio 1.50 already uses `features = ["full"]`; watch support requires no dependency or feature changes.
- 2026-10-21 — Synthetic regressions were added before the API; `../audit/evidence/cancellation-red-test.log` records the expected unresolved-API compile failure on the base. Eight final targeted regressions pass: two private unit tests and six Unix integration tests. The pre-cancel test includes an uncancelled marker positive control; the inherited-pipe fixture confirms leader exit with WNOWAIT before requesting cancellation.
- 2026-10-21 — Cancellation-path signal failure is injected through a private group-cleanup boundary after the child starts. The returned error remains sanitized cleanup Io rather than Cancelled, and waitpid reports ECHILD after direct-child cleanup. No public test hook or privileged fixture was added.
- 2026-10-21 — Full `python3 ../audit/verify.py --repo . --tag cancellation-ready` loop passed: fmt, 389 tests (four benchmarks ignored in the normal suite), all four explicit benchmarks, strict all-target/all-feature Clippy, and release build. Evidence is `../audit/evidence/cancellation-ready-verification.json` and companion logs. Release retains the baseline local rust-objcopy/libLLVM stripping warning with exit 0.
- 2026-10-21 — Ten warm debug stalled-stdin runs (2 MiB synthetic request, cancellation after child PID marker) measured request-to-joined-return at 0.218–0.494 ms, median 0.3075 ms. Five uncancelled 2 MiB request + 1 MiB stdout + 1 MiB stderr runs, including test-process startup, measured 131.554–133.833 ms, median 132.199 ms. Previous runner fixture median was 132.784 ms; these observations make no statistical speedup/overhead claim. Cross-thread/runtime wakeup is separately covered while the caller's current-thread Tokio executor is synchronously occupied. Evidence: `../audit/evidence/cancellation-synthetic-timings.json`.
- 2026-10-21 — Parent independently reviewed exact source/spec/guide hashes and reran all eight targeted regressions; no remaining findings. Review evidence: `../audit/evidence/cancellation-independent-review.json` and companion verification/log.
- 2026-10-21 — The referenced global goal-driven-development skill/script is absent after parent searches of ~/.agents/skills, ~/.codex/skills and plugin cache. Manual eligibility checks follow repository examples: required frontmatter, frozen scope, all DoD/task boxes verified, confidence above floor and append-only evidence. No code verification gate was skipped.

## Decisions

- 2026-10-21 — Use a sticky watch-backed handle; a request is not acknowledgement and no cancellation state is inferred from imported history.
- 2026-10-21 — Preserve the existing bounded cleanup path and require joined completion. Document the ordinary cancellation/result race and SIGKILL/orphan/escaped-group limits; this handle is not a durable publication fence. Durable worker jobs remain deferred to the evidence prerequisites and a separate design.
