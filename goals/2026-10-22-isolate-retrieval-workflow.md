---
goal_id: "2026-10-22-isolate-retrieval-workflow"
title: "Isolate retrieval implementation and demonstrate the primary workflow"
description: "Move find/retrieve internals out of CLI routing without contract changes."
date: "2026-10-22"
status: "in-progress"
confidence_floor: 90
created: "2026-10-22"
updated: "2026-10-22"
---

# Goal

Make search and retrieval changes easier to locate and review while making the
existing find → read → context workflow easier to discover and try safely.

## Scope and invariants

- Base on current remote main after checking #22/#23; validate their combined
  behavior before editing. Preserve the earlier branches and user changes.
- Extract find/retrieve handlers, private response types, line matching, ranking,
  and continuation into one CLI submodule with explicit imports and two entry
  points. Keep clap arguments/routing and shared helpers in the parent.
- Preserve flags, JSON, text output, ordering, pagination, failures, and MCP
  contracts. No new capabilities, aliases, frameworks, storage or dependencies.
- Demonstrate find/read/context with the existing synthetic example helper in a
  temporary HOME/project; assert native histories are unchanged. No live history,
  secret access or external model calls.
- Publish one focused PR after checks and independent review. Do not merge or
  retry an earlier denied ready action without explicit relevant authorization.

## Definition of Done

- [x] DoD-1 — Combined main baseline and existing examples pass.
- [x] DoD-2 — Extraction has a clear private boundary and preserves behavior;
  verify unchanged moved bodies and existing search/retrieval/CLI/MCP contracts.
- [x] DoD-3 — README/agent workflow and runnable synthetic example agree;
  run the documented invocation and check messages plus unchanged native files.
- [ ] DoD-4 — Full tests, four benchmarks, format, strict Clippy, release build,
  docs/diff checks and independent review pass; publish a focused PR.

## Tasks

### T1 · Extract and demonstrate retrieval · [ ]

Inspect baseline, isolate the responsibility, document entry points and workflow,
run the example and verification, obtain independent review and publish.

**Verification Contract:** all DoD checks above. **Confidence:** 98 / 90.

## Exit Conditions

- DONE — verified and published; merge remains outside this task.
- BLOCKED-DEP — required verification/publication tool unavailable.
- SCOPE-CHANGE — preserving contracts requires a broader redesign.

## Evidence

- 2026-10-22 — Remote main is `921519b4941a6dbb5ad02513c1e44b850592a11c`;
  #22 and #23 are already merged. New clean branch starts there, so no temporary
  integration merge is necessary. Work runs locally on thestudio.

## Decisions

- Keep find and retrieve together: they share line matching, snippets, and store
  search semantics. Only their two command handlers are visible to CLI routing;
  the matcher and retrieval response/ranking types remain private.

- 2026-10-23 — Combined main and extracted branch each passed `cargo fmt --check`,
  `cargo test --locked` (360 passed), all four ignored benchmarks, strict
  all-target/all-feature Clippy, and release build using a cleared environment
  and synthetic HOME. Cargo 1.97.1. Logs: `../evidence/combined-main-*.log` and
  `../evidence/retrieval-*.log` (workspace evidence, not repository files).
- 2026-10-23 — Nine focused retrieval memory contracts passed. Existing CLI
  tests cover pinned continuation, scope, pagination, source filters and errors;
  MCP tests cover stdio and HTTP. No new algorithm or redundant tests added.
- 2026-10-23 — Exact comparison of moved bodies against main passed after
  normalizing the two visibility declarations and final blank line. `cli.rs`
  moves from 8,146 to 6,780 lines; the new module is 1,388 lines. This is isolation,
  not a claim of algorithmic simplification: matching, private response types,
  ranking and continuation are together behind two parent-visible handlers and
  nine explicit parent imports. Clap parsing/routing remains in the parent.
- 2026-10-23 — New workflow example passed with expected output and byte-for-byte
  native-history preservation. Both earlier stdio and portability examples passed
  on combined main. API docs regeneration produced no diff; `git diff --check`
  passed. Initial body comparison differed only in rustfmt's trailing blank line.
- 2026-10-23 — Independent read-only reviewer approved with no blocking findings,
  separately compared parent/moved bodies and ran the new example successfully.
- 2026-10-23 — `git fetch origin` confirms main still at the base above. Prior
  branches preserved; no local integration merge or new behavior needed.
- 2026-10-23 — The required global `~/.agents/skills/goal-driven-development/`
  skill and `scripts/gdd_status.py` are still absent (also recorded by prior
  goals). Manual evidence/structure review passed. Keep this goal in-progress
  because the closeout skill requires the missing checker before `status: done`.
  This administrative gate does not prevent verified commit/PR publication.
