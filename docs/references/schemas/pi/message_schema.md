# Pi Messages Schema

This document describes the Pi session files that `mmr` ingests from `~/.pi/agent/sessions/`.

## File Layout

Pi sessions are stored as JSONL files below the sessions root:

```text
~/.pi/agent/sessions/<project-dir>/<session-file>.jsonl
```

Examples:

- Project directory: `--Users-test-pi-proj--`
- Session file: `2025-01-04T00-00-00-000Z_sess-pi-1.jsonl`

`mmr` walks the tree recursively and ingests every `.jsonl` file.

## Record Types

`updates.jsonl` is not used for Pi. Each session file is itself JSONL with one top-level object per line.

- Empty lines are skipped.
- Malformed JSON lines are skipped.
- `type == "session"` updates session metadata.
- `type == "model_change"` updates the current model for later messages.
- `type == "message"` may emit a normalized message.
- All other record types are ignored.

## `session`

Session metadata establishes:

| Field | Meaning |
| --- | --- |
| `id` | Preferred session ID |
| `cwd` | Project path used as `project_path` |

Fallback behavior:

- If no `session` record appears, `session_id` falls back to the session filename stem after the last underscore.
- Messages are skipped unless `cwd` is known.

## `model_change`

`model_change` updates the current model for later messages.

Relevant fields:

| Field | Meaning |
| --- | --- |
| `provider` | Provider prefix |
| `modelId` | Model identifier |

Formatting rules:

- provider + model => `provider/modelId`
- provider only => `provider`
- model only => `modelId`

## `message`

Only `message.role == "user"` and `message.role == "assistant"` are ingested.

Roles such as `toolResult` are ignored even when they contain text.

Relevant fields:

| Field | Meaning |
| --- | --- |
| `message.role` | Must be `user` or `assistant` |
| `message.content` | Message content |
| `message.model` | Per-message model override |
| `message.usage` | Optional token usage |
| top-level `timestamp` | Stored message timestamp |

### Content Extraction

Pi content extraction is intentionally narrow:

- strings are used directly
- arrays keep only items with `type == "text"`
- objects keep text only when `type == "text"`
- `thinking`, `toolCall`, and similar structured blocks do not contribute message text

If the extracted text is empty, the message is skipped.

### Usage Extraction

`mmr` sums token counts across several common field names:

- input: `input`, `input_tokens`, `inputTokens`, `prompt_tokens`
- output: `output`, `output_tokens`, `outputTokens`, `completion_tokens`

Each value may be:

- a number
- an object containing `total`
- an object containing `value`

## `mmr` Mapping

| `mmr` field | Source |
| --- | --- |
| `source` | `"pi"` |
| `project_name` | Parent directory name under `~/.pi/agent/sessions/` |
| `project_path` | `session.cwd` |
| `session_id` | `session.id`, else filename-derived fallback |
| `role` | `message.role` |
| `content` | Extracted text-only content |
| `model` | `message.model`, else latest `model_change` value |
| `timestamp` | Top-level `timestamp` |
| `is_subagent` | Always `false` |
| `msg_type` | Same as `role` |
| `input_tokens` | Summed from `message.usage` input-style keys |
| `output_tokens` | Summed from `message.usage` output-style keys |

## Operational Notes and Pitfalls

- Pi storage directory names are not the same as the canonical `cwd` path. `mmr` keeps the directory name as `project_name` but also records `cwd` as the project path so path-based project filtering still works.
- Tool-call and tool-result blocks are not exposed as chat messages.
- If a file never establishes `cwd`, all otherwise valid message lines are dropped because the project path is unknown.

## Fixture Example

The integration fixture in `tests/common/mod.rs` seeds this shape:

```json
{"type":"session","version":3,"id":"sess-pi-1","timestamp":"2025-01-04T00:00:00.000Z","cwd":"/Users/test/pi-proj"}
{"type":"model_change","id":"model-1","parentId":null,"timestamp":"2025-01-04T00:00:00.100Z","provider":"openai-codex","modelId":"gpt-5.5"}
{"type":"message","id":"msg-pi-a1","parentId":"msg-pi-u1","timestamp":"2025-01-04T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"internal"},{"type":"toolCall","id":"call-1","name":"read","arguments":{"path":"Cargo.toml"}},{"type":"text","text":"hi from pi assistant"}],"model":"gpt-5.5","usage":{"input":12,"output":6}}}
```
