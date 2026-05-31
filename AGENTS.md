# Repository Guidelines

## Goal-first workflow (required)

Every interaction with this codebase must start by capturing the user's request
as a goal-driven prompt document under `goals/`. Before writing or changing code,
create `goals/<YYYY-MM-DD>-<kebab-title>.md` with YAML frontmatter (`title`,
`description`, `date`, `status`) and a body that states the outcome, the surface
touched, the validation plan, and the definition of done. Drive the work from
that document and update its `status` as it progresses (`in-progress` → `done`,
or `blocked` with the smallest missing fact). Existing goals in `goals/` are the
template; `goals/2026-05-29-reverse-session-selection.md` is a worked example.

When a goal reaches done and confidence is high, commit and push the completed
work before ending the turn, provided the full relevant verification loop has
passed: tests, linter, build, benchmarks, docs checks, and any other QA required
by the goal. Do not commit or push if verification is incomplete, confidence is
low, the user asked not to, or unresolved collaborator/user changes make the
commit unsafe.

## Project Structure & Module Organization

`mmr` is a Rust CLI focused on local Claude/Codex/Cursor/Pi history parsing.

- `src/main.rs`: binary entrypoint, CLI parse + stderr error reporting.
- `src/cli.rs`: clap command surface and command routing.
- `src/types/`: public API response types and sort/source enums.
- `src/source/`: source-specific JSONL loaders (`codex.rs`, `claude.rs`, `cursor.rs`, `grok.rs`, `pi.rs`), parallel ingest wiring in `mod.rs`.
- `src/teleport/`: provider-profile native bundle pack/apply/resume/export (`provider.rs`, `providers/{codex,claude,cursor,grok,pi}.rs`); artifact paths are provider-qualified (`native/<provider>/…`); legacy flat `transcript.native.jsonl` bundles still verify on read.
- `src/query.rs`: in-memory aggregation, filtering, sorting, pagination, and contract semantics.
- `src/agent/gemini.rs`: Gemini Interactions API client (model, API key resolution, HTTP transport).
- `specs/`: canonical product and behavior specifications.
- `adrs/`: architecture decision records.
- `docs/tech-debt/`: tech-debt findings from codebase reviews — `tracked/` for open items, `handled/` for completed/dismissed (guidelines in `docs/tech-debt/AGENTS.md`).
- `tests/cli_contract.rs`: integration tests for user-facing CLI behavior (includes mock provider tests for summarization).
- `tests/cli_benchmark.rs`: ignored benchmark test (run explicitly).
- `tests/common/mod.rs`: fixture + temp `HOME` helpers.
- `src/agent/ai.rs`: Memory Agent orchestration — system prompt construction, session selection, transcript formatting, and stateless summarization helpers.
- `.cursor/rules/`: persistent repo rules for workflow, contract, ingestion, and tests.
- `.agents/skills/mmr-clap-colored-cli/`: local reusable CLI workflow references.
- `.agents/skills/mmr-teleport-providers/`: provider-profile native teleport layouts and verification notes.
- `.agents/skills/mmr/`: parent skill for the local `mmr` history tool. Use for general mmr questions or when unsure which mmr capability applies.
- `.agents/skills/mmr/session-mining/`: (subskill) retrieve previous sessions via `mmr recall` and `mmr read session`, analyze them, and produce continuity context. Critical for surviving context compaction and clearing. Use when you need to remind an agent (or yourself) of prior work.

## Cursor Rules

Treat `.cursor/rules/` as required guidance before editing code in this repo.

- `verification-loop.mdc`: mandatory verification sequence before claiming completion.
- `cli-contract.mdc`: CLI contract constraints for source semantics and response behavior.
- `ingest-parsing.mdc`: ingestion/parsing constraints for `src/source/**/*.rs`.
- `test-discipline.mdc`: fixture and benchmark expectations for `tests/**/*.rs`.

## Build, Test, and Development Commands

