---
goal_id: "expose-python-mcp-bootstrap"
title: "Expose Python MCP bootstrap from mmr CLI"
description: "Implement docs/references/rest-to-mcp.md by adding a Python FastMCP OpenAPI bootstrap surface to the mmr CLI."
date: "2026-07-02"
status: "done"
confidence_floor: 90
created: "2026-07-02"
updated: "2026-07-02"
---

# Goal: `mmr mcp python-bootstrap` can create and launch the documented FastMCP OpenAPI bootstrap without regressing the existing Rust MCP server.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal — if it isn't written here,
it didn't happen. The full procedure (boot loop, confidence rubric, logging cadence) lives in the
**goal-driven-development** skill; these rules hold even if that skill isn't loaded:

- **Scope is frozen after user confirms DoD + Tasks.** Until then, §3 and §5 may be
  edited freely. After confirm, the only permitted edits are: tick checkboxes (Task
  **and** DoD), update Confidence, append Evidence, append to the live sections
  (§6/§7/§8), and update frontmatter `status`/`updated` — never add, remove, reword,
  split, or merge a DoD item or Task, and never rewrite or delete a live-section entry.
- **Never tick below the floor.** A task is ticked done only at Confidence ≥
  `confidence_floor`. If you cannot reach it, leave it unticked and fire `CONFIDENCE-STALL`.
- **Scope change is an exit, not a decision.** If scope must change, record the
  proposal in §6 and fire `SCOPE-CHANGE` — stop and surface it to the user.
- **Live sections are append-only.** Log each decision (§6) and learning (§7) at
  the moment it happens — before ticking the task it came from. Never delete entries.

---

## 2. References

Everything the agent needs before/while working. Each entry is `path-or-url — why it matters`.

- User request — implement `docs/references/rest-to-mcp.md` by exposing the Python MCP server bootstrap from within the `mmr` CLI.
- `docs/references/rest-to-mcp.md` — source reference for the intended FastMCP `from_openapi` bootstrap, route maps, transforms, stdio/http run modes, and test client pattern.
- `docs/api/openapi.json` — current OpenAPI 3.1 contract the Python MCP bootstrap should consume by URL or file.
- `scripts/generate-api-docs.mjs` — generator for the OpenAPI contract; useful to confirm operation IDs and route tags while curating bootstrap defaults.
- `src/cli.rs` — clap command surface and command routing; existing `Commands::Mcp(crate::mcp::McpArgs)` must keep working.
- `src/mcp.rs` — existing Rust MCP server implementation and `McpArgs`; new bootstrap must not break `mmr mcp --transport stdio|http`.
- `tests/mcp_contract.rs` — current MCP subprocess and streamable HTTP smoke tests; add contract tests here for bootstrap behavior and legacy compatibility.
- `.cursor/rules/verification-loop.mdc` — final repo verification gate for meaningful changes.
- `.cursor/rules/cli-contract.mdc` — stdout/stderr and CLI compatibility constraints.
- `.cursor/rules/test-discipline.mdc` — integration test style and binary invocation requirements.

---

## 3. Definition of Done · INVARIANT

Each item is **atomic** (one verifiable assertion per checkbox), tagged with a
stable id that Tasks reference via **Closes:**, and carries a concrete `verify by:`.

Tick a `DoD-N` box only when its own `verify by:` has been run and passed (not merely
because a closing Task is ticked). Log the command and its outcome as an Evidence bullet
under the Task that **Closes:** it. DONE requires every DoD box ticked.

