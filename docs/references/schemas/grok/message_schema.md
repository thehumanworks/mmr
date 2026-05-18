# Grok Messages Schema

This document specifies the on-disk Grok session format that `mmr` ingests from `~/.grok/sessions/`.

## File layout

- **Location**: `~/.grok/sessions/<percent-encoded-project>/<session-id>/`
- **Required files**:
  - `summary.json`
  - `updates.jsonl`
- **Format**:
  - `summary.json` is a single JSON object
  - `updates.jsonl` is one JSON object per line

The project directory name is a percent-encoded path fallback such as `%2FUsers%2Fme%2Fproj`. When `summary.json` includes `info.cwd`, `mmr` prefers that decoded path instead of the directory-name fallback.

## `summary.json`

`mmr` reads these fields from `summary.json`:

| Field | Type | Purpose |
| --- | --- | --- |
| `info.id` | string | Fallback session ID |
| `info.cwd` | string | Preferred project name / project path |
| `current_model_id` | string | Initial model for later messages |
| `created_at` | string | Fallback timestamp |

If `summary.json` is missing or malformed, `mmr` falls back to the directory names and later update records where possible.

## `updates.jsonl`

Each line is parsed defensively. Malformed lines are skipped.

`mmr` reads only records where `params.update.sessionUpdate` is one of:

- `user_message_chunk`
- `agent_message_chunk`

Other update types are ignored.

### Relevant fields

| Field | Type | Description |
| --- | --- | --- |
| `params.sessionId` | string | Session identifier |
| `params.update.sessionUpdate` | string | Update type |
| `params.update.content` | mixed | Message content |
| `params.update._meta.modelId` | string | Model override seen on user chunks |
| `params._meta.agentTimestampMs` | number | Preferred timestamp source in milliseconds |
| `timestamp` | string or number | Fallback timestamp |

### Content extraction

`params.update.content` may be:

- **String**: used directly
- **Array**: recursively extracted and joined with `\n`
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise ignored

### Assistant chunk coalescing

Consecutive `agent_message_chunk` records are merged into a single assistant message. `mmr` flushes the pending assistant message:

- when the next `user_message_chunk` arrives
- or at end of file

Merged assistant chunk text is concatenated in encounter order.

### Timestamp resolution

For each ingested message, `mmr` uses the first non-empty timestamp source in this order:

1. `params._meta.agentTimestampMs` converted from Unix milliseconds to RFC 3339
2. top-level `timestamp` when it is already a non-empty string
3. top-level `timestamp` interpreted as Unix seconds when it is numeric
4. `summary.created_at`

## mmr mapping

| mmr field | Source |
| --- | --- |
| `source` | `"grok"` |
| `project_name` | `summary.info.cwd`, else decoded project directory name |
| `project_path` | same value as `project_name` |
| `session_id` | `summary.info.id`, else session directory name |
| `role` | `"user"` or `"assistant"` from the update type |
| `content` | extracted from `params.update.content` |
| `model` | current model from `summary.current_model_id` and later `params.update._meta.modelId` |
| `timestamp` | resolved using the precedence above |
| `is_subagent` | `false` |
| `msg_type` | `"user"` or `"assistant"` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |

## Example

```json
{"info":{"id":"sess-grok-1","cwd":"/Users/me/proj"},"current_model_id":"grok-build","created_at":"2025-01-09T00:00:00Z"}
```

```json
{"timestamp":1736380801,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736380801000}}}
{"timestamp":1736380802,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}},"_meta":{"agentTimestampMs":1736380802000}}}
```
