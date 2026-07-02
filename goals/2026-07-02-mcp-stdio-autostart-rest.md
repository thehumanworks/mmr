---
goal_id: "2026-07-02-mcp-stdio-autostart-rest"
title: "Autostart REST for MCP stdio"
status: "done"
confidence_floor: 90
created: "2026-07-02"
updated: "2026-07-02"
---

# Goal: `mmr mcp --transport stdio` ensures a local loopback REST backing server is available without requiring a separate user command.

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

- User request: `keep the CLI command simpler - mmr mcp --transport stdio should autostart if no running processes exist locally` — primary acceptance surface.
- `src/mcp.rs` — native MCP server, Python bootstrap, and private REST backing router live here.
- `tests/mcp_contract.rs` — protocol-level MCP subprocess tests and current REST auto-start coverage.
- `docs/api/README.md` — user-facing MCP/REST bootstrap command guidance.
- `docs/references/rest-to-mcp.md` — original REST-to-MCP design reference for OpenAPI-backed MCP.
- `.cursor/rules/verification-loop.mdc` — required final Rust verification sequence.
- `.cursor/rules/cli-contract.mdc` — stdout/stderr and CLI JSON contract rules.

---

## 3. Definition of Done · INVARIANT

Each item is **atomic** (one verifiable assertion per checkbox), tagged with a
stable id that Tasks reference via **Closes:**, and carries a concrete `verify by:`.

Tick a `DoD-N` box only when its own `verify by:` has been run and passed (not merely
because a closing Task is ticked). Log the command and its outcome as an Evidence bullet
under the Task that **Closes:** it. DONE requires every DoD box ticked.

- [x] **DoD-1** — `mmr mcp --transport stdio` starts successfully from a clean local test environment and makes a loopback REST backing endpoint available without requiring `python-bootstrap` or a separate REST command — *verify by:* `cargo test --test mcp_contract mcp_stdio_autostarts_rest_backing_when_missing -- --nocapture`
- [x] **DoD-2** — `mmr mcp --transport stdio` reuses an already-running local loopback REST backing endpoint instead of starting a duplicate server — *verify by:* `cargo test --test mcp_contract mcp_stdio_reuses_existing_rest_backing -- --nocapture`
- [x] **DoD-3** — stdio MCP protocol output remains frame-only on stdout and startup diagnostics stay on stderr — *verify by:* `cargo test --test mcp_contract mcp_stdio_subprocess_protocol_smoke -- --nocapture`
- [x] **DoD-4** — docs describe the simpler `mmr mcp --transport stdio` behavior and the implementation passes the repo verification loop — *verify by:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

The goal terminates when **any** condition holds. On exit, state which fired —
explicitly — in the response to the user. Specialize the bracketed values for this goal.

- **`DONE`** — all §3 items ticked and all §5 tasks ≥ confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Rust MCP stdio cannot coexist with the needed local REST backing after one focused redesign attempt. Exit without weakening the command contract.
- **`SCOPE-CHANGE`** — satisfying the request requires a new public daemon command, a persistent service installer, or changing the MCP transport contract. Record the proposal in §6 and exit to the user.
- **`CONFIDENCE-STALL`** — any task cannot reach the floor after two honest attempts. Exit, report the task and the gap.
- **`BUDGET`** — full verification cannot complete in the current turn after implementation and targeted tests pass. Exit and report progress.

---

## 5. Tasks · INVARIANT

Ordered, dependency-aware units of work that together satisfy the DoD. Tick the
trailing `[ ]` only when the Verification Contract passes and Confidence ≥ floor.

---

### T1 · Orient the existing MCP and REST surfaces · [x]

**Steps**
- [x] Confirm current native stdio MCP startup path and REST backing router ownership.
- [x] Identify the narrow process-local check for "already running locally" that avoids new public CLI flags.
- [x] Record the chosen interpretation and rejected broader daemon work in §6.

