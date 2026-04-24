# CLI Query and Remember Workflows

This reference documents the current user-facing behavior for `mmr` query commands and the `remember` workflow.

It is intended to answer the questions that are easy to miss when reading only ADRs or tests:

- Which commands default to the current working directory?
- When does `--all` actually matter?
- How should callers use `next_page`, `next_offset`, and `next_command`?
- What does `remember --instructions` replace, and what does it preserve?

The behavior described here is verified against:

- `src/cli.rs`
- `src/messages/service.rs`
- `src/agent/ai.rs`
- `tests/cli_contract.rs`

## Command defaults at a glance

| Command | Default project scope | Default source scope | Notes |
| --- | --- | --- | --- |
| `projects` | All projects | All sources unless `MMR_DEFAULT_SOURCE` is set | No cwd auto-discovery |
| `sessions` | Auto-discovered cwd project when discovery succeeds | All sources unless `MMR_DEFAULT_SOURCE` is set | `--all` disables cwd scoping |
| `messages` | Auto-discovered cwd project when discovery succeeds | All sources unless `MMR_DEFAULT_SOURCE` is set | `--session` has a special bypass rule |
| `export` | Current working directory when `--project` is omitted | All sources unless `--source` or `MMR_DEFAULT_SOURCE` is set | Always returns `ApiMessagesResponse` |
| `remember` | Current working directory when `--project` is omitted | All sources unless `--source` or `MMR_DEFAULT_SOURCE` is set | Agent defaults can come from env |

`--source all` is not a valid value. Omitting `--source` means "all supported sources" unless `MMR_DEFAULT_SOURCE` supplies a default.

## Project scoping and cwd auto-discovery

### `sessions` and `messages`

When `--project` is omitted, `mmr sessions` and `mmr messages` try to scope themselves to the current working directory by default.

- If cwd auto-discovery succeeds, the command behaves as if a project-local filter was supplied.
- If cwd auto-discovery fails, the command falls back to the historical global behavior and searches across all projects.
- If cwd auto-discovery succeeds but the project has no matching history, the command returns an empty result. It does **not** widen the search automatically.

Use `--all` when you explicitly want cross-project results:

```bash
mmr sessions --all
mmr messages --all
```

Environment control:

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery for `sessions` and `messages`.
- `MMR_AUTO_DISCOVER_PROJECT=1` or an unset variable keeps cwd auto-discovery enabled.

### How project names are resolved across sources

For `sessions` and `messages`, the CLI derives a canonical cwd path first. The query layer then resolves that value against known projects across the enabled sources.

That means callers can usually pass one project value and let `mmr` match the source-specific project representation:

- Codex projects match canonical filesystem paths such as `/Users/test/proj`
- Claude and Cursor projects can be resolved through the stored project metadata for the same logical project

### `export`

`mmr export` uses slightly different mechanics because it queries each source directly when `--project` is omitted.

With no `--project`:

- Codex uses the canonical cwd path as-is
- Claude uses the same path with `/` replaced by `-` and a leading hyphen
- Cursor uses the same cwd-derived name as Claude

Example:

- cwd: `/Users/test/proj`
- Codex project key: `/Users/test/proj`
- Claude/Cursor project key: `-Users-test-proj`

`export` merges those source-specific results, sorts them by timestamp ascending, and returns the normal `ApiMessagesResponse` shape.

## `messages --session` lookup rules

`messages --session <ID>` has one important exception to the default cwd behavior.

When a caller provides `--session` **without** `--project`, `mmr` searches across all projects instead of applying cwd auto-discovery. This avoids silently missing a globally unique session that belongs to a different project than the current directory.

Behavior summary:

| Invocation | Project scope | Extra behavior |
| --- | --- | --- |
| `mmr messages --session sess-123` | All projects | Prints a stderr hint suggesting `--source` |
| `mmr --source claude messages --session sess-123` | All projects | No hint |
| `mmr messages --session sess-123 --project /path/to/proj` | Explicit project only | No bypass |
| `mmr messages` | Cwd auto-discovery when enabled | Standard default behavior |

