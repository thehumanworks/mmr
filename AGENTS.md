# Repository Guidelines

## Project Structure & Module Organization

`mmr` is a Rust CLI focused on local Claude/Codex/Cursor history parsing.

- `src/main.rs`: binary entrypoint, CLI parse + stderr error reporting.
- `src/cli.rs`: clap command surface and command routing.
- `src/types/`: public API response types and sort/source enums.
- `src/source/`: source-specific JSONL loaders (`codex.rs`, `claude.rs`, `cursor.rs`), parallel ingest wiring in `mod.rs`.
- `src/messages/service.rs`: in-memory aggregation, filtering, sorting, pagination, and response contract semantics.
- `src/types/query.rs`: shared query options such as sort fields and sort order.
- `src/agent/ai.rs`: Memory Agent orchestration — system prompt construction, session selection, transcript formatting, and the `remember()` entry point.
- `src/agent/gemini_api.rs`: Gemini Interactions API client (model, API key resolution, HTTP transport).
- `adrs/`: architecture decision records.
- `docs/tech-debt/`: tech-debt findings from codebase reviews — `tracked/` for open items, `handled/` for completed/dismissed (guidelines in `docs/tech-debt/AGENTS.md`).
- `tests/cli_contract.rs`: integration tests for user-facing CLI behavior (includes mock Gemini server tests for `remember`).
- `tests/cli_benchmark.rs`: ignored benchmark test (run explicitly).
- `tests/common/mod.rs`: fixture + temp `HOME` helpers.
- `.cursor/rules/`: persistent repo rules for workflow, contract, ingestion, and tests.
- `.agents/skills/mmr-clap-colored-cli/`: local reusable workflow references.

## Cursor Rules

Treat `.cursor/rules/` as required guidance before editing code in this repo.

- `verification-loop.mdc`: mandatory verification sequence before claiming completion.
- `cli-contract.mdc`: CLI contract constraints for source semantics and response behavior.
- `ingest-parsing.mdc`: ingestion/parsing constraints for `src/source/**/*.rs`.
- `test-discipline.mdc`: fixture and benchmark expectations for `tests/**/*.rs`.

## Build, Test, and Development Commands

- `cargo run -- projects` — list all projects across all sources.
- `cargo run -- --source codex projects` — list projects from codex only.
- `cargo run -- --source cursor projects` — list projects from cursor only.
- `cargo run -- sessions` — list sessions for the auto-discovered cwd project by default; if discovery fails, fall back to all projects/sources.
- `cargo run -- sessions --all` — list sessions across all projects and sources.
- `cargo run -- sessions --project /Users/test/codex-proj` — sessions for a project (searches all supported sources unless `--source` narrows it).
- `cargo run -- --source codex sessions --project /Users/test/codex-proj` — sessions for a specific source and project.
- `cargo run -- messages` — list messages for the auto-discovered cwd project by default; if discovery fails, fall back to all projects/sources.
- `cargo run -- messages --all` — list messages across all projects and sessions.
- `cargo run -- messages --session sess-123` — messages for a specific session across all projects when `--project` is omitted; prints a stderr hint suggesting `--source` when the lookup spans every source.
- `cargo run -- --source claude messages --session sess-123` — the same session lookup narrowed to one source, suppressing the hint.
- `cargo run -- messages --project /Users/test/codex-proj --limit 25 --offset 25` — page through one project's messages; use the response `next_command` to continue with the same filters.
- `cargo run -- --source claude messages --project my-proj` — messages filtered by source and project.
- `cargo run -- export` — all messages for the current directory (cwd) as project, across Claude, Codex, and Cursor, in chronological JSON.
- `cargo run -- export --project /path/to/proj` — all messages for the given project.
- `cargo run -- remember --project /path/to/proj` — generate a continuity brief from the latest session; stdout defaults to markdown.
- `cargo run -- remember all --project /path/to/proj` — generate a continuity brief from all sessions.
- `cargo run -- remember session <session-id> --project /path/to/proj` — generate a continuity brief from one specific session.
- `cargo run -- remember --instructions "Return only a keyword."` — override the default output format and rules.
- `cargo run -- remember -O json --project /path/to/proj` — return the remember response as JSON instead of the default markdown brief.
- `cargo fmt` — format Rust code.
- `cargo test` — unit + integration tests.
- `cargo test --test cli_benchmark -- --ignored --nocapture` — run benchmark contract explicitly.
- `cargo clippy --all-targets --all-features -- -D warnings` — strict lint gate.
- `cargo build --release` — optimized production build check.

## Export and project detection

