# Pi Messages Schema

This document specifies the JSONL session files ingested by mmr from `~/.pi/agent/sessions/`.

## File Layout

- **Location**: `~/.pi/agent/sessions/**/<project_name>/*.jsonl`
- **Format**: One JSON object per line (JSONL)
- **Extension**: `.jsonl`
- **Project name**: Parent directory name containing the session file
- **Session ID fallback**: Final `_suffix` in the file stem (for example, `2025-01-04T00-00-00-000Z_sess-pi-1.jsonl` falls back to `sess-pi-1`)

## Record Types

mmr recognizes three top-level record types:

- `session`
- `model_change`
- `message`

Other record types are ignored.

### `session`

Session records establish the current session metadata.

| Field       | Type   | Description |
| ----------- | ------ | ----------- |
| `id`        | string | Session ID override |
| `cwd`       | string | Project path used for `project_path` |
| `timestamp` | string | Session timestamp; not emitted as a message |

### `model_change`

Model change records update the fallback model for later messages.

| Field      | Type   | Description |
| ---------- | ------ | ----------- |
| `provider` | string | Provider name |
| `modelId`  | string | Model name |

If both are present, mmr formats them as `<provider>/<modelId>`.

### `message`

Only message records whose nested `message.role` is `"user"` or `"assistant"` are ingested.

| Field            | Type   | Description |
| ---------------- | ------ | ----------- |
| `message.role`   | string | Must be `"user"` or `"assistant"` |
| `message.content`| mixed  | Message payload; see Content Extraction |
| `message.model`  | string | Optional per-message model |
| `message.usage`  | object | Optional token usage |
| `timestamp`      | string | Message timestamp |

If `message.model` is absent, the most recent `model_change` value is used.

## Content Extraction

`message.content` may be:

- **String**: used directly.
- **Array**: only blocks with `type: "text"` contribute output; extracted blocks are joined with `\n`.
- **Object**: only `type: "text"` with a `text` field contributes output.

This means blocks such as `thinking`, `toolCall`, and `toolResult` do not become chat messages.

## Usage Extraction

mmr aggregates token counts from several common Pi usage keys:

- input: `input`, `input_tokens`, `inputTokens`, `prompt_tokens`
- output: `output`, `output_tokens`, `outputTokens`, `completion_tokens`

Each key may be either:

- a number, or
- an object containing `total` or `value`

## Example

```json
{"type":"session","version":3,"id":"sess-pi-1","timestamp":"2025-01-04T00:00:00.000Z","cwd":"/Users/test/pi-proj"}
{"type":"model_change","provider":"openai","modelId":"gpt-5.5"}
{"type":"message","id":"msg-pi-u1","timestamp":"2025-01-04T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"hello from pi"}]}}
{"type":"message","id":"msg-pi-a1","timestamp":"2025-01-04T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"internal"},{"type":"toolCall","id":"call-1","name":"read","arguments":{"path":"Cargo.toml"}},{"type":"text","text":"hi from pi assistant"}],"usage":{"input":12,"output":6}}}
```

The assistant message above yields only `hi from pi assistant` as chat content.

## mmr Mapping

| mmr Field       | Source |
| --------------- | ------ |
| `source`        | `"pi"` |
| `project_name`  | Parent directory name containing the session file |
| `project_path`  | `session.cwd` |
| `session_id`    | `session.id` when present, else the file-stem suffix |
| `role`          | `message.role` |
| `content`       | Extracted from `message.content` |
| `model`         | `message.model` or the latest `model_change` value |
| `timestamp`     | Top-level `timestamp` from the message record |
| `is_subagent`   | `false` |
| `msg_type`      | Same as `role` |
| `input_tokens`  | Aggregated from the supported usage input keys |
| `output_tokens` | Aggregated from the supported usage output keys |

## Constraint

Pi messages are emitted only after the loader has both a project directory name and a non-empty `session.cwd`. Records missing project name, session ID, or cwd are skipped.
