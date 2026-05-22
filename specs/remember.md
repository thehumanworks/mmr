# Remember Command

`mmr remember` generates a stateless continuity brief from previously recorded sessions for a project.

This spec defines command selection, backend behavior, system-prompt construction, output formats, and operational constraints.

## Command Surface

Supported forms:

```bash
mmr remember --project /path/to/proj
mmr remember all --project /path/to/proj
mmr remember session <session-id> --project /path/to/proj
```

Selection semantics:

- Omitted selector: use the latest matching session.
- `all`: use all matching sessions, newest session first.
- `session <session-id>`: use only that specific session.

Legacy flags such as `--mode`, `--session-id`, `--continue-from`, and `--follow-up` are rejected.

## Project and Source Resolution

- `remember` accepts optional global `--source`.
- If `--project` is omitted, `remember` uses `std::env::current_dir()` as the project identifier.
- Unlike `export`, `sessions`, and `messages`, that cwd default is **not canonicalized** before lookup.

### Practical Constraint

For Codex, Grok, and Pi, project matching is path-based. A symlinked or non-canonical cwd can therefore miss sessions that `export` or `messages` would find when using their canonical cwd resolution. Scripts that need predictable matching should pass `--project` explicitly.

## Backends

`remember` supports three backends:

- `cursor`
- `codex`
- `gemini`

Defaulting rules:

- If `--agent` is present, use it.
- Else if `MMR_DEFAULT_REMEMBER_AGENT` is set to `cursor`, `codex`, or `gemini`, use that.
- Else default to `cursor`.

### Cursor Backend

- Implementation: `src/agent/cursor.rs`
- Default model: `composer-2-fast`
- Auth/runtime requirements:
  - `CURSOR_API_KEY`
  - `agent` CLI on `PATH`
- `--model` applies to Cursor.
- The system prompt and user input are wrapped into:

```text
<system>
...
</system>

<user>...</user>
```

### Codex Backend

- Implementation: `src/agent/codex.rs`
- Uses the Codex app-server websocket client.
- Fixed model: `gpt-5.4-mini`
- Fixed reasoning effort: medium
- `--model` is currently ignored for Codex.

### Gemini Backend

- Implementation: `src/agent/gemini_api.rs`
- Default model: `gemini-3.1-flash-lite-preview`
- Auth/runtime requirements:
  - `GOOGLE_API_KEY` or `GEMINI_API_KEY`
  - optional `GEMINI_API_BASE_URL`
- `--model` applies to Gemini.

## System Prompt Architecture

`remember` builds its system instructions in two layers:

1. **Base instruction** (`MEMORY_AGENT_BASE_INSTRUCTION`)
2. **Output instruction**

### Base Instruction

The base instruction is always present and contains only:

- the Memory Agent identity
- the transcript input format

It must not contain output-directing language such as:

- "continuity brief"
- "sole purpose"
- output quality directives

### Output Instruction

If `--instructions` is omitted, `remember` appends the default output instruction, which defines:

- `## Purpose`
- `## Output Format`
- `## Rules`
- `### Resume Instructions`

If `--instructions <text>` is provided, that custom text replaces the entire default output instruction. The base instruction remains intact.

The user prompt is intentionally neutral:

```text
Analyze the following AI coding session transcript(s).
```

That means the system instruction is the only part that specifies the desired output behavior.

## Transcript Selection and Formatting

Session transcripts are loaded through `src/messages/utils.rs`:

- Sessions are selected from `service.sessions(..., SortBy::Timestamp, SortOrder::Desc)`.
- Transcript messages for each selected session are loaded with chronological ordering.
- The final transcript bundle is formatted newest session first.

Each session is serialized like:

```text
=== Session: <session-id> ===
[<timestamp>] <role>: <content>
```

### Tool Message Truncation

Tool messages longer than 2000 characters are truncated before being sent to the backend and suffixed with:

```text
... [truncated]
```

Non-tool messages are not truncated by this helper.

## Output Formats

`remember` returns `RememberResponse` in JSON mode:

```rust
pub struct RememberResponse {
    pub agent: Agent,
    pub text: String,
}
```

Output modes:

- default: markdown (`-O md` / `--output-format md`)
- structured: JSON (`-O json` / `--output-format json`)

Markdown mode returns only the trimmed summary text. It does not expose thread IDs, interaction IDs, or resumability handles.

If the backend returns only whitespace, markdown mode prints:

```text
(No continuity brief returned.)
```

## Examples

```bash
# Default selector: latest session, markdown output
mmr remember --project /Users/test/proj

# Use all sessions and request JSON
mmr remember all --project /Users/test/proj -O json

# Restrict to Codex sessions only
mmr --source codex remember all --project /Users/test/proj --agent gemini

# Replace the default output instruction entirely
mmr remember --project /Users/test/proj --instructions "Return only a single keyword."
```

## Verified Constraints

- `remember` is one-shot. It does not resume prior backend conversations.
- Default markdown output is intentional; JSON must be requested explicitly.
- Explicit `--agent` overrides `MMR_DEFAULT_REMEMBER_AGENT`.
- Source filtering limits which sessions are included before transcript formatting.
