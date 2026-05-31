---
title: "Portable project selection for message defaults"
description: "Propose a safer mmr messages default and project selector model for cross-host memory recall without depending on absolute paths."
date: 2026-05-30
status: done
---

# GOAL: Portable project selection for `mmr messages`

## Outcome

Propose command semantics that make broad recency-based recall useful by default
while keeping project-specific recall portable across hosts.

## Surface touched

- Proposal only.
- Current `messages`, project alias, and Memory Fabric contracts.

## Validation plan

- Check existing command and docs around cwd defaults, aliases, and memory store
  identity.
- Recommend a compatibility-aware migration path.

## Definition of done

- Final answer states the recommended default, project identity model, conflict
  behavior, and implementation phases.
