---
goal_id: "2026-11-13-read-project-cwd-newest-page"
title: "Make read project in the current directory page like --project"
description: "Route the cwd form of read project and find through the same project lookup and paging as --project, so the first page is the newest one."
date: "2026-11-13"
status: "in-progress"
confidence_floor: 85
created: "2026-11-13"
updated: "2026-11-13"
---

# Goal

`mmr read project` with no `--project` returns the **oldest** page first,
while `mmr read project --project <same path>` returns the **newest** page
(newest window, then chronological order, the documented paging contract).
An agent that runs `mmr read project --limit 50` in its project sees the start
of history, not the recent work. Found during PR #32 and deferred until the
evaluation thread's project resolution (PR #33) landed.

The cwd form also queried each harness by its own name for the directory,
then merged and paged the result by hand. That suspected a scope gap for
subdirectory sessions too; it turned out the service already resolved each
name to the whole project, so only the paging was wrong (see Evidence).
`find` in the cwd uses the same helper.

## Surface

- `src/cli.rs::read_cwd_project_messages`, used by `read project` and `find`
  when no project, session or `--all` is given.

## Contract change

- `read project` in the cwd returns exactly what
  `read project --project <canonical cwd>` returns, including paging and
  `next_command` (which keeps omitting `--project`).
- `read project` and `find` in the cwd keep including sessions from the
  project's subdirectories and worktrees, like the other project commands.

## Validation plan

1. Tests first: the cwd and `--project` forms give identical pages for
   `--limit 1` at offsets 0, 1 and 3 (fails before the fix). A second test
   pins that a subdirectory session stays in the cwd form of `read project`
   and `find` through the change.
2. Old-vs-new differential check with the PR #31 matrix; only cwd
   `read project` and cwd `find` cases may change.
3. Full loop: fmt, clippy `-D warnings`, `cargo test`, benchmarks, release
   build, `nix build`.

## Definition of Done

- [x] DoD-1 — cwd and `--project` forms of `read project` agree.
- [x] DoD-2 — cwd `read project` and `find` keep including subdirectory sessions.
- [x] DoD-3 — Differential check explained.
- [ ] DoD-4 — Full verification loop passes; PR merged.

## Evidence

- `read_project_in_cwd_pages_like_explicit_project` failed before the fix:
  at offset 0 the cwd form returned `cwd codex question` (2025-01-07T00:00:01)
  and `--project` returned `cwd claude answer` (2025-01-07T00:02:00).
- The subdirectory test passed before the fix too: the old helper already
  went through the service's project lookup once per harness, so the scope
  was right and only the paging was wrong. The test pins the scope.
- Differential check against `main` (7bc772e), PR #31 matrix: 3 of 137 cases
  differ, all paged cwd `read project` runs (`--limit 1`, `--limit 2`, and
  `--source codex --limit 2 --offset 1`). Each now returns the newest window,
  with the same `total_messages`, `next_page`, `next_offset` and
  `next_command`. The unpaged cwd `read project` and every `find` case are
  unchanged.

## Exit conditions

- `SCOPE-CHANGE` if a command other than cwd `read project` or cwd `find`
  changes output.
