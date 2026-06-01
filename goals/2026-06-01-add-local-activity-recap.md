---
title: "Add local activity recap"
description: "Add a local-first recap over mmr history inspired by Entire activity and recap, without cloud or TUI dependencies."
date: 2026-06-01
status: proposed
---

# GOAL: Add Local Activity Recap

## Outcome

Add a local `mmr recap` or `mmr activity` command that summarizes agent work
from the local `mmr` store and raw history.

Default output should be JSON. Optional human output may be `-O md` or
`--format line`, but no cloud service or TUI is required for the first pass.

Suggested shape:

```bash
mmr recap --day
mmr recap --week
mmr recap --month
mmr recap --project /path/to/project
mmr --source codex recap --week -O md
```

## Why

Entire's `activity` and `recap` surfaces expose useful operational context:
recent work, repository breakdown, agent mix, throughput, and recent commits.
`mmr` should borrow the local value, not the cloud/TUI packaging. A local recap
would help users see coverage and continuity across providers and projects.

## Surface Touched

- CLI command surface.
- Query aggregation over sessions/messages/events.
- Store-backed status and source counts.
- Optional Markdown renderer.
- Docs and contract tests.

## Required Metrics

- session count by source and project
- message/event count by source and project
- first/last activity timestamps
- token totals where known
- imported vs raw-only coverage
- redaction blocked counts
- sync status counts
- top active projects
- continuity hints, such as projects with recent sessions but no assimilation
  handoff or no sync

## Non-Goals

- No authenticated cloud activity endpoint.
- No team dashboard in v1.
- No interactive TUI in v1.
- No mutation of memory, source files, or sync state.

## Validation Plan

- Seed fixture stores with multiple projects, sources, timestamps, redaction
  statuses, and sync states.
- Assert date range filters are deterministic via injected clock or fixed
  fixture timestamps.
- Assert JSON totals and per-source/per-project breakdowns.
- Assert markdown/line output is opt-in and derived from the JSON model.
- Run full repo verification.

## Definition of Done

- [ ] A local recap command reports useful scope, source, project, token,
      redaction, and sync metrics.
- [ ] The command works without network or API credentials.
- [ ] JSON remains the default stdout contract.
- [ ] Docs explain how recap differs from status, find, and summarize.
- [ ] Full verification loop passes.
