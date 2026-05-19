# Remember Command

`mmr remember` generates a stateless continuity brief from prior session transcripts for one project.

## Invocation

```bash
mmr remember [all | session <session-id>] [--project <path>] [--agent <agent>] [--instructions <text>] [-O <md|json>] [--model <model>]
```

The global `--source` flag also applies to `remember`.

## Project Scope

- `--project <path>` selects the project whose sessions will be summarized.
- If `--project` is omitted, `remember` uses the current working directory path.
- Unlike `sessions` and `messages`, `remember` does not use cwd auto-discovery fallback logic. If the chosen project has no matching sessions, the command fails instead of searching all projects.

## Session Selection

`remember` supports three selection modes:

- No selector: summarize the latest matching session only.
- `remember all`: summarize all matching sessions for the project.
- `remember session <session-id>`: summarize only the matching session ID within the project scope.

Selection order is based on `sessions` sorted by descending timestamp, so the default mode always picks the most recent session in the filtered project/source scope.

The optional global `--source` filter limits which sessions are eligible before selection happens.

## Backend Selection

`--agent` accepts:

- `gemini`
- `codex`
- `cursor`

Defaulting rules:

1. If `--agent` is passed, it wins.
2. Otherwise, if `MMR_DEFAULT_REMEMBER_AGENT` is set to `gemini`, `codex`, or `cursor`, that value is used.
3. Otherwise, the default backend is `cursor`.

## Backend Requirements and Model Selection

### Gemini

- Client: `src/agent/gemini_api.rs`
- Default model: `gemini-3.1-flash-lite-preview`
- Auth: `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- Optional override: `GEMINI_API_BASE_URL`
- `--model` overrides the default Gemini model

### Cursor

- Client: `src/agent/cursor.rs`
- Default model: `composer-2-fast`
- Auth/runtime requirements:
  - `CURSOR_API_KEY`
  - `agent` CLI available on `PATH`
- `--model` overrides the default Cursor model

### Codex

- Client: `src/agent/codex.rs`
- Default model: `gpt-5.4-mini`
- Reasoning effort: medium
- Auth/runtime requirements: working Codex CLI / SDK authentication
- `--model` is not currently forwarded to the Codex backend

## Prompt Construction

For all three backends, `remember` builds the request from the same transcript selection logic.

### Transcript Loading

1. Load sessions for the selected project (and optional source filter), sorted newest first.
2. Apply the selector (`latest`, `all`, or exact `session <id>`).
3. Load messages for each selected session in chronological order.
4. Format the transcript as session blocks:

```text
=== Session: <session-id> ===
[<timestamp>] <role>: <content>
```

The user prompt sent to the backend is always:

```text
Analyze the following AI coding session transcript(s).
```

followed by the formatted transcript blocks.

### System Instruction Architecture

The system instruction has two parts:

1. **Base instruction**: always present. Establishes the "Memory Agent" identity and describes the transcript input format.
2. **Output instruction**:
   - Without `--instructions`, `mmr` appends the default output instruction (`Purpose`, `Output Format`, `Rules`, and `Resume Instructions`).
   - With `--instructions <text>`, the provided text replaces the entire default output instruction.

Custom `--instructions` do not remove the base instruction. They replace only the output-directing portion.

## Output Formats

`remember` supports two output formats:

### Markdown (`-O md`, default)

- Writes only the returned summary text.
- Trims leading and trailing whitespace.
- If the returned text is empty after trimming, writes:

```text
(No continuity brief returned.)
```

- Does not expose resumability or interaction IDs.

### JSON (`-O json`)

Returns:

```json
{
  "agent": "gemini",
  "text": "continuity summary"
}
```

Rules:

- `agent` is the backend actually used.
- `text` is the backend response body as returned by `mmr`.
- The JSON response does not include resumability, thread, or interaction identifiers.

## Rejected Legacy Flags

The following legacy forms are not part of the current interface and are rejected by clap parsing:

- `--mode`
- `--session-id`
- `--continue-from`
- `--follow-up`

## Examples

Latest session, default markdown output:

```bash
mmr remember --project /Users/test/proj
```

All sessions from one source as JSON:

```bash
mmr --source codex remember all --project /Users/test/proj --agent gemini -O json
```

One session with custom output instructions:

```bash
mmr remember session sess-123 --project /Users/test/proj --instructions "Return only a single keyword."
```
