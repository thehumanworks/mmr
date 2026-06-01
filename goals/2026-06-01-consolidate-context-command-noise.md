---
title: "Consolidate context command noise"
description: "Audit the public context command and remove or merge it if it duplicates summarize and assimilate without a unique product contract."
date: 2026-06-01
status: proposed
---

# GOAL: Consolidate `mmr context` Command Noise

## Outcome

Audit `mmr context project` and `mmr context source` against the current
`read`, `summarize`, and `assimilate` contracts. Keep `context` only if it has
a distinct, testable user outcome. Otherwise, remove it from the public surface
or merge its useful behavior into `summarize --instructions` and
`assimilate`.

## Why

The Entire comparison shows that strong CLIs make command nouns carry distinct
jobs: `status` is state, `doctor` is diagnosis, `explain` is deterministic
evidence, and `recap` is activity. In `mmr`, `context` risks becoming a vague
middle command between raw reads, deterministic explain, model summaries, and
assimilation handoffs. The Memory Fabric MVP also explicitly rejected public
`context` as a learned-memory product surface, while current code now exposes
it.

## Surface Touched

- `src/cli.rs` and any context response helpers.
- `docs/mmr-command-taxonomy.md`, quickstart docs, skills, MCP tool list, and
  tests that mention `context`.
- `tests/cli_contract.rs` command removal or retention tests.
- `src/mcp.rs` if MCP currently exposes context-related tools.

## Decision Framework

Keep `context` only if all are true:

- It has a user-visible outcome not covered by `read`, `explain`, `summarize`,
  or `assimilate`.
- Its output schema is stable and fixture-tested.
- It does not silently call a model.
- It is not just a different prompt shape for `summarize`.

Remove or merge it if any are true:

- It returns a raw-message subset with a looser name than `read`.
- It duplicates `summarize --instructions`.
- It duplicates `assimilate` without the evidence bundle and output contract.
- It adds MCP/tooling surface area that agents must learn without a clear win.

## Non-Goals

- No removal of `read`, `recall`, `summarize`, or `assimilate`.
- No change to source parsing.
- No behavior change before an explicit deprecation/removal decision is recorded.

## Validation Plan

- Inspect every call path and doc reference for `context`.
- If removed, assert `mmr context` fails with a clear clap usage error and docs
  map users to replacements.
- If retained, add tests that prove the unique behavior and document when to use
  it.
- Update MCP tool exposure if the public surface changes.
- Run full repo verification.

## Definition of Done

- [ ] A written decision records keep/remove/merge for `context`.
- [ ] Public docs and MCP tools match that decision.
- [ ] Tests lock the chosen behavior.
- [ ] The command surface has less overlap, not more.
- [ ] Full verification loop passes.
