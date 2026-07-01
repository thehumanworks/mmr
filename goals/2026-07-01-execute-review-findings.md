---
title: "Execute review finding goals"
description: "Use specialized subagents in separate git worktrees to implement, test, merge, and verify the review-finding goals on local main."
date: 2026-07-01
status: in-progress
---

# GOAL: Execute the review finding backlog on local main

## Outcome

Implement the changes described by the 2026-07-01 review-finding goal files,
using specialized subagents in isolated git worktrees where useful, then merge
the completed work back to local `main` with verification evidence.

## Surface Touched

- Project-scoped Codex agent definitions under `.codex/agents/`.
- Goal docs under `goals/`.
- Rust source and tests for sync, teleport, source loading, recall/read
  continuations, test-gate stability, and retrieve performance.
- Local git worktrees and merge integration on `main`.

## Validation Plan

- Create narrow project-scoped agent definitions for mmr implementation roles.
- Create one branch/worktree per implementation slice.
- Assign bounded goals to subagents with non-reversion and verification rules.
- Integrate completed branches onto local `main`.
- Run the full repo verification loop: `cargo fmt`, `cargo test`,
  `cargo test --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo build --release`.

## Definition of Done

- [ ] Specialized project-scoped Codex agent definitions exist and validate.
- [ ] Each review-finding child goal is either implemented and verified, or
      explicitly marked blocked with the smallest missing fact.
- [ ] Completed implementation branches are merged into local `main`.
- [ ] The full repo verification loop passes on local `main`.
- [ ] This goal status is updated to `done` or `blocked`.
