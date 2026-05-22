# Cursor Messages Schema

This document specifies the JSONL schema used by Cursor agent transcript files, as ingested by mmr from `~/.cursor/projects/`.

## File Layout

- **Location**: `~/.cursor/projects/<project_name>/agent-transcripts/<session_id>/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Project name**: Directory name under `projects/` (typically slash-to-hyphen encoded, e.g. `-Users-mish-proj`)
- **Session ID**: Directory name under `agent-transcripts/`

## Message Records

mmr ingests only records whose top-level `role` is `"user"` or `"assistant"`. Other roles or malformed lines are skipped.

### Top-Level Fields

| Field       | Type   | Required | Description |
| ----------- | ------ | -------- | ----------- |
| `role`      | string | yes      | Must be `"user"` or `"assistant"` |
| `message`   | object | yes      | Message payload |
| `timestamp` | string | no       | Optional timestamp string |

### `message` Object

| Field     | Type   | Required | Description |
| --------- | ------ | -------- | ----------- |
| `content` | mixed  | yes      | Message content; see Content Extraction |
| `model`   | string | no       | Model identifier |

## Content Extraction

`message.content` may be:

- **String**: used directly.
- **Array**: only items with `type: "text"` contribute content; extracted values are joined with `\n`.
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise: empty string

Empty extracted content is skipped.

## Example

Raw JSONL examples:

```json
{"role":"user","message":{"content":[{"type":"text","text":"Hello from Cursor"}]},"timestamp":"2025-03-11T12:00:00Z"}
{"role":"assistant","message":{"content":[{"type":"text","text":"Hi from Cursor"}],"model":"composer-2-fast"},"timestamp":"2025-03-11T12:00:01Z"}
```

## mmr Mapping

| mmr Field       | Source |
| --------------- | ------ |
| `source`        | `"cursor"` |
| `project_name`  | Directory name under `~/.cursor/projects/` |
| `project_path`  | Same stored project directory name (the loader does not currently decode it back to a slash path) |
| `session_id`    | Parent session directory name |
| `role`          | Top-level `role` |
| `content`       | Extracted from `message.content` |
| `model`         | `message.model` |
| `timestamp`     | Top-level `timestamp` or empty string |
| `is_subagent`   | `false` |
| `msg_type`      | Same as `role` |
| `input_tokens`  | `0` |
| `output_tokens` | `0` |

## Constraint

Cursor's project directory name is the stored source of truth for direct ingest. Higher-level CLI project resolution can still match a slash-delimited cwd path to this encoded name, but the normalized `project_path` field from the loader remains the encoded directory name.
