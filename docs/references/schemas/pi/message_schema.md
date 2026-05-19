# Pi Messages Schema

This document specifies the JSONL schema used by Pi agent session files, as ingested by `mmr` from `~/.pi/agent/sessions/`.

## File Layout

- **Location**: `~/.pi/agent/sessions/**/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Project name**: The parent directory containing the `.jsonl` file

`mmr` walks the entire sessions tree recursively and parses every `.jsonl` file it finds.

## Record Types

`mmr` currently recognizes three top-level record types:

- `session`
- `model_change`
- `message`

Other types are ignored.

### `session`

Establishes session metadata used by later message records.

| Field | Type | Description |
| --- | --- | --- |
| `type` | string | Must be `"session"` |
| `id` | string | Session ID override |
| `cwd` | string | Working directory used as `project_path` |

If no `session` record appears, `mmr` falls back to a session ID derived from the filename stem. Messages still require a non-empty `cwd`, so a missing `session.cwd` causes those message lines to be skipped.

### `model_change`

Updates the current model for later message records.

| Field | Type | Description |
| --- | --- | --- |
| `provider` | string | Model provider |
| `modelId` | string | Model name |

`mmr` stores the current model as:

- `provider/modelId` when both are present
- `provider` when only the provider is present
- `modelId` when only the model is present

### `message`

User and assistant messages.

| Field | Type | Description |
| --- | --- | --- |
| `type` | string | Must be `"message"` |
| `timestamp` | string | Message timestamp |
| `message.role` | string | Must be `"user"` or `"assistant"` |
| `message.content` | mixed | Message content |
| `message.model` | string | Optional per-message model override |
| `message.usage` | object | Optional token usage |

Messages with roles other than `user` or `assistant` are ignored.

## Content Extraction

`message.content` may be:

- **String**: used directly
- **Array**: each object with `type == "text"` contributes its `text`; extracted entries are joined with `\n`
- **Object**: treated like one array item and used only when `type == "text"`

Non-text content blocks are ignored.

## Usage Extraction

`mmr` sums input tokens from any of these keys when present:

- `input`
- `input_tokens`
- `inputTokens`
- `prompt_tokens`

It sums output tokens from:

- `output`
- `output_tokens`
- `outputTokens`
- `completion_tokens`

Each usage value may be either:

- a number, or
- an object containing `total` or `value`

## Example

```json
{"type":"session","id":"sess-pi-123","cwd":"/Users/test/proj"}
{"type":"model_change","provider":"anthropic","modelId":"claude-sonnet-4"}
{"type":"message","timestamp":"2025-05-18T10:00:00Z","message":{"role":"user","content":"Summarize the diff"}}
{"type":"message","timestamp":"2025-05-18T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"Here is the summary"}],"usage":{"input_tokens":120,"output_tokens":45}}}
```

## mmr Mapping

| mmr Field | Source |
| --- | --- |
| `source` | `"pi"` |
| `project_name` | Parent directory containing the `.jsonl` file |
| `project_path` | `session.cwd` |
| `session_id` | `session.id`, else derived from the filename stem |
| `role` | `message.role` |
| `content` | Extracted from `message.content` |
| `model` | `message.model`, else the latest `model_change` value |
| `timestamp` | Top-level `timestamp` |
| `is_subagent` | `false` |
| `msg_type` | Same value as `role` |
| `input_tokens` | Summed from recognized input usage keys |
| `output_tokens` | Summed from recognized output usage keys |

## Practical Constraint

Pi uses two different project identifiers in `mmr`:

- `project_name`: the local session directory name
- `project_path`: the `cwd` from the session record

Direct project lookups can still match the canonical path because the project aggregate stores `project_path` as the original path value.
