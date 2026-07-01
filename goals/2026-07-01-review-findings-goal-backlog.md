---
goal_id: "2026-07-01-review-findings-goal-backlog"
title: "Convert review findings into goal backlog"
status: "done"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: The deep review findings are converted into agent-ready GDD goal files.

## 1. Invariants · the rules that must not break

This file is the only state for this authoring task. The implementation goals it
indexes are separate files under `goals/`.

- Scope for this file is authoring only: create clear goal docs, do not fix the
  reviewed defects here.
- Every child goal must have a concrete north star, atomic DoD, ordered tasks,
  verification contracts, and references to the reviewed code surface.
- Child goals remain unexecuted until a user or future agent explicitly chooses
  one to run.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — source review and verification evidence.
- `AGENTS.md` — repo goal-first workflow, verification loop, commit/push policy, and scope rules.
- `.cursor/rules/verification-loop.mdc` — repo verification-loop expectations.
- `.cursor/rules/cli-contract.mdc` — CLI stdout/stderr and response-shape constraints.
- `.cursor/rules/test-discipline.mdc` — fixture-driven test expectations.
- `/Users/mish/.agents/skills/goal-driven-development/SKILL.md` — goal document authoring contract.

---

## 3. Definition of Done · INVARIANT

- [x] **DoD-1** — Each final review finding has a corresponding executable goal file — *verify by:* `ls goals/2026-07-01-{sync-project-prefix-isolation,teleport-native-bundle-path-safety,source-filtered-provider-loading,teleport-ssh-target-hardening,recall-continuation-scope,shell-safe-next-commands,memory-fabric-test-gate-stability,retrieve-window-ranking-performance}.md`
- [x] **DoD-2** — Each child goal has no scaffold placeholders and passes GDD author validation — *verify by:* `for f in goals/2026-07-01-{sync-project-prefix-isolation,teleport-native-bundle-path-safety,source-filtered-provider-loading,teleport-ssh-target-hardening,recall-continuation-scope,shell-safe-next-commands,memory-fabric-test-gate-stability,retrieve-window-ranking-performance}.md; do python /Users/mish/.agents/skills/goal-driven-development/scripts/gdd_status.py --author "$f" >/tmp/gdd-check.json || exit 1; done`
- [x] **DoD-3** — The backlog gives a fresh agent a recommended execution order — *verify by:* `rg -n "Recommended Execution Order|2026-07-01-sync-project-prefix-isolation|2026-07-01-memory-fabric-test-gate-stability" goals/2026-07-01-review-findings-goal-backlog.md`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and every child goal parses in author mode. *(primary)*
- **`BLOCKED-DEP`** — the GDD status script is unavailable after one direct retry.
- **`SCOPE-CHANGE`** — the user asks to merge, drop, or execute the child goals instead of authoring them.
- **`CONFIDENCE-STALL`** — a child goal cannot be made concrete after 2 authoring passes.
- **`BUDGET`** — more than 2 complete validation passes are needed after the first child draft set.

---

## 5. Tasks · INVARIANT

### T1 · Create child goal contracts · [x]

**Steps**
- [x] Map each final review finding to one independently verifiable delivery goal.
- [x] Write one GDD document per finding.
- [x] Include exact source files, tests, and verification commands in each goal.

**Verification Contract**
- *Check:* all review findings have goal docs.
- *Method:* `ls goals/2026-07-01-{sync-project-prefix-isolation,teleport-native-bundle-path-safety,source-filtered-provider-loading,teleport-ssh-target-hardening,recall-continuation-scope,shell-safe-next-commands,memory-fabric-test-gate-stability,retrieve-window-ranking-performance}.md`
- *Expected:* exit 0 and all eight paths print.
- *BDD scenarios covered:* Given a fresh agent, when it chooses a review finding, then it can open a single goal file and start at T1.

**Confidence:** 95 / 90 · **Depends on:** none · **Closes:** DoD-1

**Evidence (required before tick; append-only)**
- 2026-07-01 — `ls goals/2026-07-01-{sync-project-prefix-isolation,teleport-native-bundle-path-safety,source-filtered-provider-loading,teleport-ssh-target-hardening,recall-continuation-scope,shell-safe-next-commands,memory-fabric-test-gate-stability,retrieve-window-ranking-performance}.md` — exit 0; all eight child goal paths existed.

