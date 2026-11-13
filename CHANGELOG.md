# Changelog

Each `## vX.Y.Z` section becomes the top of that tag's GitHub release notes.

## v0.3.0 (2026-11-13)

mmr is now one tool for one job: an agent fetches a project's earlier context,
whichever harness produced it. Every command reads each harness's own history
files directly. There is no store to set up, no configuration file and no model
call, and read commands write nothing.

### Breaking changes

Removed commands. Each now fails as an unknown command (exit 2).

| Removed | Use instead |
| --- | --- |
| `init`, `ingest events`, `status` | Nothing. Every command reads native history with no setup. |
| `retrieve <query>` | `find <query>`, then run the match's `read_command`. |
| `note` | None. |
| `read source`, `context source` | `read project` or `context project`, with `--source` to narrow to one harness. `list sessions --all --source <harness>` lists one harness across projects. |
| `summarize project\|session\|source`, `compact project\|session\|source` | `read session`, `read project` or `context project`, and let the agent summarise. |
| `assimilate project\|source` | None. |
| `sync`, `redact scan\|explain` | None. `share session` keeps its privacy scan. |

Removed flags:

- `--remote <ssh-target>` on `list`, `read`, `context` and `recall`. To read
  another machine's session, copy it with `import session --from <ssh-target>`
  or `share session` and `import bundle`.
- `read session|project --format tree` and `--output-dir`. Output is JSON only.
- `find --role`, `--event-type`, `-i/--ignore-case`, `-C/--context` and
  `--format line`. `find` is always case-insensitive and prints JSON.

Removed MCP tools (8 remain: `mmr_list_projects`, `mmr_list_sessions`,
`mmr_find`, `mmr_read_session`, `mmr_read_project`, `mmr_context_project`,
`mmr_recall`, `mmr_skill_load`):

- `mmr_status`, `mmr_read_source`, `mmr_context_source`
- `mmr_summarize_project`, `mmr_summarize_session`, `mmr_summarize_source`
- `mmr_compact_project`, `mmr_compact_session`, `mmr_compact_source`
- `mmr_assimilate_project`, `mmr_assimilate_source`
- the `mmr_memory_assimilation` prompt
- the `context`, `format`, `ignore_case` and `role` parameters of `mmr_find`

Settings that mmr no longer reads:

- `MMR_DEFAULT_SOURCE`. Without `--source`, every command reads every harness.
- `MMR_SUMMARISER_MODEL`, `MMR_COMPACT_MODEL`, `MMR_GITHUB_USER` and the
  `mmr/config.json` configuration file.
- Nix modules: `settings` and `defaultSource` are ignored and print a warning.

Changed output:

- `find` searches native history instead of the store `init` built. Matches
  come newest first, each with a `read_command` that reads it. New flags:
  `--all`, `--limit` (default 20) and `--offset`.
- `read project` without `--project` now returns the newest page first, the
  same as `--project <current directory>`. A larger `--offset` reaches further
  back.
- `read session <id>` exits with an error when no session matches, instead of
  printing an empty page.

The old store at `mmr/mmr.db` in your data directory (`~/.local/share` on
Linux, `~/Library/Application Support` on macOS) is no longer used. You can
delete it.

### Added

- `read session` accepts an id prefix of four or more characters, like a short
  git hash, and lists every match when several sessions share it.
- `mmr skill install` also installs the skill to `~/.claude/skills/mmr`, where
  Claude Code looks, and adds one paragraph between `<!-- mmr:begin -->` and
  `<!-- mmr:end -->` to `~/.claude/CLAUDE.md` and `~/.codex/AGENTS.md` for each
  agent set up on the machine. The paragraph tells agents to check the
  project's earlier sessions before they change code. `--no-instructions`
  skips it.
- MCP tools are marked read-only, so Codex calls them without asking for
  approval.

### Fixed

- Reads Codex 0.164+ history: user prompts recorded only as completed-item
  events, and archived `.jsonl.zst` sessions.
- Reads Cursor project folders without a leading dash and Cursor messages with
  no timestamps.
- Project reads include sessions started in a subdirectory or git worktree of
  the project.
- Project folders whose path mixes `-` and `.`, or has dot folders, decode back
  to the right project.

## v0.2.0 (2026-11-12)

First release with prebuilt binaries. See the
[GitHub release](https://github.com/thehumanworks/mmr/releases/tag/v0.2.0).
