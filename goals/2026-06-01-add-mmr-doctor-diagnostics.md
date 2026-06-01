---
title: "Add mmr doctor diagnostics"
description: "Add an evidence-first diagnostics surface for store, source, redaction, sync, and provider readiness issues."
date: 2026-06-01
status: proposed
---

# GOAL: Add `mmr doctor` Diagnostics

## Outcome

Add `mmr doctor` as a diagnostics and safe-repair command that complements
`mmr status`.

Default `mmr doctor` should be read-only and return structured JSON on stdout.
Human guidance belongs on stderr or in an optional Markdown/text output mode.
Any repair action must be explicit, scoped, and non-destructive by default.

Suggested command shape:

```bash
mmr doctor
mmr doctor --project /path/to/project
mmr doctor --fix safe
mmr doctor logs
mmr doctor bundle --to /tmp/mmr-diagnostics.zip
```

## Why

Entire's `doctor` is high value because it turns common operational failures
into inspectable, fixable states: stuck sessions, metadata branch issues, hook
trust, logs, and diagnostic bundles. `mmr` already has the raw data to diagnose
many similar problems, but today users must infer them from `status`, failed
commands, or source-specific docs.

## Surface Touched

- `src/cli.rs` command routing.
- New diagnostic module or existing status helpers.
- Store schema/status checks.
- Source root and source cursor checks.
- Redaction and sync readiness checks.
- Docs and contract tests.

## Diagnostic Checks

- Store exists, schema version matches expected, migrations are not partial.
- Current project is linked, aliases are unambiguous, cwd discovery is stable.
- Source roots exist where expected for selected sources.
- Import cursors point at readable files and parser versions are current.
- Malformed transcript tails are classified as active-write tolerant or broken.
- Redaction policy is present and latest runs are not stale or blocked.
- Sync remote is configured, reachable when requested, and not ahead/behind in a
  way that would surprise `mmr sync`.
- Summarizer configuration is explicit about missing `OPENAI_API_KEY` or model.
- Teleport cache/inbox paths are readable and not partially written.

## Non-Goals

- No automatic destructive cleanup.
- No agent hook installation.
- No provider-native branch rewrites.
- No cloud upload of diagnostic bundles.

## Validation Plan

- Add fixture stores for healthy, missing, stale schema, blocked redaction,
  partial sync, unreadable source root, malformed active tail, and ambiguous
  project alias.
- Assert default mode is read-only.
- Assert `--fix safe` only performs documented idempotent repairs.
- Assert `doctor bundle` redacts or omits raw transcript payloads by default.
- Run full repo verification.

## Definition of Done

- [ ] `mmr doctor` reports actionable checks as JSON with stable `check_id`s.
- [ ] `mmr doctor logs` and `doctor bundle` provide enough local evidence for
      bug reports without leaking raw transcript content by default.
- [ ] Safe fixes are explicit, idempotent, and fixture-backed.
- [ ] `mmr status` remains a compact state snapshot; `doctor` owns diagnosis.
- [ ] Full verification loop passes.
