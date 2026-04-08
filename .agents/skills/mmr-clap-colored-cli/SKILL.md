---
name: mmr-clap-colored-cli
description: Build and evolve the local Rust CLI in this repo using clap derive patterns and colored output rules while preserving mmr-compatible JSON schemas for projects/sessions/messages. Use when adding commands, flags, sort/pagination behavior, source filtering, fixture tests, or benchmark checks in mmr.
---

# mmr Clap + Colored CLI

## Quick Start

1. Keep stdout as JSON and reserve colored output for stderr only.
2. Use clap derive (`Parser`, `Subcommand`, `ValueEnum`) for all CLI surface changes.
3. Preserve `mmr` contract semantics:
   - `--source` accepts `claude`, `codex`, or `cursor`; omitting it means all sources unless `MMR_DEFAULT_SOURCE` supplies a default
   - `sessions` and `messages` default to the cwd project unless `--project`, `--all`, or cwd discovery fallback changes the scope
   - `messages --session` without `--project` searches all projects, and `messages` paginates from the newest window before returning the page chronologically
4. Validate before finishing:
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
- Update tests first when changing behavior (unit + integration + benchmark path).
