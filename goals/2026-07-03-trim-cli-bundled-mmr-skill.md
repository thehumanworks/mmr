---
goal_id: "2026-07-03-trim-cli-bundled-mmr-skill"
title: "Trim CLI-bundled mmr skill"
status: "done"
confidence_floor: 90
created: "2026-07-03"
updated: "2026-07-03"
---

# Goal: `mmr skill load` and `mmr skill install` ship only the consumer-facing mmr skill files while repo-development subskill directories remain local to this repo.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal — if it isn't written here,
it didn't happen. The full procedure (boot loop, confidence rubric, logging cadence) lives in the
**goal-driven-development** skill; these rules hold even if that skill isn't loaded:

- **Scope is frozen once execution begins** (`status: in-progress`). Until then, §3
  and §5 may be edited freely. Deliver the goal; user comments or adjusts. Pointing
  the agent at this file is approval to execute. After execution begins, the only
  permitted edits are: tick checkboxes (Task **and** DoD), update Confidence, append
  Evidence, append to the live sections (§6/§7/§8), and update frontmatter
  `status`/`updated` — never add, remove, reword, split, or merge a DoD item or Task,
  and never rewrite or delete a live-section entry.
- **Never tick below the floor.** A task is ticked done only at Confidence ≥
  `confidence_floor`. If you cannot reach it, leave it unticked and fire `CONFIDENCE-STALL`.
- **Scope change is an exit, not a decision.** If scope must change, record the
  proposal in §6 and fire `SCOPE-CHANGE` — stop and surface it to the user.
- **Live sections are append-only.** Log each decision (§6) and learning (§7) at
  the moment it happens — before ticking the task it came from. Never delete entries.

---

## 2. References

- User request, 2026-07-03 — remove `goal-closeout`, `review-remediation`, `docs-first-contract-change`, and `command-surface-removal` from the CLI-bundled mmr skill without deleting their repo directories.
- `src/cli.rs` — defines `BUNDLED_MMR_SKILL_FILES` and the `skill load` / `skill install` bundle behavior.
- `.agents/skills/mmr/SKILL.md` — parent skill text embedded into the bundle; frontmatter description and `## When to Trigger` must remain exactly as found.
- `tests/cli_contract.rs` — integration tests for `skill load`, `skill install`, and `skill install --local`.
- `src/mcp.rs` — intentional uncommitted user changes; do not touch.

---

## 3. Definition of Done · INVARIANT

Each item is **atomic** (one verifiable assertion per checkbox), tagged with a
stable id that Tasks reference via **Closes:**, and carries a concrete `verify by:`.

Tick a `DoD-N` box only when its own `verify by:` has been run and passed (not merely
because a closing Task is ticked). Log the command and its outcome as an Evidence bullet
under the Task that **Closes:** it. DONE requires every DoD box ticked.

- [x] **DoD-1** — `BUNDLED_MMR_SKILL_FILES` includes `SKILL.md`, `session-mining/SKILL.md`, and session-mining references, and excludes the four repo-development subskills — *verify by:* `rg -n "goal-closeout|review-remediation|docs-first-contract-change|command-surface-removal|session-mining" src/cli.rs`
- [x] **DoD-2** — `.agents/skills/mmr/SKILL.md` no longer advertises the four repo-development subskills to bundle consumers while preserving its existing frontmatter description and `## When to Trigger` text — *verify by:* `git diff -- .agents/skills/mmr/SKILL.md`
- [x] **DoD-3** — CLI contract tests assert the four repo-development subskills are absent from `skill load` and installed bundles — *verify by:* `cargo test --test cli_contract skill_`
- [x] **DoD-4** — full default Rust test suite passes after the bundle change — *verify by:* `cargo test`

---

## 4. Exit Conditions

The goal terminates when **any** condition holds. On exit, state which fired —
explicitly — in the response to the user. Specialize the bracketed values for this goal.

- **`DONE`** — all §3 items ticked and all §5 tasks ≥ confidence floor. *(primary)*
- **`BLOCKED-DEP`** — local Cargo toolchain or filesystem unavailable after one direct retry. Exit without the blocked step; name it explicitly.
- **`SCOPE-CHANGE`** — work cannot complete without changing scope. Record the
  proposal in §6 and exit to the user.
- **`CONFIDENCE-STALL`** — a task cannot reach the floor after two honest attempts. Exit, report the task and the gap.
- **`BUDGET`** — more than three verification-fix loops are required. Exit and report progress.

---

## 5. Tasks · INVARIANT

Ordered, dependency-aware units of work that together satisfy the DoD. Tick the
trailing `[ ]` only when the Verification Contract passes and Confidence ≥ floor.

---

### T1 · Remove dev subskills from the CLI bundle · [x]

**Steps**
- [x] Remove the four repo-development subskill entries from `BUNDLED_MMR_SKILL_FILES`.
- [x] Keep `SKILL.md`, `session-mining/SKILL.md`, and session-mining `references/` entries in the bundle.
- [x] Confirm the removed subskill directories still exist under `.agents/skills/mmr/`.

