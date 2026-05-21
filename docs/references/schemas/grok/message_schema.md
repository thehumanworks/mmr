# Grok Messages Schema

This document describes the Grok session files that `mmr` ingests from `~/.grok/sessions/`.

## File Layout

Each Grok session lives under a percent-encoded project directory:

```text
~/.grok/sessions/<percent-encoded-project>/<session-id>/
  summary.json
  updates.jsonl
```

Examples:

- Project directory: `%2FUsers%2Ftest%2Fgrok-proj`
- Decoded fallback project path: `/Users/test/grok-proj`

`mmr` ignores session directories that do not contain `updates.jsonl`.

## `summary.json`

`summary.json` is optional metadata. When present, `mmr` reads:

| Field | Meaning |
| --- | --- |
| `info.id` | Preferred session ID |
| `info.cwd` | Preferred project path |
| `current_model_id` | Initial model for subsequent messages |
| `created_at` | Fallback timestamp |

Fallback behavior when `summary.json` is missing or incomplete:

- `session_id` falls back to the session directory name.
- `project_name` falls back to the percent-decoded project directory name.
- `model` starts empty until a later update supplies one.

## `updates.jsonl`

`updates.jsonl` is JSONL: one JSON object per line.

- Empty lines are skipped.
- Malformed JSON lines are skipped.
- Only lines with `params.update.sessionUpdate` equal to `user_message_chunk` or `agent_message_chunk` are ingested.

Other update types such as `available_commands_update` are ignored.

### `user_message_chunk`

User message chunks produce one `user` message when the extracted text is non-empty.

Relevant fields:

| Field | Meaning |
| --- | --- |
| `params.update.content` | Message content |
| `params.update._meta.modelId` | Updates the current model when present |
| `params._meta.agentTimestampMs` | Preferred timestamp in milliseconds |
| `timestamp` | Fallback timestamp when `_meta.agentTimestampMs` is absent |

Text extraction rules:

- Strings are used directly.
- Arrays are recursively flattened and joined with newlines.
- Objects contribute `text` directly, or recurse into `content`.

### `agent_message_chunk`

Assistant output is streamed as one or more `agent_message_chunk` lines.

`mmr` accumulates consecutive assistant chunks into a single `assistant` message:

- text from each non-empty chunk is appended in order
- the stored timestamp comes from the first chunk in that assistant run
- the stored line index also comes from the first chunk in that assistant run
- the stored model is whatever the current model was when the assistant run started

The pending assistant message is flushed when:

- a later `user_message_chunk` arrives, or
- the file ends

## Timestamp Rules

Timestamps are normalized as follows:

1. `params._meta.agentTimestampMs` (preferred, converted from Unix milliseconds to RFC 3339)
2. top-level `timestamp` string
3. top-level `timestamp` numeric value (treated as Unix seconds)
4. `summary.json.created_at`

If all timestamp sources are absent, the message is still emitted with an empty timestamp string.

## `mmr` Mapping

| `mmr` field | Source |
| --- | --- |
| `source` | `"grok"` |
| `project_name` | `summary.json.info.cwd`, else percent-decoded project directory name |
| `project_path` | Same value as `project_name` |
| `session_id` | `summary.json.info.id`, else session directory name |
| `role` | `"user"` or `"assistant"` from the ingested update kind |
| `content` | Extracted text; assistant chunks are concatenated |
| `model` | `current_model_id`, then updated by `params.update._meta.modelId` |
| `timestamp` | Derived from `_meta.agentTimestampMs`, `timestamp`, or `created_at` |
| `is_subagent` | Always `false` |
| `msg_type` | Same as `role` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |

## Operational Notes and Pitfalls

- Direct Grok storage uses percent-encoded project directories, but `mmr export` and project resolution use the decoded canonical path.
- Assistant streaming is lossless only for text content. Non-text update payloads are ignored.
- Invalid JSON lines do not abort ingestion; `mmr` keeps later valid lines.

## Fixture Example

The integration fixture in `tests/common/mod.rs` seeds this shape:

```json
{"timestamp":1736035201,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello from grok"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736035201000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi "}},"_meta":{"agentTimestampMs":1736035202000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"from grok assistant"}},"_meta":{"agentTimestampMs":1736035202100}}}
```
