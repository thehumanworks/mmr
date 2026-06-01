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
- `mmr sync` reconciles redacted project memory with the configured remote.
- `mmr ingest events` ingests provider history into normalized local events.
- `mmr import session` pulls one selected native provider session bundle from an
  explicit SSH peer.
- `mmr import bundle` reads or applies an existing session bundle locator.
- `mmr share session` shares one selected native provider session from the
  source machine.
- `mmr note` adds a human-authored project event.
- `mmr list projects` lists known projects with source and recency metadata.
  `--remote <ssh-target>` also queries explicitly named SSH peers.
- `mmr list sessions` lists sessions, defaulting to the cwd project unless
  `--all` or an explicit project/source scope is used. `--remote <ssh-target>`
  includes explicitly named SSH peers.
- `mmr find` searches normalized events and learned memory. JSON is the default;
  `--format line` emits tab-delimited `citation`, `line`, `source`, `snippet`.
- `mmr recall` retrieves the previous stable session in scope. `mmr recall 2`
  reads two sessions back; age 0 remains held back unless `--include-newest` is
  passed. `--remote <ssh-target>` also queries explicitly named SSH peers.
- `mmr read session <session-id>` reads one explicit session.
- `mmr read project` reads project-scoped history across sources, defaulting to
  cwd. Use `--format tree --output-dir <dir>` for an on-disk event tree.
  `--remote <ssh-target>` also reads from explicitly named SSH peers.
- `mmr read source --source <source>` reads all history for one harness across
  projects. `--remote <ssh-target>` includes peer history for that source.
- `mmr context project` returns project-specific context across sources.
  `--remote <ssh-target>` includes explicitly named SSH peers.
- `mmr context source --source <source>` returns harness-specific context across
  projects. `--remote <ssh-target>` includes peer context.
- `mmr summarize project`, `mmr summarize source --source <source>`, and
  `mmr summarize session <session-id>` run stateless summaries. With
  `--remote <ssh-target>`, `mmr` fetches the remote transcript material first and
  runs summarization locally.
- `mmr compact project`, `mmr compact source --source <source>`, and
  `mmr compact session <session-id>` send selected transcript history to Morph
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
| `mmr messages --all --source <source>` | `mmr read source --source <source>` |
| `mmr prev [N]` | `mmr recall [N]` |
| `mmr export` | `mmr read project` |
| `mmr export --format tree` | `mmr read project --format tree` |
| `mmr summary` / `mmr remember` | `mmr summarize project/session/source` |
| `mmr dream` | `mmr assimilate project` |
| `mmr search` / `mmr rg` | `mmr find` |
| `mmr link` | `mmr init` |

## Current Peer And Movement Model

- Read/query commands use `--remote <ssh-target>`.
- Source-side session movement uses `mmr share session ...`.
- Destination-side session movement uses `mmr import session ...` or
  `mmr import bundle ...`.
- Provider event ingestion uses `mmr --source <source> ingest events ...`.
- No public compatibility aliases are retained for removed command names or
  flags.
