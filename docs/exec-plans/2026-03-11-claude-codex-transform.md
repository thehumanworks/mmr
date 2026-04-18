# Add Bidirectional Claude/Codex Message Transform Support

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

This plan was authored in accordance with the ExecPlan contract in `/Users/mish/.agents/skills/exec-plan/references/PLANS.md`. The repository does not currently contain its own checked-in `PLANS.md`, so this file must remain self-contained.

## Purpose / Big Picture

After this change, a contributor will be able to take a Claude JSONL session file and emit an equivalent Codex JSONL session file, or take a Codex JSONL session file and emit an equivalent Claude JSONL session file, from the `mmr` CLI. This matters because `mmr` already knows how to read both ecosystems, but it cannot yet bridge them. Success is visible when `mmr transform --from claude --to codex ...` and `mmr transform --from codex --to claude ...` both produce JSONL that `mmr` can ingest again, preserving message chronology, roles, session identity, project identity, and the best available model metadata under an explicit defaulting policy.

The transform is intentionally defined as a `mmr`-compatible transform, not as a promise that first-party Claude Code or Codex CLI will accept the generated files as native session state. The acceptance bar is that `mmr` can parse the transformed output and that the observable transcript semantics remain stable across the conversion.

## Progress

- [x] (2026-03-11 21:05Z) Reviewed `/Users/mish/projects/mmr/docs/references/schemas/claude/message_schema.md`, `/Users/mish/projects/mmr/docs/references/schemas/codex/message_schema.md`, and both example JSON files to map the documented raw-message contracts and the current normalized output.
- [x] (2026-03-11 21:12Z) Reviewed `/Users/mish/projects/mmr/src/types/domain.rs`, `/Users/mish/projects/mmr/src/types/api.rs`, `/Users/mish/projects/mmr/src/source/claude.rs`, `/Users/mish/projects/mmr/src/source/codex.rs`, `/Users/mish/projects/mmr/src/cli.rs`, `/Users/mish/projects/mmr/src/agent/ai.rs`, and `/Users/mish/projects/mmr/tests/cli_contract.rs` to anchor the plan in current types, parsing rules, and CLI/testing patterns.
- [x] (2026-03-11 21:25Z) Drafted this ExecPlan with explicit schema convergence/divergence analysis, a concrete `transform` CLI surface, model default rules, and validation steps.
- [ ] Implement `src/transform.rs`, expose it from `/Users/mish/projects/mmr/src/lib.rs`, and wire a new `transform` subcommand in `/Users/mish/projects/mmr/src/cli.rs`.
- [ ] Extract reusable single-file parsing entry points from `/Users/mish/projects/mmr/src/source/claude.rs` and `/Users/mish/projects/mmr/src/source/codex.rs` so the transform path can reuse the same defensive parsing rules as normal ingestion.
- [ ] Add fixture-driven integration tests covering both directions, both model-default env vars, CLI flag overrides, and a round-trip ingest check.
- [ ] Update the schema reference docs so the transform output contract, especially Codex model handling, is documented instead of living only in code.
- [ ] Run the full repository verification loop and capture the observed output in this plan.

## Surprises & Discoveries

- Observation: The two source formats already converge inside `mmr` at `MessageRecord`, so there is a natural canonical intermediate representation for the transform implementation.
  Evidence: `/Users/mish/projects/mmr/src/source/claude.rs` and `/Users/mish/projects/mmr/src/source/codex.rs` both normalize into the same `crate::types::domain::MessageRecord` struct in `/Users/mish/projects/mmr/src/types/domain.rs`.

- Observation: The documented Codex schema is stateful while the documented Claude schema is self-contained per message line.
  Evidence: Codex requires a prior `session_meta` record to establish `session_id`, `cwd`, and model metadata for subsequent lines, while each Claude `user` or `assistant` line carries its own `sessionId` and optional `cwd`.

- Observation: The current Codex documentation and loader preserve only `model_provider` such as `openai`, not a concrete model identifier such as `gpt-5.4`, so a meaningful transform requires an explicit extension to the current Codex mapping.
  Evidence: `/Users/mish/projects/mmr/docs/references/schemas/codex/message_schema.md` maps normalized `model` to `session_meta.payload.model_provider`, and `/Users/mish/projects/mmr/src/source/codex.rs` stores only that field today.

- Observation: Claude extraction is more permissive than Codex extraction, so Claude-to-Codex transforms will be lossy for non-text structure that is not preserved in `MessageRecord`.
  Evidence: Claude recursively extracts `message.content` from strings, arrays, nested `content`, and nested `parts`, while Codex assistant ingestion keeps only `output_text` blocks and ignores the rest.

