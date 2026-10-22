---
goal_id: "2026-10-22-read-only-file-handoffs"
title: "Make portable session reading the default import workflow"
description: "Document file handoffs and require explicit native apply using existing bundles."
date: "2026-10-22"
status: "done"
confidence_floor: 90
created: "2026-10-22"
updated: "2026-10-22"
---

# Goal

Make a selected session easy to carry between hosts as an existing `.mmr` JSON
bundle and read in any agent without SSH or native-format conversion. Simplify
the recommended workflow instead of adding another tool or storage layer.

## Scope and invariants

- Both `import bundle` and `import session` default to the existing read/cache
  path. Native restoration requires `--apply`; `--force` requires `--apply`.
  Explicit `--read-only` remains valid; conflicting modes remain errors.
- Keep native collectors, artifact hashes, response shapes, explicit apply,
  source selection and session contents. Validate peer bundles before caching.
- Explain `.mmr` JSON, what gets cached, source project provenance, migration,
  optional SSH, and the file-backed sync simulation. No canonical-message
  mandate, archive service, cloud credentials, aliases or language rewrite.
- Test only synthetic fixtures in temporary homes/projects; preserve user data.

## Definition of Done

- [x] DoD-1 — CLI help/spec/docs/agent skill describe read-only defaults and
  explicit apply migration; verify by targeted import/help/skill tests and review.
- [x] DoD-2 — File export/copy/import roundtrip preserves Codex/Claude/Pi messages
  across homes/projects without native writes; explicit apply still works;
  verify by portable bundle contract tests and existing import matrix.
- [x] DoD-3 — Corrupt checksums, partial inboxes, truncated inputs, traversal and
  colliding native files cannot cause unintended writes; verify by synthetic
  portable bundle tests and existing provider/apply tests.
- [x] DoD-4 — Formatting, full tests, four ignored benchmarks, strict Clippy,
  release build, diff checks and independent review pass; publish a separate PR.

## Tasks

### T1 · Specify and implement portable read-only handoffs · [x]

Update spec first, add focused contracts, reuse existing importer and validators,
reduce recommended docs/skill workflows, verify and obtain independent review.

**Verification Contract:** all DoD checks pass. **Confidence:** 98 / 90.

## Exit Conditions

- DONE — all checks and review passed.
- BLOCKED-DEP — required execution or tooling unavailable.
- SCOPE-CHANGE — a new transport, storage engine or provider conversion is needed.

## Evidence

- 2026-10-22 — Based independently on main
  `125bdbf3942bd5c8296c181d1763dc1f0d1358b2`; baseline full gates passed in first
  iteration. No unrelated changes. Existing apply/read-only file paths verified.

## Decisions

- 2026-10-22 — Reading portable context should not require installing native
  provider histories. Reuse `read_bundle`; existing scripts that require native
  restoration must add `--apply`. SSH share already sends explicit `--apply`.
- 2026-10-22 — Preserve JSON bundle version and collectors. File-backed
  FakeGithubRemote is a simulation, not a hosted synchronization service.
- 2026-10-22 — Global GDD checker unavailable as recorded by the previous goal;
  manually verify goal structure, evidence and confidence before closeout.

- 2026-10-22 — New default-mode contracts failed against the old apply-by-default
  implementation. Corrected test checksum formatting and existing Markdown JSON
  expectations; five portable bundle contracts pass across Codex/Claude/Pi homes.
- 2026-10-22 — Twelve targeted import contracts pass, including fake-peer
  integrity/path checks before cache writes and unchanged explicit apply.
- 2026-10-22 — Full verification with cleared environment and synthetic HOME:
  `cargo fmt --check`, `cargo test --locked` (360 passed), four ignored benchmarks,
  strict all-target/all-feature Clippy and release build all passed. Cargo 1.97.1.
  Logs: `../evidence/portable-*.log`. No private histories or live peer/model calls.
- 2026-10-22 — Independent reviewer approved after clarifying structured stdout
  errors and the default-source environment setting. The full gate initially
  caught a skill opening-wording assertion; retained that identity sentence,
  then reran the targeted skill test and entire gate successfully.
- 2026-10-22 — `git diff --check` clean. Manually verified goal evidence and
  confidence because the global GDD checker is unavailable. Partial-write tests
  cover incomplete inbox publication; native apply/cache write atomicity is not
  changed or claimed by this iteration.
