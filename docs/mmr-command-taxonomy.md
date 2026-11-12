# mmr command taxonomy

`mmr` uses intent-first commands. Historical top-level commands such as
`projects`, `sessions`, `messages`, `export`, `prev`, `summary`, `remember`,
`dream`, `search`, `rg`, and `link` are removed.

## Commands

- `mmr init` sets up or repairs the local store for the current project and
  ingests available source history by default. Use `--link-only` to skip
  ingestion and receive suggested `mmr --source <source> ingest events`
  commands.
- `mmr status` reports store, project, privacy, sync, and provider readiness.
- `mmr sync` reconciles redacted project memory with the configured file-backed
  remote simulation; it is not a hosted GitHub sync service.
- `mmr ingest events` ingests provider history into normalized local events.
- `mmr import session` pulls one selected native provider session bundle from an
  explicit SSH peer; it reads by default and restores native files with `--apply`.
- `mmr import bundle` reads an existing bundle by default; `--apply` explicitly
  restores native files. `--force` requires `--apply`.
- `mmr share session` shares one selected native provider session from the
  source machine.
- `mmr note` adds a human-authored project event.
- `mmr list projects` lists known projects with source and recency metadata.
- `mmr list sessions` lists sessions, defaulting to the cwd project unless
  `--all` or an explicit project/source scope is used.
- `mmr find` searches normalized events and learned memory. JSON is the default;
  `--format line` emits tab-delimited `citation`, `line`, `source`, `snippet`.
- `mmr retrieve "query" --max-sessions 1` ranks readable sessions for a match.
  For phrase → session → context, run `init`, then `find` for lines/citations,
  then `retrieve`. Pass its `selected_sessions` fields `source`,
  `source_session_id`, and `project_name` to
  `mmr --source <source> read session <source_session_id> --project <project_name>`;
  `find`'s `session_id` is an internal store ID. A known native session can be
  read without `init`.
- `mmr recall` retrieves the previous stable session in scope. `mmr recall 2`
  reads two sessions back; age 0 remains held back unless `--include-newest` is
  passed.
- `mmr read session <session-id>` reads one explicit session.
- `mmr read project` reads project-scoped history across sources, defaulting to
  cwd. Use `--format tree --output-dir <dir>` for an on-disk event tree.
- `mmr context project` returns project-specific context across sources.
  `--limit 20` bounds the returned raw messages; it does not call a model.
- `mmr summarize project` and `mmr summarize session <session-id>` run
  stateless summaries.
- `mmr compact project` and `mmr compact session <session-id>` send selected transcript history to Morph
  Compact. This removes low-relevance lines without paraphrasing surviving
  content. Set `MORPHLLM_API_KEY`; optionally set `MORPHLLM_BASE_URL` and
  `MMR_COMPACT_MODEL`, or pass `--model`.
- `mmr assimilate project` returns a prompt, runbook, output contract, and
  evidence bundle for project memory deduplication and generalisation.
- `mmr assimilate source --source <source>` returns the source-wide equivalent,
  using a bounded per-project evidence window.
- `mmr skill load` prints the bundled mmr agent skill to stdout for immediate
  agent context.
- `mmr skill install` replaces `~/.agents/skills/mmr` with the bundled skill;
  `--local` targets `.agents/skills/mmr` under the current project.
- `mmr redact scan` and `mmr redact explain` inspect privacy policy outcomes.

## Replacement Map

| Removed | Replacement |
| --- | --- |
| `mmr projects` | `mmr list projects` |
| `mmr sessions` | `mmr list sessions` |
| `mmr messages --session <id>` | `mmr read session <id>` |
| `mmr messages` | `mmr read project` |
| `mmr messages --all --source <source>` | none; read by project or session |
| `mmr read source`, `context source`, `summarize source`, `compact source` | none; read by project or session |
| `mmr prev [N]` | `mmr recall [N]` |
| `mmr export` | `mmr read project` |
| `mmr export --format tree` | `mmr read project --format tree` |
| `mmr summary` / `mmr remember` | `mmr summarize project/session/source` |
| `mmr dream` | `mmr assimilate project` |
| `mmr search` / `mmr rg` | `mmr find` |
| `mmr link` | `mmr init` |

## Current Peer And Movement Model

- Read/query commands are local only; there is no peer-read flag.
- Source-side session movement uses `mmr share session ...`.
- Destination-side session movement uses `mmr import session ...` or
  `mmr import bundle ...`.
- Provider event ingestion uses `mmr --source <source> ingest events ...`.
- No public compatibility aliases are retained for removed command names or
  flags.
