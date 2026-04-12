# `mmr remember` reference

`mmr remember` builds a stateless continuity brief from prior AI coding sessions for one project, then sends the transcript bundle to the selected backend.

This document covers the current command forms, backend selection, prompt contract, output formats, and common operator pitfalls.

## Command forms

```bash
mmr remember --project /path/to/proj
mmr remember all --project /path/to/proj
mmr remember session <session-id> --project /path/to/proj
```

Selector behavior:

- no selector: use the latest matching session
- `all`: use all matching sessions
- `session <session-id>`: use one specific session

Global flags that materially affect the result:

- `--project <path-or-name>`: project scope for session lookup
- `--source <claude|codex|cursor>`: narrow the sessions included in the transcript bundle
- `--agent <cursor|codex|gemini>`: choose the backend
- `--model <name>`: backend-specific model override where supported
- `--instructions <text>`: replace the default output/rules section of the system prompt
- `-O json|md`: choose the final output format

## Project lookup and selection rules

`remember` loads candidate sessions through `QueryService::sessions(...)` using:

- the requested project
- the optional `--source` filter
- `timestamp desc` sorting

Then it selects:

- the first session for the default "latest" mode
- all sessions for `all`
- the exact session ID for `session <id>`

If no sessions match, the command fails with:

```text
No sessions found for project <project>
```

### Important cwd pitfall

When `--project` is omitted, `remember` uses `std::env::current_dir()` as-is from `src/cli.rs`.
Unlike `export`, it does not canonicalize the cwd first.

If your shell path may differ from the stored project identifier (for example, a symlinked directory or another spelling of the same path), pass the exact project explicitly instead of relying on cwd inference.

## Backend selection and defaults

If `--agent` is omitted:

1. `MMR_DEFAULT_REMEMBER_AGENT` is checked.
2. If that env var is unset, empty, or invalid, the default backend is `cursor`.

Accepted env values are:

```bash
MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini
```

### Backend matrix

| Backend | Auth / dependency | Default model behavior | `--model` support |
| --- | --- | --- | --- |
| `cursor` | Requires `CURSOR_API_KEY` and the `agent` CLI on `PATH` | Defaults to `composer-2-fast` | Yes |
| `gemini` | Requires `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL` | Defaults to `gemini-3.1-flash-lite-preview` | Yes |
| `codex` | Uses the configured Codex CLI / SDK auth | Uses the built-in defaults from `src/agent/codex.rs` (`gpt-5.4-mini`, medium reasoning) | Not currently wired through |

Notes:

- Cursor calls `agent -f --approve-mcps --model <model> -p <prompt>`.
- Gemini posts to `<base_url>/interactions` with `x-goog-api-key`.
- Codex starts a fresh thread per request and sets `skip_git_repo_check(true)`.

## Transcript formatting sent to the backend

All three backends receive the same logical transcript content.

### User prompt

The user prompt is intentionally neutral:

```text
Analyze the following AI coding session transcript(s).
```

That prompt is followed by the formatted transcript bundle.

### Session block format

Each selected session is rendered as:

```text
=== Session: <session-id> ===
[<timestamp>] <role>: <content>
```

Additional formatting rules from `src/messages/utils.rs`:

- messages inside each session are loaded in chronological order
- the selected session list is ordered newest-first across sessions
- tool messages longer than 2000 characters are truncated and end with `... [truncated]`

## System prompt architecture

The system prompt has two parts:

1. **Base instruction** (`MEMORY_AGENT_BASE_INSTRUCTION`)
   - always present
   - establishes the "Memory Agent" identity
   - describes the input format only
   - must not contain output-directing language

2. **Output instruction**
   - without `--instructions`, the default output block is appended
   - with `--instructions <text>`, the provided text fully replaces the default output block

The default output block includes:

- `## Purpose`
- `## Output Format`
- `## Rules`
- `### Resume Instructions`

This means `--instructions` is not additive. It replaces the default output/rules section while keeping the base identity and input-format section intact.

Example:

```bash
mmr remember --project /path/to/proj --instructions "Return only a single keyword."
```

## Output formats

### Markdown (`-O md`, default)

- Returns only the response text.
- Leading and trailing whitespace is trimmed.
- If the backend returns empty or whitespace-only text, the CLI prints:

```text
(No continuity brief returned.)
```

### JSON (`-O json`)

JSON output contains only:

```json
{
  "agent": "gemini",
  "text": "..."
}
```

Notably, the CLI does **not** expose resumability identifiers such as thread IDs or interaction IDs in the public JSON response.

## Unsupported legacy flags

The following historical flags are rejected by clap and should not be used:

- `--mode`
- `--session-id`
- `--continue-from`
- `--follow-up`

## Common operator examples

Use the latest session with the default backend:

```bash
mmr remember --project /Users/test/proj
```

Use all Codex sessions and return JSON:

```bash
mmr --source codex remember all --project /Users/test/proj -O json
```

Use Gemini against a mock or alternate base URL:

```bash
GOOGLE_API_KEY=token \
GEMINI_API_BASE_URL=http://127.0.0.1:3000 \
mmr remember --project /Users/test/proj --agent gemini
```

## Related files

- `src/cli.rs`
- `src/agent/ai.rs`
- `src/agent/cursor.rs`
- `src/agent/codex.rs`
- `src/agent/gemini_api.rs`
- `src/messages/utils.rs`
- `tests/cli_contract.rs`
