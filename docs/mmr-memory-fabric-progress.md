# mmr Memory Fabric Progress

## Current State

- Branch: `codex/nhl-268-mmr-mvp-contract`
- Last green commit: `7dbcab4` baseline, not yet verified in this session
- Active Linear ticket: NHL-268
- Completed tickets: none
- Current work: architecture/schema/verification contract for the Memory Fabric
  MVP

## Current Architecture Decisions

- Local SQLite/libSQL-shaped storage is canonical for active work.
- GitHub `github:<authenticated-user>/mmr-store` is the first remote/export
  adapter, not the hot database.
- `projects`, `sessions`, `messages`, and `export` remain raw retrieval
  surfaces.
- `summary` replaces `remember`; `remember` remains a compatibility alias for
  the MVP.
- `dream` is the only public stateful learned-memory assimilation command.
- Redaction runs before sync by default and blocks sync on unresolved secrets.
- Learned memory must carry resolvable evidence refs.
- `mmr rg` preserves JSON stdout by default; any line-oriented POSIX mode must be
  explicit opt-in.

## Verification Commands And Results

- `cargo fmt`: passed
- `cargo test`: passed, including `memory_fabric_contract` with 2 active tests
  passed and 16 pending tests ignored before adversarial fixes; after fixes, 3
  active tests passed and 16 pending tests ignored
- `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
  (`elapsed_ms=644` after adversarial fixes)
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo build --release`: passed
- Adversarial review found issues in `rg` stdout semantics, downstream gate
  specificity, malformed fixture coverage, and schema detail. Fixes are applied
  and verification was rerun successfully.

## Touched Files And Modules

- `docs/mmr-memory-fabric-mvp.md`
- `adrs/003-memory-fabric-mvp-architecture.md`
- `specs/README.md`
- `tests/memory_fabric_contract.rs`
- `tests/fixtures/memory_fabric/*.jsonl`
- `docs/mmr-memory-fabric-progress.md`

## Open Blockers

- None for NHL-268.

## Known Risks

- The pending contract tests are intentionally ignored until downstream tickets
  implement the referenced store, adapter, redaction, search, summary, dream,
  and sync modules.
- The exact SQLite/libSQL crate and GitHub transport are deferred to NHL-269 and
  NHL-277.
- NHL-269 still owns exact SQL types, constraints, indexes, and migration
  implementation.

## Next Exact Action

Update NHL-268 in Linear with scope, tests, commands, risks, and the next ticket;
then begin NHL-269 storage and migrations.

## Do Not Redo

- Linear project and NHL-268 details have already been pulled.
- NHL-268 is already marked `In Progress`.
- The dependency graph has already been reconciled with the Linear document.
- The explorer review has already been incorporated into the contract harness.

## Watch-Outs

- Keep stdout JSON-only for command output.
- Preserve current raw retrieval contracts while adding the store-backed MVP
  surface.
- Leave the untracked execution plan file untouched unless it is intentionally
  added to the delivery commit.
