# Add Shared Dry-Run Planning to `mmr merge`

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

This plan was authored in accordance with `/Users/mish/.agents/skills/exec-plan/references/PLANS.md`. The repository does not currently contain a checked-in repo-local `PLANS.md`, so this file must remain self-contained.

## Purpose / Big Picture

After this change, `mmr merge` will be able to validate and describe a merge without mutating Claude or Codex history files. Users will be able to run `mmr merge --dry-run ...` to see the exact sessions, files, actions, timestamp strategy, and model strategy that a real merge would apply, and can optionally write a ZIP archive containing the exact resolved history inputs before experimenting further.

The visible proof is behavioral. Running `mmr merge --dry-run --from-session sess-claude-1 --to-session sess-codex-1` must return machine-readable JSON describing the append plan while leaving the underlying `.jsonl` files unchanged. Running the same command with `--zip-output /tmp/merge-inputs.zip` must create a ZIP archive containing the exact resolved Claude and Codex history inputs used to build that plan.

## Progress

- [x] (2026-03-19 12:20Z) Reviewed the current merge CLI in `src/cli.rs`, the merge implementation in `src/merge/mod.rs`, the merge contract tests in `tests/cli_contract.rs`, and the repo verification rules in `.cursor/rules/verification-loop.mdc`, `.cursor/rules/cli-contract.mdc`, and `.cursor/rules/test-discipline.mdc`.
- [x] (2026-03-19 12:34Z) Drafted this ExecPlan and recorded the intended dry-run, ZIP backup, response-shape, and verification behavior.
- [x] (2026-03-19 12:41Z) Added failing parser and integration-test expectations for `--dry-run`, `--zip-output`, dry-run non-mutation, plan reporting, ZIP creation, and invalid flag rejection in `src/cli.rs` and `tests/cli_contract.rs`.
- [x] (2026-03-19 13:04Z) Refactored `src/merge/mod.rs` into shared planning plus apply/finalize phases, made session resolution multi-file aware, added ZIP creation, and extended the merge response schema in `src/types/api.rs`.
- [x] (2026-03-19 13:09Z) Updated command help, `AGENTS.md`, and `.cursor/rules/cli-contract.mdc` so the dry-run and ZIP behavior is discoverable in the repo’s durable guidance.
- [x] (2026-03-19 13:18Z) Ran the targeted dry-run tests, `cargo test --test cli_contract`, `cargo fmt`, `cargo test`, `cargo test --test cli_benchmark -- --ignored --nocapture`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo build --release`.
- [x] (2026-03-19 13:20Z) Re-ran the merge help and dry-run proof scenarios and confirmed the new flags, non-mutating dry-run behavior, and ZIP archive contents match the resolved plan.

## Surprises & Discoveries

- Observation: The dry-run feature required `SessionHandle` to carry the sorted, deduplicated set of resolved history input files rather than only a single file path.
  Evidence: the final implementation collects `record.source_file` values into `SessionHandle.source_files` in `session_handle_from_api`, and both the merge response and ZIP archive are built from that list.

- Observation: Session-to-session merges currently read the destination history file as part of timestamp adjustment and Claude UUID chaining, so the dry-run backup set must include that destination file even when no write occurs.
  Evidence: the final planner uses `to.messages`, `existing_session_file(to)`, and `read_last_claude_uuid(&target_file, ...)` while constructing append plans before any write occurs.

- Observation: The `zip` crate’s current stable API is sufficient with only the `deflate` feature enabled; no additional packaging helpers or external tools are required.
  Evidence: `cargo add zip --no-default-features --features deflate` resolved to `zip v8.3.0`, and the archive implementation uses `zip::ZipWriter` plus `zip::write::SimpleFileOptions`.

## Decision Log

- Decision: Keep `mmr merge` as one shared pipeline with a pure planning phase and a final apply phase instead of adding a dedicated dry-run implementation.
  Rationale: The request explicitly asks for shared resolution and validation logic, and the existing helper set in `src/merge/mod.rs` is already suitable for reuse if the write step is moved to the end.
  Date/Author: 2026-03-19 / Codex

- Decision: Make `--zip-output` valid only with `--dry-run`.
  Rationale: The ZIP archive is intended as a safety backup for experimentation based on the dry-run plan. Allowing it during real execution would blur the contract and create avoidable ambiguity about whether the archive represents pre-merge or post-merge inputs.
  Date/Author: 2026-03-19 / Codex

- Decision: Store ZIP entries relative to the resolved home directory, such as `.claude/projects/...` and `.codex/sessions/...`.
  Rationale: Relative paths preserve enough structure to distinguish Claude and Codex harness inputs while avoiding absolute-path leakage and making archive contents portable.
  Date/Author: 2026-03-19 / Codex

- Decision: Keep `target_file` populated in dry-run responses, including create-target-session plans, and interpret “absent” as “file not created on disk.”
  Rationale: The task requires dry-run output to report the exact files and actions that would be touched. Returning the resolved destination path is necessary for that, while the contract tests separately verify that dry-run does not create the file.
  Date/Author: 2026-03-19 / Codex

## Outcomes & Retrospective

Completed. `mmr merge` now builds a single shared merge plan for both real execution and dry-run execution. The planner resolves sessions, gathers exact input files, computes timestamp and model strategies, resolves destination files, and produces response metadata before any write occurs. Real execution now applies that plan at the end, while `--dry-run` returns the same plan without mutating history inputs.

The final implementation also adds `--zip-output <PATH>` as a dry-run-only option that writes a ZIP archive of the exact resolved history inputs from the dry-run plan. The repository help text and durable CLI guidance were updated in the same change, and the verification loop passed end to end. The most important lesson was that the merge refactor stayed small only because the existing helper functions were already separable from the final write step; the main missing piece was carrying full input-file resolution through the pipeline instead of only the first source file.

## Context and Orientation

`mmr` is a Rust CLI that ingests local Claude and Codex conversation history from JSONL files under `~/.claude/projects` and `~/.codex/{sessions,archived_sessions}`. The CLI surface lives in `src/cli.rs`. The merge implementation lives entirely in `src/merge/mod.rs`. Public JSON response structs live in `src/types/api.rs`. Integration tests that execute the built binary against a temporary `HOME` tree live in `tests/cli_contract.rs`, with fixture seeding in `tests/common/mod.rs`.

In the current implementation, `merge::merge` both plans and applies a merge in one pass. Session resolution is handled by `resolve_unique_session` and `resolve_source_sessions`, which build `SessionHandle` values from `QueryService` data. Session-to-session merges append transformed messages into an existing destination file. Agent-to-agent merges synthesize a new destination session file under the target source’s history directory. That current shape means a dry-run feature must be added by factoring out the resolution, validation, transformation, and target-path computation before the final write step, not by cloning the merge algorithm.

For this task, “resolved history inputs” means the exact existing `.jsonl` files that the merge planner reads while constructing the plan. For a session-to-session merge, that includes the source session history file and the destination session history file. For an agent-to-agent merge, that includes the source session files but not the yet-to-be-created destination file. “Dry-run” means all validations and plan computations happen, JSON is emitted on `stdout`, and the history inputs remain byte-for-byte unchanged unless the user explicitly requests a ZIP archive written to a separate path.

## Plan of Work

Begin by extending the merge CLI in `src/cli.rs` with `--dry-run` and `--zip-output <PATH>`, plus request validation that rejects `--zip-output` without `--dry-run`. Keep the command help additive and consistent with existing clap derive patterns. Update the parser tests in the same file so the flag behavior is locked down before the merge code changes.

In `src/merge/mod.rs`, introduce internal plan structs that describe one merge operation and the full run. Move session resolution, message preparation, timestamp adjustment, model strategy selection, target-project resolution, and target-file calculation into functions that produce those plan structs without mutating the filesystem. Make `SessionHandle` carry a sorted, deduplicated list of existing input files instead of only the first source file, and thread that through the plan and API response. After the planner exists, add a finalizer that either writes the rendered JSONL lines or skips writes for dry-run. ZIP creation must run from the dry-run plan before any history write logic is entered.

Extend `src/types/api.rs` so the merge response can report `dry_run`, `zip_output`, `resolved_history_files`, per-session `action`, and per-session `source_files`. Keep the existing merge response fields intact so existing real-merge contract tests keep passing with only additive assertions needed for dry-run. Update `tests/cli_contract.rs` to verify plan reporting, dry-run non-mutation, and ZIP contents from a temporary archive path.

Finally, update the merge help text and repo guidance in `AGENTS.md` so future contributors can discover the new dry-run and backup behavior without reverse-engineering the tests or implementation.

## Concrete Steps

From the repository root `/Users/mish/dev/mmr`, inspect the relevant files before changing behavior:

    sed -n '1,260p' src/cli.rs
    sed -n '1,260p' src/merge/mod.rs
    sed -n '1560,2280p' tests/cli_contract.rs
    sed -n '1,180p' src/types/api.rs

Iterate on the merge contract with targeted tests first:

    cargo test --test cli_contract merge_dry_run_reports_append_plan_and_does_not_mutate_inputs -- --exact --nocapture
    cargo test --test cli_contract merge_dry_run_agent_to_agent_reports_create_plan_without_writing_target -- --exact --nocapture
    cargo test --test cli_contract merge_dry_run_zip_output_creates_archive_of_resolved_history_inputs -- --exact --nocapture
    cargo test --test cli_contract merge_rejects_zip_output_without_dry_run -- --exact --nocapture

Then run the broader verification loop:

    cargo test --test cli_contract
    cargo fmt
    cargo test
    cargo test --test cli_benchmark -- --ignored --nocapture
    cargo clippy --all-targets --all-features -- -D warnings
    cargo build --release

The post-change proof commands should include at least one dry-run scenario and one ZIP scenario, both run against a fixture-backed temporary `HOME`.

## Validation and Acceptance

Acceptance requires the following observable behavior:

`mmr merge --dry-run --from-session sess-claude-1 --to-session sess-codex-1` returns JSON with `dry_run: true`, `action: append-into-existing-session`, the resolved source and destination history paths, and the same merge semantics the real command would apply.

The source and destination history files touched by that dry run remain byte-for-byte unchanged after the command exits.

`mmr merge --dry-run --from-agent codex --to-agent claude --project /Users/test/codex-proj` returns planned create-target-session actions, reports the resolved Codex input files, and does not create the reported Claude destination files.

`mmr merge --dry-run --zip-output <PATH> ...` creates a ZIP archive whose entry names are relative to the resolved home directory and whose contents correspond exactly to the resolved history inputs from the dry-run plan.

`mmr merge --zip-output <PATH> ...` without `--dry-run` fails clearly on `stderr`.

Completion requires the full repository verification loop to pass: `cargo fmt`, `cargo test`, `cargo test --test cli_benchmark -- --ignored --nocapture`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo build --release`.

