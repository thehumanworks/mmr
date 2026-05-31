---
title: "Create archive containing goal and spec metadata directories"
description: "Create a zip archive including specific repository directories and policy files: goals, AGENTS.md, CLAUDE.md, plans, specs, adrs, and docs."
date: 2026-05-30
status: done
---

# GOAL: Create archive with selected repository files and directories

## Outcome
Bundle the requested repository artifacts into a single zip file so the result contains:
- `goals/`
- `AGENTS.md`
- `CLAUDE.md`
- `plans/`
- `specs/`
- `adrs/`
- `docs/`

## Surface touched
- New goal tracking document: this file.
- Newly generated zip artifact (to be created in repository root).

## Validation plan
- Verify the zip file is created successfully.
- Verify the archive contents include all requested top-level items.
- Record exact command output for evidence.

## Definition of Done
- Zip is created.
- Zip contains all seven requested inputs.
- No additional files are required for this goal.
- Goal status updated to `done` after verification.