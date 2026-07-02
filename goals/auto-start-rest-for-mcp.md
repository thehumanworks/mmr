---
goal_id: "auto-start-rest-for-mcp"
title: "Auto-start REST backing server for MCP"
description: "Make the mmr MCP Python bridge start its own local REST backing server so users do not manage a separate API process."
date: "2026-07-02"
status: "done"
confidence_floor: 90
created: "2026-07-02"
updated: "2026-07-02"
---

# Goal: `mmr mcp python-bootstrap --run` starts a local mmr REST backing server automatically before launching the Python FastMCP bridge.

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

- User request — `mmr` CLI should start a new `mmr` server daemon when MCP is called so users do not separately start the REST API server.
- `goals/expose-python-mcp-bootstrap.md` — previous completed goal that added `mmr mcp python-bootstrap` and documented the current REST backing requirement.
- `src/mcp.rs` — current native MCP transports plus Python bootstrap launcher; this is the primary surface for auto-start behavior.
- `scripts/mmr_rest_mcp_bootstrap.py` — generated Python FastMCP bridge that reads `API_BASE_URL` and `OPENAPI_URL` before serving MCP.
- `docs/api/openapi.json` — OpenAPI contract the Python bridge loads; the backing server must serve it.
- `tests/mcp_contract.rs` — existing MCP subprocess/HTTP/bootstrap tests; add auto-start and non-regression coverage here.
- `.agents/skills/mmr-mcp-rust-sdk/references/rmcp-server-patterns.md` — local rmcp implementation/testing patterns.
- `.cursor/rules/verification-loop.mdc` — final repository verification loop.

---

## 3. Definition of Done · INVARIANT

Each item is **atomic** (one verifiable assertion per checkbox), tagged with a
stable id that Tasks reference via **Closes:**, and carries a concrete `verify by:`.

Tick a `DoD-N` box only when its own `verify by:` has been run and passed (not merely
because a closing Task is ticked). Log the command and its outcome as an Evidence bullet
under the Task that **Closes:** it. DONE requires every DoD box ticked.

- [x] **DoD-1** — `mmr mcp python-bootstrap --run` no longer requires the user to provide or pre-start a REST API URL; by default it creates a loopback REST backing server and passes that URL to Python — *verify by:* `cargo test --test mcp_contract mcp_python_bootstrap_run_autostarts_rest_backing_server`
- [x] **DoD-2** — The auto-started REST backing server serves `openapi.json` and at least the curated read/status routes needed by the Python bootstrap without exposing a new public required workflow — *verify by:* `cargo test --test mcp_contract mcp_rest_backing_server_serves_openapi_and_status`
- [x] **DoD-3** — Existing native MCP transports and dependency-free bootstrap modes still behave as before — *verify by:* `cargo test --test mcp_contract mcp_python_bootstrap_cli_contract mcp_python_bootstrap_dry_run_and_missing_dependency_contract mcp_stdio_subprocess_protocol_smoke mcp_http_streamable_smoke`
- [x] **DoD-4** — Help/docs make clear that run mode auto-starts the REST backing server and how to override it — *verify by:* `cargo run -- mcp python-bootstrap --help && rg -n "auto-start|REST backing|no-auto-rest|API_BASE_URL|OPENAPI_URL" docs src tests`
- [x] **DoD-5** — Full repository verification loop passes after implementation — *verify by:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

The goal terminates when **any** condition holds. On exit, state which fired —
explicitly — in the response to the user.

- **`DONE`** — all §3 items ticked and all §5 tasks ≥ confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Python or Rust test tooling cannot execute after one direct retry; exit naming the failed command.
- **`SCOPE-CHANGE`** — the requested behavior requires a full public REST product server beyond the private backing routes needed for the Python bridge, or requires a new Python package manager/project dependency. Record the proposal in §6 and exit.
- **`CONFIDENCE-STALL`** — any task cannot reach 90 confidence after two implementation/verification attempts. Exit, report the task and gap.
- **`BUDGET`** — three implementation iterations pass without satisfying DoD-1 through DoD-4. Exit and report progress plus remaining blockers.

