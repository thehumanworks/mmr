# mmr Memory Fabric Progress

## Current State

- Branch: `codex/nhl-281-release-gate`
- Last green commit: pending current branch commit.
- Last verified state: NHL-281 working tree passed the full verification loop.
- Active Linear ticket: NHL-281
- Completed tickets: NHL-268, NHL-269, NHL-270, NHL-271, NHL-272, NHL-273, NHL-274, NHL-275, NHL-276, NHL-277, NHL-278, NHL-279, NHL-280, NHL-282
- Current work: finalize NHL-281 release-gate review, commit, push, and
  handoff.

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
- NHL-272 redaction uses deterministic local secret/PII scanners by default,
  records redaction runs/spans in SQLite, and exposes `mmr sync --dry-run` as a
  safety view before NHL-277 full remote sync.
- NHL-273 search uses generated local `search_documents` rows, literal substring
  matching by default, `--ignore-case` for case-insensitive matching, and JSON
  stdout except for explicit `mmr rg --line`.
- `mmr export --format tree` writes each run into a fresh `mmr-tree-*`
  subdirectory and omits local raw refs from exported files.
- NHL-274 imports Codex rollout JSONL through `CodexAdapter`, parser version
  `codex-rollout-v1`, scopes discovery to `session_meta.payload.cwd` matching
  the linked project, and keeps raw refs local-only while writing normalized
  events/cursors to the store.
- Codex `session_meta` uses cwd only for discovery; normalized event/search
  content omits absolute project paths.
- Tool result and unknown raw events require a future dedicated safe projection
  before remote sync eligibility, regardless of redaction status.
- NHL-275 imports Claude Code JSONL through `ClaudeAdapter`, parser version
  `claude-code-jsonl-v1`, scopes discovery to the first row cwd or decoded
  Claude project directory matching the linked project, and truncates large tool
  results with an explicit marker.
- NHL-276 imports Cursor agent transcript JSONL through `CursorAdapter`, parser
  version `cursor-agent-jsonl-v1`, supports nested `agent-transcripts` and flat
  JSONL layouts, and scopes discovery by cwd/workspace cwd or exact encoded
  Cursor project directory.
- Cursor tool-call projections sanitize local path segments before search
  indexing. Tool calls, tool results, and unknown raw events require a future
  dedicated safe projection before remote sync eligibility.
- NHL-277 adds `mmr link` as the first-run setup command for the current cwd
  project. It ensures the local store/project link, hydrates from the remote,
  reconciles available Codex/Claude/Cursor source roots, rebuilds search
  documents, syncs redacted projections, and prints JSON status.
- Full `mmr sync` now uses a file-backed GitHub-layout adapter with the public
  descriptor `github:<user>/mmr-store`. Tests set `MMR_FAKE_REMOTE_DIR`; the
  adapter writes immutable session-sharded event payloads, redacted search
  projections, and root-hash-addressed manifests.
- Full sync uses deterministic local PII redaction for syncable projections,
  blocks deterministic secrets, and continues to block tool calls, tool
  results, and unknown raw events until a dedicated safe projection exists.
- Fresh-host hydration replays redacted remote events into the local SQLite
  store and rebuilds usable search documents without exporting local raw refs,
  including when the receiving machine links the project at a different local
  path.
- Remote sync never exports local raw-derived event ids. Remote event ids are
  derived from redacted projections, existing remote payloads are compared
  before reuse, and hydration rejects payloads whose content hash or event id no
  longer matches the redacted JSON.
- `mmr status` reports remote-unavailable or remote-missing states when local
  rows are marked synced but the remote backing store is unavailable or missing
  expected event payloads.
- NHL-278 introduces `src/dream.rs` as the provider-neutral dream runner
  boundary. It defines strict structured output parsing, evidence-ref
  validation, runner config precedence, a deterministic mock runner, and a
  command runner adapter configured by `MMR_DREAM_COMMAND`.
- Dream evidence bundles are shared-safe by default: deterministic local PII is
  redacted and events with deterministic secret findings are omitted before any
  remote/API-style runner can see them. Raw evidence requires explicit local-only
  opt-in and is rejected for command/API runner kinds.
- Dream output evidence refs are validated against the submitted evidence bundle,
  not all project events. Command runners reject raw evidence requests even if a
  caller manually constructs one.