- Observation: The repository has no existing ExecPlan files or transform module, so both the implementation surface and the contributor documentation must be introduced from scratch.
  Evidence: `docs/exec-plans/` existed but was empty when this plan was created.

## Decision Log

- Decision: Implement the transform around the existing normalized `MessageRecord` representation and session-scoped metadata, rather than inventing a second full-fidelity internal AST for every raw source shape.
  Rationale: `MessageRecord` is already the repository’s stable internal message contract. Reusing it keeps the transform aligned with actual `mmr` behavior and makes the acceptance test straightforward: transform output must re-ingest into equivalent `MessageRecord` values.
  Date/Author: 2026-03-11 / Codex

- Decision: Scope the first version to single-file JSONL input and single-file JSONL output through a new `transform` CLI command.
  Rationale: This is the smallest useful unit. Directory-level bulk migration can be added later without changing the core serializer/parser logic.
  Date/Author: 2026-03-11 / Codex

- Decision: Add explicit target-model resolution rules instead of trying to infer target models from source files.
  Rationale: The schemas diverge too much for inference to be reliable. Claude records may omit `message.model`, and Codex currently stores only a provider string. Deterministic defaults are required for repeatable output.
  Date/Author: 2026-03-11 / Codex

- Decision: For synthetic Codex output, extend the session metadata written by `mmr` to include a concrete `model` field and a nested `reasoning.effort` field, while still keeping `model_provider: "openai"`.
  Rationale: This preserves backward compatibility with current parsing while allowing the transformed file to carry the concrete model requested by the user. It also gives `mmr` somewhere explicit to store the requested default reasoning effort of `high`.
  Date/Author: 2026-03-11 / Codex

- Decision: Keep Codex reasoning effort fixed at `high` in this first pass and do not introduce a separate env var or CLI flag for it.
  Rationale: The user requested model overrides through `CLAUDE_DEFAULT_MODEL`, `CODEX_DEFAULT_MODEL`, and CLI flags, but did not request independent reasoning configuration. A fixed reasoning value keeps the surface minimal while satisfying the requested default.
  Date/Author: 2026-03-11 / Codex

## Outcomes & Retrospective

Planning completed. The repo now has a concrete, restartable implementation plan that identifies the main schema mismatches, the required CLI additions, the transform data flow, and the validation bar. No Rust code has been changed yet, so there are no runtime outcomes to report beyond the design choices recorded here.

## Context and Orientation

`mmr` is a Rust command-line tool that loads local Claude and Codex history, normalizes both into a common internal type, and exposes JSON responses through existing subcommands in `/Users/mish/projects/mmr/src/cli.rs`. The common normalized message shape lives in `/Users/mish/projects/mmr/src/types/domain.rs` as `MessageRecord`, and the source-specific readers live in `/Users/mish/projects/mmr/src/source/claude.rs` and `/Users/mish/projects/mmr/src/source/codex.rs`.

For this task, “transform” means reading one raw JSONL session file in one ecosystem’s schema and writing one raw JSONL session file in the other ecosystem’s schema. “Canonical intermediate representation” means the internal data shape the transform uses between parse and serialize. In this repository, that canonical representation is the combination of `MessageRecord` values plus a small amount of session-level metadata that `MessageRecord` alone does not currently capture, specifically the target model choice and Codex reasoning effort.

The current schema convergence is strong enough to support a practical bridge:

Both source formats are JSONL, both carry timestamps, both have user and assistant messages, both support a session identifier, both identify a project path in some form, both eventually reduce to plain text content, and both are already normalized into `MessageRecord`.

The current schema divergence is what defines the implementation work:

Claude is line-self-contained. Each ingested line has `type`, `sessionId`, `message`, optional `cwd`, and optional `timestamp`. The `message` object may itself carry `role`, `content`, `model`, and `usage`. Claude also has a repository-specific distinction between normal sessions and `subagents/` files, which `mmr` exposes as `is_subagent`.

Codex is session-stateful. A `session_meta` line must appear before message lines. User messages appear as `event_msg` lines with `payload.type == "user_message"` and assistant messages appear as `response_item` lines with `payload.role == "assistant"`. Assistant content is an array of blocks and only `output_text` blocks are currently ingested. Codex does not currently expose per-message token counts in `mmr`, and the current mapping stores only `model_provider`, not an actual model identifier.

Those differences imply two important boundaries. First, the transform is inherently lossy because the current normalized contract is text-first and does not preserve every raw content block or every source-specific field. Second, the transform cannot be “reversible” in a byte-for-byte sense; the correct acceptance target is semantic equivalence after re-ingestion by `mmr`.