---

## 5. Tasks · INVARIANT

Ordered, dependency-aware units of work that together satisfy the DoD. Tick the
trailing `[ ]` only when the Verification Contract passes and Confidence ≥ floor.

---

### T1 · Confirm current launch gap and command contract · [x]

**Steps**
- [x] Inventory the existing Python bootstrap `--run` env behavior and native MCP transport tests.
- [x] Confirm the smallest command contract for auto-start and an override path.
- [x] Decide whether the backing server should be internal to `mmr mcp python-bootstrap --run` or a new public command.

**Verification Contract**
- *Check:* The planned behavior closes the user-facing gap without replacing native Rust MCP or making REST startup a separate user workflow.
- *Method:* `cargo run -- mcp python-bootstrap --help && cargo test --test mcp_contract mcp_python_bootstrap_dry_run_and_missing_dependency_contract`
- *Expected:* Current help/test behavior is understood before edits; any command-contract decision is logged in §6.
- *BDD scenarios covered:* Default run auto-starts backing server; explicit external API URL remains possible; native MCP unchanged.

**Confidence:** 95 / 90 · **Depends on:** none · **Closes:** none

**Evidence (required before tick; append-only)**
- *(none yet — when setting Confidence ≥ floor, append a bullet with all three: date + command/check run + outcome (exit code / test counts / artifact path))*
- 2026-07-02 — check: `cargo run -- mcp python-bootstrap --help && cargo test --test mcp_contract mcp_python_bootstrap_dry_run_and_missing_dependency_contract`; outcome: exit 0, help shows current manual `API_BASE_URL`/`OPENAPI_URL` surface and targeted dry-run/missing-runtime test passed 1 / 0.

---

### T2 · Implement auto-started REST backing server for Python run mode · [x]

**Steps**
- [x] Add an internal loopback REST backing router/server that serves OpenAPI and curated read/status routes by reusing existing CLI semantics.
- [x] Start that backing server automatically for `python-bootstrap --run` when the user has not opted out.
- [x] Pass generated `API_BASE_URL` and `OPENAPI_URL` to the Python process, while preserving explicit override behavior.

**Verification Contract**
- *Check:* Run mode produces deterministic launch metadata/tests showing an auto-started REST backing URL and usable OpenAPI/status endpoints.
- *Method:* `cargo test --test mcp_contract mcp_python_bootstrap_run_autostarts_rest_backing_server mcp_rest_backing_server_serves_openapi_and_status`
- *Expected:* Both tests pass and prove the backing server is loopback/local.
- *BDD scenarios covered:* Default run starts backing server; OpenAPI loads from backing server; status route returns JSON.

**Confidence:** 94 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2

**Evidence (required before tick; append-only)**
- *(none yet)*
- 2026-07-02 — check: `cargo fmt && cargo test --test mcp_contract mcp_python_bootstrap_run_autostarts_rest_backing_server -- --nocapture && cargo test --test mcp_contract mcp_rest_backing_server_serves_openapi_and_status -- --nocapture`; outcome: exit 0, formatter exit 0, each targeted test passed 1 / 0 after warning cleanup.

---

### T3 · Preserve bootstrap and native MCP compatibility · [x]

**Steps**
- [x] Update dry-run/write/help tests to account for auto-start metadata without importing FastMCP.
- [x] Re-run native stdio and HTTP MCP smoke tests.
- [x] Keep stdout/stderr separation for native stdio MCP.

**Verification Contract**
- *Check:* Existing dependency-free and native MCP behaviors are unchanged.
- *Method:* `cargo test --test mcp_contract mcp_python_bootstrap_cli_contract mcp_python_bootstrap_dry_run_and_missing_dependency_contract mcp_stdio_subprocess_protocol_smoke mcp_http_streamable_smoke`
- *Expected:* All named tests pass; no native MCP regression.
- *BDD scenarios covered:* Print/write/dry-run do not need FastMCP; missing Python still reports actionable error; Rust MCP stdio/http still initialize.

**Confidence:** 95 / 90 · **Depends on:** T2 · **Closes:** DoD-3

