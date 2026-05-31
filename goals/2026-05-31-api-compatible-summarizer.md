---
title: "Move summarize to API-compatible backend"
description: "Replace CLI-harness summarize backends with an OpenAI-compatible API call configured by OPENAI_API_KEY, OPENAI_BASE_URL, and MMR_SUMMARISER_MODEL."
date: 2026-05-31
status: done
---

# GOAL: API-Compatible `summarize`

## Outcome

Make `mmr summarize` maintainable by removing dependence on fast-changing
provider CLIs for summarization. The command should call an OpenAI-compatible
chat-completions API using:

- `OPENAI_API_KEY`
- `OPENAI_BASE_URL`
- `MMR_SUMMARISER_MODEL`

Users should be able to point the command at OpenAI, OpenRouter, or a compatible
proxy without changing `mmr`.

## API Decision

Use a small, strongly typed `reqwest` client for the OpenAI-compatible Chat
Completions shape instead of adding a provider SDK dependency. Current official
OpenAI guidance still documents Chat Completions and says it remains supported,
while recommending Responses for new OpenAI-native projects. That makes Chat
Completions the better compatibility contract for OpenRouter and proxies, but
not something to describe as "nowhere near deprecation" beyond the documented
"supported" status.

## Surface Touched

- Summary backend code in `src/agent/ai.rs` and related agent/API types.
- CLI options and environment-variable handling in `src/cli.rs`.
- Summary status diagnostics, integration tests, and docs.

## Validation Plan

- Add fixture-backed mock HTTP coverage for the OpenAI-compatible request shape.
- Verify model precedence and environment configuration.
- Verify `--instructions` still replaces only the output instructions while
  preserving the base input-format prompt.
- Run the full repository verification loop.

## Definition of Done

`mmr summarize project/session/source` uses a single OpenAI-compatible API
client, no longer depends on Cursor/Codex/Gemini CLI harnesses, documents the
new environment variables, and passes tests, benchmark, clippy, and release
build checks.

## Completion Evidence

- Added `ChatCompletionsClient` with typed request/response handling for
  `POST /chat/completions`.
- Removed Cursor, Codex, and Gemini summarizer harness adapters and their CLI
  `--agent` selection.
- `summarize` now uses `OPENAI_API_KEY`, optional `OPENAI_BASE_URL`, and
  `MMR_SUMMARISER_MODEL` with `--model` override.
- Ran `cargo fmt`, `cargo test`,
  `cargo test --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo build --release`.
