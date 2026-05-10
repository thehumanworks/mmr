# Remember Command

## Purpose

`mmr remember` turns one or more stored AI coding sessions into a continuity brief for a follow-on agent run.

The command resolves a project scope, selects one or more sessions inside that scope, formats the transcript(s) into a neutral analysis prompt, and sends that prompt to the configured backend agent.

## Project scope

- `--project <path>` uses the provided project value directly.
- Without `--project`, `remember` uses the current working directory path.
- `--source claude|codex|cursor` is optional. Omitting it searches all sources unless `MMR_DEFAULT_SOURCE` supplies a default.

If the scoped project has no matching sessions, the command fails instead of widening the search.

## Session selection

- `mmr remember` selects the latest matching session.
- `mmr remember all` includes all matching sessions.
- `mmr remember session <session-id>` includes only the named session.

Session transcripts are loaded in descending session recency, and each selected session's messages are sent to the agent in chronological order.

## Agent selection and prompt construction

- `--agent gemini|codex|cursor` selects the backend.
- When `--agent` is omitted, `MMR_DEFAULT_REMEMBER_AGENT` applies if set; otherwise the default backend is `cursor`.
- Cursor uses `composer-2-fast` unless `--model` overrides it.

The system prompt always has two parts:

1. A base instruction that establishes the "Memory Agent" identity and describes the transcript input format.
2. An output instruction that controls the brief structure and rules.

`--instructions <text>` replaces only the output-instruction portion. The base instruction is always preserved.

## Output formats

`remember` defaults to markdown output on stdout:

```bash
mmr remember --project /Users/test/proj
```

In markdown mode (`-O md` or default):

- stdout is the trimmed `RememberResponse.text` value only
- no JSON envelope is emitted
- no resumability metadata is added
- if the response text is empty or whitespace-only, stdout is exactly `(No continuity brief returned.)`

Use `-O json` for machine-readable output:

```bash
mmr remember --project /Users/test/proj -O json --pretty
```

In JSON mode, stdout is the serialized `RememberResponse`, and `--pretty` controls indentation the same way it does for the read/query commands.

## Examples

Generate a markdown brief from the latest session for the current directory's project:

```bash
mmr remember
```

Generate a brief from all sessions for one project with an explicit backend:

```bash
mmr remember all --project /Users/test/proj --agent gemini
```

Request one specific session and override the default brief structure:

```bash
mmr remember session sess-123 --project /Users/test/proj --instructions "Return a 5-bullet handoff."
```
