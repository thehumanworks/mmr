# Grok Messages Schema

This document specifies the session file schema used by Grok CLI, as ingested by `mmr` from `~/.grok/sessions/`.

## File Layout

- **Session directory**: `~/.grok/sessions/<project_name>/<session_id>/`
- **Updates file**: `updates.jsonl`
- **Optional summary file**: `summary.json`
- **Format**: `updates.jsonl` is one JSON object per line (JSONL)

`<project_name>` is usually a percent-encoded filesystem path. `mmr` percent-decodes that directory name when it needs a fallback project identifier.

## Session Metadata

If `summary.json` exists and parses successfully, `mmr` reads:

| Field | Description |
| --- | --- |
| `info.id` | Preferred session ID |
| `info.cwd` | Preferred project name/path |
| `current_model_id` | Initial model identifier |
| `created_at` | Fallback timestamp |

Fallbacks:

- Session ID falls back to the session directory name.
- Project name falls back to the percent-decoded project directory name.

## Update Records

Each JSONL line is expected to contain `params.update.sessionUpdate`.

`mmr` currently processes two update types:

- `user_message_chunk`
- `agent_message_chunk`

Other update types are ignored.

### Common Access Pattern

| Field | Type | Description |
| --- | --- | --- |
| `params.update.sessionUpdate` | string | Update kind |
| `params.update.content` | mixed | Text payload |
| `params.update._meta.modelId` | string | Model identifier for user chunks |
| `params._meta.agentTimestampMs` | number | Preferred timestamp in milliseconds |
| `timestamp` | string or number | Fallback timestamp |

## Content Extraction

`params.update.content` may be:

- **String**: used directly
- **Array**: each element is recursively extracted and non-empty values are joined with `\n`
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise: empty string

## Assistant Chunk Folding

`agent_message_chunk` entries are buffered and combined into a single assistant message. The buffered assistant message is flushed when:

- a `user_message_chunk` is encountered, or
- the file ends

This means multiple adjacent assistant chunks become one normalized `mmr` assistant message.

## Timestamp Resolution

Timestamps are resolved in this order:

1. `params._meta.agentTimestampMs` converted from Unix milliseconds to RFC 3339
2. Top-level `timestamp` when it is a non-empty string
3. Top-level `timestamp` when it is numeric Unix seconds
4. `summary.json.created_at`

## Example

`summary.json`:

```json
{
  "info": {
    "id": "sess-grok-123",
    "cwd": "/Users/test/proj"
  },
  "current_model_id": "grok-code-fast",
  "created_at": "2025-05-18T10:00:00Z"
}
```

`updates.jsonl`:

```json
{"params":{"update":{"sessionUpdate":"user_message_chunk","content":"Investigate the failure","_meta":{"modelId":"grok-code-fast"}},"_meta":{"agentTimestampMs":1747562400000}}}
{"params":{"update":{"sessionUpdate":"agent_message_chunk","content":[{"text":"Checking logs"}]}}}
{"params":{"update":{"sessionUpdate":"agent_message_chunk","content":[{"text":"Found the root cause"}]}}}
```

The two assistant chunks above become one assistant message with content:

```text
Checking logs
Found the root cause
```

## mmr Mapping

| mmr Field | Source |
| --- | --- |
| `source` | `"grok"` |
| `project_name` | `summary.json.info.cwd`, else percent-decoded project directory name |
| `project_path` | Same value as `project_name` |
| `session_id` | `summary.json.info.id`, else session directory name |
| `role` | `"user"` or `"assistant"` based on update type |
| `content` | Extracted from `params.update.content` |
| `model` | Most recent model from `summary.json.current_model_id` or `params.update._meta.modelId` |
| `timestamp` | Resolved timestamp |
| `is_subagent` | `false` |
| `msg_type` | Same value as `role` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |
