---
goal_id: "2026-10-21-bounded-dream-runner"
title: "Bound command dream runner I/O and process cleanup"
description: "Drain subprocess pipes concurrently and apply request, output and lifecycle budgets."
date: "2026-10-21"
status: "done"
confidence_floor: 90
created: "2026-10-21"
updated: "2026-10-21"
---

# Goal: A local command runner cannot stall on full pipes or retain unbounded transcript output.

## Invariants

- Keep public `assimilate project/source` stateless; no new CLI model calls or runner flags.
- Command execution uses structured executable/arguments; no shell interpretation.
- Tests use synthetic local processes and fixture evidence only.
- Do not include raw request, subprocess output or secret-bearing arguments in errors.
- No native history changes, credential persistence, push, merge or deployment.

## Definition of Done

- [x] DoD-1 — Write stdin while draining both output pipes under a shared deadline — verify by: large stdout/stderr and blocked-stdin regressions.
- [x] DoD-2 — Request/output allocations are bounded and failures close pipes and reap the child — verify by: budget, early-exit, timeout and inherited-descriptor tests.
- [x] DoD-3 — Library execution works inside an existing Tokio runtime and documents platform cleanup behavior — verify by: nested-runtime regression and developer docs.
- [x] DoD-4 — Full repository verification passes — verify by: formatting, all tests, ignored benchmarks, strict Clippy and release build.

## Tasks

### T1 · Add synthetic lifecycle regressions · [x]

Define fixture subprocess modes for valid output, verbose streams, non-reading stdin, failure, malformed output and inherited descriptors. Record PID cleanup and bounded error expectations.

**Verification Contract:** new targeted regressions fail with the old runner and pass with the bounded runner. **Confidence:** 97 / 90.

### T2 · Implement typed budgets and bounded async I/O · [x]

Run async pipe operations concurrently within a synchronous library boundary; enforce request/output limits and one execution deadline; clean up children and Unix process-group descendants.

**Verification Contract:** all targeted process tests pass without emitting fixture output/arguments in error messages. **Confidence:** 97 / 90.

### T3 · Document and verify · [x]

Document defaults, limits, process cleanup and platform boundaries; run the full verification loop and hand off for independent review.

**Verification Contract:** full fixture suite, ignored benchmarks, strict Clippy and release build pass. **Confidence:** 97 / 90.

## Exit Conditions

- DONE — all DoD and tasks verified with confidence >=90.
- SCOPE-CHANGE — public CLI behavior or unrelated memory policy changes become necessary.
- BLOCKED-DEP — required tool/runtime unavailable.

## Evidence

- 2026-10-21 — Read-only audit reproduced 1 MiB stdout and stderr deadlock until the existing 60-second child deadline; small-output control succeeded.

## Decisions

- 2026-10-21 — Prefer typed library limits to new environment-variable or CLI controls.

- 2026-10-21 — Added nine synthetic integration regressions and two private cleanup unit regressions. Tests cover 2 MiB request with 1 MiB stdout and stderr emitted before reading input, pre-spawn request rejection, both output budgets, non-reading stdin timeout, inherited descriptors, group leader PID ownership, direct-child reaping, invalid/nonzero output without secret echo, nested Tokio use and injected group-signal failure cleanup.
- 2026-10-21 — Final full loop `python3 ../audit/verify.py --repo . --tag runner-ready` passed: fmt, 364 tests, four ignored benchmarks, strict all-target/all-feature Clippy, release build. Logs and JSON evidence are `../audit/evidence/runner-ready-*`. Release retains baseline local rust-objcopy/libLLVM stripping warning, with build exit 0.
- 2026-10-21 — Test-only safety refinement uses a non-existent high group ID for the disarmed-ownership unit fixture; `--tag runner-safety-unit --only test --filter command_runner_exit_inspection_errors_are_safe_and_explicit` passed afterwards.
- 2026-10-21 — Five warm debug fixture runs (including test process startup) completed the 2 MiB request + 1 MiB stdout + 1 MiB stderr workflow in 131.823–134.783 ms, median 132.784 ms on this aarch64 Mac/Rust 1.99.1. Evidence: `../audit/evidence/runner-synthetic-timings.json`. This is a deadlock/throughput fixture observation, not a general production speedup comparison.
- 2026-10-21 — The referenced global goal-driven-development skill/script is absent after parent searches of ~/.agents/skills, ~/.codex/skills and plugin cache. Manual eligibility checks follow repository examples: frontmatter, frozen scope, verified DoD, task confidence and append-only evidence. No code verification gate was skipped.

## Learnings

- 2026-10-21 — Retain the Unix group leader as a waitable child with WNOWAIT until group signaling, then consume/disarm the group identity before reaping. Reaping while descendants hold pipes can expose a reused group ID to delayed cleanup.
- 2026-10-21 — A standalone synthetic OS probe confirmed Darwin returns EPERM for SIGKILL to a group containing only its unreaped zombie. The implementation accepts that Apple-specific case only after confirming leader exit, and always attempts direct-child cleanup before reporting ordinary group-signal errors.
- 2026-10-21 — Competing child reapers invalidate ownership assumptions. ECHILD disarms both group and direct-child PID-based signals; Unix waitid/WNOWAIT is required. Non-Unix compilation retains async I/O and direct-child cleanup with documented descendant limitations; it was not cross-compiled on this Mac.

- 2026-10-21 — Independent reviewer and parent found no remaining issues after fixes for group-ID reuse, ECHILD ownership loss, unconditional bounded direct cleanup and platform documentation. All DoD/tasks verified; source scope unchanged.
