---
name: mmr
description: "Earlier AI coding sessions for the current project, from every agent on this machine (Claude Code, Codex, Cursor, Grok, Pi): what was decided and why, what was tried and rejected, rules the user gave, and where the last session stopped. Use when starting or resuming work in a project, when asked to pick up or continue earlier work, and before explaining why code or config is the way it is or whether something was tried before: git shows what changed, these sessions show why."
---

# mmr: the project's earlier agent sessions

Run mmr from the project directory. It reads every agent's history for that
directory, including sessions started in its subdirectories and worktrees,
whichever agent ran them. Output is JSON on stdout; errors go to stderr.

## Start here

```bash
mmr list sessions               # newest first: source, session_id, dates, preview of the first prompt
mmr recall                      # the previous session in full (skips the newest, usually your own)
mmr read session <session_id>   # one session in full
```

## Search the whole project history

`mmr read project` returns every message of every session, oldest first. Filter
it with jq instead of reading it all:

```bash
mmr read project | jq -r '.messages[]
  | select(.content | test("redis|dedupe"; "i"))
  | "\(.timestamp[0:10]) \(.source) \(.session_id[0:8]) \(.role): \(.content[0:400])"'
```

Compact one session the same way:
`mmr read session <id> | jq -r '.messages[] | "\(.role): \(.content)"'`.

## Rules

- Before answering "why is it like this?" or "did we try X?", search here.
  If nothing relevant turns up, say so; do not infer a reason from git alone.
- Cite what you use: date, source and session. Later sessions override
  earlier ones, and the user's words override an agent's.
- Inside the project, omit `--project`. For another project, pass its absolute
  path (`--project /abs/path`); `mmr list projects` lists them all.
- Omit `--source` unless you want one agent's sessions only
  (`--source claude|codex|cursor|grok|pi`).
- In Claude Code sessions, tool output (file contents, logs) appears as user
  messages. Skim past it.
- Reading needs no setup. Do not run `mmr init` to read history.

## Other commands

- `mmr context project` returns session metadata and recent messages in one
  bounded response.
- `mmr share session <id> --to file:///abs/inbox` and `mmr import bundle <dir>`
  carry one session to another machine as a file.
- `mmr summarize` and `mmr compact` send history to an external model service.
  Use them only when the user wants that.
- `mmr mcp --transport stdio` serves the same reads as MCP tools.

For deeper analysis of long sessions, read `session-mining/SKILL.md`.
