# Pi Messages Schema

This document specifies the recursive session-file layout and record types currently ingested by `mmr` for Pi history.

## File layout

- **Location root**: `~/.pi/agent/sessions/`
- **Discovery**: recursive; every `.jsonl` file under the root is considered
- **Project name**: immediate parent directory name of the `.jsonl` file

Typical example:

```text
~/.pi/agent/sessions/--Users-test-pi-proj--/2025-01-04T00-00-00-000Z_sess-pi-1.jsonl
```

## Record types

Each line is parsed independently. Malformed JSON lines are skipped.

`mmr` recognizes three top-level `type` values:

- `session`
- `model_change`
- `message`

Other values are ignored.

## `session`

Session records establish the current session and cwd:

| Field | Description |
| --- | --- |
| `id` | preferred session ID |
| `cwd` | canonical project path |

If no `session.id` is seen, `mmr` falls back to the file name stem. When the stem contains an underscore, the portion after the last underscore is used as the fallback session ID.

## `model_change`

Model-change records update the current model fallback for later messages.

| Field | Description |
| --- | --- |
| `provider` | provider name |
| `modelId` | model identifier |

Resolved model string:

- both present: `provider/modelId`
- only one present: that single value
- neither present: empty string

## `message`

Only message records with `message.role == "user"` or `message.role == "assistant"` are ingested.

Tool-result and other non-chat roles are skipped.

### `message` payload fields

| Field | Type | Description |
| --- | --- | --- |
| `message.role` | string | `user` or `assistant` |
| `message.content` | mixed | extracted text content |
| `message.model` | string | explicit model override |
| `message.usage` | object | optional token accounting |

## Content extraction

`message.content` may be:

- **string**: used directly
- **array**: only blocks with `type == "text"` contribute output; their `text` values are joined with `\n`
- **object**: only `{ "type": "text", "text": "..." }` contributes output

Blocks such as `thinking`, `toolCall`, and other non-text items are ignored.

## Usage extraction

`mmr` sums multiple naming variants:

- input side: `input`, `input_tokens`, `inputTokens`, `prompt_tokens`
- output side: `output`, `output_tokens`, `outputTokens`, `completion_tokens`

Each value may be:

- a number
- an object with numeric `total`
- an object with numeric `value`

## Example

```json
{"type":"session","id":"sess-pi-1","timestamp":"2025-01-04T00:00:00.000Z","cwd":"/Users/test/pi-proj"}
{"type":"model_change","timestamp":"2025-01-04T00:00:00.100Z","provider":"openai-codex","modelId":"gpt-5.5"}
{"type":"message","timestamp":"2025-01-04T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"hello from pi"}]}}
{"type":"message","timestamp":"2025-01-04T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"internal"},{"type":"toolCall","name":"read"},{"type":"text","text":"hi from pi assistant"}],"model":"gpt-5.5","usage":{"input":12,"output":6}}}
```

## mmr mapping

| mmr field | Source |
| --- | --- |
| `source` | `"pi"` |
| `project_name` | immediate parent directory name of the session file |
| `project_path` | `session.cwd` |
| `session_id` | `session.id`, else filename-derived fallback |
| `role` | `message.role` |
| `content` | extracted text content |
| `model` | `message.model`, else last `model_change` value |
| `timestamp` | top-level `timestamp` |
| `is_subagent` | `false` |
| `msg_type` | same as normalized role |
| `input_tokens` | summed input usage |
| `output_tokens` | summed output usage |

## Constraints and pitfalls

- Messages are emitted only when `project_name`, `session_id`, and `session.cwd` are all non-empty.
- Pi project listings expose the parent directory name under `~/.pi/agent/sessions/...`, while filtering and export logic use the stored `cwd` path.
