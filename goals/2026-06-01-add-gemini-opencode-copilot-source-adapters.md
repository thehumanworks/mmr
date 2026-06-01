---
title: "Add Gemini OpenCode and Copilot source adapters"
description: "Extend mmr source coverage using high-value providers that Entire CLI already supports: Gemini CLI, OpenCode, and Copilot CLI."
date: 2026-06-01
status: proposed
---

# GOAL: Add Gemini, OpenCode, And Copilot Source Adapters

## Outcome

Add first-class `mmr` ingestion and read support for the highest-value Entire
CLI provider coverage that `mmr` does not yet have:

- `gemini` for Gemini CLI
- `opencode` for OpenCode
- `copilot` for Copilot CLI

The new providers must behave like existing sources in `mmr list projects`,
`mmr list sessions`, `mmr read project`, `mmr read source`, `mmr recall`,
`mmr summarize`, `mmr find`, `mmr assimilate`, and `mmr status`. They do not
need native `teleport` pack/apply in the first implementation pass unless the
provider storage format is already stable enough to prove.

## Why

Entire CLI has production parser and hook surfaces for Gemini CLI, OpenCode,
and Copilot CLI. `mmr` already has a source-neutral product thesis, so broader
local history coverage is more valuable than copying Entire's Git checkpoint
state manager.

## Surface Touched

- `src/types/domain.rs` and related `SourceFilter` / `SourceKind` plumbing.
- `src/source/` legacy raw-history readers.
- `src/capture.rs` import adapters and cursor handling.
- `src/cli.rs` source validation, default roots, import/status reporting.
- `tests/fixtures/memory_fabric/` and `tests/cli_contract.rs`.
- Docs/specs that enumerate supported sources.

## Source Evidence

- Entire README lists agent hook support for Gemini CLI, OpenCode, and Copilot
  CLI.
- Entire source tree contains provider implementations under
  `cmd/entire/cli/agent/geminicli/`, `cmd/entire/cli/agent/opencode/`, and
  `cmd/entire/cli/agent/copilotcli/`.
- Local `mmr` currently enumerates only `claude`, `codex`, `cursor`, `grok`,
  and `pi`.

## Non-Goals

- No Git hook installation in this pass.
- No Git checkpoint, rewind, or clean behavior.
- No remote/cloud dependency.
- No semantic search requirement.
- No teleport support until read/import behavior is stable and fixture-backed.

## Validation Plan

- Add one fixture per provider that exercises user, assistant, tool/result or
  provider-specific lifecycle rows, timestamps, model metadata, token usage when
  available, project identity, and malformed-tail tolerance.
- Prove source filters reject invalid values and include the new values in help.
- Prove each provider participates in project/session/read/find/status/import
  contracts with isolated temp `HOME`.
- Run:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

## Definition of Done

- [ ] `--source gemini`, `--source opencode`, and `--source copilot` are accepted.
- [ ] Local raw readers and import adapters are fixture-backed for all three.
- [ ] New sources show up in `list`, `read`, `recall`, `summarize`, `find`,
      `assimilate`, `status`, and `import` where the existing five sources do.
- [ ] Docs and help text describe the new source roots and limitations.
- [ ] Full verification loop passes.