- `cargo run -- init` — set up or repair the local mmr store for the current project and import available source history.
- `cargo run -- list projects` — list all projects across all sources.
- `cargo run -- --source codex list projects` — list projects from Codex only.
- `cargo run -- list sessions` — list sessions for the auto-discovered cwd project by default; if discovery fails, fall back to all projects/sources.
- `cargo run -- list sessions --all` — list sessions across all projects and sources.
- `cargo run -- list sessions --project /Users/test/codex-proj` — sessions for a project across all sources.
- `cargo run -- --source codex list sessions --project /Users/test/codex-proj` — sessions for a specific source and project.
- `cargo run -- recall` — read the previous stable session for the cwd project across all sources.
- `cargo run -- recall 2` — read the session two stable sessions back.
- `cargo run -- read session sess-123` — read a specific session.
- `cargo run -- read project` — read chronological project history for the current directory across all sources.
- `cargo run -- read project --project /path/to/proj` — read chronological history for an explicit project.
- `cargo run -- --source claude read project --project /path/to/proj` — read project history filtered to one source.
- `cargo run -- --source codex read source` — read Codex history across all projects.
- `cargo run -- read project --format tree --output-dir /tmp/mmr-tree` — materialize a tree of event files and print the manifest JSON.
- `cargo run -- context project` — produce project-specific context across all sources.
- `cargo run -- --source codex context source` — produce harness-specific context across all projects for Codex.
- `cargo run -- summarize project --project /path/to/proj` — generate a stateless summary over project history.
- `cargo run -- summarize session <session-id>` — generate a stateless summary over one session.
- `cargo run -- --source codex summarize source` — generate a stateless summary over one source across projects.
- `cargo run -- assimilate project` — return the project memory deduplication/generalization prompt, runbook, output contract, and evidence bundle.
- `cargo run -- --source codex assimilate source` — return the harness-wide assimilation prompt, runbook, output contract, and evidence bundle.
- `cargo run -- skill load` — print the bundled mmr agent skill to stdout for immediate agent context.
- `cargo run -- skill install` — replace `~/.agents/skills/mmr` with the bundled mmr skill.
- `cargo run -- skill install --local` — replace `.agents/skills/mmr` under the current project with the bundled mmr skill.
- `cargo fmt` — format Rust code.
- `cargo test` — unit + integration tests.
- `cargo test --test cli_benchmark -- --ignored --nocapture` — run benchmark contract explicitly.
- `cargo clippy --all-targets --all-features -- -D warnings` — strict lint gate.
- `cargo build --release` — optimized production build check.

## Read and project detection

- `mmr read project` uses the current working directory to infer the project: Codex, Grok, and Pi match on the **canonical path** (e.g. `/Users/mish/proj`); Claude and Cursor match on the same path with **slashes replaced by hyphens** and a leading hyphen (e.g. `-Users-mish-proj`).
- `mmr read project --project <path>` reads chronological history for the given project across all sources unless `--source` is set.
- `mmr list sessions` and `mmr read project` use the same cwd canonical path as their default project scope unless `--project` is provided, `--all` is set where supported, or `MMR_AUTO_DISCOVER_PROJECT=0`.
- Scripts that need only the message array can pipe through `jq '.messages'`; event-oriented commands may expose `events` instead.

## CLI default env vars

- `MMR_AUTO_DISCOVER_PROJECT=0` disables cwd project auto-discovery for project-scoped list/read/recall commands; unset or `1` keeps the default auto-discovery behavior.
- `MMR_DEFAULT_SOURCE=codex|claude|cursor|grok|pi` sets the default source filter when `--source` is omitted. Empty or unset preserves the default of all sources.
- `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` sets the default summarization backend where the legacy memory-agent runner is still used. When unset, the default backend is Cursor (`composer-2-fast` unless `--model` is set).

## Summarize command and `--instructions` system prompt architecture

The `summarize` command sends selected transcripts to the backend selected with
`--agent` (`cursor`, `codex`, or `gemini`; default `cursor` with
`composer-2-fast` when `--model` is omitted). For each backend, the memory flow
uses a system prompt composed of two parts:

