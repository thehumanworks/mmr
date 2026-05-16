# Remember Command

This spec is the canonical contract for `mmr remember`.

## Command shape

`remember` supports three selectors:

- `mmr remember` - use the latest matching session
- `mmr remember all` - use all matching sessions
- `mmr remember session <session-id>` - use exactly one matching session

If `--project` is omitted, `remember` uses the current working directory path as the project value.

`--source` is optional and filters which transcripts are loaded before the summary is generated.

## Agent selection

`remember` supports these backends:

- `cursor`
- `codex`
- `gemini`

Selection precedence is:

1. explicit `--agent`
2. `MMR_DEFAULT_REMEMBER_AGENT`
3. built-in default of `cursor`

If the built-in default is used and `--model` is omitted, the Cursor backend uses model `composer-2-fast`.

## Output formats

### Markdown output

`-O md` / `--output-format md` is the default.

Rules:

- stdout is plain markdown text, not JSON
- leading and trailing whitespace is trimmed from the generated brief
- if the backend returns only whitespace, the CLI prints `(No continuity brief returned.)`
- resumability identifiers are not printed

### JSON output

`-O json` returns:

- `agent`
- `text`

The JSON response does not include interaction IDs or thread IDs.

## System prompt architecture

The system prompt passed to the backend has two parts:

1. `MEMORY_AGENT_BASE_INSTRUCTION`
2. an output instruction

### Base instruction

The base instruction is always present. It establishes:

- the Memory Agent identity
- the transcript input format

The base instruction must not include output-directing language.

### Default output instruction

When `--instructions` is omitted, the default output instruction is appended after the base instruction. It defines the default continuity-brief purpose, output format, rules, and resume instructions.

### Custom `--instructions`

When `--instructions <text>` is provided, that custom text replaces the entire default output-instruction section.

Rules:

- the base instruction remains present
- the default output sections are removed
- the user prompt remains neutral: `Analyze the following AI coding session transcript(s).`

## Backend requirements

- Gemini requires `GOOGLE_API_KEY` or `GEMINI_API_KEY`
- Gemini optionally uses `GEMINI_API_BASE_URL`
- Cursor requires `CURSOR_API_KEY` and the `agent` CLI on `PATH`
- Codex relies on the existing Codex CLI auth setup available to `codex exec`

## Rejected legacy flags

The current CLI rejects the legacy remember flags:

- `--mode`
- `--session-id`
- `--continue-from`
- `--follow-up`
