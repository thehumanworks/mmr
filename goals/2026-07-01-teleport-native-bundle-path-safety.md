---
goal_id: "2026-07-01-teleport-native-bundle-path-safety"
title: "Harden native bundle apply paths"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: Applying a native teleport bundle cannot write outside the selected provider's owned directory.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Treat all bundle metadata as untrusted input.
- Do not remove native layout preservation for legitimate provider transcript paths.
- Do not weaken bundle hash verification or newer-existing-artifact conflict checks.
- Full `cargo test` is expected before DONE; coordinate with the test-gate goal if it is still blocked.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — review finding source and verification evidence.
- `src/teleport/apply.rs:95` — `native_write_targets` result is trusted for apply.
- `src/teleport/apply.rs:198` — parent dirs are created before write.
- `src/teleport/apply.rs:202` — remapped native content is written to destination.
- `src/teleport/provider.rs:208` — native write targets derive from bundle metadata.
- `src/teleport/provider.rs:319` — shared helper preserves unchecked relative components.
- `src/teleport/providers/codex.rs:124` — Codex destination helper joins marker-relative suffix under `~/.codex`.
- `src/teleport/providers/{claude,cursor,grok,pi}.rs` — provider destination helpers using shared relative logic.
- `tests/cli_contract.rs` and `src/teleport/bundle.rs` — teleport apply/import and provider destination tests.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — Crafted Codex native bundles with `..`, absolute, or root-like components in `metadata.native_source_file` are rejected before any filesystem write — *verify by:* `cargo test --test cli_contract import_bundle_rejects_codex_native_path_traversal -- --exact --nocapture`
- [ ] **DoD-2** — Crafted Claude, Cursor, Grok, and Pi native bundles with traversal in `metadata.native_source_file` are rejected before any filesystem write — *verify by:* `cargo test --test cli_contract import_bundle_rejects_provider_native_path_traversal_matrix -- --exact --nocapture`
- [ ] **DoD-3** — Legitimate native bundle apply still preserves provider-relative layout and path remapping — *verify by:* `cargo test --test cli_contract provider_matrix_share_file_then_import_read_only share_session_file_then_import_bundle_read_only_and_apply_round_trip -- --nocapture`
- [ ] **DoD-4** — Destination helpers enforce provider-root containment with unit coverage — *verify by:* `cargo test native_destination_path_rejects_escape_components -- --nocapture`
- [ ] **DoD-5** — Repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Rust/Cargo or temp HOME filesystem permissions are unavailable after one retry.
- **`BLOCKED-TEST-GATE`** — only the separately tracked memory-fabric gate prevents full `cargo test` after targeted teleport tests pass.
- **`SCOPE-CHANGE`** — safe apply requires changing the native bundle schema, fidelity contract, or supported provider layout.
- **`CONFIDENCE-STALL`** — provider-root containment cannot be proven for every native provider after 3 focused attempts.
- **`BUDGET`** — more than 2 full verification-loop attempts after targeted teleport tests are green.

---

## 5. Tasks · INVARIANT

### T1 · Add malicious bundle regressions · [ ]

**Steps**
- [ ] Build fixture helpers for self-consistent native bundles with forged `native_source_file`.
- [ ] Prove Codex traversal attempts do not create files outside `HOME/.codex`.
- [ ] Prove the provider matrix rejects traversal for Claude, Cursor, Grok, and Pi.

**Verification Contract**
- *Check:* traversal tests fail before the fix and pass after the fix without writing escaped files.
- *Method:* `cargo test --test cli_contract import_bundle_rejects_codex_native_path_traversal import_bundle_rejects_provider_native_path_traversal_matrix -- --nocapture`
- *Expected:* all named tests pass after the fix.
- *BDD scenarios covered:* Given a self-consistent malicious bundle, when imported with `--apply`, then the command fails and no file outside the provider root appears.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Enforce safe native destination paths · [ ]

**Steps**
- [ ] Add a shared safe relative-path validator for bundle-derived native suffixes.
- [ ] Reject parent, root, prefix, and absolute components before building write targets.
- [ ] Validate final write targets lexically stay under provider-owned roots.
- [ ] Preserve legitimate provider-relative session layouts.

**Verification Contract**
- *Check:* destination helpers reject escapes and still return valid provider paths.
- *Method:* `cargo test native_destination_path_rejects_escape_components codex_native_destination_path_preserves_relative_layout -- --nocapture`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given a normal `.codex/sessions/2026/01/session.jsonl` source, layout is preserved; given `.codex/../x`, it is rejected.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-3, DoD-4

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Verify apply/import behavior end to end · [ ]

**Steps**
- [ ] Run provider matrix import/read-only/apply tests.
- [ ] Run the full repo verification loop.
- [ ] Record `BLOCKED-TEST-GATE` if only the separately tracked memory-fabric test gate remains.

**Verification Contract**
- *Check:* path safety does not break normal native teleport workflows.
- *Method:* `cargo test --test cli_contract provider_matrix_share_file_then_import_read_only share_session_file_then_import_bundle_read_only_and_apply_round_trip -- --nocapture && cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands, or explicit `BLOCKED-TEST-GATE` if only that known suite blocker remains.
- *BDD scenarios covered:* Given a legitimate bundle, import/read/apply still works; given a malicious bundle, apply rejects before write.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-3, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: the goal must protect every provider, not just Codex, because shared helpers serve Claude/Cursor/Grok/Pi. Added provider-matrix DoD. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
