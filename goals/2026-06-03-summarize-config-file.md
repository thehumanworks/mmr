---
title: "Summarize settings from ~/.config/mmr/config.json"
description: "Load summarize apiKey, baseUrl, and model from a keyed config file instead of requiring env vars."
date: 2026-06-03
status: done
---

# GOAL: Summarize config file

## Outcome

`mmr summarize` reads OpenAI-compatible settings from `~/.config/mmr/config.json`:

```json
{
  "summarize": {
    "apiKeyEnv": "OPENAI_API_KEY",
    "baseUrl": "https://api.openai.com/v1",
    "model": "gpt-5.5"
  }
}
```

API key precedence: `summarize.apiKey` → env var named by `summarize.apiKeyEnv` →
`OPENAI_API_KEY`. When config omits `baseUrl` or `model`, fall back to
`OPENAI_BASE_URL` / `MMR_SUMMARISER_MODEL`, then defaults (`https://api.openai.com/v1`
and `gpt-5.5`). `--model` overrides the resolved model for one invocation.

## Surface

- `src/config.rs` (new): path resolution, load, summarize resolution
- `src/agent/chat_completions.rs`, `src/agent/ai.rs`: use resolved settings
- `src/cli.rs`: model resolution, status diagnostics
- `tests/cli_contract.rs`: config-file integration test

## Validation

- Unit tests for config path and field precedence
- `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`

## Definition of done

Summarize works with config-only credentials; status reflects config; tests and clippy pass.