---
title: "Define MMR REST API with OpenAPI docs"
description: "Create an OpenAPI 3.1 JSON contract for MMR's REST API and generated human-readable API docs, using specialized subagents for exploration, ideation, and adversarial review."
date: 2026-07-02
status: done
---

# GOAL: Define MMR REST API with OpenAPI docs

## Outcome

MMR has a checked-in OpenAPI 3.1 JSON document that defines its REST API and
generated API documentation that can be served locally over `0.0.0.0` for phone
access over Tailscale.

## Surfaces touched

- Project-scoped Codex subagent definitions in `.codex/agents/` for this API
  design/review workflow.
- OpenAPI contract under `docs/api/`.
- Generated static API documentation under `docs/api/`.
- Any small support script or metadata needed to regenerate or serve the docs.

## Process requirements

- Use the `codex-subagent-builder` skill to create specialized agent
  definitions.
- Use multiple subagents for independent codebase exploration and API ideation.
- Run at least two adversarial review rounds against the API structure/spec.
- Treat subagent outputs as evidence and critique, not as proof.

## Validation plan

- Validate every new `.codex/agents/*.toml` file with the subagent-builder
  validator.
- Validate the OpenAPI JSON is parseable and declares `openapi: 3.1.0`.
- Validate the generated docs are derived from the OpenAPI file and are readable
  through an HTTP request.
- Serve the docs in the background on `0.0.0.0` and report the local/Tailscale
  URL proof.
- Run the repository verification loop after meaningful changes:
  `cargo fmt`, `cargo test`, `cargo test --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo build --release`.

## Definition of done

- [x] OpenAPI JSON is present, valid JSON, and compliant with OpenAPI 3.1.0.
- [x] The spec covers MMR's REST resources, shared schemas, errors, pagination,
      source filtering, project scoping, retrieval/context, sync/redaction
      status, and session sharing surfaces at the level appropriate for a
      REST API contract.
- [x] Generated API docs are present and derived from the OpenAPI JSON.
- [x] Specialized project subagent definitions exist and validate.
- [x] At least two subagent review rounds have been completed and material
      findings are either fixed or explicitly rejected with rationale.
- [x] Docs are served in the background on `0.0.0.0` and reachable locally.
- [x] Relevant validation commands pass, or the goal is marked blocked with the
      smallest missing dependency.