- `mmr export` uses the current working directory to infer the project: Codex matches on the **canonical path** (e.g. `/Users/mish/proj`); Claude and Cursor match on the same path with **slashes replaced by hyphens** and a leading hyphen (e.g. `-Users-mish-proj`). The CLI calls `QueryService::messages` once per source when using cwd, then merges and sorts by timestamp (asc).
- `mmr export --project <path>` passes the project to a single `messages` call (all sources unless `--source` is set). Reuses existing `ApiMessagesResponse`; no new response type.
- `mmr sessions` and `mmr messages` now use the same cwd canonical path as their default project scope unless `--project` is provided, `--all` is set, or `MMR_AUTO_DISCOVER_PROJECT=0`.
- `mmr messages --session <id>` is the exception to cwd scoping: without `--project`, it skips auto-discovery and searches all projects because the session ID is already the primary lookup key. If `--source` is also omitted, the CLI prints a stderr hint suggesting `--source` to narrow the search.
- `ApiMessagesResponse` includes `next_page`, `next_offset`, and `next_command`. When another page exists, `next_command` preserves the active filters (`--source`, `--project`, `--all`, `--sort-by`, `--order`, `--limit`) so callers can continue pagination without rebuilding the command manually.
- Scripts that need only the message array can pipe through `jq '.messages'`.

## CLI default env vars

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery for `sessions` and `messages`; unset or `1` keeps the default auto-discovery behavior.
- `MMR_DEFAULT_SOURCE=codex|claude|cursor` sets the default source filter when `--source` is omitted. Empty or unset preserves the default of all sources.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` sets the default `remember --agent` value when `--agent` is omitted. When unset, the default backend is Cursor (`composer-2-fast` unless `--model` is set).

## Remember command and `--instructions` system prompt architecture

The `remember` command sends session transcripts to the backend selected with `--agent` (`cursor`, `codex`, or `gemini`; default `cursor` with `composer-2-fast` when `--model` is omitted). For each backend, the memory flow uses a system prompt composed of two parts:

1. **Base instruction** (`MEMORY_AGENT_BASE_INSTRUCTION` in `src/agent/ai.rs`): Always present. Contains only the agent's identity ("You are a Memory Agent") and the input format description. Must **never** contain output-directing language (e.g. "continuity brief", "sole purpose", output quality directives).

2. **Output instruction** (appended after the base):
   - **Without `--instructions`**: `MEMORY_AGENT_DEFAULT_OUTPUT_INSTRUCTION` is used — includes `## Purpose`, `## Output Format`, `## Rules`, and `### Resume Instructions` sections.
   - **With `--instructions <text>`**: The custom text **replaces** the entire default output instruction. The base (identity + input format) is preserved, but all output-directing sections are replaced by the user's text.

This separation ensures `--instructions` has full control over how the agent processes transcripts and formats its response, while preserving the agent's awareness of its role and input structure.

The user prompt is neutral ("Analyze the following AI coding session transcript(s).") and does not prescribe an output format, so the system instruction has sole authority over output behavior.

Environment: **Gemini** — `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL` (integration tests use a mock server). **Codex** — Codex CLI auth as configured for `codex exec`. **Cursor** — `CURSOR_API_KEY` and the `agent` CLI on `PATH`.

Operational constraints:

- `remember` defaults to markdown output (`-O md`). Use `-O json` for machine-readable automation output.
- `--model` is forwarded to the Gemini and Cursor remember backends. The Codex remember backend currently uses the built-in `gpt-5.4-mini` model with medium reasoning effort and does not expose a runtime model override.

## Coding Style & Naming Conventions

- Follow rustfmt defaults (4-space indentation, standard Rust style).
- Keep imports at file top; avoid inline imports.
- Use descriptive, domain-specific names (`ApiProjectsResponse`, `SourceFilter`, `load_codex_messages`).
- For sort comparators, include full deterministic tie-breakers so ordering is stable even when primary/secondary keys match.
- Keep stdout machine-readable JSON; reserve colored output for human-facing stderr messages.

## Testing Guidelines

- Prefer fixture-driven integration tests using temp `HOME`; do not depend on real local history.
- For cwd-dependent behavior, use `TestFixture::run_cli_in_dir` and a fixture project path under `HOME` (see `export_without_project_uses_cwd` in `tests/cli_contract.rs`).
- In tests that exec the mmr binary, use `env!("CARGO_BIN_EXE_mmr")`, not `env::var("CARGO_BIN_EXE_mmr")`, so benchmarks run correctly with `--ignored`.
- Validate behavior contracts: schema fields, source filtering, sort order, pagination semantics, and message chronology.
- Keep benchmark tests opt-in with `#[ignore]`.
- In assertions, prefer `!slice.is_empty()` over `slice.len() >= 1` (satisfies Clippy `len_zero`).

## Commit & Pull Request Guidelines

- No project commit history exists yet; use imperative, concise commit messages (e.g., `add cli source filtering tests`).
- In PRs, include: scope summary, contract changes, commands run, and relevant test/lint/build outputs.
- Avoid mixing refactors with behavior changes unless the PR clearly separates them.

## Learned User Preferences

## Learned Workspace Facts

- When invoking the mmr CLI from scripts (e.g. Python subprocess), pass `--project` and the project value as two separate arguments so the CLI receives the value correctly; avoid a single argument like `--project="value"` which can pass the quotes literally and break matching.