**Verification Contract**
- *Check:* The implementation target is clear and no unrelated daemon surface is required.
- *Method:* `rg -n "run_stdio|RestBackingServer|rest_backing_router|mcp_stdio" src/mcp.rs tests/mcp_contract.rs docs/api/README.md`
- *Expected:* Evidence shows native stdio startup and private REST backing are both in `src/mcp.rs`, with tests in `tests/mcp_contract.rs`.
- *BDD scenarios covered:* Given the user runs `mmr mcp --transport stdio`, then no separate REST command is required.

**Confidence:** 95 / 90 · **Depends on:** none · **Closes:** none

**Evidence (required before tick; append-only)**
- 2026-07-02 — ran `rg -n "run_stdio|RestBackingServer|rest_backing_router|mcp_stdio" src/mcp.rs tests/mcp_contract.rs docs/api/README.md`; exit 0; showed `run_stdio()` at `src/mcp.rs:618`, private `RestBackingServer` and router in `src/mcp.rs`, and existing stdio subprocess tests in `tests/mcp_contract.rs`.

---

### T2 · Add stdio REST backing auto-start and reuse · [x]

**Steps**
- [x] Teach stdio MCP startup to check a local loopback REST backing endpoint.
- [x] Reuse an already-healthy local endpoint instead of starting another server.
- [x] Start a private loopback REST backing task when none is healthy, without writing diagnostics to stdout.

**Verification Contract**
- *Check:* Stdio MCP subprocess exposes a reachable REST backing endpoint and preserves MCP stdout framing.
- *Method:* `cargo test --test mcp_contract mcp_stdio_ -- --nocapture`
- *Expected:* Targeted tests pass.
- *BDD scenarios covered:* Given no local REST process exists, stdio MCP starts one; given one exists, stdio MCP reuses it.

**Confidence:** 95 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- 2026-07-02 — ran `cargo test --test mcp_contract mcp_stdio_ -- --nocapture`; exit 0; 3 passed covering auto-start, existing endpoint reuse, and stdio protocol smoke.
- 2026-07-02 — ran `cargo test --test mcp_contract mcp_python_bootstrap_cli_contract -- --nocapture`; exit 0; 1 passed confirming help mentions the local loopback REST backing endpoint.

---

### T3 · Update docs and run the verification loop · [x]

**Steps**
- [x] Update user-facing MCP docs to emphasize `mmr mcp --transport stdio`.
- [x] Run targeted MCP checks and the required full Rust verification loop.
- [x] Mark the goal done only after `gdd_status.py` reports DONE-eligible.

**Verification Contract**
- *Check:* Docs mention the simpler stdio behavior and all required checks pass.
- *Method:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* Full loop exits 0.
- *BDD scenarios covered:* Given a user configures only `mmr mcp --transport stdio`, docs tell them no separate REST startup is needed.

**Confidence:** 95 / 90 · **Depends on:** T2 · **Closes:** DoD-4

**Evidence (required before tick; append-only)**
- 2026-07-02 — ran `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`; exit 0; 157 unit, 137 CLI contract, 16 MCP contract, 47 memory fabric, 4 ignored benchmarks, clippy, and release build passed.

---

## 6. Decisions · LIVE (append-only)

Meaningful choices/concessions needing visibility. Scope impact must be `none`.

- 2026-07-02 — The request targets the simple native command shape, so the public acceptance path is `mmr mcp --transport stdio`; `python-bootstrap` remains available but is not the main workflow. Alternatives rejected: requiring `mmr mcp python-bootstrap --run` or adding a public `mmr rest serve` prerequisite. Scope impact: none.
- 2026-07-02 — "No running processes exist locally" is interpreted as the default loopback REST backing endpoint not responding. The implementation may reuse a healthy local endpoint or start a private backing task owned by the stdio MCP process; it must not install a persistent OS service in this goal. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

Flash cards: trigger → wrong action → revision → correct action, with impact `1–5`.
When an attempt failed and the fix is not yet known, log the **open form** —
trigger → wrong action → *(open: revision/correct not yet found)* → pointer to the raw
failure (log path or commit) — still impact-tagged, so a dead-end is recorded before a
fresh context re-treads it.

*(none yet)*

---

## 8. Skills · LIVE (append-only)

Reusable workflows created via the **skill-creator** skill while working this goal.

*(none yet)*
