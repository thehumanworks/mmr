---
goal_id: "2026-11-12-remove-unnecessary-complexity"
title: "Remove unnecessary complexity without changing behaviour"
description: "Delete code the binary can no longer reach and collapse copy-pasted CLI remote/peer plumbing, after adding tests that pin the current behaviour."
date: "2026-11-12"
status: "done"
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

- [x] DoD-1 — New loopback and context tests pass on the unchanged code.
- [x] DoD-2 — Dead code listed in Findings 1–2 is deleted; docs and skills no
  longer point at it.
- [x] DoD-3 — Duplication listed in Finding 3 is collapsed into shared helpers.
- [x] DoD-4 — Differential command matrix is byte-identical between old and new
  binaries.
- [x] DoD-5 — Full verification loop and `nix build` pass.
- [x] DoD-6 — PR open with line-count delta, coverage delta and evidence.

## Evidence

- Commits: `04cc855` (tests first), `08b427c` (dead code), `dc61fd7`
  (duplication). Each passed `cargo fmt --check`, clippy `-D warnings` and
  `cargo test` on its own.
- Tests added: `tests/peer_loopback_contract.rs` (9 end-to-end `--remote`
  tests with a fake `ssh` that runs the real `mmr peer` against a second
  seeded `HOME`) and 4 tests in `tests/cli_contract.rs` (summarize/compact
  `--remote` transcripts for project/session/source, local `context
  project|source`, `MMR_DEFAULT_SOURCE` parsing). All passed on `main` before
  any source change.
- Differential check, old (`main`) vs new binary on fresh fixture copies:
  - 137 command cases (help, list, read, context, recall, summarize/compact
    failures, peer, share/import, store commands, cwd discovery, every
    `--remote` surface): 411 stdout/stderr/exit files, 404 byte-identical, 7
    equal after masking wall-clock timestamps and hash ids. Two runs of the
    old binary differ in the same 7 files.
  - 47 summarize/compact cases against a mock provider, including the request
    body sent to the provider: 188 files, all byte-identical.
- Size: `src/` 2,930 lines deleted, 852 added (net −2,078); `src/cli.rs`
  6,828 → 5,962 lines. Tests +934 lines.
- Line coverage (`cargo llvm-cov`): total 73.6% → 78.4%; `src/cli.rs`
  74.0% → 85.3%.
- Verification loop on the final code: `cargo fmt --check`, clippy
  `-D warnings`, `cargo test`, benchmarks, `cargo build --release` and
  `nix build` (the sandbox suite includes the new loopback tests) all pass.

Behaviour noted while pinning, deliberately not changed (contract):

- `read session X --remote` without `--project` sends the local cwd as the
  remote project, so the remote can return no messages.
- `--remote` reads page forward from the oldest merged message; local reads
  page from the newest.
- `context source --remote` keeps a session found on two machines twice;
  `context project --remote` keeps it once. Context `total_messages` counts
  duplicates.

## Exit conditions

- `SCOPE-CHANGE` if a simplification would need a contract change; leave it
  out and record it here instead.
- `CONFIDENCE-STALL` if a differential mismatch cannot be explained.
