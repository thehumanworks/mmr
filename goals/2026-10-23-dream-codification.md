---
title: "Bias dreaming toward reviewed codification proposals"
description: "Reuse assimilation evidence and validation to propose bounded checks without executing them."
date: "2026-10-23"
status: "done"
updated: "2026-10-23"
confidence_floor: 90
---

# Outcome and scope

Reuse the existing agent-driven assimilation handoff and dream observation
validator. Add optional `--review <file>` to project/source assimilation to
validate structured observations against the current projected evidence, with
optional codification proposals. No model runner, installation, new storage,
automatic execution, native history writes or additional command/tool family.

Codification stays proposed: at most 16 observations, 64 KiB input, recurring
evidence, one evidenced project scope, relative target, expected behavior and
validation description. Keep the supporting claim and provenance. Non-codifiable
judgement remains a short scoped lesson with a reason. Duplicate/superseded
instruction recommendations remain review output, not deletion.

# Definition of done

- [x] Canonical assimilation guide and handoff specify the contract before code.
- [x] CLI review validates refs, bounds and scope, rejects verified/installed
  claims, and never executes supplied validation text. Old handoff still works.
- [x] Synthetic regression tests cover accepted proposals/judgement, missing and
  omitted evidence, duplicate refs, invalid targets/scope/state, bounds and no
  writes. Example shows proposed check plus judgement lesson and preserved history.
- [x] Baseline/full format, tests, benchmarks, strict Clippy, release build,
  docs/example checks and independent review pass. Publish a focused draft PR;
  no merge or readiness changes.

# Evidence and decisions

- 2026-10-23 — Main is `2d19dc1` with #24/#25 merged, no open PRs. New branch
  starts from main; prior branches unchanged. Existing assimilation prepares
  projected evidence without running agents or writing learned memory. Existing
  dream observations already validate provenance and retain lessons.
- Choose a stateless proposal review, not execution attestation. MMR can verify
  structure/provenance membership; it cannot prove semantic correctness or test
  success from agent-supplied text. Actual checks require separately reviewed
  implementation and real execution evidence in the target project.

# Exit conditions

- DONE: all checks passed and draft PR published.
- BLOCKED-DEP: required tooling or publication unavailable.
- SCOPE-CHANGE: implementation needs new runtime/storage or automatic execution.

- 2026-10-23 — Baseline main full gates passed (360 tests, four benchmarks).
  Final working tree full gates passed: `cargo fmt --check`, `cargo test --locked`
  (364 tests), `cargo test --locked --test cli_benchmark -- --ignored --nocapture`
  (4 passed), `cargo clippy --locked --all-targets --all-features -- -D warnings`,
  `cargo build --locked --release`. Cargo 1.97.1, cleared environment and synthetic
  HOME. Workspace logs: `../evidence/dream-baseline-*.log` and
  `../evidence/codification-final-*.log`.
- 2026-10-23 — Four new regression tests exercise proposal/judgment retention,
  unknown/omitted refs, selected source-window exclusions, cross-project scope,
  distinct supporting refs, missing rationale/expected behavior, traversal,
  false verified/installed state, input/text/count limits and no execution or
  memory/event mutation. Existing native-history/portable tests remain green.
- 2026-10-23 — Initial focused test caught generic schema diagnostics; CLI now
  returns the underlying rejection reason. Initial full suite caught the old
  help assertion that --review must be absent; updated it to the intentional new
  flag and no-execution description. Targeted reruns and complete final gates pass.
- 2026-10-23 — `python3 examples/dream_codification.py` passed against debug and
  release binaries with the documented proposed-check/judgment output and native
  byte preservation. Existing find/read/context, portability and stdio examples
  also passed against release. API generator updated only the additive handoff
  schema; regeneration and `git diff --check` pass.
- 2026-10-23 — Independent read-only reviewer approved, separately ran the new
  example and reviewed the final help/rationale/window regression changes.
  Confidence 98/90. No live API, credential use, protected histories, automatic
  code execution or native restoration. Recurrence/semantic correctness and
  actual check success are intentionally not attested by proposal review.
- 2026-10-23 — Remote main remains
  `2d19dc1fe8bc75c565a6b50b2d74fb2dc024cd67`; no other open PRs. Prior branches and
  merged work preserved. Review is CLI-only; existing MCP/HTTP handoffs receive
  guidance but no file-reading review argument or new tool.

- 2026-10-23 — Published draft PR https://github.com/thehumanworks/mmr/pull/26
  at implementation commit `f00e1e85b9bcd6ab7a0de61a74986903be79a4df`.
  Local closeout evidence review passed; all required checks/work are complete.
  This final goal-only commit records publication and done status. No readiness
  or merge action was attempted; no implementation blockers remain.
