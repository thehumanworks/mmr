---
title: "Restructure mmr/session-mining as subskill under a new parent 'mmr' skill"
description: "Following guidance from skill-creator (loaded first), create a parent skill simply named 'mmr' and move the prior-session-workflow-mining skill created in the previous goal to live under it as a subskill, using the Domain organization pattern recommended in skill-creator. Update all references, AGENTS.md, and goal documents."
date: 2026-05-30
status: done
---

# GOAL: Introduce 'mmr' as parent skill with prior-session-workflow-mining as subskill

## Outcome

- A new parent skill directory `.agents/skills/mmr/` is created with a proper `SKILL.md`.
- The existing `mmr/session-mining` skill is moved to become `.agents/skills/mmr/prior-session-workflow-mining/`.
- The structure follows the "Domain organization" / variant pattern shown in `/Users/mish/.agents/skills/skill-creator/SKILL.md`.
- The parent `mmr` skill has a good triggering description and points to the subskill.
- All cross-references (AGENTS.md, the two goal documents from this conversation, internal self-references) are updated.
- The old top-level `mmr/session-mining/` directory is removed after the move.
- Work is driven from this goal document; status updated to `done` when complete.

## Why this shape

The user explicitly requested:
> "rename the skill to just be called 'mmr' and move the skill you created to be a 'subskill' of this 'mmr' parent. Load the /skill-creator skill first."

Loading skill-creator first revealed the recommended pattern under "Domain organization":

```
cloud-deploy/
├── SKILL.md (workflow + selection)
└── references/
    ├── aws.md
    ├── gcp.md
    └── azure.md
```

This provides a clean way to have a short, memorable parent name ("mmr") while keeping detailed, focused sub-capabilities organized underneath it. This improves discoverability and progressive disclosure.

The other two existing mmr-related skills (`mmr-clap-colored-cli` and `mmr-teleport-providers`) can remain as siblings for now (future consolidation is out of scope unless requested).

## Surface touched

- New goal document: `goals/2026-05-30-restructure-mmr-skill-as-parent-with-subskill.md` (this file)
- New parent skill: `.agents/skills/mmr/SKILL.md`
- Moved subskill: `.agents/skills/mmr/prior-session-workflow-mining/` (contents moved from the old location)
- Deletion of the old top-level directory `.agents/skills/mmr/session-mining/`
- Updates to:
  - `AGENTS.md`
  - `goals/2026-05-30-retrieve-claude-prior-session-consolidate-workflows-skills.md`
  - `goals/2026-05-30-adversarial-review-mmr/session-mining-skill.md`
  - Internal references inside the moved skill itself
- MCP goal-tasks tracking (new goal or tasks)

## Decisions

1. Follow the exact Domain organization pattern from skill-creator.
2. Parent skill name in frontmatter and directory: `mmr`
3. Subskill keeps the descriptive name `prior-session-workflow-mining` under the parent (most usable form).
4. The subskill's internal `name` frontmatter can stay `mmr/session-mining` or be adjusted for clarity.
5. The parent `mmr` SKILL.md should be concise, point to the subskill, and cover general mmr usage (leveraging existing knowledge from the other two mmr skills).
6. No changes to the other two mmr-* sibling skills unless necessary for references.
7. All references to the old skill path/name must be updated.

## Phased plan

1. Create this goal document (first action after loading skill-creator).
2. Read the full current content of the skill being moved and the review goal documents for accurate reference updates.
3. Create the new parent directory and `mmr/SKILL.md` (with appropriate description that makes the subskill discoverable).
4. Move the directory tree using filesystem operations.
5. Update all references in goal documents, AGENTS.md, and inside the subskill.
6. Remove the old top-level `mmr/session-mining/` directory.
7. Validate the new structure (list_dir, read key files).
8. Update this goal's DoD and status; add progress note to parent goal if relevant.
9. Close MCP tracking.

## Definition of Done

- [x] This goal document created before any directory moves or skill edits.
- [x] skill-creator was loaded first (already done in this session).
- [x] New parent `.agents/skills/mmr/SKILL.md` exists with good frontmatter and content following skill-creator patterns.
- [x] The workflow-mining skill now lives at `.agents/skills/mmr/prior-session-workflow-mining/`.
- [x] Old top-level directory removed.
- [x] All references updated (AGENTS.md, both previous goal docs, internal self-references).
- [x] Structure passes basic validation (correct files present, no broken internal links in the skill).
- [x] This goal marked `done`.

## Progress notes

(2026-05-30) Goal created after loading skill-creator. Ready to execute the restructure.

(2026-05-30) Restructure executed:
- Created parent `.agents/skills/mmr/SKILL.md`
- Moved `mmr/session-mining/` → `.agents/skills/mmr/prior-session-workflow-mining/`
- Updated AGENTS.md with both parent and subskill entries
- Updated internal self-references in the subskill
- Updated the two previous goal documents
- Old top-level directory removed
- Structure verified. The system has already announced the new `mmr` parent skill.
