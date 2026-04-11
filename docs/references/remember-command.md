# Remember Command Reference

`mmr remember` turns one or more stored AI coding sessions into a stateless continuity brief.
It does not resume a provider-native thread. Instead, it loads local session history, formats it
into a neutral transcript, and asks the selected backend to summarize that context.

## Command shapes

```bash
mmr remember [--project <path>] [--agent <cursor|codex|gemini>] \
  [--instructions <text>] [-O <md|json>] [--model <name>]

mmr remember all [--project <path>] [--agent <cursor|codex|gemini>]

mmr remember session <session-id> [--project <path>] [--agent <cursor|codex|gemini>]

mmr --source <claude|codex|cursor> remember ...
```

## What the command selects

`remember` always works on one logical project. By default it searches all sources; `--source`
narrows the input before transcript assembly.

### Session selection modes

| Invocation | Sessions included |
| ---------- | ----------------- |
| `mmr remember ...` | The latest matching session only |
| `mmr remember all ...` | All matching sessions |
| `mmr remember session <id> ...` | Only the named session |

When `all` is used, sessions are sent to the backend in most-recent-first order. Within each
session, messages stay in chronological order.

## Project resolution

- `--project <path>` scopes the lookup to that project.
- If `--project` is omitted, `remember` uses the current working directory string.
- `--source` is optional. Omitting it means "search all supported sources for this project".

### Important pitfall: cwd spelling matters

`remember` uses the raw `current_dir()` string when `--project` is omitted. By contrast,
`export`, `sessions`, and `messages` canonicalize cwd-derived project identifiers before querying.

That difference matters when the same directory can be spelled multiple ways, for example through a
symlink. In those cases, prefer an explicit canonical path:

```bash
mmr remember --project "$(pwd -P)"
```

## Backend selection, auth, and model behavior

`--agent` selects which backend writes the brief. If omitted, `MMR_DEFAULT_REMEMBER_AGENT` is
applied when set; otherwise the default backend is Cursor.

| Backend | Auth / runtime requirements | Default model behavior | `--model` behavior |
| ------- | --------------------------- | ---------------------- | ------------------ |
| `cursor` | `CURSOR_API_KEY` and the `agent` CLI on `PATH` | Defaults to `composer-2-fast` | Passed through to the Cursor CLI |
| `gemini` | `GOOGLE_API_KEY` or `GEMINI_API_KEY` | Defaults to `gemini-3.1-flash-lite-preview` | Overrides the Gemini model |
| `codex` | Local Codex auth/configuration that can start the Codex app server SDK | Always uses `gpt-5.4-mini` with medium reasoning effort | Currently ignored by the Codex backend |

### Backend examples

```bash
# Default backend (Cursor unless MMR_DEFAULT_REMEMBER_AGENT is set)
mmr remember --project /path/to/proj

# Force Gemini and return machine-readable JSON
mmr remember all --project /path/to/proj --agent gemini -O json

# Limit input to one source before summarizing
mmr --source codex remember session sess-123 --project /path/to/proj --agent codex
```

## Prompt construction and transcript format

All backends receive the same neutral user prompt prefix:

```text
Analyze the following AI coding session transcript(s).
```

The transcript body is assembled from stored session messages in this format:

```text
=== Session: <session-id> ===
[<timestamp>] <role>: <content>
```

Operational constraints:

- Session blocks are ordered most recent first.
- Messages inside a session are ordered oldest to newest.
- Tool messages longer than 2000 characters are truncated and end with `... [truncated]`.

## `--instructions` behavior

The `remember` system prompt is built in two parts:

1. A base instruction that always stays present. It establishes the Memory Agent identity and
   describes the transcript input format.
2. An output instruction that controls what kind of answer to return.

Without `--instructions`, the default output instruction asks for a structured continuity brief
with sections for:

- Status
- What Was Done
- Key Decisions & Context
- Open Items
- Relevant File Map
- Resume Instructions

With `--instructions <text>`, the custom text replaces the entire default output instruction. The
base instruction is preserved, but the default purpose, output format, rules, and resume sections
are removed.

Example:

```bash
mmr remember --project /path/to/proj \
  --agent gemini \
  --instructions "Return only a single keyword."
```

## Output formats

### Markdown output (`-O md`, default)

Markdown output prints only the backend's text response, trimmed for surrounding whitespace.

- If the backend returns an empty or whitespace-only response, `mmr` prints:

  ```text
  (No continuity brief returned.)
  ```

- No thread IDs or interaction IDs are included in markdown output.

### JSON output (`-O json`)

JSON output returns the stable `RememberResponse` shape:

```json
{
  "agent": "gemini",
  "text": "continuity summary"
}
```

The JSON response intentionally does not expose resumability IDs such as provider interaction IDs or
thread IDs.

## Troubleshooting

### `No sessions found for project ...`

The current project, source filter, or session selector did not match any stored sessions.

Checks:

1. Confirm the project path:

   ```bash
   mmr sessions --project /path/to/proj
   ```

2. If you used `--source`, verify that the session exists in that source:

   ```bash
   mmr --source codex sessions --project /path/to/proj
   ```

3. If you relied on cwd defaulting, retry with an explicit canonical path:

   ```bash
   mmr remember --project "$(pwd -P)"
   ```

### Gemini key errors

Gemini requires either `GOOGLE_API_KEY` or `GEMINI_API_KEY`. If both are missing or empty,
`remember --agent gemini` fails before making a request.

`GEMINI_API_BASE_URL` is optional and mainly useful for tests or alternate API endpoints.

### Cursor backend failures

Cursor requires both:

- `CURSOR_API_KEY`
- the `agent` executable on `PATH`

If the Cursor CLI exits non-zero, `mmr` surfaces the CLI stderr in the failure message.

## Unsupported legacy flags

The current `remember` interface rejects older flags that no longer exist:

- `--mode`
- `--session-id`
- `--continue-from`
- `--follow-up`

Use the selector subcommands instead:

```bash
mmr remember all --project /path/to/proj
mmr remember session sess-123 --project /path/to/proj
```
