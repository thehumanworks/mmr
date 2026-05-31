---
name: mmr
description: "mmr is the local tool for querying and retrieving AI coding session history across Claude, Codex, Cursor, Grok, and Pi. Use it when you need to inspect past work, pull previous sessions into context, or maintain long-term continuity. Primary entry point for all mmr-related capabilities."
---

# mmr

`mmr` is the local Rust CLI for parsing and querying history from Claude Code, Codex, Cursor, Grok, and Pi.

## Core Use Cases

- Discover and list projects and sessions
- Retrieve previous sessions by recency (`mmr prev`, `--session-back`, `--session-range`)
- Export full transcripts
- Generate continuity briefs (`mmr summary` / `mmr remember`)

## Subskills

This parent organizes specialized mmr workflows.

### session-mining (Recommended for Continuity Work)

**Location:** `mmr/session-mining`

This is the main subskill for **long-term session continuity**.

Use `session-mining` when you need to:
- Deliberately retrieve one or more prior sessions (via `--session-back`, `--session-range`, `prev`, or explicit IDs)
- Analyze them for decisions, architecture, open tasks, and rationale
- Survive context **compaction** or full **clearing** by pulling relevant history back into the current context
- Produce structured reminders or continuity briefs

It provides reusable patterns and guidance beyond what the basic `mmr` commands or `mmr summary` deliver on their own.

See the subskill documentation: `.agents/skills/mmr/session-mining/SKILL.md`

## When to Use This Parent Skill

Use the top-level `mmr` skill when:
- You have a general question about mmr commands, flags, or behavior
- You're not sure which specific capability or subskill applies
- You're setting up, linking, or troubleshooting mmr itself

For most continuity and previous-session work, load the `session-mining` subskill directly.

## Related Local Skills

- `mmr-clap-colored-cli` — Developing the mmr CLI surface, contracts, and output behavior
- `mmr-teleport-providers` — Packing and applying native session bundles across providers

Prefer the most specific skill for the task at hand.
