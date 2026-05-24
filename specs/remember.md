# Remember Command

## Purpose

`mmr remember` loads prior session transcripts from the local history store and asks a selected backend to generate a stateless continuity brief.

It is a one-shot handoff workflow:

- it does not resume an existing chat or thread
- it does not expose backend interaction IDs on stdout
- it can return either human-oriented markdown or a small JSON object for scripting

## Invocation Forms

```bash
mmr remember [flags]
mmr remember all [flags]
mmr remember session <session-id> [flags]
```

- `mmr remember` selects the latest matching session.
- `mmr remember all` includes all matching sessions.
- `mmr remember session <session-id>` includes only the named session.

Legacy selector flags are not supported. Invocations such as `--mode`, `--session-id`, `--continue-from`, and `--follow-up` are rejected by the CLI.

## Project and Source Scope

- `--project` / `-p` is optional.
- When `--project` is omitted, `remember` uses the raw current working directory string from `std::env::current_dir()`.
- Unlike `export`, `sessions`, and `messages`, `remember` does not canonicalize the cwd before lookup.
- The global `--source` filter applies to `remember` the same way it applies to the read/query commands. It limits which history sources contribute transcripts.
- If `--source` is omitted, `MMR_DEFAULT_SOURCE` may supply the default source; otherwise all sources are eligible.

Examples:

```bash
mmr remember --project /Users/test/proj
mmr --source codex remember all --project /Users/test/proj
mmr remember session sess-123 --project /Users/test/proj
```

## Session Selection and Transcript Ordering

`remember` loads matching sessions through the query service, then formats the selected transcripts for the agent backend.

- Matching sessions are selected from the requested project and optional source filter.
- The default selector (`mmr remember`) keeps only the latest session in scope.
- `remember all` keeps all matching sessions.
- `remember session <session-id>` filters to the requested session ID.
- The formatted input is grouped by session.
- Sessions are emitted most-recent-first.
- Messages inside each session are emitted in chronological order.

Each session block uses this format:

```text
=== Session: <session-id> ===
[2025-01-06T00:00:01] user: latest session question
[2025-01-06T00:00:02] assistant: latest session answer
```

Tool-role messages longer than 2000 characters are truncated with a trailing `... [truncated]` marker before being sent to the backend.

## Agent Backend Selection

Use `--agent` to choose which backend produces the continuity brief:

- `cursor`
- `codex`
- `gemini`

If `--agent` is omitted:

1. `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` is consulted.
2. Otherwise the default backend is `cursor`.

An explicit `--agent` flag overrides `MMR_DEFAULT_REMEMBER_AGENT`.

### Cursor backend

- Default model: `composer-2-fast`
- Requirements: `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- `--model` overrides the default Cursor model
- The system instruction is wrapped into the user payload as:

```text
<system>
...
</system>

<user>...</user>
```

### Codex backend

- Fixed model: `gpt-5.4-mini`
- Uses the local Codex app-server WebSocket flow
- `--model` does not affect Codex `remember` requests
- Developer instructions receive the composed system instruction

### Gemini backend

- Default model: `gemini-3.1-flash-lite-preview`
- Requires `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- Optional `GEMINI_API_BASE_URL` overrides the default API base
- `--model` overrides the default Gemini model
- Requests are sent through the Gemini Interactions API with `system_instruction`

## Prompt Construction

The effective system prompt is always built from two parts:

1. **Base instruction**: establishes the Memory Agent identity and describes the transcript input format
2. **Output instruction**: tells the backend what kind of continuity brief to produce

Behavior:

- Without `--instructions`, the default output instruction is appended.
- With `--instructions <text>`, the provided text replaces the entire default output instruction.
- The base instruction is always preserved.
- The user prompt is always neutral: `Analyze the following AI coding session transcript(s).`

The default output instruction asks for a structured continuity brief with these sections when relevant:

- Status
- What Was Done
- Key Decisions & Context
- Open Items
- Relevant File Map
- Resume Instructions

## Output Contract

`remember` is the only top-level command whose default stdout format is markdown instead of JSON.

### Markdown output

`-O md` or `--output-format md` is the default.

- stdout contains the trimmed continuity brief text only
- if the backend returns only whitespace, stdout becomes `(No continuity brief returned.)`
- backend interaction or thread identifiers are not printed

### JSON output

`-O json` or `--output-format json` returns the serialized `RememberResponse`:

```json
{
  "agent": "gemini",
  "text": "Status\n- ..."
}
```

Constraints:

- the JSON response includes only `agent` and `text`
- resumability IDs are intentionally omitted
- callers that need machine parsing should prefer `-O json`

## Common Pitfalls and Troubleshooting

### `No sessions found for project ...`

`remember` defaults to the raw cwd string, not a canonicalized project path. If the current directory text does not match the stored project identifier, pass an explicit project value:

```bash
mmr remember --project /absolute/path/to/proj
```

### `--source` vs `--agent`

- `--source` filters which history source(s) are read
- `--agent` selects which backend summarizes the transcripts

These flags are independent.

### Scripting `--project`

When invoking `mmr` from a script, pass `--project` and its value as separate argv tokens:

```bash
mmr remember --project /Users/test/proj -O json
```

Avoid passing a quoted combined token such as `--project="/Users/test/proj"` from wrapper code that may preserve the quotes literally.
