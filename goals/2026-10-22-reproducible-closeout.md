---
title: "Make agent closeout reproducible from the checkout"
description: "Replace an absent global checker with local verification and evidence guidance."
date: "2026-10-22"
status: "in-progress"
confidence_floor: 90
---

# Goal

Let contributors finish verified work using tools and examples in this checkout,
without a host-specific goal checker. This repairs contributor instructions,
not a product defect, and retains all product verification gates.

## Scope

Replace active references to the missing global goal skill/checker with the
repository's goal structure and a shared closeout evidence checklist. Clarify
simulated GitHub sync and primary find/read/context discovery where misleading.
Reuse an existing synthetic example in a runnable verification recipe. No runtime
changes, new backend, validation framework or edits to historical goal evidence.
Base independently on main; preserve pending PR24 and its branch.

## Validation and definition of done

- [x] Active instructions use the local checklist; missing global tooling is no
  longer a gate. Check all active references and linked files.
- [x] Sync claims agree with `src/sync.rs`; the command guide explains the
  existing retrieve/native-ID bridge and bounded local context.
- [x] Run the documented verification recipe, including the synthetic example;
  full tests, benchmarks, formatting, Clippy and release build pass.
- [ ] Independent review approves; publish a separate draft PR without ready or
  merge actions. Record exact checks, commit and any remaining limitations.

## Evidence

- 2026-10-22 — Main remains `921519b4941a6dbb5ad02513c1e44b850592a11c`.
  PR24 remains draft at `3d70c14f730a5b9e3548ae4a175470d66d5398f8`.
  New branch `codex/reproducible-closeout` starts independently from main.
- 2026-10-22 — Global goal skill absent under both `~/.agents/skills` and
  `~/.codex/skills`; no tracked checker exists. Six active instruction files
  require it. Prior goals are historical evidence and will stay unchanged.

## Decisions

- Use the existing Rust checks, synthetic example and explicit evidence review,
  not a new goal validator. Product checks remain mandatory.

## Exit conditions

- DONE: all checks above pass and the draft PR is published.
- BLOCKED-DEP: required product verification or publication cannot run.
- SCOPE-CHANGE: repair requires runtime changes or a new validation framework.

- 2026-10-22 — Replaced missing-checker requirements in the six instruction
  files and the root goal index. `git grep -n -E
  'gdd_status|~/.agents/skills/goal-driven-development' -- . ':!goals'` returns no
  active references (exit 1). Historical goal evidence is preserved.
- 2026-10-22 — Full recipe passed on this working tree with a cleared environment
  and synthetic test HOME: `cargo fmt --check`, `cargo test --locked` (360 passed),
  `cargo test --locked --test cli_benchmark -- --ignored --nocapture` (4 passed),
  `cargo clippy --locked --all-targets --all-features -- -D warnings`,
  `cargo build --locked --release`. Cargo 1.97.1; logs in workspace
  `../evidence/closeout-*.log`.
- 2026-10-22 — Documented example `python3 examples/portable_read_only.py
  target/release/mmr` exited 0 with the expected conversation, unchanged native
  histories and temporary-home removal messages. `git diff --check` exited 0.
  Generated API docs stayed unchanged. No runtime, test, dependency or example
  code changes; existing setup reused without a new checker/framework.
- 2026-10-22 — Independent read-only review approved after the missed global
  skill link in GOAL.md was corrected. Reviewer confirmed product gates remain
  required, changed links resolve, and sync/retrieval claims match source. Only
  root-index/goal documentation changed after the full check. Confidence 98/90.
- 2026-10-22 — PR24 branch still at
  `3d70c14f730a5b9e3548ae4a175470d66d5398f8`; no updates or ready/merge actions.
