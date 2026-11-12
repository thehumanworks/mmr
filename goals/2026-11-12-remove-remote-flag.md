---
goal_id: "2026-11-12-remove-remote-flag"
title: "Remove the --remote flag and its peer read endpoints"
description: "Delete --remote from every read, context, recall, summarize and compact command, with the client merge code and the eight peer read endpoints behind it; keep import session --from and peer status."
date: "2026-11-12"
status: "in-progress"
confidence_floor: 85
created: "2026-11-12"
updated: "2026-11-12"
---

# Goal

`mmr` no longer offers `--remote <ssh-target>`. Tomas judged it weak on
2026-11-12: the scenario it serves (seeing another machine's agent history) is
better solved by shared cloud storage. The coordinator relayed that as a
request to open a removal PR for Tomas to review; the PR is not merged without
Tomas's go-ahead.

## Scope

Remove:

- The `--remote` flag on `list projects|sessions`, `read project|session|source`,
  `context project|source`, `recall`, `summarize project|session|source` and
  `compact project|session|source` (14 commands), and the `remote` parameter of
  the MCP compact tools.
- The client code that fans out to peers and merges results (dedup, origin
  tags, `peer_results`, remote paging and `next_command` builders).
- The hidden peer endpoints `mmr peer list-projects|list-sessions|read-session|
  read-project|read-source|context-project|context-source|recall`, their request
  types, and their entries in `peer status` capabilities.
- The `origin` and `peer_results` JSON fields. Local commands never emit them
  (both are `None` and skipped), so local output does not change.
- Docs and tests that describe `--remote`.

Keep: `import session --from <ssh-target>` (peer `teleport-pack`), `share
session --to`, `peer status [--host]`, SSH target parsing and hardening, and
peer project matching (used by `teleport-pack`). The peer protocol version
stays 1, because the `teleport-pack` request does not change.

## Contract change

- `--remote` becomes an unknown argument: clap usage error, exit 2.
- `mmr peer status` lists only `teleport-pack` in `capabilities`.
- An older `mmr` that calls a removed endpoint on a newer peer gets the
  existing "peer subcommand missing" structured failure.
- Every command without `--remote` keeps byte-identical output.

## Validation plan

1. Tests first, on the unchanged code:
   - an end-to-end loopback test of `import session --from` (read-only and
     `--apply`) and `peer status --host`, through a fake `ssh` that runs the
     real `mmr peer` against a second seeded HOME;
   - a test that every one of the 14 commands rejects `--remote` (fails before
     the change, passes after).
2. Old-vs-new differential check with the PR #31 matrices: every case without
   `--remote` is byte-identical (after the known timestamp/hash masking); every
   `--remote` case becomes the same clap usage error.
3. Full loop: `cargo fmt --check`, `cargo test`, benchmarks, clippy
   `-D warnings`, `cargo build --release`, `nix build`.

## Definition of Done

- [x] DoD-1 — Loopback `import session --from` / `peer status --host` tests pass
  on the unchanged code.
- [x] DoD-2 — `--remote`, its client code, the eight peer read endpoints, the
  MCP `remote` parameter and the `origin`/`peer_results` fields are removed.
- [x] DoD-3 — Docs and skills no longer describe `--remote`.
- [x] DoD-4 — Differential check: non-remote cases identical; remote cases are
  usage errors.
- [ ] DoD-5 — Full verification loop and `nix build` pass.
- [ ] DoD-6 — PR open for Tomas's review, with a note on what shared cloud
  storage would take.

## Evidence

- DoD-1: `tests/peer_import_loopback_contract.rs` (4 tests) passed on the
  unchanged code (commit f38bfbf) and still passes. The peer status `--host`
  failure tests moved off `read --remote` in aec357b.
- DoD-2: `remote_flag_is_rejected_on_every_command` failed on the old code and
  passes now. `summarize_and_compact_send_each_scope_transcript` replaces the
  remote transcript test and passed on the old code first.
- DoD-3: `specs/session-sharing.md`, `docs/mmr-session-sharing*.md`,
  `docs/mmr-command-taxonomy.md`, the generated `docs/api/*` and two skills no
  longer describe `--remote`.
- DoD-4: the 137-case matrix (411 files) against the main binary: 105 cases
  identical (7 files equal after timestamp/hash masking); 27 `--remote` cases
  become `unexpected argument '--remote'` with exit 2; the other 5 changes are
  intended: two help texts lose the `--remote` lines, `peer status` and `peer
  status --host` list only `teleport-pack`, and `peer read-project` is now an
  unrecognized subcommand. The 47-case summarize/compact matrix: 25 cases
  byte-identical, including the provider request bodies; all 22 `--remote`
  cases become the same usage error.

## Exit conditions

- `SCOPE-CHANGE` if removing `--remote` would change any command that does not
  use it.
- `CONFIDENCE-STALL` if a non-remote differential mismatch cannot be explained.
