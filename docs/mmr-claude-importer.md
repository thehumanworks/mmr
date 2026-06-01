# mmr Claude importer

Status: implemented for NHL-275
Date: 2026-05-24

The Claude importer reads local Claude Code JSONL files through the shared source
adapter framework and writes normalized events into the local Memory Fabric
store.

## Usage

Ingest Claude history into a project:

```bash
mmr --source claude ingest events --project /path/to/project
```

Use a fixture or custom source root:

```bash
mmr --source claude ingest events --project /path/to/project --source-root /tmp/.claude
```

Without `--source-root`, the importer reads `$HOME/.claude`. Discovery scans
`projects/` when present and imports only sessions whose first `cwd` or decoded
Claude project directory matches the requested `--project` path.

## Normalization

The parser version is `claude-code-jsonl-v1`.

Supported Claude rows include:

- user text as `user_turn`
- assistant text as `assistant_turn`
- `tool_use` content blocks as `tool_call`
- `tool_result` content blocks as `tool_result`
- `thinking` blocks and summary rows as `compaction`
- known session lifecycle rows as `session_start` or `session_end`
- `queue-operation` enqueue rows as `user_turn`
- metadata-heavy rows such as `attachment` and `file-history-snapshot` as
  sanitized `unknown_raw_event` summaries
- unknown rows as `unknown_raw_event` with local raw refs for evidence

Malformed JSONL rows are skipped with actionable warnings while valid rows keep
importing. A non-newline-terminated malformed tail is treated as an active write
and left unconsumed until it becomes a complete row.

The importer uses `cwd` for project matching only. Normalized event/search
content does not copy the absolute project path. Unknown rows are not indexed as
full raw JSON; metadata-only fields remain available through local raw refs.

## Cursors And Active Capture

Each imported file records a source cursor with:

- cursor key: Claude JSONL path
- cursor value: imported line count and byte count
- parser version
- last event content hash

The shared file watcher emits only complete JSONL rows, so active-session capture
does not parse partial trailing writes until the row is complete.

Large tool results are truncated at import time with a marker so search and
future summary paths do not ingest unbounded tool payloads. The marker records
the omitted character count and full-content hash for evidence reconciliation.
Tool calls, tool results, and unknown events require a future dedicated safe
projection before remote sync eligibility.
