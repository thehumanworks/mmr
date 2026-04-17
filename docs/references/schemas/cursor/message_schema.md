# Cursor Messages Schema

This document specifies the JSONL schema that `mmr` ingests from Cursor transcript files under `~/.cursor/projects/`.

## File layout

- **Location**: `~/.cursor/projects/<project_name>/agent-transcripts/<session_id>/*.jsonl`
- **Format**: one JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Project name**: directory name under `~/.cursor/projects/`

Example path:

```text
~/.cursor/projects/-Users-me-proj/agent-transcripts/sess-123/0001.jsonl
```

## Message records

`mmr` ingests only lines where:

- the top-level `role` is `"user"` or `"assistant"`, and
- extracted message text is non-empty

Malformed lines, empty lines, other roles, and messages without usable text are skipped.

### Top-level fields

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `role` | string | yes | Must be `"user"` or `"assistant"` |
| `message` | object | yes | Message payload |
| `timestamp` | string | no | Cursor timestamp string copied into `mmr` output |

### `message` object

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `content` | mixed | yes | Content payload; see extraction rules below |
| `model` | string | no | Model identifier copied into `mmr` output |

## Content extraction

`message.content` may be a string, array, or object:

- **String**: used directly
- **Array**: only items with `type == "text"` contribute output; their `text` fields are joined with `\n`
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise: empty string

Non-text blocks are ignored.

## Example raw records

```json
{"role":"user","message":{"content":"Summarize the last deploy."},"timestamp":"2026-04-16T10:00:00Z"}
{"role":"assistant","message":{"content":[{"type":"text","text":"Deploy completed successfully."}],"model":"gpt-4.1"},"timestamp":"2026-04-16T10:00:05Z"}
```

## `mmr` mapping

| `mmr` field | Source |
| --- | --- |
| `source` | `"cursor"` |
| `project_name` | `<project_name>` directory under `~/.cursor/projects/` |
| `project_path` | Same value as `project_name` in the current implementation |
| `session_id` | `<session_id>` directory under `agent-transcripts/` |
| `role` | Top-level `role` |
| `content` | Extracted from `message.content` |
| `model` | `message.model` |
| `timestamp` | Top-level `timestamp` |
| `is_subagent` | `false` |
| `msg_type` | Same value as `role` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |

## Project-filter caveat

Unlike Codex and most Claude lookups, Cursor project filtering currently matches the stored Cursor project directory name directly. `mmr` does not decode that directory name back to a canonical filesystem path before storing `project_name` or `project_path`.

Implications:

- `--source cursor messages --project /Users/me/proj` will not match unless Cursor stored that exact literal value
- `projects --source cursor` is the easiest way to discover the exact identifier to use with `--project`
- `export` without `--project` handles the cwd-to-Cursor encoded name for you
