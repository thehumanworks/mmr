# Extraction jq Patterns for mmr Session Transcripts

These patterns are tuned for the `ApiMessagesResponse` shape returned by `mmr recall` and `mmr read session`. They assume the file has been saved because the full payload is frequently >100k lines when pretty-printed or when many turns contain large tool outputs / file pastes.

## Basic hygiene & structure

```bash
# Is this a valid transcript response?
jq -e 'has("messages") and (.messages | type == "array") and (.messages | length > 0) and has("total_messages")' the-session.json

# Session context (critical for "which session did I actually get?")
jq '.session_selection // "no session_selection (plain messages query)"' the-session.json

# Message count vs declared total (they should match for an un-paged full session fetch)
jq '{declared_total: .total_messages, actual_array: (.messages | length)}' the-session.json
```

## Role distribution (who drove the session)

```bash
jq -r '
  .messages
  | group_by(.role // .type // "unknown")
  | map({role: .[0].role // .[0].type // "unknown", count: length})
  | sort_by(-.count)
' the-session.json
```

Typical healthy coding session: many more "user" turns (the UI injecting file contents, command output, and tool results) than "assistant" turns (the reasoning steps you actually want to mine).

## All assistant turns (the "how the agent worked" trace) — write to file

```bash
jq -r '
  .messages
  | to_entries[]
  | select( (.value.role // .value.type) == "assistant" )
  | "=== TURN \(.key) @ \(.value.timestamp // .value.created_at // "n/a") ===\n\((.value.content // .value.text // "") | tostring)\n"
' the-session.json > /tmp/assistant-trace.txt && ${PAGER:-less} /tmp/assistant-trace.txt
```

## Keyword mining for ritual / workflow language (case-insensitive)

```bash
# Common verification, TDD, goal, skill, subagent, file-tool, design-panel signals
KEYWORDS='verification|verification loop|cargo (fmt|test|clippy|build)|TDD|write the (failing )?test|goal doc|goals/|SKILL.md|create skill|read_file|search_replace|subagent|spawn_subagent|design panel|four design lenses|adversarial|synthesis stage|background.*task|Task ID'

jq -r --arg re "$KEYWORDS" '
  .messages
  | to_entries[]
  | select( ((.value.content // .value.text // "") | tostring | ascii_downcase) | test($re) )
  | "[\(.key)] \(.value.role // .value.type // "?") @ \(.value.timestamp // "n/a")\n  preview: \(((.value.content // .value.text // "") | tostring | .[0:220] | gsub("\n"; " ")))"
' the-session.json
```

## Find the exact turns where a particular file was read or a command was suggested

```bash
# Example: every time AGENTS.md or a .cursor/rule was injected or discussed
jq -r '
  .messages
  | to_entries[]
  | select( ((.value.content // .value.text // "") | tostring | test("AGENTS.md|verification-loop|cli-contract|test-discipline|ingest-parsing"; "i")) )
  | "[\(.key)] \(.value.role//.value.type)"
' the-session.json
```

## Locate the "load-bearing fact verification" moments (high signal)

```bash
jq -r '
  .messages
  | to_entries[]
  | select( ((.value.content // .value.text // "") | tostring | ascii_downcase) | (contains("before i") and (contains("verify") or contains("confirm") or contains("check that") or contains("load-bearing"))) )
  | "[\(.key)] " + ((.value.content // .value.text // "") | tostring | .[0:300] | gsub("\n"; " | "))
' the-session.json
```

These turns are gold: the agent explicitly paused synthesis to go read one more source file or re-run one more command to de-risk a claim.

## Session metadata + skipped newest (live session guard)

```bash
jq '
  .session_selection
  | {scope, axis, total_sessions_in_scope, selected: [.selected[] | {age, session_id, message_count, first_timestamp, last_timestamp}], skipped_newest}
' the-session.json
```

Use the `skipped_newest.assumed_live` flag in your analysis narrative: "age 0 was correctly excluded because it was still being written."

## Export just the message array for downstream tools (when you really need the raw events)

```bash
jq '.messages' the-session.json > the-session.messages-only.jsonl   # still one JSON array
# or as NDJSON if a later tool prefers streaming lines
jq -c '.messages[]' the-session.json > the-session.messages.ndjson
```

## One-liner stats block (paste into progress notes)

```bash
echo "Transcript: $(ls -lh the-session.json | awk '{print $5, $9}')"
echo "Messages: $(jq '.messages | length' the-session.json)"
echo "Span: $(jq -r '.messages[0].timestamp // .messages[0].created_at' the-session.json) → $(jq -r '.messages[-1].timestamp // .messages[-1].created_at' the-session.json)"
echo "Session ID: $(jq -r '.session_selection.selected[0].session_id // "n/a"' the-session.json)"
echo "Assistant turns: $(jq '[.messages[] | select((.role//.type)=="assistant")] | length' the-session.json)"
```

## Tips

- The JSON from mmr is usually compact (escaped newlines inside strings). Do not pretty-print the whole file for editing; it explodes size and makes line-oriented tools (read_file with offset, grep -n) less useful.
- When you need a human-readable slice, always write the jq output to /tmp/ or a scratch file under the current goal dir rather than dumping thousands of lines into the chat.
- Cross-check any "this command was run" claim by grepping the raw file for the literal argv. The assistant often *describes* running `cargo test` without the UI actually executing it in that turn; only user messages containing the literal output or the rule file content are ground truth for "the verification loop was performed here".
- The inciting transcript that motivated this skill (2026-05-29 session 6fe9ec0e-d668-4845-b99b-b4eb6bdc68b2, the design session that produced the repo's first strict GOAL.md) is the canonical worked example in the creating goal `goals/2026-05-30-retrieve-claude-prior-session-consolidate-workflows-skills.md` (see that document's "Findings" table and its own nuance about when the dated `goals/` + YAML `status` + full DoD conventions were standardized after the session).

Add new patterns here as they prove useful across multiple mining sessions.

## Limitations of these patterns (post-review addition)

These patterns were derived from a single 41-message Claude Code transcript (minified one-line JSON, Claude-specific XML command fragments for some tool activity, heavy "user" messages that are the UI pasting full file contents after each read request, no traditional OpenAI-style `tool_calls` array in the mmr-normalized shape).

- **Provider variance:** Codex, Grok, Cursor, and Pi traces will have different shapes (different role distributions, different ways tool/file context is injected, different timestamp fields, different session metadata). The core "persist → stats → assistant turns → keyword mining for verification/GOAL/skill language" workflow is expected to transfer; the exact jq selectors may need small adjustments.
- **Session size & shape variance:** A 2000-message trace or one with heavy parallel subagent output will surface new repeated work (more sophisticated background-task polling patterns, different /tmp synthesis doc handling). The "write readable trace to /tmp first" rule becomes even more important.
- **Evidence citation style:** The parent goal that created this skill used a 5-column evidence table. Future mining runs on other projects may prefer different output formats. The skill deliberately keeps the "produce an evidence table in your current goal doc" step open-ended.

When you mine a new trace and the patterns need tuning, please contribute the adjusted jq (or new helper scripts) back to this references file so the library improves for everyone. This is exactly the "look for repeated work and bundle it" improvement loop recommended by the skill-creator contract.
