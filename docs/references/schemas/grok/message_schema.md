# Grok Messages Schema

This document specifies the session files ingested by mmr from `~/.grok/sessions/`.

## File Layout

- **Location**: `~/.grok/sessions/<percent-encoded-project>/<session_id>/`
- **Required transcript file**: `updates.jsonl`
- **Optional metadata file**: `summary.json`
- **Format**: JSON Lines for `updates.jsonl`; one JSON object per line
- **Project directory name**: Percent-encoded canonical path (for example `%2FUsers%2Fmish%2Fproj` for `/Users/mish/proj`)

mmr skips session directories that do not contain `updates.jsonl`.

## `summary.json`

`summary.json` is read opportunistically. It supplies fallback metadata before `updates.jsonl` is parsed.

| Field                 | Type   | Used for                                           |
| --------------------- | ------ | -------------------------------------------------- |
| `info.id`             | string | Preferred `session_id`; falls back to directory name |
| `info.cwd`            | string | Preferred `project_name` and `project_path`        |
| `current_model_id`    | string | Initial model for later messages                   |
| `created_at`          | string | Fallback timestamp when an update lacks one        |

If `info.cwd` is missing, mmr decodes the percent-encoded project directory name and uses that decoded path as both `project_name` and `project_path`.

## `updates.jsonl` Record Shape

mmr parses each non-empty line as JSON and then reads `params.update.sessionUpdate`.

Malformed JSON lines are skipped.

### Supported `sessionUpdate` Values

| Value                  | Ingested | Behavior |
| ---------------------- | -------- | -------- |
| `user_message_chunk`   | yes      | Emits one user message when extracted text is non-empty |
| `agent_message_chunk`  | yes      | Buffers assistant text and coalesces consecutive chunks |
| any other value        | no       | Ignored |

### Common Nested Fields

| Field                                | Type          | Description |
| ------------------------------------ | ------------- | ----------- |
| `params.sessionId`                   | string        | Present in raw events but not used for fallback resolution |
| `params.update.content`              | string/object/array | Message content payload |
| `params.update._meta.modelId`        | string        | Updates the current model when present on user chunks |
| `params._meta.agentTimestampMs`      | number        | Preferred timestamp source (Unix milliseconds) |
| `timestamp`                          | string/number | Fallback timestamp source if `_meta.agentTimestampMs` is absent |

## Content Extraction

`params.update.content` is recursively flattened:

- **String**: used directly
- **Array**: recursively extract each item, drop empty values, join with `\n`
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise ignored

Empty extracted text is skipped.

## Assistant Chunk Coalescing

Grok assistant output arrives as `agent_message_chunk` fragments. mmr does not emit one record per chunk. Instead it:

1. Starts a pending assistant message on the first non-empty `agent_message_chunk`
2. Appends later consecutive assistant chunks to the same pending message
3. Flushes the pending assistant message when the next `user_message_chunk` arrives or when the file ends

The emitted assistant message keeps the timestamp and `line_index` from the first chunk in the buffered sequence.

## Timestamp Resolution

For ingested user and assistant messages, mmr resolves timestamps in this order:

1. `params._meta.agentTimestampMs` converted from Unix milliseconds to RFC 3339
2. Top-level `timestamp` if it is already a non-empty string
3. Top-level `timestamp` interpreted as Unix seconds if it is numeric
4. `summary.json.created_at`

If all of those values are missing, mmr still emits the message with an empty timestamp.

## Example

```json
{"info":{"id":"sess-grok-1","cwd":"/Users/test/grok-proj"},"created_at":"2025-01-05T00:00:00Z","current_model_id":"grok-build"}
```

```json
{"timestamp":1736035201,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello from grok"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736035201000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi "}},"_meta":{"agentTimestampMs":1736035202000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"from grok assistant"}},"_meta":{"agentTimestampMs":1736035202100}}}
```

Those three transcript lines yield two mmr messages: a user message with content `hello from grok`, then one assistant message with content `hi from grok assistant`.

## mmr Mapping

| mmr Field       | Source |
| --------------- | ------ |
| `source`        | `"grok"` |
| `project_name`  | `summary.info.cwd`, else decoded project directory name |
| `project_path`  | Same value as `project_name` |
| `session_id`    | `summary.info.id`, else session directory name |
| `role`          | `"user"` for `user_message_chunk`, `"assistant"` for buffered assistant output |
| `content`       | Extracted `params.update.content`; assistant chunks are concatenated |
| `model`         | `summary.current_model_id`, updated by `params.update._meta.modelId` on later user chunks |
| `timestamp`     | Resolved with the timestamp precedence above |
| `is_subagent`   | `false` |
| `msg_type`      | Same as `role` |
| `input_tokens`  | `0` |
| `output_tokens` | `0` |

## Constraints

- `session_id`, `project_name`, and non-empty extracted content are required before mmr emits a message.
- `source_file` is always the `updates.jsonl` path.
- `line_index` is the zero-based line number within `updates.jsonl`.
