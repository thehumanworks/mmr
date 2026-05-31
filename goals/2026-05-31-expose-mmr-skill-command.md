---
title: "Expose mmr skill command"
description: "Add CLI commands that print or install the bundled mmr agent skill as a complement to help output."
date: 2026-05-31
status: done
---

# GOAL: Expose `mmr skill`

## Outcome

Add a top-level `mmr skill` command with:

- `mmr skill load` printing the bundled `mmr` skill to stdout for immediate agent reading.
- `mmr skill install` replacing any existing user-scoped `~/.agents/skills/mmr` with the bundled skill.
- `mmr skill install --local` replacing the project-scoped `.agents/skills/mmr` with the bundled skill.

## Surface Touched

- CLI command definitions and routing in `src/cli.rs`.
- The checked-in `.agents/skills/mmr` skill payload.
- CLI contract tests and documentation/help examples.

## Validation Plan

- Verify clap parsing for the new command family.
- Verify `skill load` emits readable skill Markdown on stdout.
- Verify user and local install modes replace pre-existing files and install the bundled skill tree.
- Run the repository verification loop after implementation.

## Definition of Done

The new `skill` command is visible in help, `load` prints the bundled skill,
both install targets replace stale pre-existing skill directories with the
current bundled skill files, tests cover the command contract, and the full
verification loop passes.

## Completion Evidence

- `mmr skill load` prints the bundled skill bundle with `mmr/SKILL.md`,
  `mmr/session-mining/SKILL.md`, and reference files.
- `mmr --help` lists `skill` and includes `mmr skill load` plus
  `mmr skill install --local` examples.
- `skill_install_replaces_user_scoped_skill` verifies replacement of
  `~/.agents/skills/mmr`.
- `skill_install_local_replaces_project_scoped_skill` verifies replacement of
  project `.agents/skills/mmr`.
- `python3 /Users/mish/.codex/skills/.system/skill-creator/scripts/quick_validate.py .agents/skills/mmr`
  passed.
- `cargo fmt`, `cargo test`,
  `cargo test --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo build --release` passed.
