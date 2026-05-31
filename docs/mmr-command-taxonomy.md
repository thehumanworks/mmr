# mmr command taxonomy

`mmr` uses intent-first commands. Historical top-level commands such as
`projects`, `sessions`, `messages`, `export`, `prev`, `summary`, `remember`,
`dream`, `search`, `rg`, and `link` are removed.

## Commands

- `mmr init` sets up or repairs the local store for the current project and
  imports available source history by default. Use `--link-only` to skip import
  and receive suggested `mmr import` commands.
- `mmr status` reports store, project, privacy, sync, and provider readiness.
- `mmr sync` reconciles redacted project memory with the configured remote.
- `mmr import` ingests source history into normalized local events.
- `mmr note` adds a human-authored project event.
- `mmr list projects` lists known projects with source and recency metadata.
- `mmr list sessions` lists sessions, defaulting to the cwd project unless
  `--all` or an explicit project/source scope is used.
- `mmr find` searches normalized events and learned memory. JSON is the default;
  `--format line` emits tab-delimited `citation`, `line`, `source`, `snippet`.
- `mmr recall` retrieves the previous stable session in scope. `mmr recall 2`
  reads two sessions back; age 0 remains held back unless `--include-newest` is
  passed.
- `mmr read session <session-id>` reads one explicit session.
- `mmr read project` reads project-scoped history across sources, defaulting to
  cwd. Use `--format tree --output-dir <dir>` for an on-disk event tree.
- `mmr read source --source <source>` reads all history for one harness across
  projects.
- `mmr context project` returns project-specific context across sources.
- `mmr context source --source <source>` returns harness-specific context across
  projects.
- `mmr summarize project`, `mmr summarize source --source <source>`, and
  `mmr summarize session <session-id>` run stateless summaries.
- `mmr assimilate project` returns a prompt, runbook, output contract, and
  evidence bundle for project memory deduplication and generalisation.
- `mmr assimilate source --source <source>` returns the source-wide equivalent,
  using a bounded per-project evidence window.
- `mmr skill load` prints the bundled mmr agent skill to stdout for immediate
  agent context.
- `mmr skill install` replaces `~/.agents/skills/mmr` with the bundled skill;
  `--local` targets `.agents/skills/mmr` under the current project.
- `mmr redact scan` and `mmr redact explain` inspect privacy policy outcomes.
- `mmr teleport ...` remains the native session handoff namespace.

## Replacement map

| Removed | Replacement |
| --- | --- |
| `mmr projects` | `mmr list projects` |
| `mmr sessions` | `mmr list sessions` |
| `mmr messages --session <id>` | `mmr read session <id>` |
| `mmr messages` | `mmr read project` |
| `mmr messages --all --source <source>` | `mmr read source --source <source>` |
| `mmr prev [N]` | `mmr recall [N]` |
| `mmr export` | `mmr read project` |
| `mmr export --format tree` | `mmr read project --format tree` |
| `mmr summary` / `mmr remember` | `mmr summarize project/session/source` |
| `mmr dream` | `mmr assimilate project` |
| `mmr search` / `mmr rg` | `mmr find` |
| `mmr link` | `mmr init` |
