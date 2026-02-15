# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A Rust app that reads conversation history from both **Claude Code** (`~/.claude/projects/`) and **OpenAI Codex** (`~/.codex/sessions/`) JSONL files, ingests them into an in-memory DuckDB database, and provides two interfaces: a **CLI tool** (for agents/scripts) and a **web server** with JSON API + SPA (for interactive browsing). Includes an OpenAPI 3.1.0 spec at `/openapi.json`.

See also `AGENTS.md` for notes on what this repo is *not* (it is unrelated to `get_memory` or agent runtime tools).

## Build & Run

```bash
cargo check              # Fast compile check (~0.1s incremental, use during development)
cargo build --release    # First build is slow (~5min) due to bundled DuckDB
cargo run --release      # Starts web server at http://0.0.0.0:3131
cargo test               # Run tests
```

### CLI Usage

With no subcommand (or `serve`), starts the web server.

With a subcommand, runs as a CLI tool outputting JSON to stdout. CLI query subcommands (`projects`, `sessions`, `messages`, `search`, `stats`) auto-refresh an on-disk DuckDB cache incrementally before returning output. `mmr ingest` (alias: `mmr refresh`) is still available as an explicit full cache rebuild.

```
mmr [OPTIONS] <COMMAND>

Commands:
  ingest     (Re)ingest conversation history and rebuild the CLI cache
  projects   List all projects
  sessions   List sessions for a project
  messages   Get messages for a session
  search     Search across all conversations
  stats      Show usage statistics
  serve      Start the web server (default)

Global Options:
  --pretty           Pretty-print JSON output
  --source <SOURCE>  Filter by source: claude, codex
  --quiet            Suppress ingest progress (stderr)
```

Examples:
```bash
mmr ingest
mmr projects --pretty
mmr sessions --project <NAME> --source claude --pretty
mmr messages --session <ID> --limit 3 --pretty
mmr search "some query" --pretty
mmr search "test" --source claude
mmr stats --pretty
mmr projects --quiet 2>/dev/null | jq .   # Clean JSON, no stderr
```

## Architecture

Single-file app (`src/main.rs`, ~1860 lines including tests).

**Startup sequence**: `main()` parses CLI args via clap.

- Server mode (no subcommand or `serve`): creates an in-memory DuckDB, ingests data, builds FTS, and starts the Axum web server.
- CLI mode (any other subcommand):
  - `ingest`: explicit full cache rebuild into on-disk DuckDB.
  - query subcommands (`projects`, `sessions`, `messages`, `search`, `stats`): open cache DB, run incremental diff refresh, then execute `cmd_*`, printing JSON to stdout.

The web server continues to use in-memory storage; persistence is only used for the CLI cache.

### Logical sections in order:

1. **JSONL Parsing Types** (lines ~15-41): `ClaudeJsonlLine`, `ClaudeMessagePayload` — serde structs for Claude's JSONL format. Codex parsing uses `serde_json::Value` directly.

2. **DB Setup & Ingestion**: `init_db()` loads DuckDB FTS and creates tables (`messages`, `projects`, `sessions`, `cache_meta`) plus incremental state tables (`ingest_files`, `ingest_projects`, `ingest_sessions`). `messages` includes `source_file` + `source_offset` for line-level provenance. Server mode still uses full ingest (`ingest_claude()` + `ingest_codex()` + `ingest_all()`). CLI mode uses `refresh_incremental_cache()` to parse only new file bytes and repair rewritten/deleted files.
 
    **Claude Code project path encoding**: Claude Code encodes project directory paths via `path.replace(/[^a-zA-Z0-9]/g, "-")` — every non-alphanumeric character (`/`, `.`, `-`, `_`, space) becomes `-`. This is a **lossy, irreversible** encoding (e.g. `/foo/bar-baz` and `/foo/bar/baz` both encode to `-foo-bar-baz`). Instead of attempting to decode the dir name, `extract_project_path_from_sessions()` reads the `cwd` field from JSONL session data to recover the true path. `decode_project_name()` is a no-op identity fallback used only when no session data contains a `cwd`.

3. **Query Param & API Response Types** (lines ~594-727): `Deserialize` + `IntoParams` structs for query params (`ProjectQuery`, `MessageQuery`, `SearchParams`). `Serialize` + `ToSchema` structs for all JSON responses (`ApiProject`, `ApiSession`, `ApiMessage`, `ApiSearchResult`, `ApiAnalyticsResponse`, etc.).

4. **SPA Handler** (line ~731): Single `spa_handler()` returning the `SPA_HTML` const. Served as fallback for all non-API routes.

5. **Search Logic** (lines ~735-871): `run_search()` tries FTS first (`fts_main_messages.match_bm25()`), falls back to `LIKE` search. Dynamic WHERE clause construction with 1-3 bind params depending on filters.

6. **JSON API Handlers** (lines ~873-1196): Five handlers annotated with `#[utoipa::path(...)]`, each returning `Json<T>`:
   - `GET /api/projects` — project listing (all sources; SPA does client-side source filtering)
   - `GET /api/sessions?name=&source=` — sessions for a project (includes first-message preview)
   - `GET /api/messages?session=` — full conversation (project & source derived from session_id)
   - `GET /api/search?q=&project=&source=&page=` — paginated FTS search
   - `GET /api/analytics` — source/model/project aggregate stats

7. **SPA Frontend** (lines ~1198-1591): `SPA_HTML` const — full HTML document with embedded CSS and JavaScript. Client-side routing via `history.pushState()`, fetches from `/api/*` endpoints. Source tab filtering on the index page is done client-side.

8. **CLI Definition & Commands**: Clap `Parser`/`Subcommand` structs (`Cli`, `Commands`). `mmr ingest` performs explicit full cache rebuild. Query subcommands always call incremental refresh first, then execute the `cmd_*` query function.

9. **OpenAPI & Router Wiring**: `ApiDoc` struct with `#[derive(OpenApi)]`. Routes registered via `OpenApiRouter` which auto-collects specs. Spec served at `GET /openapi.json`. Non-API routes fall back to `spa_handler`.

10. **Tests**: `#[cfg(test)] mod tests` with integration tests using in-memory DuckDB and `tower::ServiceExt::oneshot`. Tests cover all API endpoints and the OpenAPI spec.

## Key Technical Details

- DuckDB in-memory with bundled build (`duckdb` crate `features = ["bundled"]`)
- CLI cache: on-disk DuckDB (default under OS cache dir; override with `MMR_DB_PATH` (legacy: `MEMORY_DB_PATH`)), refreshed incrementally on every query command.
- FTS: `PRAGMA create_fts_index` with `fts_main_messages.match_bm25()` for ranked search
- CLI parsing via `clap` v4 with derive macros; no subcommand defaults to web server
- Shared state is `Arc<Mutex<Connection>>` (aliased as `AppState`) for web server mode
- OpenAPI via `utoipa` v5 + `utoipa-axum` v0.2 (OpenAPI 3.1.0 native). All API types derive `ToSchema`, query params derive `IntoParams`, handlers use `#[utoipa::path(...)]`.
- Use `str::floor_char_boundary()` / `str::ceil_char_boundary()` when byte-slicing strings to avoid panics on multi-byte chars
- Rust edition 2021
- Source filtering uses dynamic WHERE clause construction with variable bind params (1-3 binds depending on filters)
- The `duckdb` crate's `params![]` macro doesn't support dynamic-length slices — the code uses match arms for 1/2/3 bind values
