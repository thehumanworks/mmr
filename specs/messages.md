# Messages Command

## `--project`

Explicit `--project <value>` is alias-capable. The selector first preserves
existing exact project matching, including absolute paths and provider-native
project names. If no exact project matches, it resolves known project aliases,
including the basename of stored project paths and generated provider path aliases
such as `-Users-test-codex-proj`.

If an alias maps to the same canonical project across multiple sources, the
query includes each source-specific project name in that project. If the same
alias maps to multiple distinct project paths, the command fails and asks the
caller to pass an exact project path.

## `--latest`

`mmr messages --latest` selects the latest session in the current scope and returns only the latest message from that session.

`mmr messages --latest <N>` selects the latest session in the current scope and returns the latest `N` messages from that session.

The returned latest-session window is ordered chronologically. Scope filters (`--source`, `--project`, `--all`) still apply. `--latest` is a session selector and is mutually exclusive with `--session`, `--session-back`, and `--session-range` (see below).

## Reverse session selection (`--session-back`, `--session-range`, `mmr prev`)

These selectors pull the messages of a **previous** session by recency, without knowing any session ID. Sessions in the current scope are ranked newest-first and assigned one-based, unsigned ages: age 0 is the newest visible session, age 1 the previous, and so on.

| Surface | Type | Behavior |
|---|---|---|
| `--session-back <N>` | `u32`, `N ≥ 1` | The single session at recency-age `N` (counting back from, and excluding, the newest). `0` is rejected unless `--include-newest`. |
| `--session-range <FROM..TO>` | two `u32` ages, `FROM ≥ TO ≥ 1` | A contiguous span by age, both ends inclusive, written older-bound `..` newer-bound (`2..1` = ages 1 and 2). The newest session (age 0) is never range-addressable. |
| `--include-newest` | flag | Makes age 0 (the newest, assumed-live session) addressable. Off by default. |
| `mmr prev [N]` | subcommand, default `N = 1` | Sugar for `mmr messages --session-back N`; accepts `--project`, `--all`, `--source`, `--limit`, `--pretty`. |

`--session-back`, `--session-range`, `--session`, and `--latest` are mutually exclusive selectors; passing two together is a usage error on stderr with a non-zero exit. The `--source`/`--project`/`--all` scope flags define the set of sessions the ages are computed over **before** age assignment, so a bare `--session-back 1` means "the previous session in this cwd project". The message-axis flags (`--from-message-index`, `--to-message-index`, `--limit`, `--offset`, `--sort-by`, `--order`) compose on top of the selected session(s); messages from a multi-session range are merged and returned chronologically.

### Live-session-lag caveat

`mmr` reads provider transcripts at invocation time and lags un-flushed writes, so the newest session it can see is often the caller's own half-written live session. Age 0 is therefore held back by default — returning it would feed an agent fragments of its own transcript. The held-back session is reported under `session_selection.skipped_newest` (with `assumed_live: true`) so the exclusion is visible. Use `--include-newest` to address age 0 deliberately.

### `session_selection` response field

When a session axis is used, the response carries an optional `session_selection` object describing `scope`, `axis`, `total_sessions_in_scope`, the `selected` session(s) (each with `age`, `session_id`, `source`, `project_name`, timestamps, `message_count`, and an `equivalent_command` of `mmr messages --session <id>`), and `skipped_newest`. The field is omitted entirely for non-axis queries, so default `messages` output is byte-identical. Out-of-range ages and age-0-without-`--include-newest` fail loudly (structured `error_kind` on stdout, message on stderr, non-zero exit) and name the available counts; a scope with zero sessions is a legitimate empty success with `total_sessions_in_scope: 0`.

### Pagination stability

Recency ages are unstable across time, so a paged session-axis query pins its `next_command` to the resolved concrete session id(s) (the `mmr messages --session <id>` form) rather than echoing `--session-back`/`--session-range`. A session landing between page reads cannot shift the window. (`--session` is repeatable to make a multi-session range's continuation runnable.)

## `--from-message-index` and `--to-message-index`

`mmr messages --from-message-index <N>` starts the result window at zero-based message index `N` after source, project, session, sort, and latest-session filtering.

`mmr messages --to-message-index <N>` stops the result window before zero-based message index `N`.

When both flags are supplied, `--from-message-index` is inclusive and `--to-message-index` is exclusive. The returned window remains ordered according to the selected message ordering. `total_messages` continues to report the full scoped message count before applying the message-index window.
