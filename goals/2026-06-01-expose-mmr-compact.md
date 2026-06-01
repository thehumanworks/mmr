---
title: "Expose mmr compact"
description: "Add an mmr compact command that sends selected conversation history to Morph Compact instead of summarizing it, preserving surviving lines verbatim while reducing noise."
date: 2026-06-01
status: done
---

# GOAL: `mmr compact`

## Outcome

Expose `mmr compact project|source|session` as a stateless transcript compaction
command. It should select the same conversation-history surfaces as
`mmr summarize`, but call Morph's native Compact API instead of asking an LLM to
write a summary.

## Surface Touched

- CLI parsing and command routing in `src/cli.rs`.
- Morph Compact HTTP client code under `src/agent/`.
- Public response types for compact results.
- CLI/MCP/docs/tests that enumerate public commands.

## API Contract

Use Morph's native endpoint:

- `POST https://api.morphllm.com/v1/compact`
- `Authorization: Bearer $MORPHLLM_API_KEY`
- Request fields: `input`, optional `query`, `compression_ratio`,
  `preserve_recent`, `include_line_ranges`, `include_markers`, and `model`.
- Response fields used by `mmr`: `id`, `model`, `output`, `messages`, and
  `usage`.

`MORPHLLM_API_KEY` is available through Doppler project `ai`, config `prd` for
live verification.

## Validation Plan

- Add fixture-backed mock HTTP tests for request shape and response shape.
- Verify `compact project`, `compact source`, and `compact session` reuse the
  existing history selection semantics.
- Verify CLI parsing for compact-specific options.
- Run the full repository verification loop:
  `cargo fmt`, `cargo test`, `cargo test --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo build --release`.
- If local verification passes, run a live Morph smoke test through Doppler
  without printing the secret.

## Definition of Done

`mmr compact` can compact project, source, and session history with Morph
Compact, emits machine-readable JSON by default or compacted text with `-O md`,
documents required environment variables and options, and has both mocked
contract coverage and live provider evidence unless credentials or network are
blocked.

## Completion Evidence

- Added `mmr compact project|source|session` with project/source/session
  selection matching `summarize`.
- Added a native Morph Compact client for `POST /v1/compact`, configured by
  `MORPHLLM_API_KEY`, optional `MORPHLLM_BASE_URL`, `MMR_COMPACT_MODEL`, and
  `--model`.
- Added compact options: `--query`, `--compression-ratio`, `--preserve-recent`,
  `--no-line-ranges`, `--no-markers`, and `-O/--output-format json|md`.
- Added MCP tools `mmr_compact_project`, `mmr_compact_session`, and
  `mmr_compact_source`.
- Added mocked compact contract tests and parser/client unit tests.
- Ran `cargo fmt`, `cargo test`,
  `cargo test --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo build --release`.
- Ran a live Doppler-backed Morph smoke test against a synthetic temp HOME
  fixture; response returned `backend=morph-compact`, model `morph-compactor`,
  id `cmpr-6f83548f6dd9`, and usage metadata without sending real local history.
