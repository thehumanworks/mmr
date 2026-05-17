# Grok Messages Schema

This document specifies the session files that `mmr` ingests from Grok under `~/.grok/sessions/`.

## File Layout

- **Location**: `~/.grok/sessions/<percent-encoded-project>/<session-id>/`
- **Required file**: `updates.jsonl`
- **Optional file**: `summary.json`
- **Project directory name**: percent-encoded path fallback (for example `%2FUsers%2Ftest%2Fgrok-proj` decodes to `/Users/test/grok-proj`)

`mmr` reads `summary.json` first when it is present, then streams `updates.jsonl` line by line.

## `summary.json`

`summary.json` is optional metadata. When present, `mmr` uses:

| Field | Type | Used for |
| --- | --- | --- |
| `info.id` | string | Preferred session ID |
| `info.cwd` | string | Preferred project name/path |
| `current_model_id` | string | Initial model for later messages |
| `created_at` | string | Timestamp fallback |

If `summary.json` is missing or malformed, `mmr` falls back to the session directory name for `session_id` and the decoded project directory name for `project_name`.

## `updates.jsonl` Record Selection

`mmr` skips malformed JSON lines and ignores update types other than:

- `params.update.sessionUpdate == "user_message_chunk"`
- `params.update.sessionUpdate == "agent_message_chunk"`

Examples of ignored records include command availability updates and any other session-update noise.

## Content Extraction

`params.update.content` is extracted recursively:

- **String**: used directly
- **Array**: each item is recursively extracted; non-empty results are joined with `\n`
- **Object**:
  - `text`: used directly
  - `content`: recursively extracted
  - otherwise: empty string

Empty extracted content is skipped.

## User and Assistant Handling

### User chunks

For `user_message_chunk`, `mmr` emits a `"user"` message immediately.

- `params.update._meta.modelId`, when present and non-empty, becomes the current model for subsequent records.
- Timestamp preference is:
  1. `params._meta.agentTimestampMs` converted to RFC 3339
  2. top-level `timestamp`
  3. `summary.created_at`

### Assistant chunks

For `agent_message_chunk`, `mmr` buffers consecutive assistant chunks and concatenates their extracted text into a single `"assistant"` message. The buffered assistant message is flushed:

- when the next user chunk arrives, or
- at end of file

This means chunked assistant output becomes one normalized assistant message in `mmr`.

## Example

Raw fixture-style input:

```json
{"timestamp":1736035201,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello from grok"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736035201000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi "}},"_meta":{"agentTimestampMs":1736035202000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"from grok assistant"}},"_meta":{"agentTimestampMs":1736035202100}}}
```

Normalized outcome:

- one user message with content `hello from grok`
- one assistant message with content `hi from grok assistant`

## mmr Mapping

| mmr Field | Source |
| --- | --- |
| `source` | `"grok"` |
| `project_name` | `summary.info.cwd`, else decoded project directory |
| `project_path` | same value as `project_name` |
| `session_id` | `summary.info.id`, else session directory name |
| `role` | `"user"` or `"assistant"` |
| `content` | extracted chunk text; assistant chunks are concatenated |
| `model` | current model from `summary.current_model_id` or `update._meta.modelId` |
| `timestamp` | `params._meta.agentTimestampMs`, else top-level `timestamp`, else `summary.created_at` |
| `is_subagent` | `false` |
| `msg_type` | same value as `role` |
| `input_tokens` | `0` |
| `output_tokens` | `0` |
