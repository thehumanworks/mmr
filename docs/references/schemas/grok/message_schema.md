# Grok Messages Schema

This document specifies the Grok session files ingested by mmr from `~/.grok/sessions/`.

## File Layout

- **Location**: `~/.grok/sessions/<percent-encoded-project>/<session_id>/`
- **Required transcript file**: `updates.jsonl`
- **Optional metadata file**: `summary.json`
- **Format**: JSON for `summary.json`; JSONL for `updates.jsonl`

The project directory name is percent-decoded when used as the fallback project identifier.

## Summary Metadata

When present, `summary.json` supplies fallback metadata:

| Field                    | Type   | Description |
| ------------------------ | ------ | ----------- |
| `info.id`                | string | Session ID fallback |
| `info.cwd`               | string | Preferred project path/name |
| `current_model_id`       | string | Default model for messages |
| `created_at`             | string | Fallback timestamp |

## Update Records

mmr reads `updates.jsonl` line by line and looks under `params.update`.

Only these update kinds are ingested:

- `user_message_chunk`
- `agent_message_chunk`

Other update types are ignored.

### User Chunks

User chunks create a new user message immediately.

Relevant fields:

| Field                              | Type   | Description |
| ---------------------------------- | ------ | ----------- |
| `params.update.content`            | mixed  | User content |
| `params.update._meta.modelId`      | string | Updates the current model when present |
| `params._meta.agentTimestampMs`    | number | Preferred timestamp in milliseconds |
| `timestamp`                        | string/number | Fallback timestamp |

### Assistant Chunks

Assistant chunks are buffered and merged until a user chunk or EOF flushes them into one assistant message. This prevents one logical response from turning into many tiny messages.

## Content Extraction

The Grok loader accepts:

- **String** content
- **Array** content (each extracted item joined with `\n`)
- **Object** content:
  - `text`: used directly
  - `content`: recursively extracted

Empty extracted content is skipped.

## Timestamp Resolution

Timestamp precedence:

1. `params._meta.agentTimestampMs`
2. Top-level `timestamp` (string or numeric Unix seconds)
3. `summary.json.created_at`

Numeric timestamps are normalized to RFC 3339 strings.

## Example

`summary.json`:

```json
{"info":{"id":"sess-grok-1","cwd":"/Users/mish/proj"},"created_at":"2025-03-11T12:00:00Z","current_model_id":"grok-build"}
```

`updates.jsonl`:

```json
{"timestamp":1736035201,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello from grok"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736035201000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi "}},"_meta":{"agentTimestampMs":1736035202000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"from grok assistant"}},"_meta":{"agentTimestampMs":1736035202100}}}
```

The two assistant chunks above become one assistant message: `hi from grok assistant`.

## mmr Mapping

| mmr Field       | Source |
| --------------- | ------ |
| `source`        | `"grok"` |
| `project_name`  | `summary.info.cwd` when present, else the percent-decoded project directory name |
| `project_path`  | Same value as `project_name` |
| `session_id`    | `summary.info.id` when present, else the session directory name |
| `role`          | `"user"` or `"assistant"` |
| `content`       | Extracted/merged text from the update payload |
| `model`         | `current_model_id`, updated by user chunk `_meta.modelId` when present |
| `timestamp`     | Resolved timestamp string |
| `is_subagent`   | `false` |
| `msg_type`      | Same as `role` |
| `input_tokens`  | `0` |
| `output_tokens` | `0` |
