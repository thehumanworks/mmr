# mmr CLI Functionality Review

This README focuses on the `mmr` CLI behavior implemented in `src/main.rs` and validated by tests in `tests/cli_cache.rs`.

## Top Commands

### Retrieve projects

```bash
# Default source is codex
mmr projects

# Explicit source
mmr --source claude projects
mmr --source codex projects

# Cross-source listing (projects/search/stats treat this as "no source filter")
mmr --source all projects
```

What it returns:
- `projects[]` with `name`, `source`, `original_path`, `session_count`, `message_count`, `last_activity`
- top-level `total_messages` and `total_sessions`

Default ordering:
- newest project activity first (`last_activity DESC`)

### Retrieve sessions

```bash
mmr --source codex sessions --project "/Users/mish/projects/mmr"
mmr --source claude sessions --project="-Users-mish-projects-mmr"
```

Notes:
- `--project` is required
- default source is `codex` if `--source` is omitted
- for codex, project keys are normalized, so `Users/mish/projects/mmr` can resolve to `/Users/mish/projects/mmr`

Default ordering:
- newest session first (`last_timestamp DESC`)

### Retrieve messages from a session

```bash
mmr messages --session "SESSION_ID"
mmr messages --session "SESSION_ID" --limit 20
mmr messages --session "SESSION_ID" --limit 20 --offset 20
```

Notes:
- `--session` is required
- selection is done from newest messages first, then output is reversed to chronological order for readability
- this means `--offset` skips from the newest side of the windowed selection
- if derived `sessions`/`projects` rows are missing for an existing session, `mmr messages` performs a one-time derived-table repair

Default ordering in output:
- chronological within the selected window (oldest to newest)

### Free-text search over all data

```bash
mmr search "duckdb lock file"
mmr --source claude search "regression in parser"
mmr --source codex search "timeout" --project "/Users/mish/projects/mmr" --limit 20 --page 0
```

How it works:
- uses DuckDB FTS/BM25 when available
- falls back to `LIKE` search if FTS is unavailable

Useful scenarios:
- recover prior decisions after context compaction
- find where a specific error string or filename was discussed
- locate sessions that mention a migration, incident, or TODO wording
- identify relevant sessions before deeper `messages` retrieval

Default ordering:
- FTS path: relevance (`score DESC`)
- fallback path: recency (`timestamp DESC`)

### Force full ingestion of messages

```bash
mmr ingest
# alias
mmr refresh
```

This does a full cache rebuild. Use it when you want an explicit rebuild instead of incremental behavior.

## Flags

### Trigger refresh with `--refresh` / `-r`

```bash
mmr --refresh projects
mmr -r --source claude search "rate limit"
```

Behavior:
- query commands (`projects`, `sessions`, `messages`, `search`, `stats`) run synchronous incremental refresh first
- without `--refresh`, query commands return current cache immediately, then best-effort background refresh runs

Important distinction:
- `mmr refresh` is a subcommand alias of full rebuild (`ingest`)
- `--refresh` is a flag for synchronous pre-query incremental refresh

### Pick source (`codex` vs `claude`)

```bash
mmr --source codex projects
mmr --source claude projects
```

Notes:
- default is `codex`
- `--source all` behaves as no filter for `projects`, `search`, `stats`
- for `sessions`, use explicit `claude` or `codex`

### Limit result size

```bash
mmr projects --limit 10 --offset 20
mmr sessions --project "/Users/mish/projects/mmr" --limit 5
mmr messages --session "SESSION_ID" --limit 50
mmr search "auth" --limit 25 --page 2
```

Defaults:
- `projects` / `sessions` / `messages`: no default limit (returns all matching rows unless `--limit` is set)
- `search`: default `--limit 50`, `--page 0`

### Pretty JSON output

```bash
mmr --pretty projects
```

Notes:
- `--pretty` prints indented JSON
- compact JSON is the default when `--pretty` is omitted

## Concepts

### Cache invalidation model (SWR)

CLI query commands use stale-while-revalidate semantics:
1. return current on-disk cache results immediately
2. spawn best-effort detached incremental refresh (unless `--refresh` is supplied)
3. refresh worker updates a temporary DB and atomically swaps it in
4. lock + cooldown files reduce refresh stampedes

Incremental refresh is diff-based:
- file state tracks `last_offset`, size/mtime, and last message watermark
- unchanged files are skipped
- appended files ingest only new bytes
- rewritten/truncated files are reprocessed
- deleted source files are removed from cache state and their messages are dropped

Cold-start caveat:
- first read can be stale or empty until the background refresh completes

### What is a project key

Claude:
- project key is encoded directory name under `~/.claude/projects/` (for example `-Users-mish-projects-mmr`)
- `original_path` contains the actual filesystem path

Codex:
- project key is absolute `cwd` path from session metadata (for example `/Users/mish/projects/mmr`)
- CLI lookup normalizes missing leading slash for codex project arguments

### What is a session

A session is a conversation timeline keyed in derived tables by:
- `session_id`
- `project`
- `source`

Session rows are derived from non-subagent messages. `messages --session` uses session id lookup and can repair derived rows when needed.

### Default retrieval ordering

- `projects`: `last_activity DESC` (newest activity first)
- `sessions`: `last_timestamp DESC` (newest sessions first)
- `messages`: selected via `id DESC` for limit/offset, then reversed before output (chronological output)
- `search`: relevance-first under FTS (`score DESC`), recency-first under fallback (`timestamp DESC`)
