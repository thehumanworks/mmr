---
goal_id: "2026-11-12-cut-to-project-context"
title: "Cut mmr to project-centred, cross-harness context fetch"
description: "Audit every command, flag, subcommand and MCP tool against one test, and remove what fails it: does it help an agent fetch a project's earlier context, whichever harness produced it?"
date: "2026-11-12"
status: "in-progress"
confidence_floor: 85
created: "2026-11-12"
updated: "2026-11-12"
---

# Goal

Tomas, 2026-11-12: "We need to aggressively remove bloat so the tool is easy to
use by AI agents looking to gather context", and "what matters is the project
context, not which agent ran". The coordinator relayed both and asked for the
clear cuts as PR commits (tests first, old-vs-new checks), with close calls
listed rather than cut. The PR is not merged without Tomas's go-ahead.

The test for each command, flag and MCP tool: does it serve an agent that wants
a project's earlier context, across Claude, Codex, Cursor, Grok and Pi? A
surface organised around which harness ran, rather than the project, fails it.

## Scope: clear cuts

1. **Harness-scoped subcommands.** `read source`, `context source`,
   `summarize source`, `compact source` and the MCP tools `mmr_read_source`,
   `mmr_context_source`, `mmr_summarize_source`, `mmr_compact_source`. The
   global `--source` flag stays as an optional filter on project and session
   reads.
2. **Assimilation.** `assimilate project|source`, the dream runner and review
   code, the MCP tools `mmr_assimilate_project|source` and the
   `mmr_memory_assimilation` prompt. It curates learned memory; it does not
   fetch context, and the binary never writes learned memory.
3. **The simulated sync remote.** `sync`, `redact scan|explain`, and the
   file-backed `FakeGithubRemote` that `init` hydrates from and writes to. It
   is a simulation, not a hosted service. `share` keeps its privacy scan.
4. **Docs for the removed REST API.** `docs/api/openapi.json`, `index.html`, the
   generator and the docs server describe 24 REST operations that the binary
   no longer serves (removed 2026-10-22).

## Close calls: listed, not cut

Recorded in the PR with a recommendation each: `find` vs `retrieve`,
`summarize`/`compact` (external model calls), `MMR_DEFAULT_SOURCE` (wired
through the Nix modules), `ingest events` vs `init`, `read --format tree`,
`note`, `share --via http`, `context project` vs `read project`.

## Contract change

- Each removed command becomes a clap usage error (exit 2).
- Every surviving command keeps byte-identical output, except where a cut
  removes fields from it (cut 3: the `remote`, `hydration` and `sync` blocks of
  `init`, and the sync, redaction and dream diagnostics of `status`).

## Validation plan

1. Tests first: move every surviving behaviour a removed command exercised
   onto a surviving command, and prove the moved test passes on the old code.
   Add a rejection test that fails before each cut and passes after.
2. Old-vs-new differential check after each cut with the PR #31 matrices.
3. Full loop: `cargo fmt --check`, clippy `-D warnings`, `cargo test`,
   benchmarks, `cargo build --release`, `nix build`.

## Definition of Done

- [x] DoD-1 — Cut 1 removed; differential check shows only source cases change.
- [ ] DoD-2 — Cut 2 removed; differential check explained.
- [ ] DoD-3 — Cut 3 removed; differential check explained.
- [x] DoD-4 — Cut 4 removed.
- [ ] DoD-5 — README, the bundled mmr skill and the command taxonomy teach only
  the surviving surface.
- [ ] DoD-6 — Full verification loop and `nix build` pass.
- [ ] DoD-7 — PR updated with what was cut, what was kept and why, and the close
  calls with recommendations.

## Exit conditions

- `SCOPE-CHANGE` if a cut would change a surviving command in a way not listed
  under "Contract change".
- `CONFIDENCE-STALL` if a differential mismatch cannot be explained.
