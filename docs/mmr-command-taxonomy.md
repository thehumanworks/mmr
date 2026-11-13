# mmr command taxonomy

`mmr` helps an agent fetch a project's earlier context, whichever harness
produced it: Claude Code, Codex, Cursor, Grok or Pi. Every command reads each
harness's own history files on this machine. Nothing needs to be set up, no
command calls a model or needs an API key, and no read command writes anything.

## Scope

- Project commands use the current directory's project unless you pass
  `--project <name-or-path>`, or `--all` where the command offers it.
- The global `--source <source>` flag narrows any command to one harness.
  Without it, commands read every harness.
- Output is JSON on stdout. Diagnostics go to stderr. `--pretty` indents JSON.
- Paged commands return `next_page`, `next_offset` and a shell-quoted
  `next_command` that runs as printed.

## Fetch context

- `mmr find <text>` searches message content, case-insensitive, newest first.
  `--project`, `--all` and `--session` change the scope; `--limit` and
  `--offset` page. Each match has `session_id`, `source`, `project_name`,
  `timestamp`, `role`, a `snippet` around the first hit, and a `read_command`
  that reads its session.
- `mmr list sessions` lists the project's sessions, newest first.
  `mmr list projects` lists every project with source and recency metadata.
- `mmr recall` reads the previous stable session in the project. `mmr recall 2`
  reads two sessions back. The newest session is assumed live and held back
  unless `--include-newest` is passed.
- `mmr read session <session-id>` reads one session. `--project` narrows the
  lookup when the same id exists in several projects.
- `mmr read project` reads the project's history across harnesses in time
  order. With `--limit`, the first page holds the newest messages and
  `--offset` reaches further back.
- `mmr context project` returns the project's session list and a bounded array
  of its newest messages in one call. `--limit` bounds the messages.

## Move a session between machines

- `mmr share session <id|latest> --to <ssh-target|file:///inbox>` packs one
  native session from this machine. `--via http` serves it once over HTTP.
- `mmr import session --from <ssh-target>` pulls one session from a peer.
- `mmr import bundle <locator>` reads a bundle.
- Both import commands only read by default. `--apply` restores native provider
  files, and `--force` requires `--apply`.

## Agent integration

- `mmr skill load` prints the bundled agent skill. `mmr skill install` writes
  it into the user's skills directory; `--local` targets the current project.
- `mmr mcp --transport stdio|http` serves the fetch commands as MCP tools.
  See
  [mmr-mcp.md](mmr-mcp.md).

## Removed commands

No compatibility aliases are kept. Each removed name is a usage error (exit 2).

| Removed | Use instead |
| --- | --- |
| `mmr projects`, `mmr sessions` | `mmr list projects`, `mmr list sessions` |
| `mmr messages --session <id>` | `mmr read session <id>` |
| `mmr messages`, `mmr export` | `mmr read project` |
| `mmr prev [N]` | `mmr recall [N]` |
| `mmr summary`, `mmr remember`, `mmr summarize`, `mmr compact` | `mmr read session` or `mmr read project`; the calling agent condenses what it reads |
| `mmr search`, `mmr rg`, `mmr retrieve` | `mmr find`, then each match's `read_command` |
| `read`, `context` with `source` | the `project` or `session` form, with `--source` |
| `--remote <ssh-target>` | `mmr import session --from <ssh-target>` |
| `mmr init`, `mmr link`, `mmr ingest events`, `mmr status` | none; nothing needs to be set up |
| `mmr note`, `mmr dream`, `mmr assimilate` | none |
| `mmr sync`, `mmr redact` | none; `share` keeps its own privacy scan |
| `MMR_DEFAULT_SOURCE` (now ignored) | `--source` on each command |
| `mmr read project --format tree` | `mmr read project` |
