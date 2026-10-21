---
goal_id: "2026-10-21-extensible-history-adapters"
title: "Register trusted history adapters without core parser edits"
description: "Expose adapter mapping and versioned registry APIs while preserving built-in v1 conversion bytes."
date: "2026-10-21"
status: "done"
confidence_floor: 90
created: "2026-10-21"
updated: "2026-10-21"
---

# Goal: Let a separately compiled Rust consumer register a third session-history adapter without editing core parsing matches.

## Invariants

- Preserve current Codex/Claude v1 canonical/readable byte outputs, IDs, default CLI behavior and native/store paths.
- Common engine owns archives, hashes, raw spans, sequence, inherited identity, event IDs, resource budgets and tool correlations.
- Extensions are explicitly registered trusted Rust code, not executable/plugin discovery or a sandbox. Imported text cannot grant authority.
- Canonical reimport requires the registered source plus exact adapter version and fails on missing/mismatched/duplicate registrations.
- No source histories beyond synthetic/public fixtures, credentials, external models, background jobs, store changes, pushes, PR updates or merges.

## Definition of Done

- [x] DoD-1 — Public adapter/registry API accepts an external third adapter without new core parsing matches — verify by: external integration crate tests.
- [x] DoD-2 — Version/registration/capability consistency fails closed and common archival/budget/null/attachment/tool invariants remain intact — verify by: adapter conformance tests and existing session-conversion suite.
- [x] DoD-3 — Six pre-change built-in conversion fingerprints remain byte-identical — verify by: compatibility fixture assertions.
- [x] DoD-4 — Full fmt/test/ignored-bench/strict-Clippy/release gate and independent review pass — verify by: `../audit/verify.py --repo . --tag adapters` logs and reviewer evidence.

## Tasks

### T1 · Freeze minimal API contract and capture compatibility evidence · [x]

Publish design/spec, save pre-change outputs, add meaningful external-crate tests. **Confidence:** 96 / 90.

### T2 · Refactor only source-specific mapping behind registry · [x]

Expose trusted projection interface and registry-aware parse/convert wrappers; preserve common engine checks and default CLI. **Confidence:** 96 / 90.

### T3 · Verify, independently review and commit · [x]

Run targeted and full gates, document compatibility/trust limits and report the focused local commit. **Confidence:** 95 / 90.

## Exit Conditions

- DONE — DoD/tasks verified with confidence >=90.
- SCOPE-CHANGE — existing store/native/session wire compatibility cannot be preserved.
- BLOCKED-DEP — required tooling unavailable.

## Evidence

- 2026-10-21 — Baseline central parser matches Codex/Claude for session identity, content selection and block mapping; the existing adapters module is private.
- 2026-10-21 — Parent approved the frozen goal/spec before source edits. `tests/fixtures/session_conversion/v1-compatibility-hashes.json` captured six pre-extension response hashes at base `9c72eeb`; all remain unchanged in the external-crate conformance suite.
- 2026-10-21 — `cargo test --test history_adapter_extensibility --test session_conversion`: six new external-crate tests plus 14 existing conversion tests passed. Synthetic third-adapter mapping exercises exact raw bytes, explicit null vs absence, unknown source fields/opaque content, reference-only attachments, reverse-order parallel tool results, append/origin identities, malformed/truncated rows and canonical reimport. Failure checks cover absent/mismatched/duplicate registration, forbidden native/API claims, unsupported/unknown canonical kinds, invalid availability, forged relationships, metadata and materialization bounds.
- 2026-10-21 — `/usr/bin/python3 ../audit/verify.py --repo . --tag adapters` passed formatting, 387 tests, four explicitly run ignored benchmarks, strict all-target/all-feature Clippy and release build. Evidence: `../audit/evidence/adapters-verification.json` and `adapters-*.log`. Release retains the existing local rust-objcopy/libLLVM stripping warning, exit 0; no release performance improvement is claimed.
- 2026-10-21 — First full test attempt failed at unchanged `mcp_stdio_reuses_existing_rest_backing`; preserved in `../audit/evidence/adapters-test.log.first-attempt` and `adapters-verification.json.first-attempt`. The exact test passed alone (`adapters-mcp-recheck-test.log`) and all tests passed on the subsequent full gate. No baseline reproduction was performed and no pre-existing-defect claim is made.
- 2026-10-21 — Independent reviewer compiled a separate synthetic Rust consumer with repository-lock dependency versions. Nine checks passed, including deterministic restart/reimport and contradictory capability/projection rejection; final static review reported no remaining findings. Evidence: `../audit/evidence/adapter-independent-probes.json`. Review-requested canonical-kind, attachment-availability and projection-preflight guardrails are included.
- 2026-10-21 — AGENTS and operational guide document explicit trusted registration/version pinning, raw retention, unsupported-content handling, unchanged builtins-only CLI, false native/API claims and Rust `NativeSource::Custom`/non-Copy compatibility. Existing source loaders, store, teleport and root CLI files have no diff. `git diff --check` passed.
- 2026-10-21 — The global GDD skill/status script remains unavailable after prior targeted searches; parent explicitly authorized manual eligibility. Manually checked frontmatter, scoped DoD, task confidence and linked evidence against repository examples. All required implementation verification gates ran.

## Decisions

- 2026-10-21 — Keep schema1 shape and built-in `parser_version=mmr-session-v1`; an external adapter records its own version in that existing field.
- 2026-10-21 — NativeSource gains an extensible string identifier while retaining old serialized strings and enum constructors. It can no longer implement Rust Copy.