The files that will matter during implementation are:

`/Users/mish/projects/mmr/src/types/domain.rs`, which defines `SourceFilter`, `SourceKind`, and `MessageRecord`.

`/Users/mish/projects/mmr/src/types/api.rs`, which defines the API response types.

`/Users/mish/projects/mmr/src/source/claude.rs`, which currently parses Claude files defensively and recursively extracts text content.

`/Users/mish/projects/mmr/src/source/codex.rs`, which currently parses Codex files statefully and only preserves `model_provider` from `session_meta`.

`/Users/mish/projects/mmr/src/cli.rs`, which owns the public command surface and output serialization. This file will gain the new `transform` command and the CLI flags that override target model defaults.

`/Users/mish/projects/mmr/src/lib.rs`, which must export the new transform module.

`/Users/mish/projects/mmr/tests/common/mod.rs` and `/Users/mish/projects/mmr/tests/cli_contract.rs`, which define the fixture model and the integration-test style the new command must follow.

`/Users/mish/projects/mmr/docs/references/schemas/claude/message_schema.md` and `/Users/mish/projects/mmr/docs/references/schemas/codex/message_schema.md`, which must be updated so the written transform contract is documented for future contributors.

## Milestones

### Milestone 1: Define the canonical transform interface and extract reusable parsers

At the end of this milestone, the repository will have a concrete `src/transform.rs` module with public entry points, and the existing source readers will expose reusable single-file parsing helpers that follow the same defensive rules as the current home-directory ingestion path. The proof for this milestone is that unit tests or focused integration tests can parse a standalone Claude or Codex file into the canonical transform input without walking an entire fake `HOME`.

### Milestone 2: Serialize canonical messages into target-source JSONL with deterministic model defaults

At the end of this milestone, the transform module will be able to write either Claude-style or Codex-style JSONL from the canonical message/session representation. The proof is that a converted file can be parsed again by the corresponding source loader and yields the expected role/content/timestamp/session/project values, with model resolution following the exact precedence rules defined in this plan.

### Milestone 3: Expose the feature through the CLI and lock it down with contract tests

At the end of this milestone, `mmr transform` will be a documented CLI command with integration tests for both conversion directions, for env-based defaults, and for CLI-flag overrides. The proof is that the CLI writes output files, that `stdout` remains machine-readable JSON or a minimal success payload if the command returns structured status, and that the full repository verification loop passes.

## Plan of Work

Create a new module at `/Users/mish/projects/mmr/src/transform.rs`. This module should own three responsibilities. First, it should resolve target defaults. Second, it should convert parsed messages into a session-scoped canonical form suitable for serialization. Third, it should serialize that canonical form into the requested target schema as JSONL lines.

Start by extracting file-oriented parsing helpers from the existing loaders rather than reimplementing parsing logic inside the transform module. In `/Users/mish/projects/mmr/src/source/claude.rs`, extract the body of `parse_claude_file` into a helper that can be called both from the existing directory walker and from the new transform module. Keep the same behavior for malformed lines, empty content, usage extraction, `cwd` fallback, and `is_subagent`. In `/Users/mish/projects/mmr/src/source/codex.rs`, extract a similar file-oriented helper around `parse_codex_file` and make the `session_meta` parsing reusable. Do not change the existing query behavior while doing this extraction.

Then define the transform-facing types in `/Users/mish/projects/mmr/src/transform.rs`. Keep them minimal and prescriptive. A good target is a session-level structure such as `TransformSession` containing `session_id`, `project_name`, `project_path`, `source`, `messages`, `is_subagent`, and model defaults resolved for the target. The message entries can reuse `MessageRecord` directly or wrap it if that makes the serializer cleaner, but avoid duplicating fields unless the session-level serializer genuinely needs them. Also define a `TransformTarget` enum and a `TransformOptions` struct that captures the input path, output path, source kind, target kind, and optional CLI overrides.

Define the target-model resolution contract inside `src/transform.rs` and treat it as stable behavior. The precedence must be:

For Claude target output, use the CLI flag if provided, otherwise `CLAUDE_DEFAULT_MODEL` if it exists and is non-empty, otherwise `claude-opus-4.6`.

For Codex target output, use the CLI flag if provided, otherwise `CODEX_DEFAULT_MODEL` if it exists and is non-empty, otherwise `gpt-5.4`.

For Codex target output, always write `reasoning.effort = "high"` unless a later product requirement adds a dedicated override surface. Also write `model_provider = "openai"` because the current Codex loader and docs already treat that field as canonical provider metadata.

