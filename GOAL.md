# GOAL: Agent workflow codification for mmr

**North star:** Complex mmr agent workflows are repeatable from rules, skills, and goal docs — not rediscovered each session.

## How to use this index

1. **Author** — agent delivers or updates linked `goals/*.md` files; optional edits here.
2. **Approve** — point the agent at `GOAL.md` or a specific `goals/<slug>.md` path to execute.
3. **Execute** — agent sets the target goal to `in-progress`, follows the linked skill/rule, closes with evidence.

**Status vocabulary:** `draft` | `ready` | `in-progress` | `done` | `exited`  
Do not use frontmatter `blocked`. Prerequisites are ordering in this file, task **Depends on**, or exit conditions.

**Skills & rules:** `.agents/skills/mmr/` · `.cursor/rules/goal-driven-development.mdc` · `.cursor/rules/goal-closeout.mdc` · `~/.agents/skills/goal-driven-development/`

---

## Phase 1 — Foundation · **done**

| Goal | Status | Outcome |
|------|--------|---------|
| [goals/2026-07-02-codify-agent-workflows.md](goals/2026-07-02-codify-agent-workflows.md) | done | GDD pointer-as-approval; four mmr subskills; two Cursor rules; AGENTS.md + global GDD skill refresh |

Delivered subskills:

| Subskill | When to load |
|----------|----------------|
| [mmr/goal-closeout](.agents/skills/mmr/goal-closeout/SKILL.md) | Finishing any `goals/*.md` delivery |
| [mmr/review-remediation](.agents/skills/mmr/review-remediation/SKILL.md) | After deep `$code-review` → parallel worktrees |
| [mmr/docs-first-contract-change](.agents/skills/mmr/docs-first-contract-change/SKILL.md) | Specs-first CLI contract changes |
| [mmr/command-surface-removal](.agents/skills/mmr/command-surface-removal/SKILL.md) | Removing public CLI commands |

---

## Phase 2 — Ship foundation · **done**

| Goal | Status | Outcome |
|------|--------|---------|
| [goals/2026-07-02-ship-agent-workflow-foundation.md](goals/2026-07-02-ship-agent-workflow-foundation.md) | done | Commit rules, skills, AGENTS/CLAUDE/GOAL.md; run verification; `mmr skill install --local` |

**Pointer to execute:** Phase 3 items — create a goal doc per row when starting work.

---

## Phase 3 — Extended skills · **draft** (author on pointer)

Session-mined workflows not yet codified. Create one `goals/*.md` per row when starting work.

| Priority | Proposed skill | Trigger |
|----------|----------------|---------|
| 1 | `mmr/openapi-from-cli` | REST/OpenAPI docs from CLI taxonomy; adversarial review rounds |
| 2 | `mmr/mcp-lifecycle` | Native MCP dev, stdio discipline, Cursor `mcp.json` wiring |
| 3 | `mmr/security-hardening` | Review findings → reproduce → provider-matrix tests → fixture updates |
| 4 | `mmr/orchestrated-feature-refinement` | Orchestrator: 1 writer + 2 read-only reviewers for contract changes |
| 5 | `mmr/structured-review-job` | Generic agent job JSON output (`verdict`, `findings`, `checks_run`) |

Each Phase 3 item: goal doc → skill under `.agents/skills/mmr/` → entry in this index → parent [mmr/SKILL.md](.agents/skills/mmr/SKILL.md).

---

## Related completed mmr work (2026-07-02)

| Goal | Status |
|------|--------|
| [goals/2026-07-02-mmr-rest-openapi-docs.md](goals/2026-07-02-mmr-rest-openapi-docs.md) | done |
| [goals/2026-07-02-auto-start-api-docs-pitchfork.md](goals/2026-07-02-auto-start-api-docs-pitchfork.md) | done |
| [goals/2026-07-02-mcp-stdio-autostart-rest.md](goals/2026-07-02-mcp-stdio-autostart-rest.md) | done |
| [goals/2026-07-02-remove-python-bootstrap-mcp.md](goals/2026-07-02-remove-python-bootstrap-mcp.md) | done |

---

## Decisions

- **2026-07-02 — Pointer-as-approval** — Referencing `GOAL.md` or a goal path approves execution; no separate confirmation gate. Scope freezes at `in-progress` on the target goal.
- **2026-07-02 — GOAL.md is index only** — Delivery contracts live in `goals/*.md`; this file tracks order, status, and links — not task checkboxes or evidence.
