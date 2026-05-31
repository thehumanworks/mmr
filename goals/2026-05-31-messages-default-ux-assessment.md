---
title: "Assess mmr messages default UX"
description: "Evaluate whether the current bare `mmr messages` behavior is optimal and propose better alternatives if not."
date: 2026-05-31
status: done
---

# GOAL: Assess `mmr messages` Default UX

## Outcome

Give an honest product/CLI UX assessment of the current default behavior and
name better alternatives.

## Surface Touched

- Current `messages` default behavior
- Existing related alternatives: `--latest`, `prev`, `--session-back`,
  `--session-range`, `--all`, `export`

## Validation Plan

- Ground recommendation in current implementation and existing specs/tests.
- Separate optimal default behavior from compatibility-preserving migration
  choices.

## Definition of Done

The answer states whether the current UX is optimal, why, and gives concrete
alternative command semantics with a recommended path.
