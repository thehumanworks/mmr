# Cursor Messages Schema

This document specifies the JSONL schema used by Cursor agent transcript files, as ingested by mmr from `~/.cursor/projects/`.

## File Layout

- **Location**: `~/.cursor/projects/<project_name>/agent-transcripts/<session_id>/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Project name**: Directory name under `projects/` (for example `-Users-mish-proj`)
- **Session ID**: Directory name under `agent-transcripts/`

mmr walks every `.jsonl` file within a session directory and preserves deterministic ordering with `source_file` plus `line_index`.

## Message Records

mmr ingests only lines where `role` is `"user"` or `"assistant"`. Empty lines, malformed JSON, unsupported roles, and records whose extracted content is empty are skipped.

### Top-Level Fields

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `role` | string | yes | Must be `"user"` or `"assistant"` |
| `message` | object | yes | Message payload |
| `timestamp` | string | no | Optional timestamp copied into mmr output |

### `message` Object

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `content` | mixed | yes | Message content; see extraction rules below |
| `model` | string | no | Model identifier for assistant messages |

### Content Extraction

`message.content` may be:

- **String**: used directly
- **Array**: only items with `type == "text"` contribute content; their `text` values are joined with `\n`
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise: empty string

Unlike the Claude loader, Cursor array extraction is narrow: array items without `type: "text"` are ignored.

## Example

Raw JSONL examples:

```json
{"role":"user","timestamp":"2025-03-22T10:00:00Z","message":{"content":[{"type":"text","text":"Summarize this repo."}]}}
{"role":"assistant","timestamp":"2025-03-22T10:00:05Z","message":{"model":"composer-2-fast","content":[{"type":"text","text":"Here is a summary."}]}}
```

## mmr Mapping

| mmr Field | Source |
| --- | --- |
| `source` | `"cursor"` |
| `project_name` | Directory name under `~/.cursor/projects/` |
| `project_path` | Same value as `project_name` |
| `session_id` | Session directory name under `agent-transcripts/` |
| `role` | Top-level `role` |
| `content` | Extracted from `message.content` |
| `model` | `message.model` or empty string |
| `timestamp` | Top-level `timestamp` or empty string |
| `is_subagent` | `false` |
| `msg_type` | Same value as `role` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |

## Constraints and pitfalls

- Cursor project matching is currently based on the stored `project_name` directory, not a decoded filesystem path.
- Token usage is not extracted from Cursor transcript files; mmr emits zeros for `input_tokens` and `output_tokens`.
- If a session directory contains multiple transcript files, all matching `.jsonl` files are ingested.
