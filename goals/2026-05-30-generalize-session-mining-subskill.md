---
title: "Rename subskill to 'session-mining' and generalize for session continuity (compaction + clearing survival)"
description: "Rename the mmr/prior-session-workflow-mining subskill to session-mining. Significantly broaden its scope from narrow workflow extraction to general previous-session retrieval (supporting --session-back, --session-range, and explicit --session), analysis, and continuity reminding. The goal is to help agents survive context compaction and full clearing by reliably pulling prior work into context when needed."
date: 2026-05-30
status: done
---

# GOAL: Rename + Generalize the mmr session subskill to `session-mining` for robust continuity

## Outcome

- The subskill currently at `.agents/skills/mmr/prior-session-workflow-mining/` is renamed/moved to `.agents/skills/mmr/session-mining/`.
- The skill is substantially generalized:
  - Supports the full session selection surface of `mmr` (`--session-back`, `--session-range`, `--session`, `mmr prev`, etc.).
  - Focus shifts from "extracting rituals to turn into skills" to **session continuity**: retrieving, analyzing, and surfacing prior work to survive context compaction and clearing.
  - Strong emphasis on "reminding" — producing useful continuity briefs, key decisions, open tasks, architecture context, etc.
- The parent `mmr/SKILL.md` is updated to reflect the new name and broader purpose.
- All references in AGENTS.md and previous goal documents are updated.
- The skill follows guidance from skill-creator (loaded first) for scope, description quality, and generalization.

## Why this change

The original `prior-session-workflow-mining` skill was very specific (evidence mining for new skill creation). The user now wants a more fundamental and reusable capability:

> Generalise so it can be used to retrieve previous sessions (via --session-back and via --session-range), analyse them and work from them to improve agent capabilities in **session continuity**, surviving both compaction and clearing by being able to retrieve previous sessions and remind them of the work as needed.

This is higher-leverage infrastructure for long-running agent work.

`mmr` already has good primitives here (`--session-back`, `--session-range`, `prev`, and the newer `summary`/`remember` commands). The subskill should become the **recommended way** to use those primitives for continuity and memory recovery.

## Surface touched

- New goal document: `goals/2026-05-30-generalize-session-mining-subskill.md`
- Directory rename/move: `mmr/prior-session-workflow-mining/` → `mmr/session-mining/`
- Major rewrite of:
  - `.agents/skills/mmr/session-mining/SKILL.md`
  - `.agents/skills/mmr/session-mining/references/extraction-jq-patterns.md` (may be renamed or split)
- Updates to:
  - `.agents/skills/mmr/SKILL.md` (parent)
  - `AGENTS.md`
  - Previous goal documents (`2026-05-30-retrieve-...` and the review + restructure goals)
- Possible new or expanded references (e.g., continuity patterns, compaction survival techniques)

## Decisions

1. **Name**: Subskill becomes `session-mining` (clean, general, matches the new purpose).
2. **Scope**: Broadened to session retrieval + analysis + continuity reminding. Still mmr-specific (uses mmr's session axis heavily).
3. **Relationship to `mmr summary` / `remember`**: The skill should complement these (perhaps by doing deeper retrieval + custom analysis before feeding into summary generation).
4. **Backward compatibility**: Old name references will be updated; we treat this as a deliberate rename + generalization, not a drop-in replacement.
5. Follow skill-creator principles: generalize away from the single birth transcript, improve description triggering, keep prompt lean, explain the *why*.

## Phased plan

1. Load skill-creator (done).
2. Create this goal document.
3. Research current mmr session selection surface (via `--help`).
4. Read the current subskill content.
5. Design the new generalized SKILL.md + references structure.
6. Perform the directory rename/move.
7. Write the new generalized content.
8. Update parent `mmr/SKILL.md`, AGENTS.md, and previous goals.
9. Validate structure and basic usability.
10. Update this goal to `done`.

## Definition of Done

- [x] Goal document created.
- [x] Subskill directory is now `mmr/session-mining/`.
- [x] SKILL.md and references rewritten for the broader continuity use case, with accurate coverage of `--session-back`, `--session-range`, etc.
- [x] Parent `mmr` skill and AGENTS.md reflect the new name and purpose.
- [x] All previous goal documents updated with the new paths/names.
- [x] No broken references remain.
- [x] This goal marked `done`.

## Progress notes

(2026-05-30) Goal created after loading skill-creator. mmr help consulted.

(2026-05-30) Execution complete:
- Subskill renamed from `prior-session-workflow-mining` → `session-mining`
- Directory moved to `.agents/skills/mmr/session-mining/`
- SKILL.md fully rewritten for the broader session continuity mission (supports `--session-back`, `--session-range`, `prev`, explicit sessions; strong focus on compaction/clearing survival and "reminding")
- New primary references file: `references/session-retrieval-patterns.md`
- Parent `mmr/SKILL.md` and AGENTS.md updated
- Previous goal documents cleaned up
- Structure verified. The system has announced the updated parent skill.
