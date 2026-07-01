---
goal_id: "2026-07-01-shell-safe-next-commands"
title: "Quote continuation commands"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: Every emitted continuation command is executable as printed for project paths and remotes containing shell-significant characters.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Preserve existing JSON response shape and `next_command` intent.
- Use safe shell quoting; do not drop options or rely on ambient cwd to avoid quoting.
- Keep successful stdout JSON-only.
- Full `cargo test` is expected before DONE; coordinate with the test-gate goal if it is still blocked.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — review finding source and reproduction summary.
- `src/cli.rs:2021` — `build_next_read_project_command`.
- `src/cli.rs:2042` — remote read-project continuation builder.
- `src/cli.rs:4110` — existing `shell_quote_path` helper.
- `src/cli.rs:6034` — retrieve continuation builder already uses `shell_quote`.
- `src/cli.rs:7121` — `build_next_recall_command_with_remotes`.
- `tests/cli_contract.rs:3403` — existing simple-path read-project pagination test.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — `read project --project "/tmp/project with spaces" --limit 1` emits a `next_command` that executes successfully as printed — *verify by:* `cargo test --test cli_contract read_project_next_command_quotes_project_paths_with_spaces -- --exact --nocapture`
- [ ] **DoD-2** — remote read-project continuation quotes project paths and remote names safely enough to execute as printed in the existing shell harness — *verify by:* `cargo test --test cli_contract read_project_remote_next_command_quotes_shell_values -- --exact --nocapture`
- [ ] **DoD-3** — recall continuation with `--project "/tmp/project with spaces"` quotes the project path and executes as printed — *verify by:* `cargo test --test cli_contract recall_next_command_quotes_project_paths_with_spaces -- --exact --nocapture`
- [ ] **DoD-4** — retrieve pinned continuation still passes its existing shell-execution tests after shared quoting changes — *verify by:* `cargo test --test cli_contract retrieve_pinned_next_command_executes_as_printed_and_freezes_sessions retrieve_next_command_preserves_debug_and_full_message_history -- --nocapture`
- [ ] **DoD-5** — Repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — shell execution harness or Rust/Cargo is unavailable after one retry.
- **`BLOCKED-TEST-GATE`** — only the separately tracked memory-fabric gate prevents full `cargo test` after targeted continuation tests pass.
- **`SCOPE-CHANGE`** — fixing quoting requires replacing string `next_command` with structured argv in public JSON.
- **`CONFIDENCE-STALL`** — shell quoting cannot cover valid project/remotes without response-shape changes after 3 focused attempts.
- **`BUDGET`** — more than 2 full verification-loop attempts after targeted continuation tests are green.

---

## 5. Tasks · INVARIANT

### T1 · Add shell-execution regressions for continuations · [ ]

**Steps**
- [ ] Add a read-project pagination fixture whose project path contains spaces.
- [ ] Execute the emitted `next_command` through the existing shell helper.
- [ ] Add remote and recall variants for the same quoting class.
- [ ] Keep retrieve pinned continuation tests in the suite as a guardrail.

**Verification Contract**
- *Check:* continuation commands execute as printed for shell-significant values.
- *Method:* `cargo test --test cli_contract read_project_next_command_quotes_project_paths_with_spaces read_project_remote_next_command_quotes_shell_values recall_next_command_quotes_project_paths_with_spaces -- --nocapture`
- *Expected:* all named tests pass after implementation.
- *BDD scenarios covered:* Given a project path with spaces, when page 1 emits `next_command`, then page 2 executes successfully and returns the expected next messages.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Apply shared shell quoting to continuation builders · [ ]

**Steps**
- [ ] Reuse `shell_quote` or `shell_quote_path` for project, remote, and other shell values.
- [ ] Update read-project local, read-project remote, and recall continuation builders.
- [ ] Confirm retrieve continuation behavior remains unchanged.

**Verification Contract**
- *Check:* all continuation builders quote user/path-derived tokens consistently.
- *Method:* `cargo test --test cli_contract read_project_next_command_quotes_project_paths_with_spaces read_project_remote_next_command_quotes_shell_values recall_next_command_quotes_project_paths_with_spaces retrieve_pinned_next_command_executes_as_printed_and_freezes_sessions -- --nocapture`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given local, remote, recall, and retrieve continuations, each emitted command remains executable as printed.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-3, DoD-4

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Verify continuation and repo gates · [ ]

**Steps**
- [ ] Run continuation-related CLI tests.
- [ ] Run full verification loop or exit on the separate test-gate blocker.

**Verification Contract**
- *Check:* quoting changes do not regress pagination or retrieve continuations.
- *Method:* `cargo test --test cli_contract read_project_pagination_includes_next_page_and_next_command retrieve_pinned_next_command_executes_as_printed_and_freezes_sessions session_axis_pagination_pins_to_concrete_session_not_recency_age -- --nocapture && cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands, or explicit `BLOCKED-TEST-GATE` if only that known suite blocker remains.
- *BDD scenarios covered:* Existing pagination behavior and next-command semantics remain compatible.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-4, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: a path-with-spaces test alone would miss remote and recall builders, so this goal covers all reviewed raw interpolation sites. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
