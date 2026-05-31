---
title: "Investigate mmr messages default behavior"
description: "Determine the current behavior of running `mmr messages` without additional flags, including project, session, source, and limit semantics."
date: 2026-05-31
status: done
---

# GOAL: Investigate `mmr messages` Defaults

## Outcome

Answer whether bare `mmr messages` pulls messages up to a limit across multiple
sessions, multiple projects, and all sources, or only the latest session in the
cwd-associated project across all sources.

## Surface Touched

- CLI command routing for `messages`
- message query service behavior
- tests and docs that lock default scoping

## Validation Plan

- Inspect `src/cli.rs` routing for bare `messages`.
- Inspect `src/messages/service.rs` filtering and pagination behavior.
- Inspect tests covering cwd defaults, `--all`, `--latest`, and session axes.
- Optionally run or cite focused tests if needed for confidence.

## Definition of Done

The answer states the current behavior precisely across project scope, session
scope, source scope, limit/pagination, and how to request the alternative
behavior.