The hint is:

```text
hint: searching all sources for session; pass --source to narrow the search
```

This hint is diagnostic output on `stderr`; JSON on `stdout` stays machine-readable.

See also: `docs/references/session-lookup-invariants.md`.

## Messages pagination contract

`messages` returns pagination metadata in `ApiMessagesResponse`:

| Field | Meaning |
| --- | --- |
| `total_messages` | Total number of messages that matched the filter before pagination |
| `next_page` | `true` when another page is available |
| `next_offset` | The offset to use for the next page |
| `next_command` | A ready-to-run CLI command for the next page; omitted when there is no next page |

Example:

```json
{
  "messages": [ ... ],
  "total_messages": 6,
  "next_page": true,
  "next_offset": 2,
  "next_command": "mmr --source codex messages --project /Users/test/codex-proj --limit 2 --offset 2"
}
```

### Ordering semantics

The default `messages` command has a subtle contract:

- Sorting defaults to `--sort-by timestamp --order asc`
- Pagination still selects the **newest** window first
- The returned page is then re-ordered into chronological order before serialization

In practice, that means the default output is "latest N messages, displayed oldest-to-newest within the page".

If the caller changes the sort key or order, `mmr` paginates the sorted list directly and `next_command` preserves those flags.

### Operational guidance

- Prefer replaying `next_command` when scripting interactive paging from CLI output.
- If you only need the next cursor value, use `next_offset`.
- Do not assume `next_command` is present when `next_page` is `false`.

## `remember` workflow

`remember` builds a stateless summary from one or more session transcripts for a project.

### Session selection

`remember` supports three selectors:

- `mmr remember` -> latest matching session
- `mmr remember all` -> all matching sessions
- `mmr remember session <session-id>` -> one explicit session

If `--project` is omitted, `remember` uses the current working directory as the project path.

`--source` still applies, so callers can restrict the transcripts used to build the summary:

```bash
mmr --source codex remember all --project /Users/test/proj
```

### Agent selection

Supported agents:

- `cursor`
- `codex`
- `gemini`

Resolution order:

1. Explicit `--agent`
2. `MMR_DEFAULT_REMEMBER_AGENT`
3. Built-in default: `cursor`

When `cursor` is used and `--model` is omitted, the default model is `composer-2-fast`.

### System prompt architecture

`remember` constructs the system instruction in two parts:

1. **Base instruction**: always present; establishes "You are a Memory Agent" and explains the transcript input format
2. **Output instruction**: controls the output format and rules

Behavior of `--instructions`:

- Without `--instructions`, `remember` appends the default output instruction (`Purpose`, `Output Format`, `Rules`, and `Resume Instructions`)
- With `--instructions <text>`, the custom text replaces the entire default output instruction
- The base instruction is always preserved

This is intentional: callers can fully redefine the output contract without losing the agent identity or transcript-format context.

### Output formats

- `-O json` returns the structured `RememberResponse`
- `-O md` returns only the summary text, not JSON wrapper fields

Example:

```bash
mmr remember --project /Users/test/proj --agent gemini -O json
mmr remember --project /Users/test/proj --instructions "Return only a single keyword."
```

## Common pitfalls

- `--source all` is rejected; omit `--source` for all sources.
- `messages --session` ignores cwd auto-discovery unless `--project` is also supplied.
- `MMR_DEFAULT_SOURCE=""` is treated as unset, not as a literal empty source.
- `MMR_DEFAULT_REMEMBER_AGENT=""` is treated as unset, not as an error.
- `remember -O md` does not emit JSON, so downstream parsers should use `-O json`.
- Legacy `remember` flags such as `--mode`, `--session-id`, `--continue-from`, and `--follow-up` are rejected.
