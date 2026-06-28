---
title: "Retrieve debug metadata and snippet output"
description: "Adjust mmr retrieve so default JSON is concise, broad-scope metadata moves behind --debug, and full message history is opt-in."
date: 2026-06-28
status: done
---

# GOAL: Make `mmr retrieve` default output concise

## Outcome

`mmr retrieve` keeps the useful search-to-read matches, session identity, and
project metadata in default JSON, but removes broad execution metadata and full
provider message history unless the caller explicitly asks for it.

## Surface Touched

- `mmr retrieve` CLI flags and JSON response shape in `src/cli.rs`.
- Retrieval contract docs in `specs/retrieval.md` and the docs site.
- CLI contract fixtures and tests in `tests/cli_contract.rs` and
  `tests/common/mod.rs`.

## Contract

- Add `--debug` as a retrieve-specific flag.
- Default output must not include debug-only execution metadata such as the
  searched-project list, project count, or selected source-scope debug details.
- Default `selected_sessions[]` entries must include session id, project,
  source, ranking/match metadata, and short snippets via `matches[]`.
- Default `selected_sessions[]` entries must not include the full `messages[]`
  provider history.
- Add `--full-message-history` to include provider message windows in
  `selected_sessions[].messages`.
- Preserve existing matching, ranking, pagination, pinned-session continuation,
  `--all-projects`, and `--all-sources` semantics.
- Debug output must remain machine-readable JSON and successful stdout must stay
  JSON-only.

## Validation Plan

- Delegate implementation to a bounded worker with ownership of retrieve code,
  docs, tests, and this goal file.
- Run independent read-only reviewers for output-contract risks and verification
  gaps.
- Verify targeted retrieve contracts, full CLI contract test, formatter, clippy,
  release build, and benchmark gate. Record the known full-suite external-key
  blocker if it still applies.

## Definition of Done

- [x] `mmr retrieve <query>` default JSON omits debug metadata and omits
      `selected_sessions[].messages`.
- [x] `mmr retrieve <query> --debug` includes debug metadata, including searched
      project details.
- [x] `mmr retrieve <query> --full-message-history` includes provider message
      windows.
- [x] `next_command` preserves `--debug` and `--full-message-history` when
      required for pagination continuation.
- [x] Docs describe the concise default and the two new flags.
- [x] Tests cover concise default output, debug metadata, full message history,
      and continuation flags.
- [x] Status is updated to `done` or `blocked` with verification evidence.

## Verification Evidence

- `cargo fmt`
- `cargo test --test cli_contract retrieve_ -- --nocapture` - 14 passed.
- `cargo test --test memory_fabric_contract retrieve_ -- --nocapture` - 7 passed.
- `cargo test --test cli_contract` - 130 passed.
- `cargo fmt --check`
- `git diff --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --release`
- `cargo test --test cli_benchmark -- --ignored --nocapture` - 4 passed.

Full `cargo test` was not run because this task was directed to avoid it unless
`CLI_PROXY_API_KEY` is available; the targeted and requested gates above passed.