- NHL-279 wires the public `mmr dream` assimilation command. It supports
  project-scoped analysis, `--dry-run`, `--review`, `--runner`, `--model`,
  `--evidence-mode`, and `--allow-raw-evidence`.
- Mutating dream runs create a `dream_runs` audit row, persist internal
  `dream_candidates`, and write `learned_memory` only for high-confidence,
  non-sensitive, counterevidence-free claims with valid project-scoped evidence
  refs.
- Top-level counterevidence and per-claim counterevidence keep proposed memory
  pending instead of active. Sensitive/identity-like or PII-bearing claims are
  rejected.
- Active learned memory is discoverable through existing `search` results as
  `source: learned_memory`; no public learn/context/candidates/knowledge/promote
  or reject command was added.
- Sync uploads active learned-memory payloads only after remapping local evidence
  refs to redacted remote event refs. Remote learned-memory payloads are
  validated against the remote event set during hydration, then remapped to the
  local hydrated event refs.
- Plain dream `observations` are internal candidates/audit material and are not
  promoted directly to active learned memory. Active learned memory must come
  from `learned_memory_updates` or explicit `claims`.
- Replaying the same learned-memory claim/evidence tuple is idempotent: the
  existing row is preserved rather than overwritten by a later dream run.
- NHL-280 makes the lean MVP flow discoverable through command help, a
  quickstart/recovery guide, status diagnostics, and smoke-tested CLI examples.
- `mmr status` now reports local DB path/existence, schema version and expected
  version, linked project state, source-root availability, remote/auth status,
  privacy-filter coverage, continuity-provider readiness, dream-runner
  readiness, and consolidated recovery actions.
- `mmr summary` routes through the existing stateless continuity brief runner,
  while `remember` remains a compatibility alias.
- NHL-281 adds the final offline release gate:
  `mvp_release_gate_e2e_fixture_scenario` proves the blank non-Git path through
  fixture-backed source import, notes, raw retrieval, search, summary/remember,
  redaction blocking, dream assimilation, sync safety, and fresh HOME/store
  hydration.

## Verification Commands And Results

- `cargo fmt`: passed
- `cargo test`: passed, including 70 unit tests, 65 CLI contract tests, and
  `memory_fabric_contract` with 37 active tests passed and 0 ignored MVP
  contracts
- `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
  (`elapsed_ms=1008`)
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo build --release`: passed
- NHL-281 adversarial review found direct `link` proof, all-source raw
  retrieval, imported-source privacy coverage, exact summary citation,
  fresh-host evidence remapping, and optional dream-provider smoke gaps. Fixes
  are applied and verification was rerun successfully.
- NHL-280 UX review found summary/docs drift, noisy status actions,
  copy/paste recovery gaps, fixture-specific auth wording, and non-diagnostic
  store existence reporting. Fixes are applied and verification was rerun
  successfully.
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
- NHL-272 implementation is in progress; source-filtered and read-only dry-run
  fixes from adversarial review are applied. Follow-up review found no blockers.
  Full verification is green.
- Latest NHL-272 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 56 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 10 active tests passed and 11 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=800` after the follow-up import-warning sanitization fix)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed
- NHL-273 focused checks:
  - `cargo test --test memory_fabric_contract rg_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract search_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract search_document_contract_is_implemented -- --nocapture`:
    passed
- Search/citation adversarial review found stale tree export files, raw local ref
  leakage, `search --line` ambiguity, and colon-delimited `rg --line` parsing
  issues. Fixes are applied and verification was rerun successfully.
- Latest NHL-273 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 57 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 13 active tests passed and 8 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=819`)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed
- NHL-274 focused checks:
  - `cargo test --test memory_fabric_contract codex_importer_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract codex_import_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract codex_active_session_watcher_uses_complete_rows_only -- --nocapture`:
    passed
  - `cargo test import_command_parses_with_global_source_after_subcommand -- --nocapture`:
    passed
  - `cargo test tool_results_need_safe_projection_even_after_passing_redaction -- --nocapture`:
    passed
- Codex importer adversarial review found project-scope leakage, session-id
  drift from response/tool payload IDs, absolute cwd content leakage,
  partial-tail cursor consumption, and raw tool-output sync risk. Fixes are
  applied and verification was rerun successfully.
