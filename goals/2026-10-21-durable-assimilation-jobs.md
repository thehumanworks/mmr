---
title: "Run bounded assimilation jobs with durable recovery"
description: "Add opt-in finite queue execution using completed evidence snapshots and pending-only receipts, with truthful uncertainty after owner death."
date: "2026-10-21"
status: "done"
confidence_floor: 90
---

# Goal

Allow a user-managed scheduler to invoke a finite local assimilation run without
duplicating uncertain external execution or publishing stale/model-active memory.
Build on PR20; keep all existing PR heads intact and publish one focused PR.

## Surface and non-goals

Existing Store schema, new store jobs module, a small orchestration module and
explicit `assimilate jobs` CLI. No daemon, schedule installation, second database,
generic agent framework, automatic session discovery/ingestion, external model
experiment, live private history or trusted memory activation.

The initial queue selects explicit completed sessions/snapshots. Optional explicit
caller completion attestation is distinct from imported lifecycle/model claims.

## Definition of done

- [x] Explicit config defaults disabled; disabled enqueue/run opens no Store or runner. Existing assimilation project/source remains stateless. No ambient runner/model/key configuration is selected.
- [x] Enqueue keys bind snapshot/evidence, policy and explicit runner semantics; duplicate enqueue/restart cannot produce duplicate receipts. Queue rows, claims, batch, time and I/O are bounded.
- [x] A short immediate transaction claims with an incrementing fence. Expired pre-launch claims can be reclaimed; a durable execution-intent barrier precedes spawn. Expired execution is unknown/reconciliation-needed and never automatically retried.
- [x] Durable cancellation and lease/fence checks prevent stale owners committing. Proposal receipt and terminal job state commit in one transaction with current evidence validation. All output collections remain pending/rejected, with no learned-memory writes.
- [x] One-shot/bounded drain uses existing bounded command transport and cooperative cancellation; all controller/runner work joins. Owner-crash orphan limits are explicit and tested, without exactly-once external execution claims.
- [x] Synthetic tests cover two claimers, expiry/fencing, before-launch and during/after-execution crashes, cancellation, stale evidence, bounded resources, restart and deduplication. Independently reviewed; combined tests, all relevant benchmarks, fmt, strict Clippy and release pass.
- [x] Scoped commit and focused PR stacked after20; old heads remain unchanged. Goal records verification and exact remaining boundaries.

## Tasks

1. Freeze queue, worker and CLI contract in `specs/assimilation-jobs.md` before source changes. Align Store/worker interfaces across isolated worktrees.
2. Implement Store state machine and atomic receipt integration; targeted fixture tests first.
3. Implement bounded shared-safe projection, explicit command policy, finite orchestration and CLI contract with synthetic fake commands.
4. Independently review state transitions and crash/command lifecycle. Run real process restart/SIGKILL fixtures and combined verification. Close only with measured evidence and no unexamined uncertainty auto-retry.

## Validation

Run targeted job Store/worker/CLI fixtures, existing evidence and command tests,
then `cargo fmt --all -- --check`, `cargo test --offline --locked`, all applicable
ignored benchmarks, strict all-target/all-feature Clippy and release build.
Use the existing audit allowlisted environment; no provider keys, network model
endpoints or private provider roots. Tests may create local fake executables and
kill only owned fixture processes. Record workload counts/bytes and timings.

## Exit boundary

If a full job flow would require a generic queue/agent framework, shrink to explicit
sealed-session enqueue and finite command execution. Do not shrink away fencing,
pending-only/current-evidence commit or the unknown-outcome no-retry barrier.

## Evidence

- Base PR20: `dc889d3ed94fc3a0ea8d54d3ca39ffd7b94a609b`, combined tree `cdff8cfee96c9611b31e86b91eecf8c2d37d90ed`;416 tests and five benchmarks pass.
- Existing command transport supports bounded concurrent I/O, deadlines and cooperative cancellation. It cannot guarantee subprocess death after its owner is killed; job recovery must retain uncertainty rather than retry execution.
- Global GDD helper is unavailable in this environment; prior goals record the searches. Manual DoD eligibility is required alongside all executable verification.

### 2026-10-21 final verification before publication

- Confidence: 95. Store14 integration +4 private unit, worker9 integration +2
  private unit, and8 CLI fixtures pass. Both independent reviews have no remaining
  findings on pinned source hashes. The full combined tree passes453 tests, with
  five workload benchmarks run explicitly, formatting, strict all-target/all-feature
  Clippy and release build. Verification uses offline locked dependencies and an
  allowlisted environment; no live history or model credentials.
- Commands: `cargo fmt --all -- --check`; `cargo test --offline --locked`;
  `cargo test --offline --locked --test cli_benchmark -- --ignored --nocapture`;
  `cargo test --offline --locked --test evidence_contract -- --ignored --nocapture`;
  `cargo clippy --offline --locked --all-targets --all-features -- -D warnings`;
  `cargo build --offline --locked --release`. All exit0. The pre-existing optional
  rust-objcopy/libLLVM strip warning remains; release still succeeds.
- Owned real-process fixtures distinguish SIGKILL before the intent barrier
  (one fenced reclaim) from during execution and a public Store/runner consumer
  after joined result/before receipt (unknown, zero receipts, no automatic retry).
  Actual CLI cancellation joins cleanup; source changes before launch or receipt
  produce stale jobs without receipt. Concurrent CLI claimers run one command.
  Restart/dedup preserves one pending receipt and zero learned-memory writes.
- Five release measurements per workload:50/200 synthetic rows in4 sessions;
  request JSON21148/83948 bytes. At200 rows, median enqueue22.29ms, local fake
  proposal99.32ms, restart dedup16.36ms. These measure local plumbing including
  fake Python startup, not model latency/retrieval quality or a claimed speedup.
- Supporting audit artifacts in the delegated workspace: `jobs-final-verification.json`,
  `jobs-production-workflow.json`, `jobs-worker-cli-independent-review.json`,
  `jobs-review.md`, source manifests and executable scratch probes.
- Global GDD helper remains unavailable (prior searches documented). Manual DoD
  eligibility is clear for implementation/verification at95 confidence; publication
  checkbox remains open until a focused stacked PR is confirmed.

### Deliberate remaining boundaries

No daemon, installed schedule, automatic history ingestion, memory activation or
unknown-job retry/reconciliation command is added. Explicit completed session IDs
are required. Queue caps include terminal rows and there is no automatic GC.
Leases use Store wall time, with no renewal; runtime execution budgets are monotonic.
An abruptly killed owner can leave an external command alive until it exits: the
real synthetic fixture demonstrates this. Fencing blocks late publication and
recovery never automatically reexecutes unknown work. The trusted command has the
operator's privileges; clearing environment is not a network/filesystem sandbox.

### 2026-10-21 publication and manual goal closeout

- Scoped implementation commit published as
  `fc6aacb6101ae17800bec3c46cde96ac2ce039b8`, exact verified source tree
  `e7dc3a54b3205229ac38016899efd7e70c8cfeb2`.
- Focused PR: https://github.com/thehumanworks/mmr/pull/21, based on PR20.
  Existing seven feature heads and main were checked unchanged before publication.
  No merge, deployment, daemon/schedule installation or persistent credential change.
- All seven DoD checks are now manually eligible at95 confidence. The global GDD
  helper is unavailable; executable gates and both independent reviews supply
  evidence. This closeout changes only this goal document; production source,
  tests and verified release binary are unchanged.
