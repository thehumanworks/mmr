---
goal_id: "2026-10-21-assimilation-evidence-fences"
title: "Fence completed-session evidence before background assimilation"
description: "Add bounded local evidence seals, immutable snapshots and durable stale/revocation checks in the existing Store."
date: "2026-10-21"
status: "done"
confidence_floor: 90
created: "2026-10-21"
updated: "2026-10-21"
---

# Goal: A future worker can accept only proposals based on a bounded, explicitly completed, still-current evidence snapshot.

## Invariants

- Existing SQLite store only; modular `src/store/evidence.rs`. No second DB, runner, queue, daemon, configuration, CLI command, provider access or learned-memory writes.
- Completion is an explicit trusted caller attestation of the current local session revision, never elapsed idle time, a timestamp cursor or model inference.
- Relevant insert/update/delete/revocation invalidates the completion seal and old snapshot, including equal timestamps. Exact duplicate event/batch replay must remain a no-op for evidence revision.
- Revocation is local logical evidence exclusion; raw history is retained and tombstones survive physical delete/reinsert/restart. No claim of global/physical deletion or cryptographic authenticity.
- Selection is explicit bounded session IDs plus an indexed bounded session-change feed. Preflight session/event/semantic-byte budgets before materializing source strings; no hidden full project read.
- Store-owned and supported external SQLite writers require foreign_keys=ON and recursive_triggers=ON; unconfigured writer mutations are outside the fence contract.
- A dedicated proposal-only receipt validates snapshot currency in the same SQLite immediate transaction as receipt persistence. Future job lease/cancel/watermark checks remain separate future work, with an internal transaction validation helper.
- Public `assimilate project/source` remains stateless and existing native/sync/source APIs retain their intended behavior.

## Definition of Done

- [x] DoD-1 — Schema/API design is aligned with parent/worker plan before implementation — verify by: recorded design response.
- [x] DoD-2 — Completion and immutable snapshot survive reopen and identical replay; append/update/delete/revocation stale them deterministically — verify by: synthetic evidence contract tests.
- [x] DoD-3 — Tombstones cannot be bypassed by deleting/reinserting evidence; current snapshots exclude revoked IDs — verify by: physical SQL deletion/reinsert and concurrent-handle tests.
- [x] DoD-4 — Selection/materialization/result budgets fail before unbounded allocation; snapshot digest includes ordered full semantic metadata and versions — verify by: count/byte/oversized-metadata and hash sensitivity tests.
- [x] DoD-5 — Proposal-only receipt commits only under a current snapshot in one transaction, remains idempotent, and writes no learned-memory/job state — verify by: stale/concurrent/reopen/duplicate receipt tests.
- [x] DoD-6 — Migration and full format/test/benchmark/Clippy/release gate pass, independent review is clear — verify by: saved synthetic-only logs and reviewer result.

## Tasks

### T1 · Freeze API/schema design · [x]

Write `specs/assimilation-evidence.md` and send exact API/state/acceptance boundary to parent before code. **Verification Contract:** parent aligns worker dependency; no speculative job/scheduler framework. **Confidence:** 95 / 90.

### T2 · Implement bounded evidence module and tests · [x]

Add schema-v3 revision ledger, explicit completion, local durable revocation, immutable bounded snapshots/change feed and atomic proposal receipt. Keep monolithic store edits to module/migration wiring and exact replay session-upsert hygiene. **Verification Contract:** synthetic crash/reopen/replay, stale writes at equal timestamps, physical deletion, concurrent handles and budget cases pass. **Confidence:** 95 / 90.

### T3 · Verify, review and commit · [x]

Run full gate, independent review and manual eligibility; commit only this branch after acceptance. Parent publishes; no existing PR changes or merges. **Verification Contract:** full fixture loop and review clear, scoped clean commit returned. **Confidence:** 95 / 90.

## Exit Conditions

- DONE — all frozen DoD verified and scoped commit delivered to parent.
- SCOPE-CHANGE — a worker/queue/second DB or automatic activation becomes necessary.
- BLOCKED-DEP — required tooling/schema assumptions cannot be verified.

