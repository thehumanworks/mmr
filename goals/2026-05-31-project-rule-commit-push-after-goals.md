---
title: "Project rule for commit and push after goals"
description: "Add a durable project rule requiring high-confidence agents to commit and push at the end of completed goals after verification, then commit and push the current work."
date: 2026-05-31
status: done
---

# GOAL: Project Rule For Commit And Push After Goals

## Outcome

Update the repository guidance so completed goals are committed and pushed when
confidence is high and the appropriate verification loop has passed, then stage,
commit, and push the current completed work.

## Surface Touched

- `AGENTS.md`
- Current worktree staging, commit, and remote push

## Validation Plan

- Confirm the project rule is present in `AGENTS.md`.
- Confirm the completed taxonomy work has already passed the full verification
  loop.
- Review `git status` before staging and after commit.
- Push the resulting commit to the configured remote branch.

## Definition Of Done

The rule is documented, the goal doc is marked done, all current work is staged
and committed, and the commit is pushed to the remote.
