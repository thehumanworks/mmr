---
goal_id: "2026-10-21-canonical-session-conversion"
title: "Preserve portable session history with explicit conversion losses"
description: "Add isolated local-file conversion for Codex and Claude histories without changing native replay or storage."
date: "2026-10-21"
status: "done"
confidence_floor: 90
created: "2026-10-21"
updated: "2026-10-21"
---

# Goal: Convert explicit session files into verified canonical history and honest readable harness projections.

## Invariants

- Keep native archival bytes distinct from canonical events and derived memory.
- Preserve original UTF-8 history once with a hash and byte spans. Never execute imported content, fetch attachments, load live provider roots, call models or mutate provider/store files.
- Existing source loaders, SQLite schema, native bundle sharing/apply/resume and response types remain unchanged.
- Readable Codex/Claude output is never advertised as a resumable native session or validated API replay.
- CLI stdout remains one JSON object. Explicit origin is required for native input; canonical reimport retains its origin. No pagination or next-command behavior changes.

## Definition of Done

- [x] DoD-1 — Versioned archive and event/block/correlation data survive canonical reimport and process restart, with tampering/version errors — verify by: `cargo test --test session_conversion`.
- [x] DoD-2 — Codex/Claude adapters retain opaque, malformed and attachment records and produce structured conversion findings — verify by: `cargo test --test session_conversion`.
- [x] DoD-3 — CLI conversion reads only its explicit input, emits one JSON response and writes no provider/store files — verify by: isolated fixture CLI tests and `cargo test --test cli_contract`.
- [x] DoD-4 — Full formatting/test/ignored-benchmark/strict-Clippy/release gate passes — verify by: `../audit/verify.py --repo . --tag conversion` with synthetic loopback fixtures.

## Tasks

### T1 · Freeze contract and fixture cases · [x]

Write `specs/session-conversion.md` before code. Add native fixtures and discriminating conformance/restart/tamper tests. **Verification Contract:** all named preservation and explicit-loss assertions pass. **Confidence:** 96 / 90.

### T2 · Implement standalone adapters and CLI · [x]

Add conversion types/logic and handle the CLI before QueryService loads histories. Document privacy, origin identity and readable-only capability. **Verification Contract:** no source/store/native replay changes; fixture tests pass. **Confidence:** 96 / 90.

### T3 · Verify and review · [x]

Run targeted and full gates, inspect the diff, and hand off the focused commit for independent review. **Verification Contract:** DoD-1 through DoD-4 proven with saved logs. **Confidence:** 96 / 90.

## Exit Conditions

- DONE — all DoD and task evidence complete at confidence >=90.
- SCOPE-CHANGE — storage migration or native resumption becomes necessary.
- BLOCKED-DEP — required toolchain cannot run.
- CONFIDENCE-STALL — opaque preservation or deterministic identities cannot be proven.

## Evidence

- 2026-10-21 — Parent audit confirmed normalized Codex tool IDs/image loss and unreported Claude tool-output truncation while native archival replay remained byte-exact.

## Decisions

- 2026-10-21 — This additive first slice does not promote canonical data into the current SQLite store. Original native archive remains authoritative; projections are reproducible views.
- 2026-10-21 — Public adapters initially cover Codex and Claude. Other providers fail as unsupported instead of claiming compatibility.

- 2026-10-21 — `cargo test --test session_conversion`: 14 synthetic conformance tests passed, including actual separate-process restart/move replay, explicit nulls, archive/span/event/version tampering, 4,000 repeated-ID rows, attachments and invalid-UTF8 provider-load negative control. Source conversion still succeeds while an ordinary read-source fails on the poisoned fixture, proving conversion does not discover provider histories.
- 2026-10-21 — Full final gate passed: formatting, 367 tests (353 existing + 14 conversion), four ignored benchmarks, strict all-target/all-feature Clippy and release build. Logs: `../audit/evidence/conversion-final-*.log`; summary: `../audit/evidence/conversion-final-verification.json`. Release retains the baseline local rust-objcopy/libLLVM stripping warning, exit 0.
- 2026-10-21 — Independent read-only review identified and verified fixes for quadratic ambiguous-link expansion, inherited-identity/output amplification, absent-vs-null roundtrip, malformed attachment availability and fabricated source role. Final reviewer reported no remaining findings. Independent 400→800-event repeated-ID probe produced 763,487→1,528,020 output bytes with bounded ambiguity counts and empty candidate edge lists. Evidence: `../audit/evidence/conversion-review-fixed.json`, `../audit/evidence/conversion-review-scaling-fixed.json`.
- 2026-10-21 — `scripts/benchmark-session-conversion.py` measured generated text-only release workloads, three separate processes per case, preserving archive hashes and byte-identical repeated output. For 2,000 messages: native→canonical median 24.655 ms; canonical restart→Claude-readable median 43.142 ms. All samples, bytes and scope are in `../audit/evidence/conversion-performance.json`; first 200-message cold launch was 637.429 ms and was retained. No speedup, memory-RSS or quality claim is made.
- 2026-10-21 — Python benchmark source compiled in memory without creating user cache files; benchmark ran successfully. `git diff --check` passed.
- 2026-10-21 — Global GDD skill/status script is unavailable after the parent's targeted searches. Manually validated goal frontmatter, fixed scope, checked DoD, task confidence and evidence against repository examples; no implementation verification gate was skipped.