- [x] **DoD-1** — The CLI exposes a `mmr mcp python-bootstrap` surface that can print/write the Python FastMCP bootstrap and supports stdio/http launch options without removing `mmr mcp --transport stdio|http` — *verify by:* `cargo test --test mcp_contract mcp_python_bootstrap_cli_contract mcp_stdio_subprocess_protocol_smoke mcp_http_streamable_smoke`
- [x] **DoD-2** — The generated/bundled Python bootstrap imports FastMCP/httpx, loads an OpenAPI document, builds `FastMCP.from_openapi(...)`, applies MMR-specific tool-name/description curation, and runs over stdio or HTTP — *verify by:* `python3 -m py_compile scripts/mmr_rest_mcp_bootstrap.py && cargo test --test mcp_contract mcp_python_bootstrap_writes_expected_server`
- [x] **DoD-3** — The launcher has deterministic dependency/runtime behavior: dry-run/write modes work without FastMCP installed, and run mode either starts the Python process or exits with a clear missing-dependency/actionable error — *verify by:* `cargo test --test mcp_contract mcp_python_bootstrap_dry_run_and_missing_dependency_contract`
- [x] **DoD-4** — User-facing docs/help describe how to invoke the Python bootstrap, required env vars, OpenAPI URL/base URL options, and how it differs from the native Rust MCP server — *verify by:* `cargo run -- mcp python-bootstrap --help && rg -n "python-bootstrap|FastMCP|API_BASE_URL|OPENAPI_URL" docs src tests`
- [x] **DoD-5** — The full repository verification loop passes after implementation — *verify by:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

The goal terminates when **any** condition holds. On exit, state which fired —
explicitly — in the response to the user.

- **`DONE`** — all §3 items ticked and all §5 tasks ≥ confidence floor. *(primary)*
- **`BLOCKED-DEP`** — local Python cannot execute `py_compile`, or the Rust toolchain cannot run the targeted MCP contract tests, after one direct retry. Exit without the blocked step and name the failed dependency.
- **`SCOPE-CHANGE`** — the existing `mmr mcp --transport stdio|http` surface cannot be preserved while adding `python-bootstrap`, or the implementation requires adding a Python package manager/project dependency not covered by this goal. Record the proposal in §6 and exit to the user.
- **`CONFIDENCE-STALL`** — any task cannot reach 90 confidence after two honest implementation/verification attempts. Exit, report the task and the gap.
- **`BUDGET`** — three implementation iterations pass without satisfying DoD-1 through DoD-4. Exit and report progress plus remaining blockers.

---

## 5. Tasks · INVARIANT

Ordered, dependency-aware units of work that together satisfy the DoD. Tick the
trailing `[ ]` only when the Verification Contract passes and Confidence ≥ floor.

---

### T1 · Confirm command contract and bootstrap boundaries · [x]

**Steps**
- [x] Inventory the current `mmr mcp` clap routing and tests before editing.
- [x] Confirm the concrete `python-bootstrap` command shape can coexist with `mmr mcp --transport stdio|http`.
- [x] Decide where the Python bootstrap lives and whether the CLI prints, writes, and/or runs it.

**Verification Contract**
- *Check:* The planned command shape is concrete, preserves existing Rust MCP invocations, and requires no unapproved repo-wide Python packaging setup.
- *Method:* `cargo run -- mcp --help && cargo run -- mcp --transport stdio --help`
- *Expected:* Current `mmr mcp` help is understood before edits; any planned changes are documented in §6 before implementation.
- *BDD scenarios covered:* Existing Rust MCP users keep `mmr mcp --transport stdio`; new users discover `mmr mcp python-bootstrap`.

**Confidence:** 95 / 90 · **Depends on:** none · **Closes:** none

**Evidence (required before tick; append-only)**
- *(none yet — when setting Confidence ≥ floor, append a bullet with all three: date + command/check run + outcome (exit code / test counts / artifact path))*
- 2026-07-02 — check: `cargo run -- mcp --help && cargo run -- mcp --transport stdio --help`; outcome: exit 0 for both, current native MCP help requires `--transport <stdio|http>` and remains the compatibility contract before edits.

---

### T2 · Implement Python bootstrap artifact and CLI launcher · [x]

