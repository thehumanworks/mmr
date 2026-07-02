---
goal_id: "2026-07-02-ship-agent-workflow-foundation"
title: "Ship agent workflow foundation"
status: "in-progress"
confidence_floor: 90
created: "2026-07-02"
updated: "2026-07-02"
---

# Goal: Phase 1 agent-workflow artifacts are committed, verified, and installed locally via `mmr skill install --local`.

## 1. Invariants

- Scope is frozen once `status: in-progress`. Pointer at this file approves execution.
- Follow [mmr/goal-closeout](.agents/skills/mmr/goal-closeout/SKILL.md) for closeout.
- Index: [GOAL.md](../GOAL.md) Phase 2.

## 2. References

- `GOAL.md` — master index; update Phase 2 to done when complete.
- `.cursor/rules/goal-driven-development.mdc` — GDD approval rule.
- `.cursor/rules/goal-closeout.mdc` — verification closeout rule.
- `.agents/skills/mmr/` — four new subskills + updated parent SKILL.md.
- `AGENTS.md`, `CLAUDE.md` — updated GDD approval model.

## 3. Definition of Done

- [x] **DoD-1** — All Phase 1 artifacts present on disk — *verify by:* `test -f GOAL.md && ls .cursor/rules/goal-*.mdc .agents/skills/mmr/*/SKILL.md`
- [x] **DoD-2** — No Rust regression after bundling subskills in `src/cli.rs` — *verify by:* full verification loop (see T2 evidence)
- [x] **DoD-3** — `GOAL.md` Phase 2 marked done and this goal `done` — *verify by:* `rg -n "Phase 2.*done" GOAL.md`
- [ ] **DoD-4** — Changes committed with imperative message (user pointed at GOAL.md approving full phase) — *verify by:* `git log -1 --oneline` mentions agent workflow

## 4. Exit Conditions

- **`DONE`** — all §3 ticked.
- **`SCOPE-CHANGE`** — user wants Phase 3 started in same commit; record in §6 and stop.

## 5. Tasks

### T1 · Verify artifact tree · [x]

**Verification Contract**

- Check: every file listed in `goals/2026-07-02-codify-agent-workflows.md` DoD still exists.
- Method: `git status` + path checks.
- Expected: rules, skills, AGENTS.md, CLAUDE.md, GOAL.md, both goal docs.

**Confidence:** 95 / 90 · **Depends on:** none · **Closes:** DoD-1

**Evidence**
- 2026-07-02 — subagent verified 12/12 artifacts present; `ls .agents/skills/mmr/*/SKILL.md` lists 5 subskills including four new ones.
- 2026-07-02 — restored four subskills after `skill install --local` wipe (pre-bundle fix); paths confirmed on disk.

### T2 · Full verification · [x]

**Verification Contract**

- Check: repo builds/tests after doc additions and skill bundle update in `src/cli.rs`.
- Method: full verification loop per `.cursor/rules/verification-loop.mdc`.
- Expected: exit 0 on all steps.

**Confidence:** 95 / 90 · **Depends on:** T1 · **Closes:** DoD-2

**Evidence**
- 2026-07-02 — subagent loop: `cargo fmt --check` 0; `cargo test` 353 passed; `cli_benchmark` 4 passed; `clippy` 0 warnings; `cargo build --release` ok.

### T3 · Bundle subskills + skill install · [x]

**Verification Contract**

- Check: `BUNDLED_MMR_SKILL_FILES` includes four new subskills; `mmr skill install --local` installs all five subskill dirs.
- Method: `rg goal-closeout src/cli.rs && cargo run -- skill install --local && ls .agents/skills/mmr/*/SKILL.md`
- Expected: 5 subskill SKILL.md files after install.

**Confidence:** 95 / 90 · **Depends on:** T2 · **Closes:** none

**Evidence**
- 2026-07-02 — added four `include_str!` entries to `src/cli.rs` `BUNDLED_MMR_SKILL_FILES`.
- 2026-07-02 — `cargo run -- skill install --local` installed 8 files; `ls .agents/skills/mmr/*/SKILL.md` → 5 subskills.

### T4 · Update GOAL.md Phase 2 status · [x]

**Verification Contract**

- Check: Phase 2 row shows done; ship goal marked done.
- Method: edit `GOAL.md` and this file frontmatter; `gdd_status.py` reports done-eligible.

**Confidence:** 95 / 90 · **Depends on:** T3 · **Closes:** DoD-3

**Evidence**
- 2026-07-02 — `rg -n "Phase 2.*done" GOAL.md` → line 35 `## Phase 2 — Ship foundation · **done**`; table row status `done`.

### T5 · Commit · [ ]

**Verification Contract**

- Check: scoped commit of Phase 1 + GOAL.md + ship goal + skill bundle.
- Method: user pointed at `GOAL.md` for full implementation ship.
- Expected: clean `git status` for scoped files.

**Confidence:** 95 / 90 · **Depends on:** T4 · **Closes:** DoD-4

**Evidence**
