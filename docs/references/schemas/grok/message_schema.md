# Grok Messages Schema

This document specifies the session directory layout and update stream currently ingested by `mmr` for Grok history.

## File layout

- **Location**: `~/.grok/sessions/<percent-encoded-project>/<session_id>/`
- **Required chat file**: `updates.jsonl`
- **Optional summary file**: `summary.json`

`mmr` scans each session directory that contains `updates.jsonl`.

## Project and session identity

- The project directory name is percent-decoded as a fallback project identifier.
- `summary.json.info.cwd` overrides that fallback project identifier when present.
- `summary.json.info.id` overrides the session directory name when present.

## `summary.json`

`mmr` reads a small summary when available:

| Field | Description |
| --- | --- |
| `info.id` | preferred session ID |
| `info.cwd` | preferred project path/name |
| `current_model_id` | starting model value |
| `created_at` | fallback timestamp |

If `summary.json` is missing or malformed, `mmr` falls back to directory-derived values.

## `updates.jsonl` filtering

Each line is parsed independently. Malformed JSON lines are skipped.

Only updates where `params.update.sessionUpdate` is one of the following are ingested:

- `user_message_chunk`
- `agent_message_chunk`

Other update types are ignored.

## Content extraction

`params.update.content` may be:

- **string**: used directly
- **array**: recursively extracted and joined with `\n`
- **object**:
  - `text`: used directly
  - `content`: extracted recursively
  - otherwise ignored

## Assistant chunk coalescing

Consecutive `agent_message_chunk` records are buffered and emitted as a single assistant message.

- The buffered content is concatenated exactly as emitted by Grok.
- The assistant message is flushed when the next `user_message_chunk` arrives or when the file ends.

## Timestamp resolution

For each ingested update, `mmr` prefers timestamps in this order:

1. `params._meta.agentTimestampMs` converted from Unix milliseconds to RFC 3339
2. top-level `timestamp` string
3. top-level `timestamp` numeric seconds converted to RFC 3339
4. `summary.json.created_at`

## Model resolution

- `summary.json.current_model_id` seeds the current model.
- A non-empty `params.update._meta.modelId` on a user chunk replaces the current model.
- Buffered assistant chunks use the current model value at the time they are emitted.

## Example

`summary.json`

```json
{"info":{"id":"sess-grok-1","cwd":"/Users/test/grok-proj"},"created_at":"2025-01-05T00:00:00Z","current_model_id":"grok-build"}
```

`updates.jsonl`

```json
{"timestamp":1736035201,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello from grok"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736035201000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi "}},"_meta":{"agentTimestampMs":1736035202000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"from grok assistant"}},"_meta":{"agentTimestampMs":1736035202100}}}
```

## mmr mapping

| mmr field | Source |
| --- | --- |
| `source` | `"grok"` |
| `project_name` | `summary.json.info.cwd`, else percent-decoded project directory |
| `project_path` | same value as `project_name` |
| `session_id` | `summary.json.info.id`, else session directory name |
| `role` | derived from update type (`user` or `assistant`) |
| `content` | extracted update content, with assistant chunks coalesced |
| `model` | current Grok model derived from summary and update metadata |
| `timestamp` | resolved timestamp described above |
| `is_subagent` | `false` |
| `msg_type` | same as normalized role |
| `input_tokens` | `0` |
| `output_tokens` | `0` |

## Constraints

- Empty `session_id`, empty `project_name`, and empty message content are all dropped.
- Non-chat updates remain visible only in the raw JSONL; they do not appear in `mmr` output.