**Steps**
- [x] Add the Python bootstrap template or script using `docs/references/rest-to-mcp.md` as the source contract.
- [x] Add clap routing for `mmr mcp python-bootstrap` while preserving existing native MCP paths.
- [x] Implement print/write/dry-run/run behavior with explicit env/argument handling for `API_BASE_URL`, `OPENAPI_URL`, token env, stdio, and HTTP mode.

**Verification Contract**
- *Check:* The CLI can expose the bootstrap artifact and prepare or start the Python FastMCP server without requiring FastMCP for non-run modes.
- *Method:* `python3 -m py_compile scripts/mmr_rest_mcp_bootstrap.py && cargo test --test mcp_contract mcp_python_bootstrap_cli_contract mcp_python_bootstrap_writes_expected_server`
- *Expected:* Python syntax check passes; targeted tests prove generated server content and CLI contract.
- *BDD scenarios covered:* Print/write bootstrap; run/launch path assembled with URL/env options; existing `mmr mcp` still routes to Rust server.

**Confidence:** 94 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2

**Evidence (required before tick; append-only)**
- *(none yet)*
- 2026-07-02 — check: `python3 -m py_compile scripts/mmr_rest_mcp_bootstrap.py && cargo test --test mcp_contract mcp_python_bootstrap` plus exact single-test reruns for `mcp_python_bootstrap_writes_expected_server` and `mcp_python_bootstrap_cli_contract`; outcome: py_compile exit 0, prefix test 3 passed / 0 failed, each single-test rerun 1 passed / 0 failed.

---

### T3 · Add launcher failure-mode and compatibility tests · [x]

**Steps**
- [x] Add fixture-driven tests for dry-run JSON or equivalent deterministic launch metadata.
- [x] Add missing-dependency/error-message tests without relying on real FastMCP installation.
- [x] Retain and re-run existing `mcp_stdio_subprocess_protocol_smoke` and `mcp_http_streamable_smoke`.

**Verification Contract**
- *Check:* Runtime edge cases and legacy Rust MCP compatibility are covered by integration tests.
- *Method:* `cargo test --test mcp_contract mcp_python_bootstrap_dry_run_and_missing_dependency_contract mcp_stdio_subprocess_protocol_smoke mcp_http_streamable_smoke`
- *Expected:* All named tests pass; failures identify either launcher behavior or a native MCP regression.
- *BDD scenarios covered:* FastMCP absent; dry-run/write mode; native stdio/http MCP still initializes.

**Confidence:** 95 / 90 · **Depends on:** T2 · **Closes:** DoD-1, DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*
- 2026-07-02 — check: individual Cargo filters for `mcp_python_bootstrap_dry_run_and_missing_dependency_contract`, `mcp_stdio_subprocess_protocol_smoke`, `mcp_http_streamable_smoke`, and `mcp_python_bootstrap_cli_contract`; outcome: each command exited 0 with 1 passed / 0 failed, preserving native Rust MCP stdio/http while proving bootstrap dry-run and missing-runtime behavior.

---

### T4 · Update user-facing docs and help text · [x]

**Steps**
- [x] Update CLI help/examples or nearby docs so users know when to use Rust MCP vs Python FastMCP bootstrap.
- [x] Document required env vars/options and the expected OpenAPI/REST server relationship.
- [x] Keep docs narrowly scoped to this bootstrap; do not add generic MCP or FastMCP tutorials beyond the existing reference.

**Verification Contract**
- *Check:* Help/docs explain the new surface and can be found by exact command/env names.
- *Method:* `cargo run -- mcp python-bootstrap --help && rg -n "python-bootstrap|FastMCP|API_BASE_URL|OPENAPI_URL" docs src tests`
- *Expected:* Help exits 0 and search finds command/env references in intended docs/help/test surfaces.
- *BDD scenarios covered:* New user discovers required env vars; existing user sees the distinction from native MCP.

**Confidence:** 96 / 90 · **Depends on:** T2 · **Closes:** DoD-4

