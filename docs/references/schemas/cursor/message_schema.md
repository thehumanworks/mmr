# Cursor Messages Schema

This document specifies the JSONL layout used by Cursor agent transcript files as currently ingested by `mmr`.

## File layout

- **Location**: `~/.cursor/projects/<project_name>/agent-transcripts/<session_id>/*.jsonl`
- **Format**: one JSON object per line (JSONL)
- **Project name**: directory name under `~/.cursor/projects/` (for example `-Users-mish-proj`)
- **Session ID**: parent directory name under `agent-transcripts/`

`mmr` scans every `.jsonl` file in each session directory.

## Record filtering

`mmr` ingests only records where the top-level `role` is `"user"` or `"assistant"`.

Other records are skipped.

## Top-level fields

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `role` | string | yes | Must be `"user"` or `"assistant"` to be ingested |
| `message` | object | yes | Message payload |
| `timestamp` | string | no | Preserved when present; empty string otherwise |

## `message` object

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `content` | mixed | yes | Extracted text content |
| `model` | string | no | Cursor model identifier |

## Content extraction

`message.content` may be:

- **string**: used directly
- **array**: only items with `type == "text"` contribute output; their `text` values are joined with `\n`
- **object**:
  - `text`: used directly
  - `content`: extracted recursively
  - otherwise ignored

Blocks without text content contribute nothing.

## Example

```json
{"role":"user","message":{"content":[{"type":"text","text":"hello from cursor"}]}}
{"role":"assistant","message":{"content":[{"type":"text","text":"hi from cursor assistant"}],"model":"cursor-model"}}
```

## mmr mapping

| mmr field | Source |
| --- | --- |
| `source` | `"cursor"` |
| `project_name` | directory name under `~/.cursor/projects/` |
| `project_path` | currently the same encoded project directory name |
| `session_id` | parent directory name under `agent-transcripts/` |
| `role` | top-level `role` |
| `content` | extracted from `message.content` |
| `model` | `message.model` |
| `timestamp` | top-level `timestamp`, or empty string when absent |
| `is_subagent` | `false` |
| `msg_type` | top-level `role` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |

## Constraints and pitfalls

- Cursor project directories are stored as encoded names such as `-Users-mish-proj`.
- The current loader keeps that encoded value as both `project_name` and `project_path`.
- As a result, direct `--project` filtering for Cursor currently matches the encoded project directory name, not the decoded filesystem path.
- `mmr export` without `--project` still works from cwd because the CLI encodes the current path before querying Cursor history.