### T2 · Validate and self-review goal quality · [x]

**Steps**
- [x] Run GDD author validation on every child goal.
- [x] Remove scaffold placeholders.
- [x] Run adversarial self-review and record the result in §6.

**Verification Contract**
- *Check:* every child goal parses cleanly in author mode and contains concrete DoD/tasks.
- *Method:* `for f in goals/2026-07-01-{sync-project-prefix-isolation,teleport-native-bundle-path-safety,source-filtered-provider-loading,teleport-ssh-target-hardening,recall-continuation-scope,shell-safe-next-commands,memory-fabric-test-gate-stability,retrieve-window-ranking-performance}.md; do python /Users/mish/.agents/skills/goal-driven-development/scripts/gdd_status.py --author "$f" >/tmp/gdd-check.json || exit 1; done`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given a future agent, when it runs `gdd_status.py --author` or normal status on a child goal, then the file is structurally usable.

**Confidence:** 95 / 90 · **Depends on:** T1 · **Closes:** DoD-2

**Evidence (required before tick; append-only)**
- 2026-07-01 — `for f in goals/2026-07-01-{sync-project-prefix-isolation,teleport-native-bundle-path-safety,source-filtered-provider-loading,teleport-ssh-target-hardening,recall-continuation-scope,shell-safe-next-commands,memory-fabric-test-gate-stability,retrieve-window-ranking-performance}.md; do python /Users/mish/.agents/skills/goal-driven-development/scripts/gdd_status.py --author "$f" >/tmp/gdd-check.json || exit 1; done` — exit 0 after final edits.

### T3 · Record recommended execution order · [x]

**Steps**
- [x] Put test-gate stability first because current `cargo test` is blocked.
- [x] Put P1 data/security defects before P2 contract/performance items.
- [x] Keep independent goals separate so agents can take them one at a time.

**Verification Contract**
- *Check:* this index names all child goals in priority order.
- *Method:* `rg -n "Recommended Execution Order|P1|P2|2026-07-01-retrieve-window-ranking-performance" goals/2026-07-01-review-findings-goal-backlog.md`
- *Expected:* exit 0 and matching lines print.
- *BDD scenarios covered:* Given several possible next tasks, when a coordinator asks what to run first, then this file provides a priority queue.

**Confidence:** 95 / 90 · **Depends on:** T2 · **Closes:** DoD-3

**Evidence (required before tick; append-only)**
- 2026-07-01 — `rg -n "Recommended Execution Order|P1|P2|2026-07-01-retrieve-window-ranking-performance" goals/2026-07-01-review-findings-goal-backlog.md` — exit 0; priority queue present.

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Split the review into eight child goals instead of one mega-goal because each finding has distinct proof, risk, and code ownership. Scope impact: none.
- 2026-07-01 — Put `memory-fabric-test-gate-stability` first in the execution order because the current full `cargo test` gate is blocked and other goals should not waive it silently. Scope impact: none.
- 2026-07-01 — Adversarial self-review: checked alignment, DoD coverage, fresh-session usefulness, and runnable verification. Fixed the main risk by making dependencies and targeted regression tests explicit in each child goal. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*

---

## Recommended Execution Order

1. P2 gate unblocker: `goals/2026-07-01-memory-fabric-test-gate-stability.md`
2. P1 data isolation: `goals/2026-07-01-sync-project-prefix-isolation.md`
3. P1 native bundle write safety: `goals/2026-07-01-teleport-native-bundle-path-safety.md`
4. P1 source-filtered provider loading: `goals/2026-07-01-source-filtered-provider-loading.md`
5. P2 SSH target hardening: `goals/2026-07-01-teleport-ssh-target-hardening.md`
6. P2 recall continuation scope: `goals/2026-07-01-recall-continuation-scope.md`
7. P2 shell-safe continuation commands: `goals/2026-07-01-shell-safe-next-commands.md`
8. P2 retrieve ranking/window performance: `goals/2026-07-01-retrieve-window-ranking-performance.md`
