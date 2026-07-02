---
goal_id: "2026-07-02-codify-agent-workflows"
title: "Codify mmr agent workflows as rules and skills"
status: "done"
confidence_floor: 90
created: "2026-07-02"
updated: "2026-07-02"
---

# Goal: Persist high-value mmr agent workflows as Cursor rules and project skills, and simplify GDD approval to goal-file pointer acceptance.

## 3. Definition of Done

- [x] **DoD-1** — Cursor rules exist for GDD approval model and goal closeout — *verify by:* `ls .cursor/rules/goal-driven-development.mdc .cursor/rules/goal-closeout.mdc`
- [x] **DoD-2** — Four mmr subskills exist (goal-closeout, review-remediation, docs-first-contract-change, command-surface-removal) — *verify by:* `ls .agents/skills/mmr/*/SKILL.md`
- [x] **DoD-3** — `AGENTS.md` documents new skills, rules, and GDD approval — *verify by:* `rg -n "goal-closeout|ready|pointing" AGENTS.md`
- [x] **DoD-4** — Global GDD skill removes blocked status and explicit pre-execution approval gate — *verify by:* `python ~/.agents/skills/goal-driven-development/scripts/test_gdd_status.py` (21 passed)

## 6. Decisions

- **Pointer-as-approval** — User pointing at a goal file replaces explicit scope confirmation; scope freezes at `in-progress`. Scope impact: none (process only).
- **No frontmatter `blocked`** — Prerequisites use `GOAL.md` ordering, task Depends-on, or Decisions; runtime external deps still use `BLOCKED-DEP` exit. Scope impact: none.

## 8. Skills

- **goal-closeout** — mmr goal finish loop — `.agents/skills/mmr/goal-closeout/SKILL.md` — after implementation, before marking goals done
- **review-remediation** — review → worktrees → merge — `.agents/skills/mmr/review-remediation/SKILL.md` — after deep `$code-review` findings
- **docs-first-contract-change** — specs-first CLI changes — `.agents/skills/mmr/docs-first-contract-change/SKILL.md` — CLI contract / retrieve output work
- **command-surface-removal** — safe command deletion — `.agents/skills/mmr/command-surface-removal/SKILL.md` — removing public subcommands
