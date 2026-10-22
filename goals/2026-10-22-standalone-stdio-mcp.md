---
goal_id: "2026-10-22-standalone-stdio-mcp"
title: "Make stdio MCP standalone"
description: "Remove unused REST backing startup and lifecycle while preserving MCP contracts."
date: "2026-10-22"
status: "done"
confidence_floor: 90
created: "2026-10-22"
updated: "2026-10-22"
---

# Goal

Run stdio MCP without binding or probing any REST endpoint. Keep the existing
19 tools, five prompts, direct CLI execution and explicit streamable HTTP transport.
User authorization covers implementation, verification, PR creation and merge.

## Scope and invariants

- Change `src/mcp.rs`, MCP contract tests and active MCP documentation.
- Remove only the private REST implementation used by stdio startup; retain the
  published OpenAPI artifact and explicit HTTP MCP transport.
- Preserve source/query/output contracts and user data. Use synthetic homes only.
- No new tool aliases, storage system, native conversion or language rewrite.

## Definition of Done

- [x] DoD-1 — stdio initializes, lists tools, reads and finds fixture content with
  occupied backing ports and irrelevant/invalid REST environment settings, without
  connecting to the probe endpoint or opening a listener; verify by MCP contract tests.
- [x] DoD-2 — explicit HTTP initialization and representative tools still work;
  verify by MCP HTTP contract tests.
- [x] DoD-3 — full format, test, benchmark, Clippy and release gates pass; verify by
  the recorded verification loop, diff review and independent review.

## Tasks

### T1 · Remove stdio REST coupling and verify contracts · [x]

Add behavior tests, remove private REST startup/probe/lifecycle code, update docs,
run targeted tests and full verification, obtain independent review, then publish
a focused PR and merge when verified.

**Verification Contract:** all DoD checks pass. **Confidence:** 98 / 90.

## Exit Conditions

- DONE — all checks and review passed.
- BLOCKED-DEP — execution/tooling dependency prevents verification.
- SCOPE-CHANGE — public MCP contracts must change.

## Evidence

- 2026-10-22 — Clean checkout of main `125bdbf3942bd5c8296c181d1763dc1f0d1358b2`;
  no open PRs. Read AGENTS.md, repository skills, Cursor rules and MCP tests.

## Decisions

- 2026-10-22 — MCP handlers already call `run_cli` directly. The private REST
  router exists only to support an unused stdio side effect. Removing that code
  leaves explicit HTTP MCP unchanged.
- 2026-10-22 — Global goal-driven-development skill/checker is unavailable at the
  prescribed location, also documented by the previous verification goal. Use
  repository goal structure and manually verify evidence and completion fields.

- 2026-10-22 — Baseline full verification passed: formatting, 353 tests, four
  ignored benchmarks, strict Clippy and release build. Logs `../evidence/baseline-*.log`.
- 2026-10-22 — New stdio tests failed against original REST-dependent code.
  After implementation and fixture correction, all 12 MCP tests passed, including
  HTTP read/find, ignored invalid settings, occupied ports, no probe/listener and EOF.
- 2026-10-22 — Full final verification passed: `cargo fmt --check`, `cargo test
  --locked`, `cargo test --locked --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --locked --all-targets --all-features -- -D warnings`, `cargo build
  --locked --release`. Logs `../evidence/stdio-*.log`. Tests used a synthetic HOME
  and cleared environment; no private histories or live model services were used.
- 2026-10-22 — API docs regenerated; OpenAPI/HTML unchanged. `git diff --check`
  passed. Independent reviewer approved after fixture and HTTP cleanup fixes;
  verified tool handlers and explicit HTTP transport unchanged. Manual goal
  closeout confirmed all DoD evidence and confidence above floor.
