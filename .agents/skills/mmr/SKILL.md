---
name: mmr
description: "Surface memory and knowledge from past AI coding sessions (Claude Code, Codex, Cursor, Grok, Pi) before acting on work that may have prior history. Trigger when: starting or resuming work on an existing project or long-running task; the user references earlier work ('continue', 'as we discussed', 'last time', 'again'); recovering context after compaction or /clear; checking what was already tried, decided, or rejected before proposing an approach; or answering questions about past sessions. Also the entry point for any mmr setup, usage, or troubleshooting question."
---

# mmr

`mmr` is the local Rust CLI for parsing and querying history from Claude Code, Codex, Cursor, Grok, and Pi.

## When to Trigger

The purpose of this skill is continuity: act with awareness of prior work instead of rediscovering or contradicting it. Reach for it *before* starting, not only when explicitly asked about history.

- **Proactively**, before beginning or resuming work that plausibly has prior sessions: an existing project, a long-running task, a bug someone has looked at before.
- **On continuity language** from the user: "continue", "pick up where we left off", "as we discussed", "last time", "why did we...".
- **After context loss**: compaction, `/clear`, or a fresh session on ongoing work.
- **Before proposing an approach** that may already have been tried, decided, or rejected.

If `mmr` is not installed or a query fails, do not abandon the goal — fall back to raw provider transcripts and any memory directory, and note that retrieval was degraded. Retrieval indexes can lag the newest sessions; verify against raw transcripts when freshness matters.

## Core Use Cases

- Discover and list projects and sessions (`mmr list projects`, `mmr list sessions`)
- Retrieve the previous stable session (`mmr recall`, `mmr recall N`)
- Read raw history (`mmr read session`, `mmr read project`, `mmr read source --source <source>`)
- Generate summaries (`mmr summarize project|session|source`)
- Prepare assimilation handoffs (`mmr assimilate project|source`)
- Load or install this skill from the CLI (`mmr skill load`, `mmr skill install`, `mmr skill install --local`)

## Subskills

This parent organizes specialized mmr workflows.

### session-mining (Recommended for Continuity Work)

**Location:** `mmr/session-mining`

This is the main subskill for **long-term session continuity**.

Use `session-mining` when you need to:
- Deliberately retrieve prior sessions (via `mmr recall` or explicit `mmr read session <id>`)
- Analyze them for decisions, architecture, open tasks, and rationale
- Survive context **compaction** or full **clearing** by pulling relevant history back into the current context
- Produce structured reminders or continuity briefs with `mmr summarize`

It provides reusable patterns and guidance beyond what the basic `mmr` commands or `mmr summarize` deliver on their own.

See: `.agents/skills/mmr/session-mining/SKILL.md`

## When to Use This Parent Skill

Use the top-level `mmr` skill when:
- You have a general question about mmr commands, flags, or behavior
- You're not sure which specific capability or subskill applies
- You're setting up, initializing, or troubleshooting mmr itself
- You need to bootstrap this skill into a user or project skill directory

For most continuity and previous-session work, load the `session-mining` subskill directly.

## Related Local Skills

These exist only in the mmr development repo; skip this section if they are not present on this machine.

- `mmr-clap-colored-cli` — Developing the mmr CLI surface, contracts, and output behavior
- `mmr-native-bundle-providers` — Maintaining native session bundle profiles across providers
- `goal-driven-development` — Authoring and executing `goals/*.md` delivery contracts (`gdd_status.py`)

Prefer the most specific subskill for the task at hand; fall back to this parent when unsure.