Implement the Claude serializer so each output message is an independent JSON object with top-level `type`, `sessionId`, `cwd`, and `timestamp`, plus a nested `message` object containing `role`, `content`, `model`, and `usage` when tokens are available. Preserve the canonical role/content/timestamp values exactly. Preserve `input_tokens` and `output_tokens` only when they are greater than zero; otherwise omit `usage` rather than emitting misleading zeroes if that keeps the output closer to the source convention.

Implement the Codex serializer so each file starts with one `session_meta` record, followed by chronological message records. The `session_meta.payload` must contain `id`, `cwd`, `model_provider`, `model`, and `reasoning`. The message records must follow the currently documented ingestion pattern: user messages become `event_msg` with `payload.type = "user_message"` and `payload.message = <content>`, assistant messages become `response_item` with `payload.role = "assistant"` and `payload.content = [{"type":"output_text","text":<content>}]`. Preserve timestamps on every record. Do not attempt to synthesize unsupported multimodal structures in this first pass.

Update `/Users/mish/projects/mmr/src/source/codex.rs` so the loader prefers `session_meta.payload.model` when present, falling back to `session_meta.payload.model_provider` only when the concrete model is absent. If the synthetic `reasoning.effort` field is parsed anywhere, keep it confined to the transform path unless a clear user-visible use appears; the current API responses do not need a new reasoning field for this plan to succeed.

Add the new command to `/Users/mish/projects/mmr/src/cli.rs`. The public interface should be:

    mmr transform --from <claude|codex> --to <claude|codex> --input <path> --output <path> [--claude-model <model>] [--codex-model <model>]

Reject `--from` and `--to` when they are equal, because a same-source transform is not useful and would blur the behavior contract. Keep the command output machine-readable. The simplest acceptable shape is a small JSON object that reports `from`, `to`, `input`, `output`, and `messages_written`. If the implementation instead reuses an existing API response type, document that choice here when the work starts.

Add integration tests in `/Users/mish/projects/mmr/tests/cli_contract.rs`. Seed one Claude input file and one Codex input file directly inside a temp directory. Run `mmr transform` against each direction. Then place the output file under the matching fake `HOME` location and verify that an existing `messages` query returns the expected normalized transcript. Add separate tests for:

CLI override beats env default for Claude target model.

CLI override beats env default for Codex target model.

Missing env vars use `claude-opus-4.6` and `gpt-5.4`.

Synthetic Codex output re-ingests with normalized `model == "gpt-5.4"` or the overridden model, not `openai`.

Same-source transforms are rejected with a non-zero exit status and a human-readable error on `stderr`.

Update the schema docs last, once the code shape is settled. The Claude doc should gain a short section that explains the transform writer’s emitted subset. The Codex doc should be revised to state that `mmr` may emit and ingest `session_meta.payload.model` and `session_meta.payload.reasoning.effort`, with `model_provider` retained as provider metadata. Also update the example JSON files if they are intended to remain representative of transformed output.

## Concrete Steps

From the repository root `/Users/mish/projects/mmr`, inspect the current transform-relevant files again before editing:

    sed -n '1,220p' src/source/claude.rs
    sed -n '1,220p' src/source/codex.rs
    sed -n '1,220p' src/cli.rs
    sed -n '1,220p' tests/common/mod.rs
    sed -n '700,1200p' tests/cli_contract.rs

Implement the module and CLI wiring, then format:

    cargo fmt

Run focused tests while iterating:

    cargo test --test cli_contract transform

If the focused filter does not match because test names differ, run the full contract suite instead:

    cargo test --test cli_contract

Run the mandatory repository verification loop before claiming completion:

    cargo test
    cargo test --test cli_benchmark -- --ignored --nocapture
    cargo clippy --all-targets --all-features -- -D warnings
    cargo build --release

Expected implementation-time outcomes:

The new `transform` command writes an output file at the requested path.

Re-ingesting that file through the existing source loader yields the expected message count and preserves chronological order.

Codex target output reports the resolved concrete model string after re-ingestion, not just the provider string.

When `--claude-model` or `--codex-model` is supplied, the CLI result and re-ingested transcript reflect the override regardless of environment variables.

## Validation and Acceptance

Acceptance is behavioral and must be demonstrated in two ways.

First, the direct CLI behavior must work. From the repository root, create one standalone Claude JSONL input file and one standalone Codex JSONL input file in a temporary directory. Run:

    cargo run -- transform --from claude --to codex --input /tmp/claude.jsonl --output /tmp/codex.jsonl
    cargo run -- transform --from codex --to claude --input /tmp/codex.jsonl --output /tmp/claude.jsonl

