---
title: "Adversarial review of mmr/prior-session-workflow-mining skill (via two skill-creator-loaded subagents) and incorporation of improvements"
description: "Launch two independent subagents. Each must first load and internalize /Users/mish/.agents/skills/skill-creator/SKILL.md. Task them with a rigorous adversarial (critical, gap-finding) review of the newly created .agents/skills/mmr/prior-session-workflow-mining/ skill and its references. Collect concrete, prioritized suggested improvements from both. Then edit the skill (and supporting files) to address the highest-value feedback. Update the parent goal doc, run the skill's own verification steps, and close with evidence that the reviews were performed and improvements landed."
date: 2026-05-30
status: done
---

# GOAL: Adversarial review (2× skill-creator subagents) + remediation of mmr/prior-session-workflow-mining skill

## Outcome

Two separate subagents are spawned. **Each is explicitly instructed (and verified) to first read and load the full content of `/Users/mish/.agents/skills/skill-creator/SKILL.md`** before looking at any of my work.

Each performs a thorough **adversarial review** of:
- [.agents/skills/mmr/prior-session-workflow-mining/SKILL.md](/Users/mish/projects/mmr/.agents/skills/mmr/prior-session-workflow-mining/SKILL.md)
- [.agents/skills/mmr/prior-session-workflow-mining/references/extraction-jq-patterns.md](/Users/mish/projects/mmr/.agents/skills/mmr/prior-session-workflow-mining/references/extraction-jq-patterns.md)
- The context in which it was created (the parent goal + the mined transcript)

They produce structured, critical feedback with specific, actionable improvement suggestions (not vague praise).