**Verification Contract**
- *Check:* Bundle source references only the retained consumer-facing skill files.
- *Method:* `rg -n "goal-closeout|review-remediation|docs-first-contract-change|command-surface-removal|session-mining" src/cli.rs && ls .agents/skills/mmr/{goal-closeout,review-remediation,docs-first-contract-change,command-surface-removal}/SKILL.md`
- *Expected:* `src/cli.rs` only references `session-mining`; all four repo-development `SKILL.md` files still exist on disk.
- *BDD scenarios covered:* Consumer loads or installs the CLI-bundled skill and receives no repo-development subskill files.

**Confidence:** 95 / 90 · **Depends on:** none · **Closes:** DoD-1

**Evidence (required before tick; append-only)**
- 2026-07-03 - command: `rg -n "goal-closeout|review-remediation|docs-first-contract-change|command-surface-removal|session-mining" src/cli.rs && ls .agents/skills/mmr/{goal-closeout,review-remediation,docs-first-contract-change,command-surface-removal}/SKILL.md` - outcome: exit 0; `src/cli.rs` only reported retained `session-mining` bundle entries, and all four repo-development `SKILL.md` files still existed on disk.

---

### T2 · Update parent skill copy and contract tests · [x]

**Steps**
- [x] Remove or relocate the four repo-development subskill sections from `.agents/skills/mmr/SKILL.md` without touching the frontmatter description or `## When to Trigger` section.
- [x] Add tests that assert `skill load`, `skill install`, and `skill install --local` do not expose the four removed subskills.
- [x] Run the targeted skill contract tests.

**Verification Contract**
- *Check:* Skill copy and tests reflect the consumer bundle contract.
- *Method:* `git diff -- .agents/skills/mmr/SKILL.md tests/cli_contract.rs && cargo test --test cli_contract skill_`
- *Expected:* Diff preserves the requested parent skill sections and targeted tests pass.
- *BDD scenarios covered:* Bundle load output omits dev subskills; installed user/local skill dirs do not contain dev subskill directories.

**Confidence:** 95 / 90 · **Depends on:** T1 · **Closes:** DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- 2026-07-03 - command: `git diff -- .agents/skills/mmr/SKILL.md tests/cli_contract.rs src/cli.rs` - outcome: exit 0; diff removes only the four repo-development subskill sections from the parent skill body and adds negative bundle/install assertions.
- 2026-07-03 - command: `cargo test --test cli_contract skill_` - outcome: exit 0; 3 passed, 0 failed, 134 filtered out.

---

### T3 · Run full default verification · [x]

**Steps**
- [x] Run `cargo test`.
- [x] Inspect the final diff to ensure no changes touched `src/mcp.rs` and no removed skill directories were deleted.

**Verification Contract**
- *Check:* Full default Rust tests pass and final diff is scoped to the goal.
- *Method:* `cargo test && git diff --stat && git status --short`
- *Expected:* `cargo test` exits 0; diff is limited to goal doc, `src/cli.rs`, `.agents/skills/mmr/SKILL.md`, and `tests/cli_contract.rs`, plus pre-existing user changes are preserved.
- *BDD scenarios covered:* Regression suite exercises the CLI contract after bundle trimming.

**Confidence:** 95 / 90 · **Depends on:** T2 · **Closes:** DoD-4

**Evidence (required before tick; append-only)**
- 2026-07-03 - command: `cargo fmt` - outcome: exit 0.
- 2026-07-03 - command: `cargo test` - outcome: exit 0; 157 unit tests passed, 137 CLI contract tests passed, 12 MCP contract tests passed, 47 memory fabric contract tests passed, doc tests passed.
- 2026-07-03 - command: `cargo test --test cli_benchmark -- --ignored --nocapture` - outcome: exit 0; 4 passed, 0 failed.
- 2026-07-03 - command: `cargo clippy --all-targets --all-features -- -D warnings` - outcome: exit 0.
- 2026-07-03 - command: `cargo build --release` - outcome: exit 0.
- 2026-07-03 - command: `target/debug/mmr skill load | python3 -c ...` - outcome: exit 0; `session-mining` present and forbidden matches list empty.
- 2026-07-03 - command: `git status --short && git diff --stat && git diff --check` - outcome: exit 0; `src/mcp.rs` remains modified from intentional pre-existing work, and no removed subskill directories were deleted.

---

## 6. Decisions · LIVE (append-only)

Meaningful choices/concessions needing visibility. Scope impact must be `none`.

- 2026-07-03 — Treat the user's explicit implementation request as approval to execute this repo-required goal document immediately; waiting for a separate pointer would conflict with the requested end-to-end change. Scope impact: none.
- 2026-07-03 — Keep the four repo-development skill directories in the repo but remove their advertisement from the consumer parent skill instead of moving them under a repo-only heading; this avoids shipping local workflow names in `mmr skill load`. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

Flash cards: trigger → wrong action → revision → correct action, with impact `1–5`.
When an attempt failed and the fix is not yet known, log the **open form** —
trigger → wrong action → *(open: revision/correct not yet found)* → pointer to the raw
failure (log path or commit) — still impact-tagged, so a dead-end is recorded before a
fresh context re-treads it.

*(none yet)*

---

## 8. Skills · LIVE (append-only)

Reusable workflows created via the **skill-creator** skill while working this goal.

*(none yet)*
