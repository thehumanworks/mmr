# mmr Codex importer

Status: implemented for NHL-274
Date: 2026-05-24

The Codex importer reads local Codex rollout JSONL files through the shared
source adapter framework and writes normalized events into the local Memory
Fabric store.

## Usage

Ingest Codex history into a project:

```bash
mmr --source codex ingest events --project /path/to/project
```

Use a fixture or custom source root:

```bash
mmr --source codex ingest events --project /path/to/project --source-root /tmp/.codex
```

Without `--source-root`, the importer reads `$HOME/.codex`, including
`sessions/` and `archived_sessions/` when present. Discovery is scoped to the
linked project: a rollout is imported only when its `session_meta.payload.cwd`
canonicalizes to the requested `--project` path.

## Normalization

The parser version is `codex-rollout-v1`.

Supported Codex rollout rows include:

- `session_meta` as `session_start`
- `event_msg` user messages as `user_turn`
- assistant `response_item` rows as `assistant_turn`
- function/tool calls as `tool_call`
- function/tool outputs as `tool_result`
- context compaction/reasoning rows as `compaction`
- unknown rows as `unknown_raw_event` with raw JSON text preserved locally

Malformed JSONL rows are skipped with actionable warnings while valid rows keep
importing. A non-newline-terminated malformed tail is treated as an active write
and left unconsumed until it becomes a complete row. Raw refs are local-only
`path:line` citations and are not used as remote-syncable payloads.

`session_meta` rows do not copy the absolute `cwd` into normalized event
content. The cwd is used only for project matching during discovery.

## Cursors And Active Capture

Each imported file records a source cursor with:

- cursor key: rollout path
- cursor value: imported line count and byte count
- parser version
- last event content hash

The shared file watcher emits only complete JSONL rows, so active-session capture
does not parse partial trailing writes until the row is complete.

Tool calls, tool output, and unknown raw Codex events remain local evidence until
a later safe projection exists. `sync --dry-run` reports them as requiring a
dedicated safe sync projection even if deterministic redaction finds no blocking
secret.

## Search

Imported events are immediately discoverable through `mmr find` and `mmr find`.
Search commands rebuild missing `search_documents` rows from normalized events on
demand, so historical imports do not need a separate document rebuild step.
