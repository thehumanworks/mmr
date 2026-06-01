---
title: "Declare Git checkpoint non-goals"
description: "Record that mmr should not copy Entire CLI's Git checkpoint, rewind, trail, review, or investigation orchestration surfaces."
date: 2026-06-01
status: proposed
---

# GOAL: Declare Git Checkpoint Non-Goals

## Outcome

Write an ADR or spec update that explicitly keeps `mmr` out of Git state
management and agent orchestration surfaces that belong to tools like Entire:

- Git checkpoint branches
- working-tree rewind/reset
- Git hook installation
- branch trails
- review agent orchestration
- multi-agent investigation orchestration
- cloud/team activity services

The ADR should map any useful adjacent `mmr` need to existing or proposed
surfaces: `read`, `recall`, `explain`, `summarize`, `assimilate`, `teleport`,
`doctor`, and `recap`.

## Why

Entire's checkpoint and orchestration features are coherent for a Git-native
session recorder. They are not automatically good fits for `mmr`, whose product
thesis is source-neutral memory fabric over local work. Copying those surfaces
would add large mutation and safety obligations without improving the core
history retrieval and memory workflows.

## Surface Touched

- New ADR under `adrs/` or spec update under `specs/`.
- Command taxonomy docs.
- Future-goal references that might otherwise suggest adding checkpoint/rewind
  behavior.

## Explicit Boundaries

- `mmr` may read source histories and store normalized events.
- `mmr` may move one selected session between machines through `teleport`.
- `mmr` may diagnose and explain local state.
- `mmr` must not mutate the user's working tree, reset branches, install Git
  hooks, or run review/investigation agents as a side effect of memory commands.

## Non-Goals

- No code behavior change unless docs currently imply these features are planned.
- No removal of existing `teleport` native apply behavior, because teleport is
  selected-session handoff, not Git checkpoint rewind.
- No criticism of Entire's design for Entire's product scope.

## Validation Plan

- Search docs/goals/specs for checkpoint, rewind, trail, review, investigate,
  hook installation, and Git branch mutation references.
- Update only wording that could invite `mmr` scope creep.
- Verify docs still describe `teleport` accurately as selected-session handoff.
- No cargo verification required unless code or tests change; otherwise run the
  full verification loop.

## Definition of Done

- [ ] ADR/spec boundary is written and linked from command taxonomy docs.
- [ ] Existing docs do not imply `mmr` will become a Git state manager.
- [ ] Useful Entire concepts are mapped to safer `mmr` surfaces.
- [ ] Only documentation changes are made unless a stale code path is discovered.
