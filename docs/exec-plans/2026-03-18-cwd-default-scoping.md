# Add CWD-Scoped Defaults and Env-Driven CLI Overrides

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

This plan was authored in accordance with `/Users/mish/.agents/skills/exec-plan/references/PLANS.md`. The repository does not currently contain a checked-in repo-local `PLANS.md`, so this file must remain self-contained.

## Purpose / Big Picture

After this change, `mmr sessions` and `mmr messages` will prefer the current working directory as the default project scope instead of scanning every project. That makes the common case match the directory the user is standing in, while still providing an escape hatch through `--all`, through an opt-out environment variable, and through graceful fallback to the existing global behavior when automatic project discovery fails. The visible proof is that running `cargo run -- sessions` or `cargo run -- messages` from a seeded project directory returns only that project by default, that `cargo run -- sessions --all` and `cargo run -- messages --all` return cross-project results, and that `cargo run -- remember ...` picks up the configured default agent and source when the new environment variables are present.

The acceptance bar is behavioral, not internal. A user must be able to stand in a project directory and observe project-scoped results by default, disable that behavior explicitly or via environment, and still receive an empty JSON list rather than a silent fallback when the discovered project exists but has no messages.

## Progress

- [x] (2026-03-18 10:20Z) Reviewed the current CLI surface in `src/cli.rs`, the query/filter implementation in `src/messages/service.rs`, the fixture helpers in `tests/common/mod.rs`, and the contract tests in `tests/cli_contract.rs`.
- [x] (2026-03-18 10:28Z) Reviewed the planning contract in `/Users/mish/.agents/skills/exec-plan/references/PLANS.md` and the repository verification rules in `.cursor/rules/verification-loop.mdc` and `.cursor/rules/test-discipline.mdc`.
- [x] (2026-03-18 10:34Z) Drafted this ExecPlan with the required user-visible behaviors, touched files, validation loop, and implementation sequencing.
- [x] (2026-03-18 10:47Z) Implemented CLI default-resolution helpers in `src/cli.rs` for cwd project scoping, `--all`, `MMR_DEFAULT_SOURCE`, and `MMR_DEFAULT_REMEMBER_AGENT`, while keeping the existing response schemas intact.
- [x] (2026-03-18 10:51Z) Updated durable repository guidance in `AGENTS.md`, `.cursor/rules/cli-contract.mdc`, and a new ADR `adrs/002-cwd-scoped-defaults.md` so the new defaults do not leave stale documentation behind.
- [x] (2026-03-18 11:14Z) Added and updated contract coverage in `tests/cli_contract.rs` and `tests/common/mod.rs` for cwd-scoped defaults, `--all`, `MMR_AUTO_DISCOVER_PROJECT`, `MMR_DEFAULT_SOURCE`, `MMR_DEFAULT_REMEMBER_AGENT`, and the new empty-result behavior for discovered projects with no messages.
- [x] (2026-03-18 11:22Z) Added unit coverage in `src/cli.rs` for source and agent env parsing, project-scope selection precedence, and missing-path discovery failure returning `None`.
- [x] (2026-03-18 11:29Z) Ran the full required verification loop successfully: `cargo fmt`, `cargo test`, `cargo test --test cli_benchmark -- --ignored --nocapture`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo build --release`.

## Surprises & Discoveries

- Observation: The current CLI already has one cwd-aware code path, but it lives only in `export` and `remember`, not in `sessions` or `messages`.
  Evidence: `src/cli.rs` resolves cwd only in the `Export` and `Remember` branches, while `Sessions` and `Messages` pass `project.as_deref()` directly into `QueryService`.

- Observation: The query layer already supports the user-requested empty-result behavior without extra branching.
  Evidence: `QueryService::sessions` and `QueryService::messages` filter against the resolved project name and return an empty vector if no message records match the chosen project.

- Observation: `SourceFilter` and `Agent` are already `clap::ValueEnum`, so the safest env-driven defaults are small parsing helpers that return `Option<SourceFilter>` and `Option<Agent>` instead of duplicating clap parsing rules.
  Evidence: `src/types/domain.rs` defines both enums with stable serialized spellings.

- Observation: `RememberArgs.agent` needed to stop using a clap `default_value` so the runtime could distinguish “flag omitted” from “flag provided” and apply `MMR_DEFAULT_REMEMBER_AGENT` with correct precedence.
  Evidence: clap materializes the default before command dispatch, which would otherwise hide whether `--agent` was absent.

- Observation: An integration test that tries to force cwd discovery failure by mutating the parent test process cwd is parallel-test-hostile, while an “unrecognized but existing cwd” is not a discovery failure at all.
  Evidence: a valid but unknown cwd correctly returns an empty project-scoped result under the requested behavior; true discovery failure is better covered by pure helper tests around missing-path canonicalization and scope selection.

## Decision Log

- Decision: Keep default-resolution logic in `src/cli.rs` rather than pushing cwd auto-discovery into `QueryService`.
  Rationale: The behavior change is a CLI policy change, not a query-engine capability change. Keeping it at the boundary preserves the existing service API and avoids accidental changes to programmatic callers.
  Date/Author: 2026-03-18 / Codex

- Decision: Treat `--all` as an override that disables only the new default project auto-discovery for `sessions` and `messages`, not as a synonym for clearing `--source`.
  Rationale: The request defines `--all` in terms of project/session scope. Source filtering remains independently controllable through `--source` and the new `MMR_DEFAULT_SOURCE` environment variable.
  Date/Author: 2026-03-18 / Codex

- Decision: Preserve the current fallback-to-global behavior only when auto-discovery fails, but not when discovery succeeds and yields zero matches.
  Rationale: The user explicitly requested two distinct branches: discovery failure falls back to global results, while a valid discovered project with no messages must return an empty list.
  Date/Author: 2026-03-18 / Codex

- Decision: Apply `MMR_DEFAULT_SOURCE` to the CLI-level default source only when `--source` is absent, and accept only `codex`, `claude`, or empty.
  Rationale: This mirrors existing flag precedence and keeps invalid env input non-fatal. A bad env var should degrade to the historical "both sources" behavior rather than break the CLI.
  Date/Author: 2026-03-18 / Codex

- Decision: Apply `MMR_DEFAULT_REMEMBER_AGENT` only to the `remember` subcommand default, leaving explicit `--agent` untouched.
  Rationale: The request scopes that environment variable to `mmr remember --agent`, and clap defaults alone cannot express env parsing with graceful invalid-value fallback in the exact requested way.
  Date/Author: 2026-03-18 / Codex

- Decision: Update `AGENTS.md`, `.cursor/rules/cli-contract.mdc`, and the ADR set in the same change.
  Rationale: The previous guidance explicitly documented unfiltered `sessions` and `messages` as global. Leaving those files unchanged would create an immediate documentation and future-agent contract mismatch even if the binary behavior was correct.
  Date/Author: 2026-03-18 / Codex

- Decision: Cover cwd discovery failure with unit-tested helper functions instead of an integration test that mutates the test runner’s global cwd.
  Rationale: The integration approach is fragile under parallel test execution and conflates “discovery failed” with “discovered project exists but has no history.” Helper tests give deterministic coverage for the missing-path failure branch while keeping the integration suite focused on observable command behavior from valid directories.
  Date/Author: 2026-03-18 / Codex

## Outcomes & Retrospective

Completed. `mmr sessions` and `mmr messages` now scope to the auto-discovered cwd project by default, with `--all` and `MMR_AUTO_DISCOVER_PROJECT=0` restoring the previous global behavior. `MMR_DEFAULT_SOURCE` now supplies the default source filter when `--source` is omitted, and `MMR_DEFAULT_REMEMBER_AGENT` now supplies the default `remember` agent when `--agent` is omitted. The repository guidance and ADR set were updated so the behavior is documented where future contributors will read it.

The final implementation kept the query engine unchanged and localized all policy changes to the CLI boundary. That made the change smaller, preserved the response schemas, and avoided turning a user-facing defaulting change into a deeper service refactor. The integration suite now proves the new defaults from valid project directories, and the unit suite proves the discovery-failure branch for missing paths without relying on flaky global-cwd mutation.

## Context and Orientation

`mmr` is a Rust command-line tool that loads Claude and Codex conversation history into in-memory message, session, and project aggregates. The command-line surface lives in `src/cli.rs`. The query and filtering engine lives in `src/messages/service.rs`. Public response shapes and enums live in `src/types/`. Integration tests that exercise the built CLI against a temporary `HOME` tree live in `tests/cli_contract.rs`, with fixture helpers in `tests/common/mod.rs`.

For this task, “auto-discovered project” means deriving a project identifier from the current working directory. This repository already does that for `export`: Codex matching uses the canonical cwd path, while Claude matching uses the same path encoded with slashes replaced by hyphens and a leading hyphen. `remember` already uses cwd as its default project path. “Scoped by default” means that `sessions` and `messages` must behave as if `--project <cwd-derived-project>` had been supplied when the user did not pass `--project`, when `--all` is absent, and when `MMR_AUTO_DISCOVER_PROJECT` does not disable the feature.

The main files involved are:

`src/cli.rs`, which defines clap arguments, default values, cwd resolution helpers, and the command dispatch.

`src/messages/service.rs`, which resolves optional project filters and returns `ApiSessionsResponse` and `ApiMessagesResponse`.

`src/types/domain.rs`, which defines `SourceFilter` and `Agent`, the two enums affected by new environment-driven defaults.

`tests/common/mod.rs`, which provides hermetic fixture helpers and cwd-aware CLI execution support.

`tests/cli_contract.rs`, which holds the behavioral contract tests for `sessions`, `messages`, `export`, and `remember`.

Any documentation updates should stay close to the command surface and repository usage guidance. The change should not alter response schemas on stdout; it changes only which records are selected by default and which defaults are used when flags are omitted.

## Milestones

### Milestone 1: Lock down the new default behavior with tests

At the end of this milestone, the test suite will describe every requested branch of the new policy: default cwd scoping, fallback to global when cwd discovery fails, empty results when cwd maps to a project with no history, `--all` bypass, `MMR_AUTO_DISCOVER_PROJECT=0` bypass, `MMR_AUTO_DISCOVER_PROJECT=1` explicit opt-in, `MMR_DEFAULT_SOURCE`, and `MMR_DEFAULT_REMEMBER_AGENT`. The proof is that the new tests fail against the pre-change implementation and pass after the code is updated.

### Milestone 2: Implement CLI-level default resolution

At the end of this milestone, `src/cli.rs` will compute an effective source, an effective remember agent, and an effective auto-discovered project for `sessions` and `messages` before calling `QueryService`. The proof is that no response schema changes are required and that the existing service methods continue to receive plain optional filters.

### Milestone 3: Document and verify the finished behavior

At the end of this milestone, the new defaults will be discoverable in command help or repository docs, the ExecPlan will record the final decisions and outcomes, and the full Rust verification loop will pass. The proof is a clean run of the repository’s required commands and short behavioral examples captured in this plan.

## Plan of Work

Begin in `tests/cli_contract.rs`. Add integration tests that exercise `sessions` and `messages` from a real directory under the temporary fixture `HOME` so the cwd path can be canonicalized and matched. Reuse `TestFixture::run_cli_in_dir` for cwd-aware cases and `run_cli_with_home_and_env` for environment-variable cases. Add one fixture directory whose path is known to the seeded history and a second directory that exists on disk but has no seeded history so the empty-result branch is exercised separately from the “cwd discovery failed” branch.

After the tests are in place, update `src/cli.rs`. Introduce small helper functions that read the environment variables, validate them, and resolve the effective defaults. The helper for `MMR_DEFAULT_SOURCE` should parse `codex`, `claude`, and empty string. The helper for `MMR_DEFAULT_REMEMBER_AGENT` should parse `codex`, `gemini`, and empty string. The helper for project auto-discovery should apply only to `sessions` and `messages`, should return `None` when the user provided `--project` or `--all`, should respect `MMR_AUTO_DISCOVER_PROJECT=0` by disabling discovery, should treat unset or `1` as enabled, and should use the existing cwd-to-Codex/Claude normalization rules already present in `resolve_project_from_cwd`.

Keep the command handlers simple. For `sessions`, compute the effective project and effective source, then call `service.sessions` exactly once with those resolved values. For `messages`, compute the effective session, project, and source, then call `service.messages` exactly once. The logic must not perform a second query to determine whether the discovered project has results; the requested empty-list behavior falls out naturally from querying the discovered project directly. Only when cwd resolution itself fails should the command fall back to the global unscoped behavior.

Extend the clap surface in `src/cli.rs` by adding `--all` to `Sessions` and `Messages`, with help text that makes the override semantics clear. Update the `RememberArgs` agent handling so the default can come from an environment helper instead of a fixed clap `default_value`. Keep explicit `--agent` and `--source` flags higher priority than the environment.

Finally, update the repository documentation that describes command behavior. At minimum, the command doc comments in `src/cli.rs` should explain the new defaults. If there is a suitable reference doc or ADR that already explains CLI behavior, extend it so the environment variables and fallback branches are written down for future contributors.

## Concrete Steps

From the repository root `/Users/mish/dev/mmr`, inspect the current CLI and test files before editing:

    sed -n '1,260p' src/cli.rs
    sed -n '1,220p' src/types/domain.rs
    sed -n '280,760p' tests/cli_contract.rs
    sed -n '1,160p' tests/common/mod.rs

Add the new contract tests first, then run the focused integration suite while iterating:

    cargo test --test cli_contract

Once the tests describe the new behavior, implement the CLI helpers and rerun:

    cargo fmt
    cargo test --test cli_contract

Run the mandatory verification loop before completion:

    cargo test
    cargo test --test cli_benchmark -- --ignored --nocapture
    cargo clippy --all-targets --all-features -- -D warnings
    cargo build --release

Expected observable examples after implementation:

    cargo run -- sessions
    # when run from a seeded project directory, returns only that project's sessions

    cargo run -- sessions --all
    # returns sessions across all projects, subject to source filters and pagination

    cargo run -- messages
    # when run from a seeded project directory, returns only that project's messages

    MMR_AUTO_DISCOVER_PROJECT=0 cargo run -- messages
    # returns cross-project results because auto-discovery is disabled

    MMR_DEFAULT_SOURCE=codex cargo run -- sessions --all
    # returns only codex sessions unless --source overrides it

    MMR_DEFAULT_REMEMBER_AGENT=gemini cargo run -- remember --project /Users/test/proj
    # uses gemini unless --agent overrides it

## Validation and Acceptance

Acceptance requires both targeted behavior checks and the full repository loop.

The integration tests must show:

`sessions` in a seeded cwd returns only that project by default.

`messages` in a seeded cwd returns only that project by default.

`sessions --all` and `messages --all` ignore cwd project auto-discovery and return cross-project results.

When cwd resolution fails, default behavior falls back to cross-project results instead of erroring.

When cwd resolution succeeds but the discovered project has no history, the command returns an empty JSON list with the matching total count of zero.

`MMR_AUTO_DISCOVER_PROJECT=0` disables cwd auto-discovery, while `MMR_AUTO_DISCOVER_PROJECT=1` keeps it enabled.

`MMR_DEFAULT_SOURCE` supplies the default source for commands that use `cli.source`, but explicit `--source` overrides it and empty or invalid env values preserve the historical “both sources” behavior.

`MMR_DEFAULT_REMEMBER_AGENT` supplies the default `remember` agent, but explicit `--agent` overrides it.

Full completion requires successful execution of:

    cargo fmt
    cargo test
    cargo test --test cli_benchmark -- --ignored --nocapture
    cargo clippy --all-targets --all-features -- -D warnings
    cargo build --release

## Idempotence and Recovery

The test fixtures are hermetic and can be rerun repeatedly because they create a fresh temporary `HOME` each time. The CLI code changes are safe to retry because they are additive policy helpers around existing service calls. If a helper introduces incorrect defaults, recovery is straightforward: rerun the targeted `cli_contract` tests, inspect the effective project/source/agent resolution, and adjust the boundary logic in `src/cli.rs` without touching stored history files.

The only risky branch is cwd canonicalization because it depends on a real directory. Keep that logic reusing the existing `resolve_project_from_cwd` helper so `export` and the new default scoping stay aligned. If a change would break `export`, back it out and preserve the shared canonicalization rules in one place.

## Artifacts and Notes

Important current behavior, to preserve or intentionally change:

`sessions` and `messages` currently treat `--project` as optional and global by default.

`export` already uses cwd-derived Codex and Claude project identifiers when `--project` is omitted.

`messages` has a historical pagination contract: when sorting by ascending timestamp, it pages from the newest window and then returns that window in chronological order.

`remember` already defaults the project from the current working directory, so this task should reuse that style of boundary-level defaulting rather than introducing a second project-resolution system elsewhere.

## Interfaces and Dependencies

Keep using the existing crates already present in `Cargo.toml`: `clap` for argument parsing, `serde` and `serde_json` for machine-readable output, and `anyhow` for error propagation. No new dependencies should be necessary.

At the end of the change, `src/cli.rs` should expose helpers with a shape similar to:

    fn effective_source(cli_source: Option<SourceFilter>) -> Option<SourceFilter>
    fn effective_remember_agent(cli_agent: Option<Agent>) -> Agent
    fn effective_project_scope(command_all: bool, explicit_project: Option<&str>, source: Option<SourceFilter>) -> Option<String>

The exact names can differ, but the responsibilities must be clear and isolated. `QueryService` should continue to accept only plain optional filters and should not gain knowledge of environment variables or cwd policy.

Revision note: 2026-03-18 / Codex. Created the initial ExecPlan because the requested behavior change spans CLI defaults, env-driven policy, integration tests, and documentation, which is broad enough to require a restartable implementation plan.

Revision note: 2026-03-18 / Codex. Updated the plan after implementing the CLI boundary and repository docs so the progress log, discoveries, decisions, and retrospective reflect the current state instead of the initial draft.

Revision note: 2026-03-18 / Codex. Updated the plan after landing the contract tests, helper unit tests, and full verification results so the document matches the completed implementation.
