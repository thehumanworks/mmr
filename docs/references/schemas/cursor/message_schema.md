# Cursor Messages Schema

This document specifies the JSONL schema used by Cursor agent transcript files, as ingested by `mmr` from `~/.cursor/projects/`.

## File Layout

- **Location**: `~/.cursor/projects/<project_name>/agent-transcripts/<session_id>/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Project name**: The directory name under `projects/`, which is currently the slash-to-hyphen encoded project identifier used by Cursor
- **Session ID**: The directory name under `agent-transcripts/`

`mmr` currently keeps the Cursor project directory name as-is for both `project_name` and `project_path`.

## Message Records

`mmr` ingests only lines where top-level `role` is `"user"` or `"assistant"`. Other roles and malformed lines are skipped.

### Top-Level Fields

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `role` | string | yes | `"user"` or `"assistant"` |
| `timestamp` | string | no | Message timestamp |
| `message` | object | yes | Message payload |

### `message` Object

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `content` | mixed | yes | Message content |
| `model` | string | no | Model identifier |

## Content Extraction

`message.content` may be:

- **String**: used directly
- **Array**: each element with `type == "text"` contributes its `text`; extracted entries are joined with `\n`
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise: empty string

Non-text array items are ignored.

## Example

```json
{"role":"user","timestamp":"2025-03-20T12:00:00Z","message":{"content":"Hello from Cursor"}}
{"role":"assistant","timestamp":"2025-03-20T12:00:01Z","message":{"model":"composer-2-fast","content":[{"type":"text","text":"Hello back"}]}}
```

## mmr Mapping

| mmr Field | Source |
| --- | --- |
| `source` | `"cursor"` |
| `project_name` | `<project_name>` directory under `~/.cursor/projects/` |
| `project_path` | Same value as `project_name` in the current loader |
| `session_id` | `<session_id>` directory under `agent-transcripts/` |
| `role` | Top-level `role` |
| `content` | Extracted from `message.content` |
| `model` | `message.model` |
| `timestamp` | Top-level `timestamp` |
| `is_subagent` | `false` |
| `msg_type` | Same value as `role` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |

## Practical Constraint

Because the current Cursor loader preserves the encoded project directory name, direct Cursor-only project filters should use that encoded identifier. `mmr export` handles the Cursor project-name conversion automatically when it infers the project from the current working directory.
