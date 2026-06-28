---
title: "Docs-first search-to-read retrieval implementation"
description: "Create a hindsight-engineered docs site for mmr retrieve, then implement the search-to-read retrieval pipeline from the docs and frozen goal scope with subagent assistance."
date: 2026-06-28
status: done
---

# GOAL: Ship docs-first `mmr retrieve`

## Outcome

Create a local static documentation site that describes the completed
search-to-read retrieval pipeline with Human and Agent views, then implement the
CLI feature from that documentation and the frozen retrieval goal scope.

## Surface Touched

- Hindsight docs site and `.well-known/agents.json`.
- Retrieval product docs/specs.
- `mmr retrieve` CLI parser, response model, search-to-read selection logic, tests,
  and existing find/read contracts as needed.

## Validation Plan

- Validate the static docs site contract: indexed pages, Human/Agent sections,
  JSON index validity, and markdown access.
- Spawn bounded subagents from the docs source of truth for implementation slices.
- Run targeted retrieval tests, existing find/search tests, formatter, full Rust
  tests, benchmark contract, clippy, and release build.
- Run a coding-excellence review panel before finalizing meaningful code changes.

## Definition of Done

- [x] Static docs site exists and `.well-known/agents.json` indexes all pages.
- [x] Each indexed page has separate Human and Agent pages plus markdown access.
- [x] `mmr retrieve <query>` behavior matches the docs and frozen goal scope.
- [x] Retrieval tests cover ranking, identity mapping, citations, windows,
      unreadable matches, continuation, filters, privacy, and smoke behavior.
- [x] Existing `find` behavior remains unchanged.
- [x] Full verification loop passes or the smallest blocker is recorded.
- [x] Status is updated to `done` or `blocked`.

## Verification

- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test --test cli_contract` passed: 124 tests.
- `cargo test --test cli_contract retrieve_ -- --nocapture` passed: 8 tests.
- `cargo test --test memory_fabric_contract retrieve_ -- --nocapture` passed: 7 tests.
- `git diff --check` passed.
- `.well-known/agents.json` validates with `python3 -m json.tool`.
- Docs HTTP checks passed for `overview-human.html`, `retrieval-agent.md`, and `.well-known/agents.json`.

Full `cargo test` was started. It passed unit tests, `cli_contract`, `mcp_contract`,
and all retrieval-focused `memory_fabric_contract` tests before reaching
`mvp_release_gate_e2e_fixture_scenario`, which fails because
`CLI_PROXY_API_KEY` is not set for the existing summary e2e path. The run was
then interrupted while a later summary provider test was stuck in
`mmr summarize project`; no cargo/test child process remained afterward.
