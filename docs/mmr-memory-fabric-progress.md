# mmr Memory Fabric Progress

## Current State

- Branch: `codex/nhl-271-note`
- Last green commit: `b589318` (`add source adapter framework`)
- Active Linear ticket: NHL-271
- Completed tickets: NHL-268, NHL-269, NHL-270
- Current work: human-authored note ingestion

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
- Store implementation uses `rusqlite` with bundled SQLite and deterministic
  hash-derived ids.
- Event insertion is transaction-scoped; blob refs require explicit content
  hashes; cursor reads expose parser version and last event hash.
- Hidden dev smoke command `mmr __db-info` inspects DB path/schema version and
  can insert/read a synthetic event for isolated CLI verification.
- Source adapter framework is separate from existing raw-history loaders so raw
  retrieval contracts remain storage-free.
- Watcher emits complete-line byte deltas only; parsing and degraded warnings
  stay adapter-owned.
- `mmr note` requires the cwd project to be linked, writes source `note` events
  to the local store, and creates search document citations for later search.

## Verification Commands And Results

- `cargo fmt`: passed
- `cargo test`: passed, including 44 unit tests, 65 CLI contract tests, and
  `memory_fabric_contract` with 6 active tests passed and 14 pending tests
  ignored
- `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
  (`elapsed_ms=633` after NHL-269 fixes)
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo build --release`: passed
- Adversarial review found issues in `rg` stdout semantics, downstream gate
  specificity, malformed fixture coverage, and schema detail. Fixes are applied
  and verification was rerun successfully.
- NHL-269 targeted checks so far:
  - `cargo test store:: -- --nocapture`: passed, 7 store tests
  - `cargo test --test memory_fabric_contract -- --nocapture`: passed, 6 active
    tests and 14 pending ignored contracts
- Storage adversarial review found production transaction safety, blob hash,
  redaction policy FK, cursor metadata, and doc drift issues. Fixes are applied;
  verification was rerun successfully.
- NHL-270 targeted checks so far:
  - `cargo test capture:: -- --nocapture`: passed, 5 capture tests
  - `cargo test --test memory_fabric_contract -- --nocapture`: passed, 7 active
    tests and 13 pending ignored contracts
- Source adapter adversarial review found partial-tail replay, same-size rotation,
  session cohesion, fixture normalization, source version persistence, and
  reconciler reporting issues. Fixes are applied and verification was rerun
  successfully.
- Latest NHL-270 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 49 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 7 active tests passed and 13 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=647`)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed
- NHL-271 targeted checks:
  - `cargo test --test memory_fabric_contract note_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract note_requires_linked_project -- --nocapture`:
    passed
  - `cargo test store_api_covers_query_cursor_blob_and_rollback -- --nocapture`:
    passed
- Note source-neutrality/sync-risk review found event/search-document atomicity,
  eager raw-history loading, and local-project-id provenance risks. Fixes are
  applied and verification was rerun successfully.
- Latest NHL-271 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 50 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 9 active tests passed and 12 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=645`)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed

## Touched Files And Modules

- `docs/mmr-memory-fabric-mvp.md`
- `adrs/003-memory-fabric-mvp-architecture.md`
- `specs/README.md`
- `tests/memory_fabric_contract.rs`
- `tests/fixtures/memory_fabric/*.jsonl`
- `docs/mmr-memory-fabric-progress.md`
- `src/store.rs`
- `src/lib.rs`
- `src/cli.rs`
- `docs/mmr-memory-fabric-store.md`
- `Cargo.toml`
- `Cargo.lock`
- `src/capture.rs`
- `docs/mmr-source-adapters.md`
- `docs/mmr-note.md`

## Open Blockers

- None for NHL-271.

## Known Risks

- The remaining pending contract tests are intentionally ignored until downstream
  tickets implement the referenced adapter, redaction, search, summary, dream,
  and sync modules.
- GitHub transport is still deferred to NHL-277.
- NHL-269 locks the initial SQL schema, but future migrations must stay additive
  unless an ADR explicitly approves a breaking change.
- Public `link`, `sync`, and `status` are still deferred to NHL-277; `__db-info`
  is hidden dev-only smoke plumbing.
- Provider-specific Codex, Claude, and Cursor adapters are deferred to NHL-274,
  NHL-275, and NHL-276. NHL-270 only supplies the provider-neutral framework and
  fixture adapter.
- `mmr note` creates search documents, but public `mmr search`/`mmr rg` remains
  deferred to NHL-273.

## Next Exact Action

Commit and push NHL-271, update Linear with scope/tests/risks, then begin the
next unblocked MVP ticket.

## Do Not Redo

- Linear project and NHL-268 details have already been pulled.
- NHL-268 is already marked `Done` in Linear.
- NHL-269 is already marked `Done` in Linear.
- NHL-270 is already marked `Done` in Linear.
- NHL-271 is already marked `In Progress`.
- The dependency graph has already been reconciled with the Linear document.
- The explorer review has already been incorporated into the contract harness.

## Watch-Outs

- Keep stdout JSON-only for command output.
- Preserve current raw retrieval contracts while adding the store-backed MVP
  surface.
- Leave the untracked execution plan file untouched unless it is intentionally
  added to the delivery commit.
