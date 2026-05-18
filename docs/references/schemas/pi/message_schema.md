# Pi Messages Schema

This document specifies the JSONL session format that `mmr` ingests from `~/.pi/agent/sessions/`.

## File layout

- **Location**: `~/.pi/agent/sessions/**/*.jsonl`
- **Format**: one JSON object per line
- **Project name**: the immediate parent directory of each `.jsonl` file
- **Fallback session ID**: the file stem, or the suffix after the last `_` in the file stem

For example, `~/.pi/agent/sessions/my-proj/session_abc123.jsonl` has:

- project name `my-proj`
- fallback session ID `abc123`

## Record types

`mmr` reads three top-level `type` values:

- `session`
- `model_change`
- `message`

All other record types are ignored. Malformed lines are skipped.

## `session`

Session records establish context for later message lines.

| Field | Type | Purpose |
| --- | --- | --- |
| `id` | string | Preferred session ID |
| `cwd` | string | Required project path for emitted messages |

If `cwd` is empty, later `message` records are skipped because `mmr` cannot populate `project_path`.

## `model_change`

Model-change records update the current model for later message lines.

| Field | Type | Purpose |
| --- | --- | --- |
| `provider` | string | Model provider |
| `modelId` | string | Model identifier |

`mmr` combines them as:

- `provider/modelId` when both are present
- `provider` or `modelId` when only one is present

## `message`

`mmr` ingests only `message.role == "user"` or `message.role == "assistant"`.

### Relevant fields

| Field | Type | Description |
| --- | --- | --- |
| `message.role` | string | Must be `user` or `assistant` |
| `message.content` | mixed | Message content |
| `message.model` | string | Explicit model override |
| `message.usage` | object | Optional token counts |
| `timestamp` | string | Message timestamp |

### Content extraction

`message.content` may be:

- **String**: used directly
- **Array**: only elements with `type == "text"` contribute text; extracted items are joined with `\n`
- **Object**: only objects with `type == "text"` and a `text` field contribute text

Other structured content is ignored.

### Usage extraction

`mmr` sums token counts from several common keys:

- input side: `input`, `input_tokens`, `inputTokens`, `prompt_tokens`
- output side: `output`, `output_tokens`, `outputTokens`, `completion_tokens`

Each value may be:

- a number
- an object with `total`
- an object with `value`

### Model resolution

For each emitted message, `mmr` uses:

1. `message.model` when present
2. otherwise the most recent `model_change`

## mmr mapping

| mmr field | Source |
| --- | --- |
| `source` | `"pi"` |
| `project_name` | parent directory name of the `.jsonl` file |
| `project_path` | `session.cwd` |
| `session_id` | `session.id`, else fallback from the file name |
| `role` | `message.role` |
| `content` | extracted from `message.content` |
| `model` | `message.model`, else most recent `model_change` |
| `timestamp` | top-level `timestamp` |
| `is_subagent` | `false` |
| `msg_type` | same value as `role` |
| `input_tokens` | extracted from `message.usage` |
| `output_tokens` | extracted from `message.usage` |

Messages are emitted only when all of these are non-empty:

- project name
- session ID
- `session.cwd`

## Example

```json
{"type":"session","id":"sess-pi-1","cwd":"/Users/me/proj"}
{"type":"model_change","provider":"openai","modelId":"gpt-5"}
{"type":"message","timestamp":"2025-01-09T00:00:01Z","message":{"role":"user","content":"hello"}}
{"type":"message","timestamp":"2025-01-09T00:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":12,"output_tokens":8}}}
```