- Latest NHL-274 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 59 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 16 active tests passed and 8 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=727`)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed
- NHL-275 focused checks:
  - `cargo test --test memory_fabric_contract claude_importer_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract claude_import_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract claude_active_session_watcher_uses_complete_rows_only -- --nocapture`:
    passed
  - `cargo test import_command_parses_with_global_source_after_subcommand -- --nocapture`:
    passed
- Claude importer adversarial review found raw unknown-row metadata leakage,
  missing handling for `queue-operation`/`attachment`/`file-history-snapshot`,
  lossy hyphenated project-directory fallback, irreversible large tool-result
  truncation, and silent missing-content rows. Fixes are applied and
  verification was rerun successfully.
- Latest NHL-275 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 59 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 19 active tests passed and 8 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=807`)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed
- NHL-276 focused checks:
  - `cargo test --test memory_fabric_contract cursor_importer_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract cursor_import_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract cursor_active_session_watcher_uses_complete_rows_only -- --nocapture`:
    passed
  - `cargo test tool_results_need_safe_projection_even_after_passing_redaction -- --nocapture`:
    passed
- Cursor importer adversarial review found real Cursor project aliases without
  a leading dash, local path leakage through tool-call arguments, and undefined
  direct flat-root behavior. Fixes are applied and verification was rerun
  successfully.
- Latest NHL-276 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 59 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 22 active tests passed and 8 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=739`)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed
- NHL-277 focused checks:
  - `cargo test --test memory_fabric_contract -- --nocapture`: passed, 26
    active tests and 4 pending ignored contracts
  - `cargo test --test memory_fabric_contract link_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract sync_cli_contract_is_implemented -- --nocapture`:
    passed
  - `cargo test --test memory_fabric_contract sync_manifest_contract_is_implemented -- --nocapture`:
    passed
- NHL-277 adversarial review found local resync duplication through redacted
  remote ids, raw-derived `original_event_id` leakage, missing remote integrity
  checks, stale `status` reporting when the remote disappears, and minor local
  path exposure in status JSON. Fixes are applied for the blockers/high issues
  and remote root/source-root path exposure; local store/project paths remain in
  local status JSON as intentional diagnostics.
- Latest NHL-277 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 59 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 26 active tests passed and 4 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=726`)
  - `cargo clippy --all-targets --all-features -- -D warnings`: passed
  - `cargo build --release`: passed
- NHL-278 focused checks so far:
  - `cargo test dream:: -- --nocapture`: passed, 9 dream runner unit tests
  - `cargo test --test memory_fabric_contract dream_runner_contract_is_implemented -- --nocapture`:
    passed
- NHL-278 adversarial review found output validation against all project events
  instead of the submitted evidence bundle, possible raw evidence leakage if a
  command runner received a manually built raw request, ambiguous command-env
  argv semantics, reserved best-of/retry knobs without behavior, and silent
  `claims` discard.
  Fixes are applied and verification was rerun successfully.
- Latest NHL-278 full verification:
  - `cargo fmt`: passed
  - `cargo test`: passed, including 65 unit tests, 65 CLI contract tests, and
    `memory_fabric_contract` with 27 active tests passed and 4 pending ignored
    contracts
  - `cargo test --test cli_benchmark -- --ignored --nocapture`: passed
    (`elapsed_ms=829`)
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
- `src/sync.rs`
- `docs/mmr-memory-fabric-store.md`
- `docs/mmr-memory-fabric-quickstart.md`
- `docs/mmr-memory-fabric-release-gate.md`
- `Cargo.toml`
- `Cargo.lock`
- `src/capture.rs`
- `docs/mmr-source-adapters.md`
- `docs/mmr-note.md`
- `src/redaction.rs`
- `docs/mmr-redaction.md`
- `docs/mmr-search.md`
- `docs/mmr-codex-importer.md`
- `docs/mmr-claude-importer.md`
- `docs/mmr-cursor-importer.md`
- `src/dream.rs`
- `docs/mmr-dream-runner.md`

## Open Blockers

- None for NHL-281.

## Known Risks

- MVP memory-fabric contract tests are active; NHL-280 removed the pending
  summary/remember compatibility ignores.
- NHL-269 locks the initial SQL schema, but future migrations must stay additive
  unless an ADR explicitly approves a breaking change.
