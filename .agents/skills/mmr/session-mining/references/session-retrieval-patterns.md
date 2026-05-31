# Session Retrieval & Continuity Analysis Patterns for mmr

These patterns help you use `mmr recall` and `mmr read session` to pull prior work back into context, especially to survive compaction and clearing.

## Basic Hygiene (always do this)

```bash
# After any retrieval
TRANSCRIPT="/tmp/prior.json"

jq -e 'has("messages") and (.messages | length > 0)' "$TRANSCRIPT"
jq '.session_selection // "no session_selection"' "$TRANSCRIPT"
jq '{total: .total_messages, actual: (.messages | length)}' "$TRANSCRIPT"
```

## Retrieve Different Scopes

```bash
# Previous stable session (most common for "what did we do yesterday?")
mmr recall > "$TRANSCRIPT"

# N sessions back
mmr recall 5 > "$TRANSCRIPT"

# Explicit known session
mmr read session 6fe9ec0e-d668-4845-b99b-b4eb6bdc68b2 > "$TRANSCRIPT"

# Include the newest (age 0) only when you deliberately want the live session
mmr recall 0 --include-newest > "$TRANSCRIPT"
```

## Extract High-Value Continuity Content

Focus on these when analyzing for compaction/clearing survival:

- Major decisions and their rationale
- Architecture and invariants
- Open problems / blocked work / future work that was discussed
- Things that were "in flight" at the end of the session
- Explicit handoff notes or "remember to..." statements

Useful keyword families (adapt per project):

```bash
# Decisions + rationale
grep -iE 'decided|decision|we chose|because|rationale|tradeoff' "$TRANSCRIPT" | ...

# Open / future work
grep -iE 'TODO|FIXME|later|next|future|blocked|pending|open question' "$TRANSCRIPT" | ...

# Architecture / invariants
grep -iE 'invariant|must|never|always|architecture|design|principle' "$TRANSCRIPT" | ...
```

## Produce Usable Reminder Artifacts

Good outputs from this skill are things like:

- Structured "Key Decisions Since Last Major Session"
- "Work That Was In Progress" list
- "What a New Agent Would Need to Know"
- Updated project `CONTEXT.md` or `Handoff.md` sections

Feed these back into the current session, store them with `mmr note`, or use them to improve `mmr summarize` prompts.

## Limitations

- These patterns were heavily influenced by Claude Code traces (heavy file-paste user messages, specific XML tool fragments). Other providers (Codex, Grok, Cursor) will have different shapes.
- Very large source or project reads can produce enormous transcripts. Always persist first and consider sampling or targeted keyword extraction.
- Session age is relative to when `mmr` reads the provider history. Live sessions can still be in flight.

Contribute improved or provider-specific patterns back to this file.
