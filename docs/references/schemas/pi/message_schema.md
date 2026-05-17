# Pi Messages Schema

This document specifies the session files that `mmr` ingests from Pi under `~/.pi/agent/sessions/`.

## File Layout

- **Location**: `~/.pi/agent/sessions/**/*.jsonl`
- **Format**: one JSON object per line (JSONL)
- **Project name**: immediate parent directory name of the session file
- **Session ID fallback**: filename stem suffix after the last underscore

Example path:

```text
~/.pi/agent/sessions/--Users-test-pi-proj--/2025-01-04T00-00-00-000Z_sess-pi-1.jsonl
```

For the example above:

- `project_name` fallback is `--Users-test-pi-proj--`
- `session_id` fallback is `sess-pi-1`

## Record Types

`mmr` recognizes three top-level record types:

- `session`
- `model_change`
- `message`

All other record types are ignored.

## `session`

The `session` record establishes session metadata for later messages.

| Field | Type | Used for |
| --- | --- | --- |
| `id` | string | Preferred session ID |
| `cwd` | string | Project path |

If either value is missing, previously known fallback values stay in effect.

## `model_change`

`model_change` updates the current model used for later `message` records that do not carry their own `message.model`.

`mmr` combines:

- `provider`
- `modelId`

into:

- `provider/modelId` when both are present
- `provider` or `modelId` when only one is present

## `message`

Only `message.message.role == "user"` or `"assistant"` is ingested. Other roles, such as `toolResult`, are skipped.

### Content Extraction

`message.content` may be:

- **String**: used directly
- **Array**: only items with `type == "text"` contribute content; their `text` values are joined with `\n`
- **Object**: only `type == "text"` objects contribute content

Non-text blocks such as `thinking` and `toolCall` are ignored.

### Usage Extraction

When `message.usage` is present, `mmr` sums several synonymous keys:

- Input tokens: `input`, `input_tokens`, `inputTokens`, `prompt_tokens`
- Output tokens: `output`, `output_tokens`, `outputTokens`, `completion_tokens`

Each usage value may be a number or an object with `total` or `value`.

## Validation and Skips

`mmr` skips:

- malformed JSON lines
- records without `message`
- roles other than `user` or `assistant`
- empty extracted content
- messages where `project_name`, `session_id`, or `cwd` are still empty

## Example

Raw fixture-style input:

```json
{"type":"session","version":3,"id":"sess-pi-1","timestamp":"2025-01-04T00:00:00.000Z","cwd":"/Users/test/pi-proj"}
{"type":"model_change","id":"model-1","timestamp":"2025-01-04T00:00:00.100Z","provider":"openai-codex","modelId":"gpt-5.5"}
{"type":"message","id":"msg-pi-u1","timestamp":"2025-01-04T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"hello from pi"}]}}
{"type":"message","id":"msg-pi-a1","timestamp":"2025-01-04T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"internal"},{"type":"toolCall","id":"call-1","name":"read","arguments":{"path":"Cargo.toml"}},{"type":"text","text":"hi from pi assistant"}],"usage":{"input":12,"output":6}}}
{"type":"message","id":"msg-pi-t1","timestamp":"2025-01-04T00:00:03.000Z","message":{"role":"toolResult","content":[{"type":"text","text":"ignored"}]}}
```

Normalized outcome:

- one user message with content `hello from pi`
- one assistant message with content `hi from pi assistant`
- token counts `input_tokens = 12`, `output_tokens = 6`
- no `toolResult` message

## mmr Mapping

| mmr Field | Source |
| --- | --- |
| `source` | `"pi"` |
| `project_name` | parent directory name of the session file |
| `project_path` | `session.cwd` |
| `session_id` | `session.id`, else filename suffix |
| `role` | `message.role` (`user` or `assistant`) |
| `content` | extracted text blocks from `message.content` |
| `model` | `message.model`, else latest `model_change` value |
| `timestamp` | top-level `timestamp` |
| `is_subagent` | `false` |
| `msg_type` | same value as `role` |
| `input_tokens` | summed from accepted input usage keys |
| `output_tokens` | summed from accepted output usage keys |
