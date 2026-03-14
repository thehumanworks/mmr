# Codex Messages Schema

This document specifies the JSONL schema used by Codex CLI session files, as ingested by mmr from `~/.codex/`.

## File Layout

- **Locations**:
  - `~/.codex/sessions/**/*.jsonl`
  - `~/.codex/archived_sessions/**/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Structure**: Flat; all sessions in recursive directory tree

## Record Types

Each line has a top-level `type` and optional `payload`. mmr processes three record types; others (e.g. `event_msg` with `payload.type: "task_started"`) are ignored.

### Common Top-Level Fields

| Field       | Type   | Required | Description                    |
| ----------- | ------ | -------- | ------------------------------ |
| `type`      | string | yes      | Record type (see below)        |
| `timestamp` | string | no       | ISO 8601 or similar timestamp  |
| `payload`   | object | varies   | Type-specific data             |

### `session_meta`

Session metadata. Sets `session_id`, `cwd`, and `model_provider` for subsequent messages in the same file.

| Field                    | Type   | Description                    |
| ------------------------ | ------ | ------------------------------ |
| `payload.id`             | string | Session identifier             |
| `payload.cwd`            | string | Working directory (project)    |
| `payload.model_provider` | string | Model provider (e.g. `openai`) |

### `event_msg` (User Messages)

User messages. Only `payload.type == "user_message"` is ingested.

| Field              | Type   | Description                    |
| ------------------ | ------ | ------------------------------ |
| `payload.type`     | string | Must be `"user_message"`        |
| `payload.message`  | string | User message text              |
| `payload.images`   | array  | Optional; ignored by mmr       |
| `payload.local_images` | array | Optional; ignored by mmr    |
| `payload.text_elements` | array | Optional; ignored by mmr   |

### `response_item` (Assistant Messages)

Assistant messages. Only `payload.role == "assistant"` is ingested. Other roles (e.g. `developer`, `user`) are system context, not assistant output.

| Field              | Type   | Description                    |
| ------------------ | ------ | ------------------------------ |
| `payload.type`     | string | Often `"message"`               |
| `payload.role`     | string | Must be `"assistant"`           |
| `payload.content`  | array  | Content blocks (see below)      |
| `payload.phase`    | string | Optional (e.g. `"commentary"`) |

#### `payload.content` Array

Each element is an object. mmr extracts text only from items with `type == "output_text"`:

- `type`: `"output_text"`
- `text`: string

Other content types (`input_text`, etc.) are ignored. Extracted text blocks are joined with newlines.

## Processing Order

Session metadata (`session_meta`) is stateful: it establishes `session_id`, `cwd`, and `model_provider` for later lines. Messages without prior `session_meta` (or with empty `id`/`cwd`) are skipped.

## Example

See `example.json` for mmr output (normalized messages). Raw JSONL examples:

```json
{"type":"session_meta","timestamp":"2025-03-11T12:00:00Z","payload":{"id":"sess-xyz","cwd":"/Users/mish/proj","model_provider":"openai"}}
{"type":"event_msg","timestamp":"2025-03-11T12:00:01Z","payload":{"type":"user_message","message":"Hello"}}
{"type":"response_item","timestamp":"2025-03-11T12:00:02Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hi there!"}]}}
```

## mmr Mapping

| mmr Field       | Source                                      |
| --------------- | ------------------------------------------- |
| `source`        | `"codex"`                                   |
| `project_name`  | `session_meta.payload.cwd`                  |
| `project_path`  | `session_meta.payload.cwd`                  |
| `session_id`    | `session_meta.payload.id`                  |
| `role`          | `"user"` or `"assistant"` from record type  |
| `content`       | `payload.message` (user) or extracted text (assistant) |
| `model`         | `session_meta.payload.model_provider`       |
| `timestamp`     | `timestamp`                                 |
| `is_subagent`   | `false` (Codex has no subagent distinction) |
| `msg_type`      | `"user"` or `"assistant"`                   |
| `input_tokens`  | `0` (not present in Codex JSONL)             |
| `output_tokens` | `0` (not present in Codex JSONL)            |
