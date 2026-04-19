# Cursor Messages Schema

This document specifies the JSONL transcript layout ingested by `mmr` from local Cursor history.

## File Layout

- **Location**: `~/.cursor/projects/<project_name>/agent-transcripts/<session_id>/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Project name**: Directory name under `~/.cursor/projects/` (for example `-Users-mish-proj`)
- **Session ID**: Directory name under `agent-transcripts/`

`mmr` scans every project directory, then every session directory under `agent-transcripts/`, and ingests all `.jsonl` files it finds.

## Message Records

`mmr` ingests only lines where the top-level `role` is `"user"` or `"assistant"`. Other record types are skipped.

### Top-Level Fields

| Field       | Type   | Required | Description |
| ----------- | ------ | -------- | ----------- |
| `role`      | string | yes      | Must be `"user"` or `"assistant"` |
| `timestamp` | string | no       | Message timestamp |
| `message`   | object | yes      | Message payload |

### `message` Object

| Field     | Type   | Required | Description |
| --------- | ------ | -------- | ----------- |
| `content` | mixed  | yes      | Content payload; text is extracted recursively |
| `model`   | string | no       | Model identifier |

### Content Extraction

`message.content` may be:

- **String**: Used directly.
- **Array**: Only items with `type: "text"` contribute output; their `text` values are joined with `\n`.
- **Object**:
  - `text`: string used directly
  - `content`: recursively extracted
  - Otherwise: empty string

Empty extracted content is skipped.

## Example

Raw JSONL example:

```json
{"role":"user","timestamp":"2025-03-11T12:00:00Z","message":{"content":"Summarize the latest changes."}}
{"role":"assistant","timestamp":"2025-03-11T12:00:01Z","message":{"model":"gpt-4.1","content":[{"type":"text","text":"Here is the summary."}]}}
```

## mmr Mapping

| mmr Field       | Source |
| --------------- | ------ |
| `source`        | `"cursor"` |
| `project_name`  | Directory name under `~/.cursor/projects/` |
| `project_path`  | Same encoded directory name currently stored by the loader |
| `session_id`    | Directory name under `agent-transcripts/` |
| `role`          | Top-level `role` |
| `content`       | Extracted from `message.content` |
| `model`         | `message.model` |
| `timestamp`     | Top-level `timestamp` |
| `is_subagent`   | `false` |
| `msg_type`      | Same as `role` |
| `input_tokens`  | `0` (not present in Cursor transcript JSONL) |
| `output_tokens` | `0` (not present in Cursor transcript JSONL) |

## Filtering caveat

When `mmr` auto-discovers the current working directory, it converts the canonical path into Cursor's encoded project directory name before querying Cursor history.

Direct `--project /path/to/proj` filters are different: the current loader stores Cursor `project_name` and `project_path` as the encoded directory name, so path-style filters do not automatically normalize to Cursor's on-disk naming convention. Use:

- cwd-based workflows such as `mmr export`, `mmr sessions`, or `mmr messages` from inside the project directory, or
- the encoded Cursor project directory name when filtering Cursor directly.
