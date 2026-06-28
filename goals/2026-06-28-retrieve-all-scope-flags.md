---
title: "Retrieve all-projects and all-sources scope flags"
description: "Update the docs-first retrieval contract, then implement mmr retrieve flags that allow system-wide and harness-wide retrieval across projects and sources."
date: 2026-06-28
status: blocked
---

# GOAL: Expand `mmr retrieve` scope controls

## Outcome

`mmr retrieve` exposes explicit `--all-projects` and `--all-sources` flags so
agents can search-to-read across the whole local history system and across all
supported harnesses when they deliberately opt in.

## Surface Touched

- Hindsight docs site and `.well-known/agents.json`.
- Retrieval product contract in `specs/retrieval.md`.
- `mmr retrieve` CLI parser, scoping behavior, response metadata, continuation
  command generation, and tests.

## Contract

- Goal: add explicit broad-scope retrieval flags without changing the default
  cwd-project behavior.
- Non-goals: remote retrieval, MCP-first retrieval, semantic/vector retrieval,
  and legacy aliases.
- Inputs/outputs: `mmr retrieve <query> [--all-projects] [--all-sources]` returns
  the existing retrieve JSON shape plus scope metadata.
- Invariants: selected sessions still use public `source_session_id`; pinned
  continuations remain executable as printed; successful stdout remains JSON;
  `find` behavior stays unchanged.
- Risks: accidental broad scans by default, ambiguous interaction with
  `MMR_DEFAULT_SOURCE`, and unbounded whole-system result volume.

## Validation Plan

- Patch docs before implementation and validate Human/Agent routes plus
  `.well-known/agents.json`.
- Add CLI contract tests for parser behavior, broad all-project retrieval,
  all-sources overriding `MMR_DEFAULT_SOURCE`, and pinned continuation command
  preservation.
- Run targeted retrieve tests, formatter, clippy, and relevant broader contract
  tests.
- Run coding-excellence review panel when available; if orchestration stalls,
  complete a main-agent review pass and record the evidence.

## Definition of Done

- [x] Docs describe completed broad-scope behavior and expose `/human/...` and
      `/agent/...` routes with toggle links.
- [x] `.well-known/agents.json` indexes all pages with human, agent, and agent
      markdown URLs.
- [x] `mmr retrieve --all-projects` searches every provider-discovered local
      project before output limits are applied, while still including Store
      matches and learned memory when present.
- [x] `mmr retrieve --all-sources` searches all sources even when
      `MMR_DEFAULT_SOURCE` is set.
- [x] `--project` and `--all-projects` are mutually exclusive; `--source` and
      `--all-sources` are mutually exclusive.
- [x] Tests cover the new scope flags and existing retrieval behavior remains
      green.
- [x] Full relevant verification passes or the smallest blocker is recorded.
- [x] Status is updated to `done` or `blocked`.

## Verification

Passed:

- `cargo test --test cli_contract retrieve_ -- --nocapture`
- `cargo test --test memory_fabric_contract retrieve_ -- --nocapture`
- `python3 -m json.tool .well-known/agents.json >/tmp/mmr-agents-json.pretty`
- `curl -fsS http://127.0.0.1:8000/human/retrieve/ | rg 'provider transcripts|Agent'`
- `curl -fsS http://127.0.0.1:8000/agent/retrieve.md | rg 'provider|parallelized|all-sources'`
- `curl -fsS http://127.0.0.1:8000/agent/overview/ | rg 'Human|Agent|retrieve'`
- `cargo run --quiet -- retrieve "hindsight engineering" --all-projects --all-sources --max-sessions 1 --limit 1`
  returned `total_matches: 2`, `total_selected_sessions: 1`,
  `scope.total_projects_searched: 709`, first session
  `codex:/Users/mish/projects/mmr:019edc03-af4e-7bf0-ab0a-4b0006f58c3e`.
- `cargo test --test cli_contract`
- `cargo fmt --check`
- `git diff --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --release`
- `cargo test --test cli_benchmark -- --ignored --nocapture`

Blocked:

- Full `cargo test` is not green in this environment because
  `mvp_release_gate_e2e_fixture_scenario` requires `CLI_PROXY_API_KEY`
  (`stderr=error: environment variable CLI_PROXY_API_KEY (from summarize.apiKeyEnv) must be set for summarize`).
  After that failure, the same full run had
  `summarize_config_api_key_contract_is_implemented` running for over 60
  seconds via a nested `mmr summarize project` process; the hung full-test
  subprocesses were terminated. The isolated release-gate test reproduces the
  missing-key failure.
