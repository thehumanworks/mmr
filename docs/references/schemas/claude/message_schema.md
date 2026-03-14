# Claude Messages Schema

This document specifies the JSONL schema used by Claude Code session files, as ingested by mmr from `~/.claude/projects/`.

## File Layout

- **Location**: `~/.claude/projects/<project_name>/` or `~/.claude/projects/<project_name>/<session>/subagents/`
- **Format**: One JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Project name**: Directory name under `projects/` (e.g. `-Users-mish-proj` for path `/Users/mish/proj`)

## Message Records

mmr ingests only lines where `type` is `"user"` or `"assistant"`. Other record types (e.g. `system`, `file-history-snapshot`) are skipped.

### Top-Level Fields

| Field       | Type   | Required | Description                                      |
| ----------- | ------ | -------- | ------------------------------------------------ |
| `type`      | string | yes      | `"user"` or `"assistant"`                        |
| `sessionId` | string | yes      | Session identifier; empty values are skipped     |
| `message`   | object | yes      | Message payload (see below)                       |
| `cwd`       | string | no       | Working directory; used for project path         |
| `timestamp` | string | no       | ISO 8601 or similar timestamp                     |

### `message` Object

| Field     | Type   | Required | Description                                      |
| --------- | ------ | -------- | ------------------------------------------------ |
| `content` | mixed  | yes      | Message content (see Content Extraction)         |
| `role`    | string | no       | Role; falls back to top-level `type` if absent   |
| `model`   | string | no       | Model identifier                                 |
| `usage`   | object | no       | Token usage (see Usage)                          |

### Content Extraction

`message.content` may be:

- **String**: Used directly.
- **Array**: Each element is recursively extracted; non-empty results are joined with `\n`.
- **Object**:
  - `text`: string used directly
  - `content`: recursively extracted
  - `parts`: recursively extracted
  - Otherwise: empty string

Content blocks with `type: "thinking"`, `type: "tool_use"`, or `type: "tool_result"` yield text only when they include a `text`, `content`, or `parts` key. Blocks without those keys contribute nothing to the extracted content.

### Usage Object

Token counts may appear as:

- Flat: `input_tokens`, `output_tokens` as numbers
- Nested: object with `total` or `value` as number

mmr reads only `input_tokens` and `output_tokens`; other usage fields (e.g. `cache_creation_input_tokens`) are ignored.

```json
{ "input_tokens": 100, "output_tokens": 50 }
```
or
```json
{ "input_tokens": { "total": 100 }, "output_tokens": { "total": 50 } }
```

## Example

See `example.json` for mmr output (normalized messages). Raw JSONL examples:

```json
{"type":"user","sessionId":"sess-abc","message":{"content":"Hello","role":"user"},"cwd":"/Users/mish/proj","timestamp":"2025-03-11T12:00:00Z"}
{"type":"assistant","sessionId":"sess-abc","message":{"content":[{"type":"text","text":"Hi there!"}],"role":"assistant","model":"claude-sonnet-4","usage":{"input_tokens":10,"output_tokens":5}},"cwd":"/Users/mish/proj","timestamp":"2025-03-11T12:00:01Z"}
```

## mmr Mapping

| mmr Field       | Source                                      |
| --------------- | ------------------------------------------- |
| `source`        | `"claude"`                                  |
| `project_name`  | Directory name under `projects/`            |
| `project_path`  | First `cwd` in file, else decoded from name |
| `session_id`    | `sessionId`                                 |
| `role`          | `message.role` or `type`                    |
| `content`       | Extracted from `message.content`            |
| `model`         | `message.model`                             |
| `timestamp`     | `timestamp`                                 |
| `is_subagent`   | `true` if file is under `subagents/`        |
| `msg_type`      | `type`                                      |
| `input_tokens`  | `message.usage.input_tokens`                |
| `output_tokens` | `message.usage.output_tokens`               |
