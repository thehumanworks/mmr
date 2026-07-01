---
goal_id: "2026-07-01-teleport-ssh-target-hardening"
title: "Harden teleport SSH targets"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: Teleport SSH sharing rejects option-like or shell-fragment targets before invoking `ssh` or `scp`.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Keep `share session --to user@host` and current dry-run plan JSON working.
- Reject unsafe targets before any external process is spawned.
- Prefer sharing parser behavior with `src/peer.rs` or mirroring its tested constraints.
- Do not add a legacy or alternate SSH command surface.
- Full `cargo test` is expected before DONE; coordinate with the test-gate goal if it is still blocked.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — review finding source.
- `src/teleport/ssh.rs:33` — teleport SSH target parser currently rejects URLs only.
- `src/teleport/ssh.rs:84` — `ssh_base_args` puts host directly after options without `--`.
- `src/peer.rs:135` — safer peer SSH parser.
- `src/peer.rs:158` — safer SSH argv builder inserts `--`.
- `src/peer.rs:316` — peer parser rejects whitespace/metacharacters/leading dash.
- `tests/cli_contract.rs` — share session dry-run and remote target tests.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — Teleport SSH target parsing rejects leading-dash targets, whitespace, and shell metacharacters — *verify by:* `cargo test parse_ssh_target_rejects_option_like_and_shell_fragments -- --nocapture`
- [ ] **DoD-2** — Teleport SSH argv construction places `--` before the host where OpenSSH supports it, and still builds the expected probe/stream/scp fallback plan — *verify by:* `cargo test ssh_base_args_delimit_host_and_share_plan_still_matches -- --nocapture`
- [ ] **DoD-3** — `share session --to -oProxyCommand=sh --dry-run` fails as structured usage instead of returning a runnable SSH plan — *verify by:* `cargo test --test cli_contract share_session_ssh_rejects_option_like_target -- --exact --nocapture`
- [ ] **DoD-4** — Valid SSH targets still work in parser and dry-run contract tests — *verify by:* `cargo test parse_ssh_target_accepts_user_host share_session_ssh_dry_run_reports_import_bundle_plan -- --nocapture`
- [ ] **DoD-5** — Repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Rust/Cargo or OpenSSH argv expectations cannot be validated locally after one retry.
- **`BLOCKED-TEST-GATE`** — only the separately tracked memory-fabric gate prevents full `cargo test` after targeted SSH tests pass.
- **`SCOPE-CHANGE`** — unifying peer and teleport SSH parsing requires changing accepted target syntax or public dry-run JSON.
- **`CONFIDENCE-STALL`** — platform compatibility for `ssh -- host` cannot be established after 2 focused checks.
- **`BUDGET`** — more than 2 full verification-loop attempts after targeted SSH tests are green.

---

## 5. Tasks · INVARIANT

### T1 · Add unsafe-target regressions · [ ]

**Steps**
- [ ] Add unit coverage for `-oProxyCommand=sh`, whitespace, semicolons, and URL targets.
- [ ] Add CLI dry-run coverage proving unsafe targets fail before plan construction.
- [ ] Add argv coverage for host delimiter placement.

**Verification Contract**
- *Check:* unsafe teleport SSH targets are rejected at parser/CLI boundaries.
- *Method:* `cargo test parse_ssh_target_rejects_option_like_and_shell_fragments ssh_base_args_delimit_host_and_share_plan_still_matches -- --nocapture && cargo test --test cli_contract share_session_ssh_rejects_option_like_target -- --exact --nocapture`
- *Expected:* all named tests pass after implementation.
- *BDD scenarios covered:* Given an option-like target, when sharing over SSH, then no SSH argv plan is emitted.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Reuse or mirror the safe peer SSH target rules · [ ]

**Steps**
- [ ] Factor common SSH target validation if it stays small and avoids coupling churn.
- [ ] Otherwise mirror peer parser constraints in teleport SSH with tests.
- [ ] Insert host delimiter in teleport SSH argv if supported by the chosen command shape.
- [ ] Preserve existing valid target behavior and dry-run response shape.

**Verification Contract**
- *Check:* unsafe targets fail, valid targets succeed, and dry-run JSON remains useful.
- *Method:* `cargo test parse_ssh_target_rejects_option_like_and_shell_fragments parse_ssh_target_accepts_user_host ssh_base_args_delimit_host_and_share_plan_still_matches -- --nocapture && cargo test --test cli_contract share_session_ssh_dry_run_reports_import_bundle_plan -- --nocapture`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given `bob@macbook`, SSH plan still builds; given `-oProxyCommand=sh`, parsing fails.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-4

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Verify teleport and repo gates · [ ]

**Steps**
- [ ] Run teleport SSH unit and CLI tests.
- [ ] Run the full repo verification loop or exit on the separate test-gate blocker.

**Verification Contract**
- *Check:* hardening does not regress normal teleport sharing/import behavior.
- *Method:* `cargo test teleport::ssh:: -- --nocapture && cargo test --test cli_contract share_session_ssh_dry_run_reports_import_bundle_plan -- --nocapture && cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands, or explicit `BLOCKED-TEST-GATE` if only that known suite blocker remains.
- *BDD scenarios covered:* Given normal dry-run SSH sharing, the plan is still reported; given unsafe target syntax, usage fails early.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: `ssh -- host` compatibility is a real concern, so the goal allows either reuse or mirroring of peer parser rules but requires argv validation before DONE. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
