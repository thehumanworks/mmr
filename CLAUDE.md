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

With no subcommand (or `serve`), starts the web server. With a subcommand, runs as a CLI tool outputting JSON to stdout:

```
memory [OPTIONS] <COMMAND>

Commands:
  projects   List all projects
  sessions   List sessions for a project
  messages   Get messages for a session
  search     Search across all conversations
  stats      Show usage statistics
  serve      Start the web server (default)

Global Options:
  --pretty           Pretty-print JSON output
  --source <SOURCE>  Filter by source: claude, codex
  --quiet            Suppress ingestion progress (stderr)
```

Examples:
```bash
memory projects --pretty
memory sessions --project <NAME> --source claude --pretty
memory messages --session <ID> --limit 3 --pretty
memory search "some query" --pretty
memory search "test" --source claude
memory stats --pretty
memory projects --quiet 2>/dev/null | jq .   # Clean JSON, no stderr
```

## Architecture

Single-file app (`src/main.rs`, ~1860 lines including tests).

**Startup sequence**: `main()` parses CLI args via clap. If a CLI subcommand is given, it runs `run_cli()` which creates an in-memory DuckDB, ingests data, builds FTS, runs the `cmd_*` function, and prints JSON to stdout. Otherwise (no subcommand or `serve`), it starts the Axum web server. All data lives in memory — there is no persistent storage.

### Logical sections in order:

1. **JSONL Parsing Types** (lines ~15-41): `ClaudeJsonlLine`, `ClaudeMessagePayload` — serde structs for Claude's JSONL format. Codex parsing uses `serde_json::Value` directly.

2. **DB Setup & Ingestion** (lines ~43-590): `init_db()` creates three tables (`messages`, `projects`, `sessions`) all with a `source` column (`'claude'` or `'codex'`). `ingest_claude()` reads `~/.claude/projects/{project-dir}/{uuid}.jsonl` (dash-encoded paths, e.g. `-Users-mish-memory` → `/Users/mish/memory`). `ingest_codex()` walks `~/.codex/sessions/` recursively. `ingest_all()` orchestrates both and populates the `sessions` table via aggregate INSERT.

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

8. **CLI Definition & Commands**: Clap `Parser`/`Subcommand` structs (`Cli`, `Commands`). Five `cmd_*` functions that take `&Connection` + direct args and return `Result<T>` — these add source filtering and `--limit` support on top of the same DB queries. `run_cli()` orchestrates ingestion → FTS → command dispatch → JSON output.

9. **OpenAPI & Router Wiring**: `ApiDoc` struct with `#[derive(OpenApi)]`. Routes registered via `OpenApiRouter` which auto-collects specs. Spec served at `GET /openapi.json`. Non-API routes fall back to `spa_handler`.

10. **Tests**: `#[cfg(test)] mod tests` with integration tests using in-memory DuckDB and `tower::ServiceExt::oneshot`. Tests cover all API endpoints and the OpenAPI spec.

## Key Technical Details

- DuckDB in-memory with bundled build (`duckdb` crate `features = ["bundled"]`)
- FTS: `PRAGMA create_fts_index` with `fts_main_messages.match_bm25()` for ranked search
- CLI parsing via `clap` v4 with derive macros; no subcommand defaults to web server
- Shared state is `Arc<Mutex<Connection>>` (aliased as `AppState`) for web server mode
- OpenAPI via `utoipa` v5 + `utoipa-axum` v0.2 (OpenAPI 3.1.0 native). All API types derive `ToSchema`, query params derive `IntoParams`, handlers use `#[utoipa::path(...)]`.
- Use `str::floor_char_boundary()` / `str::ceil_char_boundary()` when byte-slicing strings to avoid panics on multi-byte chars
- Rust edition 2021
- Source filtering uses dynamic WHERE clause construction with variable bind params (1-3 binds depending on filters)
- The `duckdb` crate's `params![]` macro doesn't support dynamic-length slices — the code uses match arms for 1/2/3 bind values