1. **Base instruction** (`MEMORY_AGENT_BASE_INSTRUCTION` in `src/agent/ai.rs`): Always present. Contains only the agent's identity ("You are a Memory Agent") and the input format description. Must **never** contain output-directing language (e.g. "continuity brief", "sole purpose", output quality directives).

2. **Output instruction** (appended after the base):
   - **Without `--instructions`**: `MEMORY_AGENT_DEFAULT_OUTPUT_INSTRUCTION` is used — includes `## Purpose`, `## Output Format`, `## Rules`, and `### Resume Instructions` sections.
   - **With `--instructions <text>`**: The custom text **replaces** the entire default output instruction. The base (identity + input format) is preserved, but all output-directing sections are replaced by the user's text.

This separation ensures `--instructions` has full control over how the agent processes transcripts and formats its response, while preserving the agent's awareness of its role and input structure.

The user prompt is neutral ("Analyze the following AI coding session transcript(s).") and does not prescribe an output format, so the system instruction has sole authority over output behavior.

Environment: **Gemini** — `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL` (integration tests use a mock server). **Codex** — Codex CLI auth as configured for `codex exec`. **Cursor** — `CURSOR_API_KEY` and the `agent` CLI on `PATH`.

## Coding Style & Naming Conventions

- Follow rustfmt defaults (4-space indentation, standard Rust style).
- Keep imports at file top; avoid inline imports.
- Use descriptive, domain-specific names (`ApiProjectsResponse`, `SourceFilter`, `load_codex_messages`).
- For sort comparators, include full deterministic tie-breakers so ordering is stable even when primary/secondary keys match.
- Keep stdout machine-readable JSON; reserve colored output for human-facing stderr messages.

## Testing Guidelines

- Prefer fixture-driven integration tests using temp `HOME`; do not depend on real local history.
- For cwd-dependent behavior, use `TestFixture::run_cli_in_dir` and a fixture project path under `HOME`.
- In tests that exec the mmr binary, use `env!("CARGO_BIN_EXE_mmr")`, not `env::var("CARGO_BIN_EXE_mmr")`, so benchmarks run correctly with `--ignored`.
- Validate behavior contracts: schema fields, source filtering, sort order, pagination semantics, and message chronology.
- Keep benchmark tests opt-in with `#[ignore]`.
- In assertions, prefer `!slice.is_empty()` over `slice.len() >= 1` (satisfies Clippy `len_zero`).

## Commit & Pull Request Guidelines

- No project commit history exists yet; use imperative, concise commit messages (e.g., `add cli source filtering tests`).
- In PRs, include: scope summary, contract changes, commands run, and relevant test/lint/build outputs.
- Avoid mixing refactors with behavior changes unless the PR clearly separates them.

## Learned User Preferences

- Expect `mmr teleport read` to print session messages on stdout (JSON `messages` array; `-O md` for readable text); caching must not require a separate export step.
- Prefer stateless one-shot CLI flows for `summarize` (no continuation or follow-up parameters).
- For GitHub-backed mmr features, use standard `GITHUB_TOKEN` or `GH_TOKEN` plus gh user config rather than `MMR_`-prefixed credential env vars.
- `mmr assimilate` should return a prompt, runbook, output contract, and evidence bundle; it must not launch an agent as a side effect.

## Learned Workspace Facts

- On macOS/BSD, TCP sockets accepted from a non-blocking `teleport serve` listener inherit non-blocking mode; set accepted streams to blocking before large bundle writes to avoid EAGAIN.
- `mmr teleport read` response includes a `messages` array with the same shape as `mmr read session`; re-reading a cached bundle returns the same messages with `status: "skipped"`.
- When invoking the mmr CLI from scripts (e.g. Python subprocess), pass `--project` and the project value as two separate arguments so the CLI receives the value correctly; avoid a single argument like `--project="value"` which can pass the quotes literally and break matching.
