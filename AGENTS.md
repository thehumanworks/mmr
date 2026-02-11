# Agent Notes (This Repo)

## What This Repository Is

This repo is a local tool to ingest and browse conversation history from the host machine across:

- Claude Code logs (under `~/.claude/projects/`)
- OpenAI Codex logs (under `~/.codex/sessions/` and `~/.codex/archived_sessions/`)

It is **distinct** from the agent runtime `get_memory` tool.

## CLI Cache

CLI query commands read from an on-disk DuckDB cache. Build/refresh it with `mmr ingest` (alias: `mmr refresh`). Override the cache location with `MMR_DB_PATH` (legacy: `MEMORY_DB_PATH`). Server mode remains in-memory.

## `get_memory` Tool Clarification

`get_memory` reads a host-managed “stored memory” payload keyed by a `memory_id`. It does **not**
read or derive data from this repo or from Codex/Claude session JSONL logs unless the host system
has explicitly persisted those details into memory.
