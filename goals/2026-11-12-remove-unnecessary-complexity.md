---
goal_id: "2026-11-12-remove-unnecessary-complexity"
title: "Remove unnecessary complexity without changing behaviour"
description: "Delete code the binary can no longer reach and collapse copy-pasted CLI remote/peer plumbing, after adding tests that pin the current behaviour."
date: "2026-11-12"
status: "in-progress"
confidence_floor: 85
created: "2026-11-12"
updated: "2026-11-12"
---

# Goal

The mmr codebase is smaller and easier to change, and every user-visible
behaviour is the same as before. The user request ("Identify unnecessary
complexity in the code and remove it, ensuring you have enough coverage to
validate there are no regressions introduced by the change") is the approval to
execute.

## Findings (survey of `main` at 9f6170c)

Method: read `src/cli.rs` and the modules it calls; a scratch build with every
library module made private so `rustc` reports items the `mmr` binary cannot
reach; `cargo llvm-cov` for line coverage (baseline 73.6% lines overall,
`src/cli.rs` 74.0%).

1. **Dead teleport code.** The public `teleport` namespace was removed on
   2026-06-01 (`goals/2026-06-01-remote-share-import-command-surface.md`), but
   `src/teleport/{export,inspect,resume}.rs` (about 520 lines, 0% covered) and
   their helpers stayed. Also dead: `TeleportProviderProfile::{conflict_check_path,
   supports_resume_transform, primary_native_path}` (the first is only called as
   `let _ = …`), five `*_conflict_timestamp` functions, a `#[cfg(any())]`
   function that never compiles, unused locator resolvers and `from_arg` parsers.
2. **Other unreachable code.** `QueryService::latest_session_messages`,
   `MessageQueryOptions::with_message_index_range`, `source::load_messages`,
   the identity function `decode_project_name`, `ChatCompletionsClient::from_env`,
   `RememberSelection::{Latest, Session}`, and store/capture helpers that neither
   the binary nor any test calls.
3. **Copy-paste in `src/cli.rs` (6,828 lines).**
   - `run_cli` dispatches 13 commands twice: an `if let` chain returns early,
     then a `match` repeats the same arms as unreachable code.
   - Eight `peer_*` client wrappers differ only in the peer subcommand name.
   - Three `annotate_peer_*` functions, two identical pagination helpers, two
     `read project` next-command builders.
   - The "fetch from each `--remote`, tag origin, dedup, sort, page" loop is
     written out seven times (read project/session/source, recall, and three
     summary/compact loaders), and remote context twice.
   - Local `context project|source` and the peer-server context responses build
     the same response in four places.
   - `SourceFilter` ↔ name mapping is hand-written in seven places across
     `cli.rs`, `cli/retrieval.rs`, `messages/{service,utils}.rs` and
     `teleport/pack.rs`.
4. **Coverage gaps on the code to change.** The peer server side (`mmr peer
   list-projects|…|recall`) is 0% covered; tests only fake its JSON. Local
   `context project|source` without `--remote`, and summarize/compact
   `session|source --remote`, are also uncovered.

Out of scope (recorded, not changed): the dream runner (`MockDreamRunner`,
`CommandDreamRunner`, `run_and_validate_dream`) and store dream/watch APIs are
unused by the binary but exercised by integration tests, so removing them is a
product decision, not a cleanup.

## Scope and invariants

- Surface: `src/cli.rs`, `src/teleport/**`, `src/messages/**`, `src/source/**`,
  `src/agent/**`, `src/types/**`, `src/store.rs`, `src/capture.rs`, tests.
- No CLI contract change: same commands, flags, JSON fields, field order,
  message order, exit codes, stderr hints and `next_command` strings.
- Tests are added before the code they protect changes. No existing test is
  weakened or deleted, except unit tests of deleted dead code.
- Library items used by integration tests stay.

## Validation plan

1. **Pin behaviour first.** Add `tests/peer_loopback_contract.rs`: a fake `ssh`
   runs the real `mmr peer …` against a second seeded `HOME`, so client and
   server peer code run end to end. Cover every `--remote` surface (list
   projects/sessions, read project/session/source, context project/source,
   recall with paging, summarize and compact project/session/source with a
   mock provider), asserting merged order, dedup, origins, `peer_results`,
   totals and `next_command`. Add local `context project|source` tests.
2. **Differential check.** Before and after the refactor, run one fixed command
   matrix with the old and the new binary on the same fixtures; outputs must be
   byte-identical.
3. **Full loop.** `cargo fmt --check`, `cargo test`, benchmarks
   (`cargo test --test cli_benchmark -- --ignored --nocapture`),
   `cargo clippy --all-targets --all-features -- -D warnings`,
   `cargo build --release`, and `nix build` (the package runs the suite in the
   sandbox).
4. Coverage after the change is reported next to the baseline.

## Definition of Done

- [ ] DoD-1 — New loopback and context tests pass on the unchanged code.
- [ ] DoD-2 — Dead code listed in Findings 1–2 is deleted; docs and skills no
  longer point at it.
- [ ] DoD-3 — Duplication listed in Finding 3 is collapsed into shared helpers.
- [ ] DoD-4 — Differential command matrix is byte-identical between old and new
  binaries.
- [ ] DoD-5 — Full verification loop and `nix build` pass.
- [ ] DoD-6 — PR open with line-count delta, coverage delta and evidence.

## Exit conditions

- `SCOPE-CHANGE` if a simplification would need a contract change; leave it
  out and record it here instead.
- `CONFIDENCE-STALL` if a differential mismatch cannot be explained.
