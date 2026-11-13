---
goal_id: "2026-11-13-cut-model-calls-and-default-source"
title: "Cut summarize, compact and MMR_DEFAULT_SOURCE"
description: "Remove the two model-backed commands and their MCP tools, the summarize config they need, and the default-source setting, so every mmr command fetches raw project context with no setup or API key."
date: "2026-11-13"
status: "done"
confidence_floor: 85
created: "2026-11-13"
updated: "2026-11-13"
---

# Goal

Tomas, 2026-11-12: "You can merge too. I'm giving you full ownership of this.
The tool is for agents, not me." PR #32 listed two close calls and decided to
cut them in a follow-up:

- `summarize` and `compact` send history to an external model and need an
  OpenAI or Morph API key. No agent used them in any evaluation run, and the
  calling agent is itself a model that can condense what it reads.
- `MMR_DEFAULT_SOURCE` makes every command read one harness by default, which
  works against "project context, not which agent ran".

## Surface

- Commands `summarize project|session` and `compact project|session`, and the
  MCP tools `mmr_summarize_project|session` and `mmr_compact_project|session`.
- `src/agent/` (chat completions and Morph clients), `src/config.rs` (the
  `summarize` table of `~/.config/mmr/config.json`), the response types they
  use, and the `reqwest` dependency if nothing else needs it.
- `MMR_DEFAULT_SOURCE` and the Nix options `defaultSource` and `settings`. The
  modules keep accepting both and warn that they are ignored, so an existing
  Nix configuration still evaluates.
- Docs, the bundled skill text and help that mention any of these.

## Contract change

- The removed commands are clap usage errors (exit 2); the removed MCP tools
  are absent from `tools/list`.
- `MMR_DEFAULT_SOURCE` is ignored: without `--source`, every command reads
  every harness.
- Every other command keeps byte-identical output, except help texts that list
  the removed commands or mention `MMR_DEFAULT_SOURCE`.

## Validation plan

1. Tests first: a rejection test for the removed commands and a test that
   `MMR_DEFAULT_SOURCE` no longer narrows a read; both fail before the cut.
2. Old-vs-new differential check with the PR #31 command matrix.
3. Full loop: fmt, clippy `-D warnings`, `cargo test`, benchmarks, release
   build, `nix build`, and the NixOS module and VM checks.

## Definition of Done

- [x] DoD-1 — Commands, MCP tools, model clients and config removed.
- [x] DoD-2 — `MMR_DEFAULT_SOURCE` and its Nix options removed, with warnings.
- [x] DoD-3 — Differential check explained.
- [x] DoD-4 — Docs, skill text and help teach only the surviving surface.
- [x] DoD-5 — Full verification loop passes; PR merged after the evaluation
  thread's PR.

## Evidence

- `model_commands_are_rejected` and `default_source_env_is_ignored` failed
  before the cut and pass after it.
- Differential check against `main` (0a59506), PR #31 matrix: 22 of 137
  cases differ (30 of 411 files). 12 are help texts: the top-level help and
  9 subcommand helps (the global `--source` line no longer mentions
  `MMR_DEFAULT_SOURCE`), plus `summarize --help` and `compact --help`, which
  are now usage errors. 8 are summarize/compact cases, now exit 2. One is
  `list projects` with `MMR_DEFAULT_SOURCE=" Codex "`, which now returns all
  7 projects from 5 harnesses instead of 3 Codex projects. One is
  `skill load`, whose text changed. Three share cases differ only in
  timestamps and the hashes that cover them, as they do between two runs of
  the same binary. All 47 summarize/compact cases
  of the model matrix are now exit 2.
- After merging `main` with #33 (eece0d3), the same matrix against a binary
  built from eece0d3 gives the same 22 changed cases, and `skill load`
  differs only in the three session-mining lines this PR rewrites.
- Verification loop on the merge: `cargo fmt --check`, clippy with
  `-D warnings`, `cargo test` (220 tests), benchmarks, both examples,
  `cargo build --release`, `nix build`, and the `nixos-module`,
  `home-manager` and `nixos-vm` checks pass. PR #34 CI builds all four
  targets.
- No runtime dependency compiles C any more (`cargo tree -i cc` finds only
  the Haiku time-zone crate).

## Exit conditions

- `SCOPE-CHANGE` if another surviving command changes output.
