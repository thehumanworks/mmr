# Agent Notes (This Repo)

## What This Repository Is

This repo is a local tool to ingest and browse conversation history from the host machine across:

- Claude Code logs (under `~/.claude/projects/`)
- OpenAI Codex logs (under `~/.codex/sessions/` and `~/.codex/archived_sessions/`)

It is **distinct** from the agent runtime `get_memory` tool.

## CLI Cache

CLI query commands (`projects`, `sessions`, `messages`, `search`, `stats`) now run an automatic **incremental refresh** on every invocation before returning JSON. The refresh is diff-based:

- `ingest_files` tracks per-source JSONL checkpoints (`last_offset`, file size/mtime, and last message watermark).
- `ingest_projects` tracks first/last seen + last ingested times per project across `claude` and `codex`.
- `ingest_sessions` tracks the last ingested message watermark per session.

Only new appended bytes are parsed for unchanged files; rewritten/truncated files are repaired by reprocessing just that file. Deleted source files are removed from cache state and their messages are dropped.

`mmr ingest` (alias: `mmr refresh`) remains available as an explicit full cache rebuild path. Override cache location with `MMR_DB_PATH` (legacy: `MEMORY_DB_PATH`). Server mode remains in-memory.

For Codex project lookups in CLI commands, project keys are absolute `cwd` paths. The CLI normalizes missing-leading-slash inputs (for example `Users/mish/memory` resolves to `/Users/mish/memory`).

## `get_memory` Tool Clarification

`get_memory` reads a host-managed “stored memory” payload keyed by a `memory_id`. It does **not**
read or derive data from this repo or from Codex/Claude session JSONL logs unless the host system
has explicitly persisted those details into memory.
