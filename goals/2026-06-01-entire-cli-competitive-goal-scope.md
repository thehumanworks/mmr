---
title: "Entire CLI competitive feature research"
description: "Research entireio/cli with wit, compare its useful and noisy surfaces against mmr, and write goal proposals for high-value additions or removals without implementing them."
date: 2026-06-01
status: done
---

# GOAL: Entire CLI Competitive Feature Research

## Outcome

Use `wit` to inspect the public `entireio/cli` codebase, compare its features,
strengths, and weaknesses against `mmr`, and turn the highest-value findings
into concrete `goals/` proposal files for future work.

This turn must stop at research, scoping, and goal/spec writing. Do not change
Rust implementation, tests, command behavior, or generated artifacts outside
the proposed goal documents.

## Surface Touched

- Remote GitHub repository: `entireio/cli`, read-only through `wit`.
- Local `mmr` repository docs, specs, command surface, and tests for comparison.
- `goals/` proposal documents only.

## Validation Plan

- Confirm the remote repository identity and inspect its README, package
  metadata, command definitions, and representative implementation files.
- Inspect current `mmr` command and spec surface from the local worktree instead
  of relying only on memory.
- Create separate goal files for each recommended high-value addition or removal.
- Verify that only `goals/*.md` files changed.

## Definition of Done

- [x] Entire CLI capabilities are summarized from repository evidence.
- [x] Current `mmr` strengths, weaknesses, and overlapping surfaces are compared.
- [x] Proposed `goals/` files capture outcome, surface, validation plan, and
      definition of done.
- [x] No implementation work is performed.
- [x] This goal is updated to `done` or `blocked` with the smallest missing fact.

## Research Summary

Entire CLI is a Git-native agent session recorder. Its strongest reusable ideas
for `mmr` are broader provider coverage, a capability-declared external adapter
protocol, operational diagnostics, deterministic explain views, and activity
recaps. Its checkpoint, rewind, trail, review, investigate, hook installation,
and cloud/team surfaces are coherent in Entire's product, but they would pull
`mmr` away from source-neutral memory fabric and into Git state management.

The proposed `mmr` goal files therefore separate high-value additions from
scope boundaries and possible command-surface cleanup.
