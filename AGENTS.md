# Agent Notes (This Repo)

## What This Repository Is

This repo is a local tool to ingest and browse conversation history from the host machine across:

- Claude Code logs (under `~/.claude/projects/`)
- OpenAI Codex logs (under `~/.codex/sessions/` and `~/.codex/archived_sessions/`)

It is **distinct** from the agent runtime `get_memory` tool.

## CLI Cache

CLI query commands (`projects`, `sessions`, `messages`, `search`, `stats`) now follow **stale-while-revalidate** semantics by default:

- They return current on-disk cache results immediately.
- After responding, they best-effort spawn a detached background incremental refresh so the next run is fresher.
- The worker refreshes a temporary cache snapshot and atomically swaps it into place.
- A lock + cooldown gate prevents stampedes from repeated invocations.
- Pass `--refresh` (or `-r`) to force a synchronous incremental refresh before returning results.

The background refresh itself is diff-based:

- `ingest_files` tracks per-source JSONL checkpoints (`last_offset`, file size/mtime, and last message watermark).
- `ingest_projects` tracks first/last seen + last ingested times per project across `claude` and `codex`.
- `ingest_sessions` tracks the last ingested message watermark per session.

Only new appended bytes are parsed for unchanged files; rewritten/truncated files are repaired by reprocessing just that file. Deleted source files are removed from cache state and their messages are dropped.

`mmr ingest` (alias: `mmr refresh`) remains available as an explicit full cache rebuild path. Override cache location with `MMR_DB_PATH` (legacy: `MEMORY_DB_PATH`). Server mode remains in-memory.

`mmr messages --session <ID>` also includes an on-demand derived-table repair path: if the session has non-subagent messages but missing `sessions`/`projects` rows, it rebuilds derived tables once before returning output.

For Codex project lookups in CLI commands, project keys are absolute `cwd` paths. The CLI normalizes missing-leading-slash inputs (for example `Users/mish/memory` resolves to `/Users/mish/memory`).

## `get_memory` Tool Clarification

`get_memory` reads a host-managed “stored memory” payload keyed by a `memory_id`. It does **not**
read or derive data from this repo or from Codex/Claude session JSONL logs unless the host system
has explicitly persisted those details into memory.