## Idempotence and Recovery

The dry-run path is intentionally idempotent because it must not mutate history inputs. Re-running the same dry-run command should return the same planned actions and leave the same files untouched. ZIP creation is safe to retry as long as the output path does not already exist; the implementation should reject existing ZIP paths rather than overwrite them.

The real merge path remains append-or-create behavior against history files, so its current safety constraints still apply. If the refactor causes plan generation to diverge from real execution, the recovery path is to rerun the targeted dry-run tests and the existing real-merge tests together until the reported plan and applied behavior match exactly.

## Artifacts and Notes

Important pre-change behaviors that must remain true after the refactor:

Session-to-session merges may shift copied timestamps forward so the appended messages stay after the destination session’s last message.

Merging into Codex collapses per-message source model values to a single provider string at session scope.

Merging into Claude preserves or expands assistant model values on each assistant message, with subagent ancestry flattened when creating new Claude sessions.

The merge response remains machine-readable JSON on `stdout`, with human-facing validation failures reported on `stderr`.

## Interfaces and Dependencies

Add only one new dependency for archive creation: the Rust `zip` crate in `Cargo.toml`, and update `Cargo.lock` accordingly. Keep using existing dependencies such as `anyhow`, `clap`, `serde`, `serde_json`, and `time`.

At the end of the change, the public merge response in `src/types/api.rs` must include:

`dry_run: bool`

`zip_output: Option<String>`

`resolved_history_files: Vec<String>`

and each `ApiMergeSession` must also include:

`action: String`

`source_files: Vec<String>`

The internal merge planner in `src/merge/mod.rs` may use different private type names, but it must separate plan construction from final side effects.

Revision note: 2026-03-19 / Codex. Created the initial ExecPlan after reviewing the merge implementation, repository planning contract, and verification rules, and updated it immediately after landing the first dry-run parser and contract tests so the living document reflects actual progress.

Revision note: 2026-03-19 / Codex. Updated the plan after completing the planner/apply refactor, ZIP archive support, CLI and repo guidance changes, and the full verification loop so the living sections reflect the finished implementation and proof commands.
