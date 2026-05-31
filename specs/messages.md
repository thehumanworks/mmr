# Recall And Read Session Behavior

## `read project --project`

Explicit `--project <value>` is alias-capable. The selector first preserves
existing exact project matching, including absolute paths and provider-native
project names. If no exact project matches, it resolves known project aliases,
including the basename of stored project paths and generated provider path aliases
such as `-Users-test-codex-proj`.

If an alias maps to the same canonical project across multiple sources, the
query includes each source-specific project name in that project. If the same
alias maps to multiple distinct project paths, the command fails and asks the
caller to pass an exact project path.

## `mmr recall`

`mmr recall` retrieves the previous stable session in scope. Sessions in the
current scope are ranked newest-first and assigned unsigned recency ages: age 0
is the newest visible session, age 1 is the previous stable session, and so on.

| Surface | Type | Behavior |
|---|---|---|
| `mmr recall` | subcommand, default `N = 1` | The previous stable session in the cwd project. |
| `mmr recall <N>` | `u32`, `N >= 1` | The single session at recency age `N`, counting back from and excluding the newest. |
| `--include-newest` | flag | Makes age 0 addressable. Off by default. |
| `--project <path>` | scope | Computes recency within an explicit project. |
| `--all` | scope | Computes recency across all projects for the selected source filter. |

The global `--source codex|claude|cursor|grok|pi` filter may be combined with
`recall`. Source-wide recall without a source filter is allowed only when the
caller explicitly chooses `--all`.

Age 0 is rejected unless `--include-newest` is present. Out-of-range ages fail
loudly with structured `error_kind` JSON on stdout, a message on stderr, and a
non-zero exit that names the available counts. A scope with zero sessions is a
legitimate empty success with `total_sessions_in_scope: 0`.

## Live-Session-Lag Caveat

`mmr` reads provider transcripts at invocation time and lags un-flushed writes,
so the newest session it can see is often the caller's own half-written live
session. Age 0 is held back by default because returning it would feed an agent
fragments of its own transcript. The held-back session is reported under
`session_selection.skipped_newest` with `assumed_live: true` so the exclusion is
visible.

## `session_selection`

`recall` responses include a `session_selection` object describing `scope`,
`axis`, `total_sessions_in_scope`, `selected`, and `skipped_newest`. Each
selected session includes `age`, `session_id`, `source`, `project_name`,
timestamps, `message_count`, and an `equivalent_command` of
`mmr read session <id>`.

Plain `read project`, `read source`, and `read session` responses omit
`session_selection`.

## Pagination Stability

Recency ages are unstable across time, so a paged recall response pins
`next_command` to the resolved concrete session id:

```bash
mmr --source codex read session <session-id> --limit <N> --offset <M>
```

A session landing between page reads cannot shift the window.

## Raw Reads

`mmr read session <session-id>` reads one explicit session. If no `--project` or
global `--source` is provided, it searches all projects and sources and prints a
stderr hint explaining how to narrow the lookup.

`mmr read project` reads the cwd project by default, or an explicit project with
`--project <path>`. JSON output supports `--limit` and `--offset`; tree output is
available through `--format tree --output-dir <dir>`.

`mmr --source <source> read source` reads all history for one harness across all
projects. The source must be explicit on the global `--source` flag.