**Evidence (required before tick; append-only)**
- *(none yet)*
- 2026-07-02 — check: individual Cargo filters for `mcp_python_bootstrap_cli_contract`, `mcp_python_bootstrap_dry_run_and_missing_dependency_contract`, `mcp_stdio_subprocess_protocol_smoke`, and `mcp_http_streamable_smoke`; outcome: all four commands exited 0, each with 1 passed / 0 failed.

---

### T4 · Update help/docs and run final verification · [x]

**Steps**
- [x] Update help/docs for auto-start, override, and REST env behavior.
- [x] Run help/search docs verification.
- [x] Run the full repository verification loop and inspect changed artifacts.

**Verification Contract**
- *Check:* Docs/help explain the new default and the whole repo remains healthy.
- *Method:* `cargo run -- mcp python-bootstrap --help && rg -n "auto-start|REST backing|no-auto-rest|API_BASE_URL|OPENAPI_URL" docs src tests && cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* Help/search exit 0 and full verification exits 0.
- *BDD scenarios covered:* User sees that no separate REST API server is needed; user sees how to override.

**Confidence:** 96 / 90 · **Depends on:** T3 · **Closes:** DoD-4, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*
- 2026-07-02 — check: `cargo run -- mcp python-bootstrap --help && rg -n "auto-start|REST backing|no-auto-rest|API_BASE_URL|OPENAPI_URL" docs src tests`; outcome: exit 0, help lists auto-start/`--no-auto-rest` behavior and search finds docs/source/test coverage.
- 2026-07-02 — check: `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`; outcome: exit 0 on rerun after clippy fix, full `cargo test` passed 157 unit + 137 CLI contract + 14 MCP contract + 47 memory fabric + doctests with 0 failures, ignored benchmark command passed 4 / 0, clippy exited 0, release build exited 0.

---

## 6. Decisions · LIVE (append-only)

Meaningful choices/concessions needing visibility. Scope impact must be `none`.

- 2026-07-02 — Context: user asked that `mmr mcp` start a new `mmr` server daemon so users do not have to spin up the REST API server separately. Decision: implement the default for the Python FastMCP bridge run path (`mmr mcp python-bootstrap --run`) because native `mmr mcp --transport stdio|http` is already a self-contained Rust MCP server and has no REST dependency; keep any REST backing server private/internal unless the implementation proves a public command is required. Alternatives rejected: requiring Pitchfork/manual REST startup, replacing native Rust MCP with REST-to-MCP, or adding a new repo-wide Python project dependency. Scope impact: none.
- 2026-07-02 — Context: T1 confirmed the current CLI exposes `--api-base-url` and `--openapi-url` but does not auto-provide a REST runtime. Decision: add default auto-started loopback REST backing for `--run`, with `--no-auto-rest` as the explicit external-server override; leave print/write/dry-run dependency-free and keep native Rust MCP transports unchanged. Alternatives rejected: changing `mmr mcp --transport stdio|http` semantics or adding a public REST command in this goal. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

Flash cards: trigger → wrong action → revision → correct action, with impact `1–5`.
When an attempt failed and the fix is not yet known, log the **open form** —
trigger → wrong action → *(open: revision/correct not yet found)* → pointer to the raw
failure (log path or commit) — still impact-tagged, so a dead-end is recorded before a
fresh context re-treads it.

*(none yet)*
- 2026-07-02 — trigger: a goal method again named multiple Cargo test filters in one command → wrong action: running both exact names together produced `unexpected argument` before tests executed → revision: keep the goal text stable, run each exact filter as its own command for proof → correct action: use one Cargo filter per targeted contract test or a shared prefix when available. Impact: 3.
- 2026-07-02 — trigger: full verification reached `cargo clippy --all-targets --all-features -- -D warnings` → wrong action: initial implementation left `.arg(&script)` in a generic-arg call → revision: remove the needless borrow and rerun the full loop → correct action: treat clippy failures as blocking even after all tests pass. Impact: 4.

---

## 8. Skills · LIVE (append-only)

Reusable workflows created via the **skill-creator** skill while working this goal.

*(none yet)*