- NHL-277 ships a file-backed GitHub-layout adapter for deterministic local/E2E
  verification. Live GitHub API transport remains an optional future hardening
  path; the MVP happy path still uses descriptor `github:<user>/mmr-store`.
- `__db-info` remains hidden dev-only smoke plumbing.
- NHL-274 supplies the Codex adapter, NHL-275 supplies the Claude adapter, and
  NHL-276 supplies the Cursor adapter on top of the NHL-270 provider-neutral
  framework.
- The optional `openai/privacy-filter` model runtime is not bundled; redaction
  reports degraded PII coverage while deterministic blocking remains active.
- Under degraded PII coverage, `sync --dry-run` treats every event as blocked
  and omits payload previews so false negatives cannot leak through dry-run JSON.
- Full sync must continue to evaluate active policy coverage instead of
  treating `events.sync_status = "redacted"` alone as sufficient upload
  permission.
- False-positive allowlist and hard-purge flows are documented as explicit
  future policy surfaces, not silent MVP behavior.
- `mmr export --format tree` writes local raw search material for external
  tools into a fresh run directory and requires explicit `--output-dir`; it is
  not a remote sync path.
- Codex parser drift is handled by preserving unknown rows as local
  `unknown_raw_event` records where possible and emitting warnings for malformed
  JSONL rows.
- Codex import remains conservative: sessions without a matching cwd are skipped
  instead of imported into the wrong project. Future alias support can broaden
  matching intentionally.
- Claude import sanitizes unknown/provider-metadata rows instead of indexing raw
  JSON. Large tool results store bounded projections plus omitted character
  count and full-content hash.
- Cursor import accepts current no-leading-dash Cursor project aliases, legacy
  dash aliases, and direct custom flat roots. Cursor tool-call path-like content
  is sanitized in normalized/search text.
- `mmr dream` uses the deterministic mock runner by default for offline local
  use. Real provider behavior is available through the local `command` runner
  configured with `MMR_DREAM_COMMAND`.
- Pending/rejected dream candidates remain internal for MVP. There is
  intentionally no public candidate review/promote/reject surface.
- Learned-memory sync uploads active rows only; pending/rejected/superseded rows
  remain local until a future governance ticket defines remote behavior.

## Next Exact Action

Finish the NHL-281 adversarial review, commit and push
`codex/nhl-281-release-gate`, then update Linear NHL-281 with the verification
results and mark it Done.

## Do Not Redo

- Linear project and NHL-268 details have already been pulled.
- NHL-268 is already marked `Done` in Linear.
- NHL-269 is already marked `Done` in Linear.
- NHL-270 is already marked `Done` in Linear.
- NHL-271 is already marked `Done` in Linear.
- NHL-272 is already marked `Done` in Linear.
- NHL-273 is already marked `Done` in Linear.
- NHL-274 is already marked `Done` in Linear.
- NHL-275 is already marked `Done` in Linear.
- NHL-276 is already marked `Done` in Linear.
- NHL-277 is committed, pushed, commented in Linear, and marked `Done`.
- NHL-278 is committed, pushed, commented in Linear, and marked `Done`.
- NHL-279 is committed, pushed, commented in Linear, and marked `Done`.
- The dependency graph has already been reconciled with the Linear document.
- The explorer review has already been incorporated into the contract harness.
- The Codex importer adversarial review findings have already been fixed and
  verified.
- The Claude importer adversarial review findings have already been fixed and
  verified.
- The Cursor importer adversarial review findings have already been fixed and
  verified.
- The link/sync/status adversarial review findings have already been fixed and
  verified.
- The dream runner adversarial review findings have already been fixed and
  verified.
- The dream assimilation adversarial review findings have already been fixed and
  verified: sensitive `kind` values are blocked before local application/remote
  sync, duplicate learned memory cannot silently overwrite prior rows, plain
  observations are not directly promoted to active learned memory, top-level
  counterevidence keeps memory pending, project-scoped evidence is validated,
  remote hydration binds learned-memory refs to the remote event set, active-only
  learned-memory sync is implemented, and no public reserved execution flags are
  exposed.

## Watch-Outs

- Keep stdout JSON-only for command output.
- Preserve current raw retrieval contracts while adding the store-backed MVP
  surface.
- Leave the untracked execution plan file untouched unless it is intentionally
  added to the delivery commit.
