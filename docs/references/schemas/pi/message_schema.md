# Pi Messages Schema

This document specifies the session files ingested by mmr from `~/.pi/agent/sessions/`.

## File Layout

- **Location**: `~/.pi/agent/sessions/**/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Project name**: Immediate parent directory name of each `.jsonl` file
- **Session ID fallback**: File stem after the last underscore, or the full stem when no underscore exists

mmr walks the `sessions/` tree recursively and parses every `.jsonl` file it finds.

## Record Types

Each line has a top-level `type`. mmr uses three record types to maintain parser state and emit chat messages.

### `session`

Session metadata. Updates parser state for later records.

| Field       | Type   | Description |
| ----------- | ------ | ----------- |
| `id`        | string | Preferred `session_id` |
| `cwd`       | string | Required project path for emitted messages |
| `timestamp` | string | Ignored for state, not copied to emitted messages |

If `id` is missing, mmr keeps the fallback session ID derived from the filename.

### `model_change`

Model-change metadata. Updates the default model for later message records.

| Field       | Type   | Description |
| ----------- | ------ | ----------- |
| `provider`  | string | Model provider prefix |
| `modelId`   | string | Model identifier |

mmr normalizes the current model as:

- `provider/modelId` when both are present
- `provider` when only `provider` is present
- `modelId` when only `modelId` is present
- empty string when both are missing

### `message`

Chat message records. Only user-visible chat roles are ingested.

| Field                 | Type   | Description |
| --------------------- | ------ | ----------- |
| `message.role`        | string | Must be `"user"` or `"assistant"` |
| `message.content`     | mixed  | Message content payload |
| `message.model`       | string | Optional per-message model override |
| `message.usage`       | object | Optional token usage object |
| `timestamp`           | string | Emitted message timestamp |

Records with other roles such as `toolResult` are ignored.

## Content Extraction

`message.content` is interpreted as follows:

- **String**: used directly
- **Array**: only items with `type == "text"` contribute output; non-empty text values are joined with `\n`
- **Object**: only objects with `type == "text"` contribute output

Items like `thinking` blocks and tool-call descriptors are ignored even when they appear alongside text blocks.

## Usage Extraction

When `message.usage` is present, mmr computes token counts by summing several supported key variants.

### Input Tokens

mmr adds any values found at:

- `input`
- `input_tokens`
- `inputTokens`
- `prompt_tokens`

### Output Tokens

mmr adds any values found at:

- `output`
- `output_tokens`
- `outputTokens`
- `completion_tokens`

Each usage value may be either:

- a number
- an object containing `total`
- an object containing `value`

Missing or unrecognized shapes contribute `0`.

## State and Emission Rules

- `project_name` comes from the file's parent directory name
- `project_path` comes from the latest `session.cwd`
- `session_id` comes from the latest `session.id`, or the filename-derived fallback if none was seen
- `model` comes from `message.model` when present, otherwise from the latest `model_change`

mmr emits a record only when all of the following are non-empty:

- `project_name`
- `session_id`
- `cwd` from a prior `session` record

Non-empty extracted content is also required.

## Example

```json
{"type":"session","version":3,"id":"sess-pi-1","timestamp":"2025-01-04T00:00:00.000Z","cwd":"/Users/test/pi-proj"}
{"type":"model_change","id":"model-1","parentId":null,"timestamp":"2025-01-04T00:00:00.100Z","provider":"openai-codex","modelId":"gpt-5.5"}
{"type":"message","id":"msg-pi-u1","parentId":"model-1","timestamp":"2025-01-04T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"hello from pi"}]}}
{"type":"message","id":"msg-pi-a1","parentId":"msg-pi-u1","timestamp":"2025-01-04T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"internal"},{"type":"toolCall","id":"call-1","name":"read","arguments":{"path":"Cargo.toml"}},{"type":"text","text":"hi from pi assistant"}],"model":"gpt-5.5","usage":{"input":12,"output":6}}}
{"type":"message","id":"msg-pi-t1","parentId":"msg-pi-a1","timestamp":"2025-01-04T00:00:03.000Z","message":{"role":"toolResult","content":[{"type":"text","text":"tool output should not be a chat message"}]}}
```

Those lines yield two mmr messages: a user message with content `hello from pi`, then an assistant message with content `hi from pi assistant`.

## mmr Mapping

| mmr Field       | Source |
| --------------- | ------ |
| `source`        | `"pi"` |
| `project_name`  | Parent directory name of the session file |
| `project_path`  | `session.cwd` |
| `session_id`    | `session.id`, else filename-derived fallback |
| `role`          | `message.role` |
| `content`       | Extracted from `message.content` text blocks only |
| `model`         | `message.model`, else current `model_change` value |
| `timestamp`     | Top-level `timestamp` on the `message` record |
| `is_subagent`   | `false` |
| `msg_type`      | Same as `role` |
| `input_tokens`  | Summed from supported input usage keys |
| `output_tokens` | Summed from supported output usage keys |

## Constraints

- `source_file` is the `.jsonl` path being parsed.
- `line_index` is the zero-based line number within that file.
- Malformed JSON lines and unsupported record types are skipped.
