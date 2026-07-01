---
goal_id: "2026-07-01-source-filtered-provider-loading"
title: "Load only selected providers"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: Commands with an explicit or default source filter load only the selected provider history.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Keep stdout machine-readable JSON and diagnostics on stderr.
- Preserve `--source` and `MMR_DEFAULT_SOURCE` semantics across list/read/context/summarize/retrieve.
- Do not hide errors for the selected provider.
- For all-source commands, preserve current all-provider behavior unless explicitly changed by a DoD.
- Full `cargo test` is expected before DONE; coordinate with the test-gate goal if it is still blocked.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — review finding source and reproduction summary.
- `src/cli.rs:1109` — `run_cli` eagerly constructs `QueryService::load`.
- `src/messages/service.rs:150` — `QueryService::load` calls all-source loading.
- `src/source/mod.rs:21` — `load_messages` loads Codex, Claude, Cursor, Grok, and Pi in parallel.
- `tests/cli_contract.rs` — source-filter CLI contract tests.
- `tests/mcp_contract.rs` — MCP list/read source-filter parity.
- `.cursor/rules/cli-contract.mdc` — source filtering and JSON contract rules.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — `mmr --source codex list projects` succeeds when an unrelated Claude fixture is malformed, and still fails when the Codex fixture is malformed — *verify by:* `cargo test --test cli_contract source_filtered_commands_ignore_unselected_provider_parse_errors -- --exact --nocapture`
- [ ] **DoD-2** — `MMR_DEFAULT_SOURCE=codex mmr list projects` uses the same filtered loading behavior as explicit `--source codex` — *verify by:* `cargo test --test cli_contract default_source_filters_provider_loading -- --exact --nocapture`
- [ ] **DoD-3** — all-source commands still surface provider parse errors with enough context to diagnose the failing source file — *verify by:* `cargo test --test cli_contract all_source_commands_report_provider_parse_errors -- --exact --nocapture`
- [ ] **DoD-4** — source-filtered read/context/summarize/retrieve/MCP contract tests pass — *verify by:* `cargo test --test cli_contract source_ retrieve_filters_apply_source_env_session_role_event_and_context -- --nocapture && cargo test --test mcp_contract`
- [ ] **DoD-5** — Repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — fixture temp HOME or Rust/Cargo is unavailable after one retry.
- **`BLOCKED-TEST-GATE`** — only the separately tracked memory-fabric gate prevents full `cargo test` after targeted source-filter tests pass.
- **`SCOPE-CHANGE`** — the fix requires changing public `--source`, `MMR_DEFAULT_SOURCE`, or MCP source-filter response semantics.
- **`CONFIDENCE-STALL`** — filtered loading cannot be threaded without ambiguous command-source precedence after 3 focused attempts.
- **`BUDGET`** — more than 2 full verification-loop attempts after targeted source-filter tests are green.

---

## 5. Tasks · INVARIANT

### T1 · Add source-filter loading regressions · [ ]

**Steps**
- [ ] Add malformed unselected-provider fixtures for explicit `--source`.
- [ ] Add matching malformed selected-provider coverage to prove selected errors still fail.
- [ ] Add `MMR_DEFAULT_SOURCE` coverage for the same behavior.
- [ ] Add all-source coverage that preserves current diagnostic failure.

**Verification Contract**
- *Check:* source filtering affects provider loading, not only post-load filtering.
- *Method:* `cargo test --test cli_contract source_filtered_commands_ignore_unselected_provider_parse_errors default_source_filters_provider_loading all_source_commands_report_provider_parse_errors -- --nocapture`
- *Expected:* all named tests pass after implementation.
- *BDD scenarios covered:* Given a corrupt Claude file and `--source codex`, the command succeeds; given a corrupt Codex file and `--source codex`, the command fails.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Thread source filters into provider loading · [ ]

**Steps**
- [ ] Add a filtered loader path in `src/source/mod.rs`.
- [ ] Change `QueryService::load` or add `QueryService::load_with_source_filter`.
- [ ] Ensure command routing passes the effective filter after CLI/env precedence is resolved.
- [ ] Keep all-source loading unchanged for unfiltered commands.

**Verification Contract**
- *Check:* selected-provider commands never invoke unselected provider parsers.
- *Method:* `cargo test --test cli_contract source_filtered_commands_ignore_unselected_provider_parse_errors default_source_filters_provider_loading -- --nocapture`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given `MMR_DEFAULT_SOURCE=codex`, when listing projects, then only Codex source history is loaded.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Verify source-filter parity across surfaces · [ ]

**Steps**
- [ ] Run source-related CLI contract tests.
- [ ] Run retrieve source-filter tests.
- [ ] Run MCP source-filter tests.
- [ ] Run full verification loop or exit on the separate test-gate blocker.

**Verification Contract**
- *Check:* filtered loading preserves public behavior across CLI, retrieve, and MCP.
- *Method:* `cargo test --test cli_contract source_ retrieve_filters_apply_source_env_session_role_event_and_context -- --nocapture && cargo test --test mcp_contract && cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands, or explicit `BLOCKED-TEST-GATE` if only that known suite blocker remains.
- *BDD scenarios covered:* Given each public source surface, when a source filter is active, then output and failure modes stay consistent.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-4, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: the goal must not swallow selected-provider failures. Added selected-provider and all-source error DoD to prevent over-broad error suppression. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