I then:
- Synthesise the two reviews (common themes + unique high-value items)
- Make targeted edits to the skill + references to address the feedback
- Update this goal document (and the parent 2026-05-30 goal's progress notes) with the reviews received + changes made
- Re-execute the skill's own verification commands and any new quality checks suggested by the reviewers
- Mark all checkboxes and set status `done`

The final state of the skill is measurably stronger because of the adversarial process.

## Why this shape (read before acting)

The skill I just created (`mmr/prior-session-workflow-mining`) is meta — it helps agents improve the harness by mining real prior sessions. It must itself be high-quality, non-duplicative, and follow the standards defined in the very `skill-creator` skill that exists for this purpose.

Running the review through subagents that are *forced* to load `skill-creator` first ensures the critique uses the project's own (or global) definition of "what makes an excellent skill" rather than my own judgment. Using *two* independent reviewers with an adversarial stance reduces single-reviewer bias and surfaces blind spots.

This is the "review / check-work / skill-creator" loop made concrete and mandatory for any newly authored skill in this environment.

## Surface touched

- New goal document: `goals/2026-05-30-adversarial-review-mmr/prior-session-workflow-mining-skill.md` (this file — created first)
- The skill under review and its references (edits only after reviews received)
- Parent goal `goals/2026-05-30-retrieve-claude-prior-session-consolidate-workflows-skills.md` (append-only progress notes + any new DoD items)
- Possibly `AGENTS.md` (only if a discoverability improvement is recommended and accepted)
- MCP goal-tasks (new goal or tasks under id 7 or a fresh goal for this review phase)
- Subagent outputs (will be captured in the goal doc or /tmp/ review files)

**No changes to mmr source code, CLI, or existing rules are in scope** unless a reviewer makes an extremely strong case that rises to a separate goal.

## Missing context at kickoff

- The detailed quality rubric inside `/Users/mish/.agents/skills/skill-creator/SKILL.md` (must be read by the subagents and by me)
- What the two independent adversarial reviewers will actually surface (unknown until they run)
- Which of their suggestions are high-value vs. stylistic or out-of-scope

## Decisions (locked)

1. Subagents **must** load the skill-creator skill as their very first action (instructed in the spawn prompt + verified by asking them to quote or summarize key sections from it).
2. Reviews are adversarial by design: the prompt will explicitly say "be critical, look for flaws, gaps, over-claims, missing sections, poor triggers, duplication risks, clarity problems, and violations of the skill-creator standards".
3. Two reviewers (not one). Slight differentiation in focus:
   - Reviewer A: Completeness, structure, adherence to skill-creator dimensions, clarity for the target user (agent doing optimise-claude style work).
   - Reviewer B: Overlap/duplication with globals, trigger accuracy ("Use when"), longevity/maintainability, evidence quality in the skill itself.
4. I will not edit the skill until *both* reviews are received and I have synthesised them in this goal doc.
5. All suggested improvements that are accepted will be implemented with before/after diffs recorded here.
6. The parent transcript and the parent goal doc remain the ground truth for any "evidence" claims in the skill.

## Non-goals

- Re-reviewing the *parent* goal or the transcript mining process itself (unless a reviewer legitimately calls the skill's description of its own origin into question).
- Creating additional new skills as a result of this review (that would be a follow-up goal).
- Forcing every possible micro-suggestion into the skill (we will prioritise by value-to-effort and fidelity to the skill-creator rubric).

## Phased plan

1. **Bootstrap (this step)**
   - Create this goal document (first write).
   - Register via `goal-tasks__set_goal` (new goal or continuation tasks).
   - Create MCP tasks for "launch reviewers", "receive & synthesise reviews", "implement improvements", "re-verify + close".
   - Confirm exact path to skill-creator: `/Users/mish/.agents/skills/skill-creator/SKILL.md`.
   - Read the skill-creator myself (so I understand the criteria the reviewers will apply).

2. **Launch the two adversarial subagents**
   - Use `spawn_subagent` (twice, in parallel).
   - Each prompt begins with: "FIRST ACTION: Read the entire file at `/Users/mish/.agents/skills/skill-creator/SKILL.md` using the read_file tool. Summarize its core evaluation dimensions and quality bar in your thinking before you touch the target skill."
   - Provide the target skill paths + parent goal + context about why it was created.
   - Instruct them to produce a structured adversarial review report (template provided in prompt: scores or red/yellow/green per dimension, specific quotes from the skill, concrete "Suggested improvement: ..." items with rationale).

3. **Receive, capture, and synthesise the two reviews**
   - Use `get_command_or_subagent_output` (or wait) until both complete.
   - Paste key excerpts + full suggested improvement lists into this goal document (or linked /tmp/ files).
   - Produce a merged prioritised improvement backlog (Must / Should / Could).

4. **Implement the accepted improvements**
   - Edit the SKILL.md and/or references/ using search_replace (smallest changes that address the feedback).
   - Record before/after for each material change.
   - If a reviewer recommends something that would require a new goal (e.g. "this should be a global skill instead"), document the decision and do not do it here.

5. **Re-verify + close**
   - Re-run every verification command listed inside the (now improved) skill.
   - Run any additional quality checks the reviewers or skill-creator recommend (e.g. if it has an eval mode).
   - Update parent goal's progress notes.
   - Flip all checkboxes here.
   - Set this goal `status: done` and MCP to 100%.
   - Deliver final response that explicitly walks the user-mandated 5-step rule and shows the review artifacts + the diffs of improvements made.

## Validation (exact commands to run after improvements)

At minimum (from the skill itself):

```bash
# Retrieval hygiene (still works after any edits to the skill)
test -f goals/2026-05-30-claude-prior-session.json
jq -e 'has("messages") and (.messages | length == 41)' goals/2026-05-30-claude-prior-session.json

# The skill's own verification examples (run the jq patterns it documents)
jq -e 'has("messages") and (.messages | length > 0)' goals/2026-05-30-claude-prior-session.json
# ... plus the specific assistant-turn and keyword extractions the skill recommends
```

Additional post-review checks:
- `cat .agents/skills/mmr/prior-session-workflow-mining/SKILL.md | head -50` (frontmatter + Quick Start still clean)
- Grep for any new "Use when" language added on reviewer advice.
- If skill-creator has an explicit `review` or `eval` subcommand or script, run it against our skill (document what was run).

All command output must be captured in this goal doc or linked files.

## Definition of Done

- [ ] This adversarial review goal document created *before* any subagent is spawned or the skill is edited.
- [ ] New (or continuation) goal registered in goal-tasks MCP with clear verification contract.
- [ ] Both subagents were given prompts that *mandate* loading `/Users/mish/.agents/skills/skill-creator/SKILL.md` as their literal first action (evidence: their early output quotes or summarizes sections of it).
- [ ] Two independent adversarial review reports received and pasted (or linked) into this document.
- [ ] Prioritised improvement backlog created from the union of the two reviews.
- [ ] All "Must" and all reasonable "Should" items implemented with recorded diffs.
- [ ] Skill's own verification commands re-executed successfully against the updated files.
- [ ] Parent goal `2026-05-30-retrieve-...` has an append-only progress note summarising the review outcome and changes.
- [ ] All checkboxes in this document checked; `status: done`; MCP shows 100% for the review goal.
- [ ] Final user response explicitly traverses `understand -> retrieve_context -> act -> synthesise -> respond` and includes:
  - Summary of the two reviews (strengths called out + key criticisms)
  - The concrete improvements made (with file + line or diff excerpts)
  - Paths to the review artifacts if they are large
  - Confirmation that the skill is now stronger per the skill-creator standards.

## References & inputs for the reviewers (and for me)

- Skill under review: `.agents/skills/mmr/prior-session-workflow-mining/SKILL.md` + `references/extraction-jq-patterns.md`
- Origin story & evidence: the parent goal `goals/2026-05-30-retrieve-claude-prior-session-consolidate-workflows-skills.md` (especially the "Findings from the mined transcript" table) and the transcript file itself.
- The skill-creator contract: `/Users/mish/.agents/skills/skill-creator/SKILL.md` (mandatory first read for every reviewer)
- Related existing local skills (for overlap check): the two mmr-* siblings.
- Global patterns the skill already references (best-of-n, harness-engineering, optimise-claude, review-skill, create-skill, etc.).

## Progress notes (append-only, newest at top)

(2026-05-30 initial creation) Goal document for the adversarial review phase created. Skill-creator path confirmed as `/Users/mish/.agents/skills/skill-creator/SKILL.md`. Read key sections of skill-creator (anatomy, progressive disclosure, writing style, validation, improvement loop). MCP goal id=8 + 5 phase tasks registered. 

(2026-05-30 12:xx) Phase 2 launch complete. Two independent general-purpose subagents spawned in parallel (background):
- Reviewer A (completeness/structure/clarity/skill-creator adherence focus): subagent_id 019e787c-7830-79c2-ad67-b495a5a3dd77 (152.8s, 13 tool calls)
- Reviewer B (duplication/triggering/maintainability/evidence narrowness/longevity focus): subagent_id 019e787c-919e-7250-9ea4-241a09dedbec (282.4s, 22 tool calls)

Both prompts contained the non-negotiable first action. Both complied exactly (verified in their outputs): each used read_file on the *entire* 502-line /Users/mish/.agents/skills/skill-creator/SKILL.md as their literal first tool call, then (before reading the target skill or even the parent goal in some paths) quoted/summarised the required elements:
- Core creation/improvement loop (draft → test prompts → Codex-with-skill runs + baseline → qualitative/quantitative eval via generate_review.py + benchmark.json → rewrite → repeat)
- Anatomy + progressive disclosure (<500 lines SKILL.md body ideal; references/ for large supporting material; clear pointers)
- Writing style (imperative, "explain the why" instead of heavy MUSTs, generalize/not super-narrow to examples, Principle of Lack of Surprise)
- Validation (run skills-ref validate or equivalent after any edit, before packaging)
- Improvement philosophy (generalize from feedback, keep lean, explain why, look for repeated work across cases and bundle into scripts/)

(2026-05-30 post-review) Phase 3 synthesis complete. Both reviewers produced convergent, high-signal adversarial reports (full raw outputs in the tool history for this goal; key excerpts below). 

**Convergent Overall Assessment (quoted from Reviewer A; B is nearly identical):**
"The skill is a conscientious one-shot capture of real, high-signal patterns from a single 41-message transcript (the design session that produced the repo's first strict GOAL.md), with a genuinely useful 118-line jq reference library and clean directory hygiene. However, it was authored and 'self-announced' while completely bypassing the skill-creator's required creation loop (no test prompts, no `evals/evals.json`, no runs with/without the skill, no `eval-viewer`, no iteration, and no `skills-ref validate` execution recorded anywhere in the parent goal or skill itself). It hard-codes its own birth-event filename and counts into the 'Verification' section (guaranteed to mislead or fail future users) and repeatedly over-claims 'birth transcript' status in ways that contradict the nuance the parent goal itself documents. It reads like a high-quality personal notebook that was prematurely promoted to a reusable skill without the hardening the skill-creator process exists to provide."

**Must fixes (identical core items from both reviewers; these are non-negotiable):**
1. **Verification section hard-codes birth artifacts** (`goals/2026-05-30-claude-prior-session.json`, `== 41`, specific grep). Must generalize with variables + comment "adjust numbers" (directly violates skill-creator "generalize... not super-narrow to specific examples").
2. **No test cases / evals / demonstration of the skill-creator loop** the skill claims to respect. Must add at least a "Test cases & evals" section or minimal `evals/evals.json` + 2-3 realistic prompts.
3. **Over-claim on "birth transcript"** language (SKILL.md:45, references:115) that the parent goal itself nuances (the dated `goals/`, YAML `status`, Progress notes, full DoD were standardized *after* the mined session; the mined session produced an earlier root GOAL.md). Must qualify the language to match the parent goal's own precise timeline note.

**Strong Should (converged):**
- Run `skills-ref validate` (the mandatory post-creation step from the very skill-creator we forced them to load) and record output.
- Extract repeated long jq one-liners into `scripts/` (per "look for repeated work" philosophy in skill-creator).
- Lead Quick Start with the "write readable trace to /tmp first" usability tip (currently buried).
- Add explicit "Limitations of these patterns" subsection in references/ (one 41-msg minified Claude trace; other providers differ).

**Could + other observations:** More trigger phrases in description; honest meta note that this skill itself skipped the full eval loop at birth; tiny self-contained example evidence table; strong recommendation that a full skill-creator eval/iteration loop (3-4 new prior sessions across providers) is warranted before wider use. Both note the jq library itself is the highest-value part and worth hardening.

Full raw reviewer outputs (including their own compliance verification, tool call counts, and cross-checks against the actual 238k-char minified transcript + parent goal) are in the conversation history for this goal. The convergence between two independent adversarial reviewers (different emphasis prompts, different durations/tool counts) is high-signal validation of the real issues.

Prioritised backlog for Phase 4 implementation (all Must + top 4 Should):
- Must 1: Generalize Verification section (highest impact, lowest effort).
- Must 2: Add minimal Test cases & evals section.
- Must 3: Qualify all "birth transcript" claims.
- Should 1: Run `skills-ref validate` (if the binary/script is discoverable in the environment) or document the attempt + note the blocker.
- Should 2: Extract one high-value repeated jq helper into scripts/ + update Quick Start/Core Rules.
- Should 3: Reorder Quick Start to lead with the minified-trace usability tip.
- Should 4: Add Limitations subsection to references/.

No changes outside the skill + references + goal docs + parent progress note. No Rust. After edits: re-run the (now generalized) verification commands from the skill itself.

## Reproducibility

Exact skill-creator file that must be loaded first by both subagents:
`/Users/mish/.agents/skills/skill-creator/SKILL.md`

Target skill (the one being reviewed):
`/Users/mish/projects/mmr/.agents/skills/mmr/prior-session-workflow-mining/SKILL.md`

Parent context (the session that was mined and the goal that created the skill):
- Transcript: `goals/2026-05-30-claude-prior-session.json`
- Parent goal: `goals/2026-05-30-retrieve-claude-prior-session-consolidate-workflows-skills.md`

## Subagent prompt template (to be used when spawning)

```
You are an expert, highly critical reviewer of AI agent skills.

FIRST ACTION — MANDATORY AND NON-NEGOTIABLE:
Use the read_file tool (or equivalent) to load the *entire* content of:
  /Users/mish/.agents/skills/skill-creator/SKILL.md
Do not proceed to the target skill until you have done this. In your first output, quote or accurately summarise at least the core evaluation dimensions, quality bar, and any checklists or red flags defined in skill-creator. This is how we will verify you followed the instruction.

After you have internalised skill-creator, perform a rigorous *adversarial* review of the target skill at:
  /Users/mish/projects/mmr/.agents/skills/mmr/prior-session-workflow-mining/SKILL.md
(and its references/ subdirectory).

Context you may also read (strongly recommended):
- The parent goal that created it: /Users/mish/projects/mmr/goals/2026-05-30-retrieve-claude-prior-session-consolidate-workflows-skills.md (focus on the "Findings" table and the evidence citations)
- The actual transcript that was mined: the json file referenced in the parent goal.

Your review must be adversarial: actively hunt for weaknesses, gaps, over-claims, unclear triggers, violations of the skill-creator standards, duplication risks with global skills, poor structure, missing examples, maintainability problems, etc.

Structure your final report as:
1. Overall assessment (2-4 sentences, brutally honest)
2. Dimension-by-dimension analysis (use whatever dimensions skill-creator defines, plus any others you find relevant). For each: Green / Yellow / Red + 1-3 sentence justification with direct quotes from the target skill.
3. Concrete suggested improvements (prioritised: Must / Should / Could). Each item must be specific ("Change the Quick Start example to...", "Add a 'When NOT to use' section because...", "The trigger phrase 'Use when you want to improve agent harness' is too vague — suggest tightening to...").
4. Any other high-signal observations.

Be direct. Do not soften criticism. The goal is to make the skill stronger before it is widely used.
```

(The second reviewer gets a slightly varied focus prompt.)

This goal will capture the actual prompts used and the raw subagent outputs.