Each command must exit successfully and create the requested output file.

Second, the transformed file must be observable through `mmr`’s existing ingest path. The integration tests should copy the output into the correct fake `HOME` tree and then run `mmr messages` or `mmr export` against that seeded location. The normalized results must preserve:

Message count.

Chronological order.

Role values.

Content text.

Session identifier.

Project path or project name semantics appropriate to the target source.

Resolved model according to the precedence rules in this plan.

Full completion acceptance requires the entire verification loop to pass: `cargo fmt`, `cargo test`, `cargo test --test cli_benchmark -- --ignored --nocapture`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo build --release`.

## Idempotence and Recovery

This work is safe to repeat because the transform command writes a new output file at an explicit path and does not mutate existing session history in place. When rerunning the command against the same output path, either overwrite the file atomically or remove it first and rewrite it completely; do not append, because JSONL concatenation would silently create invalid merged sessions.

If parser extraction from the source modules causes regressions in existing query behavior, stop and restore behavior by keeping the old public loader entry points untouched while moving shared parsing logic underneath them. The risk is not data loss but changed ingest semantics, so recovery means rerunning the existing test suite until the original query contract is intact again.

If the Codex model-field extension proves awkward, keep `model_provider` parsing backward-compatible and make the richer `payload.model` optional. That gives a safe fallback path where old files still parse exactly as before while transformed files gain better normalized model values.

## Artifacts and Notes

Schema convergence summary captured during planning:

Both formats are JSONL.

Both expose user and assistant turns.

Both carry timestamps and session identity.

Both have project identity, although Claude uses a directory-name encoding and Codex uses `cwd` directly.

Both are already normalized to `MessageRecord`.

Schema divergence summary captured during planning:

Claude is per-line self-contained; Codex needs a stateful `session_meta` header.

Claude content extraction is recursive and permissive; Codex assistant extraction only keeps `output_text`.

Claude may carry per-message token usage; Codex currently does not.

Claude exposes message-level `model`; Codex currently exposes only session-level provider metadata in the documented loader path.

Claude has `subagents/` provenance; Codex does not.

Representative output shapes that the implementation should target:

Synthetic Claude line:

    {"type":"assistant","sessionId":"sess-1","cwd":"/Users/test/proj","timestamp":"2026-03-11T21:00:00Z","message":{"role":"assistant","content":"hello","model":"claude-opus-4.6","usage":{"input_tokens":10,"output_tokens":5}}}

Synthetic Codex header plus assistant line:

    {"type":"session_meta","timestamp":"2026-03-11T21:00:00Z","payload":{"id":"sess-1","cwd":"/Users/test/proj","model_provider":"openai","model":"gpt-5.4","reasoning":{"effort":"high"}}}
    {"type":"response_item","timestamp":"2026-03-11T21:00:01Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}

Record final command transcripts and any adjustments to these example lines here once implementation begins.

## Interfaces and Dependencies

In `/Users/mish/projects/mmr/src/transform.rs`, define stable transform-facing types and functions. The exact names may vary slightly during implementation, but the repository should end with an equivalent public surface:

    pub enum TransformSource {
        Claude,
        Codex,
    }

    pub struct TransformOptions {
        pub from: TransformSource,
        pub to: TransformSource,
        pub input: std::path::PathBuf,
        pub output: std::path::PathBuf,
        pub claude_model_override: Option<String>,
        pub codex_model_override: Option<String>,
    }

    pub struct TransformResult {
        pub from: TransformSource,
        pub to: TransformSource,
        pub input: String,
        pub output: String,
        pub messages_written: usize,
    }

    pub fn run_transform(options: TransformOptions) -> anyhow::Result<TransformResult>;

`run_transform` should parse the source file, resolve defaults, serialize the target file, and return a machine-readable summary used by `/Users/mish/projects/mmr/src/cli.rs`.

In `/Users/mish/projects/mmr/src/source/codex.rs`, the effective model-resolution logic should end with the equivalent of:

    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| payload.get("model_provider").and_then(Value::as_str))
        .unwrap_or("");

In `/Users/mish/projects/mmr/src/cli.rs`, add a new `Transform` variant to `Commands` and keep the command output machine-readable JSON on `stdout`, with any rejection or usage errors on `stderr`.

Plan revision note: Created on 2026-03-11 to turn the newly added schema reference docs into a concrete implementation plan for bidirectional Claude/Codex transforms, with explicit handling for synthetic model defaults and Codex reasoning metadata.
