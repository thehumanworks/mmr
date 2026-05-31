---
title: "Make mmr dream return a prompt and runbook"
description: "Change the public `mmr dream` command so it no longer runs a mock or command AI runner or writes learned memory. Instead, it returns a deterministic system prompt and runbook that empowers the calling AI agent to perform memory deduplication, knowledge assimilation, and generalisation from evidence."
date: 2026-05-31
status: done
---

# GOAL: Make `mmr dream` a prompt/runbook generator

## Outcome

Calling `mmr dream` returns machine-readable JSON containing:

- the linked project identity
- shared-safe evidence metadata and refs
- a system prompt for the calling AI agent
- a concrete runbook for memory deduplication, knowledge assimilation, and generalisation
- a strict output contract the calling agent should produce after doing the work

The command must not run an AI provider, mock runner, or command runner as a side
effect, and it must not write `dream_runs`, `dream_candidates`, or
`learned_memory`.

## Surface Touched

- `src/cli.rs`: public `dream` command args and response handler
- `docs/`: user-facing memory-fabric and dream-runner docs
- `tests/memory_fabric_contract.rs`: CLI contract tests for the new behavior

## Validation Plan

1. Update the contract tests so `mmr dream` proves it returns the guide shape and
   does not persist learned memory.
2. Run focused dream tests while iterating.
3. Run the repository verification loop from `.cursor/rules/verification-loop.mdc`.

## Definition of Done

- `mmr dream --pretty` succeeds for a linked project with evidence.
- Output includes prompt/runbook fields for deduplication, assimilation, and
  generalisation.
- `MMR_DREAM_MOCK_OUTPUT`, `MMR_DEFAULT_DREAM_RUNNER`, and `MMR_DREAM_COMMAND`
  no longer affect public `mmr dream` behavior.
- Store inspection after `mmr dream` shows no learned-memory writes.
- Docs no longer describe `mmr dream` as provider-backed stateful assimilation.