## Evidence

- 2026-10-21 — Base 9c72eeb has schema v2, never writes sessions.ended_at, and exposes no durable revision/tombstone/completion fence.
- 2026-10-21 — Existing event insertion uses ON CONFLICT DO NOTHING but updates session started_at/raw ref before conflict resolution. Identical replay must not invalidate an otherwise unchanged seal through those incidental upserts.

## Decisions

- 2026-10-21 — This prerequisite does not execute jobs or activate memory. Trusted callback/lease semantics will not be claimed by the standalone snapshot receipt.

- 2026-10-21 — Parent accepted the five-table prerequisite API/schema boundary, then refined durable revocation to global local-Store event IDs and automatic deletion tombstones (including cross-project reimport/cascades). No worker or activation scope added.

- 2026-10-21 — Parent approved required per-connection FK/recursive-trigger settings, rather than brittle INSERT conflict guards. Store enables both; supported REPLACE has deletion/tombstone/counter tests including cross-project moves and redaction-policy replacement. Unconfigured external SQL is explicitly outside the guarantee.

## Closeout evidence

- 2026-10-21 — T1 / DoD-1: parent accepted the five-table API/spec prerequisite, composite cursor refinement and explicit per-connection FK/recursive-trigger contract before final verification. No worker, CLI, activation or model scope was added.
- 2026-10-21 — T2 / DoD-2–5: actual allowlisted `cargo test --offline --locked --test evidence_contract -- --nocapture` passed 16 contracts (1 opt-in benchmark ignored), including restart/replay, full semantic/dependency fences, equal timestamps, raw retention, physical delete/reinsert/cascades/cross-project exclusion, configured REPLACE, fixed-through tied paging and a genuinely concurrent blocked writer. Evidence: `../audit/evidence/evidence-targeted-final.log`. Migration, rollback/lock, tuple seek, bounded stored-deserialization and required connection pragma unit tests also pass in the full suite.
- 2026-10-21 — T3 / DoD-6: `/usr/bin/python3 ../audit/verify.py --repo . --tag evidence` passed format check, all 402 tests, all 4 existing opt-in CLI benchmarks, strict all-target/all-feature Clippy with `-D warnings`, and release build. Evidence: `../audit/evidence/evidence-verification.json` and associated logs. The wrapper uses an allowlisted synthetic HOME/offline toolchain; temporary fixture loopback is the only elevated test capability.
- 2026-10-21 — T3: explicitly ran `cargo test --offline --locked --test evidence_contract benchmark_completed_evidence_selection_and_receipt -- --ignored --nocapture`; 1 passed. Synthetic workload: 100 sessions / 2,000 total events, 10 sessions / 200 selected; import 2,017 ms, seal+snapshot 39 ms, current read 19 ms, receipt 19 ms on this debug build. This is a workload measurement, not a speedup/RSS guarantee. Evidence: `../audit/evidence/evidence-bounded-benchmark.log` and JSON command report.
- 2026-10-21 — Independent `/root/gate_review` final review: no remaining actionable findings. SHA manifest `../audit/evidence/evidence-reviewed-source.json`; report `../audit/evidence/evidence-fences-review.md`. Independent 8-case configured REPLACE matrix and adversarial API/allocation probes pass; the reviewer made no source edits. Identified source rename, identity preflight, rowid aliases, tie cursor and policy REPLACE issues were corrected before this final gate.
- 2026-10-21 — New guide/spec relative links valid; `git diff --check` clean. No private histories, external model/provider calls, credentials, jobs or learned-memory writes used. Existing PRs14–17 remain untouched.
- 2026-10-21 — Manual DONE eligibility: all six DoD checks have explicit passing evidence; all three tasks meet confidence95 >= floor90; exact reviewed-file hashes still match; full loop/benchmarks/docs checks pass. Global `goal-driven-development/scripts/gdd_status.py` remains absent after prior searches; parent explicitly allowed manual eligibility. Scratch record: `../audit/evidence/evidence-manual-eligibility.json`. Scoped commit authorized; publication/merge left to parent.
