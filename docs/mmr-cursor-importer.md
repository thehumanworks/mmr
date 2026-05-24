# mmr Cursor importer

Status: implemented for NHL-276
Date: 2026-05-24

The Cursor importer reads local Cursor agent transcript JSONL files through the
shared source adapter framework and writes normalized events into the local
Memory Fabric store.

## Usage

Import Cursor history into a project:

```bash
mmr import --source cursor --project /path/to/project
```

Use a fixture or custom source root:

```bash
mmr import --source cursor --project /path/to/project --source-root /tmp/.cursor
```

Without `--source-root`, the importer reads `$HOME/.cursor`. Discovery scans
`projects/` when present and supports nested transcripts under
`agent-transcripts/<session>/` plus flat JSONL files. A transcript is imported
only when its `cwd`/workspace cwd matches `--project`, when its encoded Cursor
project directory matches the canonical project path, or when a custom flat
source root contains project-local JSONL files directly.

## Normalization

The parser version is `cursor-agent-jsonl-v1`.

Supported Cursor rows include:

- user text as `user_turn`
- assistant text as `assistant_turn`
- tool call blocks as `tool_call`
- tool result blocks as `tool_result`
- thinking/reasoning and summary rows as `compaction`
- session lifecycle rows as `session_start` or `session_end`
- unknown rows as sanitized `unknown_raw_event` summaries

Malformed JSONL rows are skipped with actionable warnings while valid rows keep
importing. A non-newline-terminated malformed tail is treated as an active write
and left unconsumed until it becomes a complete row.

Unknown rows are not indexed as full raw JSON. Local evidence remains available
through raw refs, while normalized/search content avoids provider metadata such
as local project paths.

Cursor tool-call projections also sanitize local filesystem path segments before
they enter search documents. Tool calls, tool results, and unknown events require
a future dedicated safe projection before remote sync eligibility.

## Cursors And Active Capture

Each imported file records a source cursor with:

- cursor key: Cursor transcript JSONL path
- cursor value: imported line count and byte count
- parser version
- last event content hash

The shared file watcher emits only complete JSONL rows, so active-session capture
does not parse partial trailing writes until the row is complete.
