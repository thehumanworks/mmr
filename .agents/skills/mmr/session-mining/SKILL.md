---
name: session-mining
description: "Use mmr recall and read session to retrieve previous coding sessions, analyze them, and extract continuity context. Essential for surviving context compaction and full clearing. Load this when you need to remind an agent (or yourself) of prior work, decisions, architecture, open tasks, or rationale from earlier sessions."
---

# mmr Session Mining

Retrieve, analyze, and work from previous AI coding sessions using `mmr recall` and `mmr read session`.

## Core Value

Modern agent sessions frequently suffer from:
- Context **compaction** (important details get summarized or dropped)
- Full **clearing** (the agent starts with almost no memory of prior work)

This subskill turns `mmr` into a reliable long-term memory layer. It lets you deliberately pull earlier sessions back into context so the agent can continue, reference, or be reminded of what actually happened.

## Supported Retrieval Methods

`mmr` offers two primary ways to select previous sessions:

- `mmr recall` / `mmr recall N` — the previous stable session, or N sessions back.
- `mmr read session <id>` — an explicit session ID when you already know it.

Prefer `mmr recall` when you are inside a project and want the immediate previous session without memorizing IDs.

## Quick Start

1. Retrieve the prior session into a file:
   ```bash
   # The single previous session (full content)
   mmr recall --source claude --limit 100000 > /tmp/prior-session.json

   # A known session ID
   mmr read session <session-id> --source claude > /tmp/session.json
   ```

2. Confirm you actually got the complete data (not a truncated page). Compare `total_messages` against the array length and check `next_page`:
   ```bash
   jq '{
     selected_sessions: [.session_selection.selected[].equivalent_command],
     declared_total:    .total_messages,
     retrieved:         (.messages | length),
     has_more_pages:    .next_page   # must be false
   }' /tmp/prior-session.json
   ```
   If `has_more_pages` is true, increase `--limit` or follow the `next_command` field.

3. Analyze for continuity value:
   - What were the major decisions, architecture choices, and open problems?
   - What work was in progress that might have been lost to compaction?
   - What context would a fresh agent (or a cleared context) desperately need?

4. Produce a **reminder artifact** (continuity brief, key decisions list, task inventory, etc.) that can be fed back into the current session or stored for later.

5. Use the results to improve long-term agent behavior (better handoff prompts, explicit memory notes via `mmr note`, improved `mmr summarize` workflows, etc.).

## Why This Skill Exists

Even excellent agents lose critical context over time. The ability to cheaply and reliably reach back into previous sessions is one of the highest-leverage capabilities for long-running projects.

This skill exists to make that retrieval + analysis + reminding loop repeatable, high-signal, and resistant to the two main context-loss events (compaction and clearing).

## Core Rules

- **Persist large retrievals to disk first.** Never rely on the model keeping the entire transcript in context.
- **Prefer `mmr recall`** over raw session IDs when you are working inside a project and want immediate continuity.
- **Age 0 is dangerous by default.** The newest session is often the caller's own live/incomplete session. Use `--include-newest` only when you deliberately want it.
- **Focus on continuity value**, not just "what happened." Ask: What would a future version of me (or a new agent) need to know that is at risk of being lost?
- **Produce reminder artifacts** that are actually usable (structured decisions, open tasks, rationale, invariants, etc.).
- **Update durable memory** when useful (`mmr note`, `mmr assimilate project`, project docs, etc.).
- **Subagent delegation is encouraged** for deep analysis of large ranges.

## Common Patterns

See `references/session-retrieval-patterns.md` for reusable jq helpers and analysis approaches, including:
- Extracting assistant reasoning turns
- Finding decision points and rationale
- Surfacing open tasks / TODOs that may have been dropped
- Comparing work across a session range
- Detecting compaction events (sudden drop in detail)

## Verification

After using the skill on a real trace, you should be able to answer:

- Which exact command(s) retrieved the session(s)?
- What was the `session_selection` metadata?
- What continuity-critical information did you surface that would otherwise have been lost?

## When NOT to Use This Skill

- You just need the raw messages from one known session for immediate context (use `mmr read session <id>` directly).
- The prior work is trivial or fully captured in current docs.
- You are doing ordinary day-to-day work inside a well-maintained project with low risk of context loss.

## Related Assets

- Parent: `.agents/skills/mmr/SKILL.md`
- `mmr summarize` — built-in continuity brief generator (this skill can feed richer input into it)
- `mmr note` + `mmr assimilate project` — for turning analysis into durable memory
- Other mmr siblings: `mmr-clap-colored-cli`, `mmr-native-bundle-providers`

## References

- `references/session-retrieval-patterns.md` — recommended patterns for retrieving and analyzing sessions for continuity
- `references/extraction-jq-patterns.md` — lower-level jq helpers (still useful)
- `~/.agents/skills/harness-engineering` — broader patterns for long-running agent memory (if present in your environment)
- `~/.claude/commands/optimise-claude.md` (and equivalents) — higher-level drivers that benefit from strong session mining
