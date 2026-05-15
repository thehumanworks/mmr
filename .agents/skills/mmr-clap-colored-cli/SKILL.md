---
name: mmr-clap-colored-cli
description: Build and evolve the local Rust CLI in this repo using clap derive patterns and colored output rules while preserving mmr-compatible JSON schemas for projects/sessions/messages. Use when adding commands, flags, sort/pagination behavior, source filtering, fixture tests, or benchmark checks in mmr.
---

# mmr Clap + Colored CLI

## Quick Start

1. Keep stdout as JSON and reserve colored output for stderr only.
2. Use clap derive (`Parser`, `Subcommand`, `ValueEnum`) for all CLI surface changes.
3. Preserve `mmr` contract semantics:
   - `--source` is optional on read commands and accepts `claude|codex|cursor|pi`
   - `sessions` and `messages` default to the auto-discovered cwd project unless `--project` or `--all` overrides that scope
   - `messages --session <id>` without `--project` searches all projects and prints a narrowing hint when `--source` is omitted
   - `messages`: paginate from the newest matching window, then return the selected window in chronological order
4. Validate before finishing:
   - `cargo fmt`
   - `cargo test`
   - `cargo test --test cli_benchmark -- --ignored --nocapture`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo build --release`

## Read These References

- `references/clap-derive-patterns.md` — when changing CLI argument structure.
- `references/colored-output-policy.md` — when adding or changing terminal styling.
- `references/mmr-query-contract.md` — when touching sort/filter/pagination or JSON fields.
- `references/test-and-benchmark-loop.md` — when updating fixtures, integration tests, or benchmark checks.

## Core Rules

- Prefer small composable command handlers; avoid mixing parsing logic and ingest/query logic.
- Keep source loading and parsing parallelized (rayon) for throughput.
- Never color machine-readable output.
- Keep contributor docs, repo rules, and skill references aligned with the live CLI contract.
- Update tests first when changing behavior (unit + integration + benchmark path).