**Evidence (required before tick; append-only)**
- *(none yet)*
- 2026-07-02 — check: `cargo run -- mcp python-bootstrap --help && rg -n "python-bootstrap|FastMCP|API_BASE_URL|OPENAPI_URL" docs src tests`; outcome: exit 0, help lists bootstrap actions/options and search finds command/env references in docs, source, and tests.

---

### T5 · Run full verification and completion review · [x]

**Steps**
- [x] Run the full repo verification loop.
- [x] Inspect changed files and generated/written bootstrap artifacts.
- [x] Perform an explicit completion review against every DoD before marking done.

**Verification Contract**
- *Check:* All repo gates pass and the completion claim is backed by inspected artifacts.
- *Method:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* Every command exits 0; completion review finds no unproven DoD.
- *BDD scenarios covered:* Entire CLI/MCP contract remains healthy after adding bootstrap.

**Confidence:** 97 / 90 · **Depends on:** T3, T4 · **Closes:** DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*
- 2026-07-02 — check: `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`; outcome: exit 0, full `cargo test` passed 157 unit + 137 CLI contract + 12 MCP contract + 47 memory fabric + doctests with 0 failures, ignored benchmark command passed 4 / 0, clippy exited 0, release build exited 0.
- 2026-07-02 — check: `cargo run -- mcp python-bootstrap --write <tmp>/server.py && python3 -m py_compile <tmp>/server.py && rg ... <tmp>/server.py`; outcome: exit 0, generated artifact reported 7207 bytes and contained `FastMCP.from_openapi`, `mmr_list_projects`, `API_BASE_URL`, `OPENAPI_URL`, and templated `/v1/sessions/{session_id}/messages` route-map coverage.

---

## 6. Decisions · LIVE (append-only)

Meaningful choices/concessions needing visibility. Scope impact must be `none`.

- 2026-07-02 — Context: adversarial author review checked whether the goal drifted into replacing the native Rust MCP server or adding broad Python project infrastructure. Decision: keep the goal to one new bootstrap surface under `mmr mcp python-bootstrap`, preserve `mmr mcp --transport stdio|http`, and treat Python dependency installation as caller environment rather than adding a repo-wide package manager unless implementation proves that impossible. Alternatives rejected: replacing Rust MCP, adding a generic FastMCP tutorial, or silently requiring FastMCP in dry-run/write modes. Scope impact: none.
- 2026-07-02 — Context: T1 help probe confirmed current `mmr mcp` is a flag-only command with required `--transport <stdio|http>`. Decision: implement `python-bootstrap` as an optional MCP subcommand and keep the existing flag path valid for native Rust MCP; the bootstrap artifact lives in `scripts/mmr_rest_mcp_bootstrap.py`, while the CLI can print, write, dry-run, or launch it without adding repo-wide Python packaging. Alternatives rejected: a separate top-level command or changing native `mmr mcp --transport ...` syntax. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

Flash cards: trigger → wrong action → revision → correct action, with impact `1–5`.
When an attempt failed and the fix is not yet known, log the **open form** —
trigger → wrong action → *(open: revision/correct not yet found)* → pointer to the raw
failure (log path or commit) — still impact-tagged, so a dead-end is recorded before a
fresh context re-treads it.

- 2026-07-02 — trigger: a frozen goal method used two Cargo test filters in one command → wrong action: running it verbatim produced `unexpected argument 'mcp_python_bootstrap_writes_expected_server'` before tests executed → revision: preserve the frozen goal text, run Cargo with a shared prefix or individual filters for evidence → correct action: use `cargo test --test mcp_contract mcp_python_bootstrap` for all bootstrap tests and exact single-filter commands for DoD-specific proof. Impact: 3.

---

## 8. Skills · LIVE (append-only)

Reusable workflows created via the **skill-creator** skill while working this goal.

*(none yet)*
