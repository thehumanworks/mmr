---
goal_id: "2026-07-02-remove-python-bootstrap-mcp"
title: "Remove Python MCP Bootstrap"
status: "done"
confidence_floor: 90
created: "2026-07-02"
updated: "2026-07-02"
---

# Goal: `mmr mcp` exposes only the native Rust MCP transports, with no `python-bootstrap` command, docs, tests, or script left as compatibility surface.

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

- User request: remove the `python-bootstrap` subcommand and do not leave it as backwards compatibility — primary acceptance surface.
- `src/mcp.rs` — owns the `mmr mcp` clap args, native stdio/http runners, and private REST backing implementation.
- `src/cli.rs` — top-level help examples currently still mention `mmr mcp python-bootstrap`.
- `tests/mcp_contract.rs` — MCP contract tests currently include Python bootstrap tests that should be removed/replaced with rejection/help assertions.
- `docs/api/README.md` — user-facing MCP docs currently advertise the Python bootstrap.
- `scripts/mmr_rest_mcp_bootstrap.py` — Python bootstrap artifact to remove if no longer referenced.
- `.cursor/rules/verification-loop.mdc` — required final Rust verification sequence.
- `.cursor/rules/cli-contract.mdc` — stdout/stderr and CLI command contract rules.

---

## 3. Definition of Done · INVARIANT

Each item is **atomic** (one verifiable assertion per checkbox), tagged with a
stable id that Tasks reference via **Closes:**, and carries a concrete `verify by:`.

Tick a `DoD-N` box only when its own `verify by:` has been run and passed (not merely
because a closing Task is ticked). Log the command and its outcome as an Evidence bullet
under the Task that **Closes:** it. DONE requires every DoD box ticked.

- [x] **DoD-1** — `mmr mcp --help` no longer lists `python-bootstrap` and top-level examples do not advertise it — *verify by:* `cargo run -- mcp --help && cargo run -- --help`
- [x] **DoD-2** — `mmr mcp python-bootstrap` is rejected by clap instead of preserved as a compatibility path — *verify by:* `cargo test --test mcp_contract mcp_python_bootstrap_subcommand_is_removed -- --nocapture`
- [x] **DoD-3** — source, scripts, and active docs contain no live Python bootstrap implementation references — *verify by:* `rg -n "python-bootstrap|PythonBootstrap|PYTHON_BOOTSTRAP|mmr_rest_mcp_bootstrap|FastMCP|API_BASE_URL|OPENAPI_URL|no-auto-rest" src scripts docs/api docs/references README.md Cargo.toml`
- [x] **DoD-4** — native Rust MCP stdio/http behavior remains covered and the repo verification loop passes — *verify by:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

The goal terminates when **any** condition holds. On exit, state which fired —
explicitly — in the response to the user. Specialize the bracketed values for this goal.

- **`DONE`** — all §3 items ticked and all §5 tasks ≥ confidence floor. *(primary)*
- **`BLOCKED-DEP`** — the native MCP tests fail after one focused repair attempt because removing the bootstrap uncovers an unrelated MCP transport failure. Exit with the concrete failing command.
- **`SCOPE-CHANGE`** — fulfilling removal requires deleting completed historical goal docs or the OpenAPI REST contract itself. Record the proposal and exit; completed historical goals are not active product surface.
- **`CONFIDENCE-STALL`** — any task cannot reach the floor after two honest attempts. Exit, report the task and the gap.
- **`BUDGET`** — full verification cannot complete in the current turn after targeted checks pass. Exit and report progress.

---

## 5. Tasks · INVARIANT

Ordered, dependency-aware units of work that together satisfy the DoD. Tick the
trailing `[ ]` only when the Verification Contract passes and Confidence ≥ floor.

---

### T1 · Locate and classify Python bootstrap surface · [x]

**Steps**
- [x] Search live code, tests, scripts, and docs for Python bootstrap symbols.
- [x] Separate active product surface from completed historical goal docs.
- [x] Record the no-backwards-compatibility decision in §6.

**Verification Contract**
- *Check:* The removal target is bounded to active command/docs/tests/script files.
- *Method:* `rg -n "python-bootstrap|PythonBootstrap|PYTHON_BOOTSTRAP|FastMCP|API_BASE_URL|OPENAPI_URL|no-auto-rest" src tests scripts docs/api README.md Cargo.toml goals`
- *Expected:* Active hits are in `src/mcp.rs`, `src/cli.rs`, `tests/mcp_contract.rs`, `docs/api/README.md`, and `scripts/mmr_rest_mcp_bootstrap.py`; completed goal docs are history.
- *BDD scenarios covered:* Given the user asks no compatibility, then active command code and docs are removed rather than hidden.

**Confidence:** 95 / 90 · **Depends on:** none · **Closes:** none

**Evidence (required before tick; append-only)**
- 2026-07-02 — ran `rg -n "python-bootstrap|PythonBootstrap|PYTHON_BOOTSTRAP|FastMCP|API_BASE_URL|OPENAPI_URL|no-auto-rest" src tests scripts docs/api README.md Cargo.toml goals`; exit 0; active product hits found in `src/mcp.rs`, `src/cli.rs`, `tests/mcp_contract.rs`, `docs/api/README.md`, and `scripts/mmr_rest_mcp_bootstrap.py`; completed goal docs retained as history.

---

### T2 · Remove command, implementation, script, and live docs · [x]

**Steps**
- [x] Delete the `McpCommand` / `PythonBootstrapArgs` clap surface and bootstrap runner code.
- [x] Remove Python bootstrap contract tests and replace them with rejection/help assertions.
- [x] Delete `scripts/mmr_rest_mcp_bootstrap.py` and scrub active docs/help examples.

**Verification Contract**
- *Check:* The subcommand is absent from help, rejected by clap, and not referenced in live product files.
- *Method:* `cargo test --test mcp_contract mcp_python_bootstrap_subcommand_is_removed -- --nocapture && cargo run -- mcp --help && rg -n "python-bootstrap|PythonBootstrap|PYTHON_BOOTSTRAP|mmr_rest_mcp_bootstrap|FastMCP|API_BASE_URL|OPENAPI_URL|no-auto-rest" src scripts docs/api docs/references README.md Cargo.toml`
- *Expected:* Test passes; help omits `python-bootstrap`; search exits non-zero for active files.
- *BDD scenarios covered:* Given a user runs `mmr mcp python-bootstrap`, then the command fails instead of preserving compatibility.

**Confidence:** 95 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-3

**Evidence (required before tick; append-only)**
- 2026-07-02 — ran `cargo fmt && cargo test --test mcp_contract mcp_python_bootstrap_subcommand_is_removed -- --nocapture && cargo run -- mcp --help && cargo run -- --help && ! rg -n "python-bootstrap|PythonBootstrap|PYTHON_BOOTSTRAP|mmr_rest_mcp_bootstrap|FastMCP|API_BASE_URL|OPENAPI_URL|no-auto-rest" src scripts docs/api docs/references README.md Cargo.toml`; exit 0; targeted test passed, help omitted the subcommand, top-level examples omitted the subcommand, and the active-surface search returned no matches.

---

### T3 · Verify native MCP and full repo loop · [x]

**Steps**
- [x] Run targeted native MCP stdio/http tests after removal.
- [x] Run the required full verification loop.
- [x] Mark the goal done only after `gdd_status.py` says DONE.

**Verification Contract**
- *Check:* Native MCP still works and all repo gates pass.
- *Method:* `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* Full loop exits 0.
- *BDD scenarios covered:* Given the Python compatibility surface is gone, native `mmr mcp --transport stdio|http` still initializes.

**Confidence:** 95 / 90 · **Depends on:** T2 · **Closes:** DoD-4

**Evidence (required before tick; append-only)**
- 2026-07-02 — ran `cargo test --test mcp_contract mcp_stdio_ -- --nocapture`; exit 0; 3 passed covering stdio auto-start, existing backing reuse, and stdio protocol smoke.
- 2026-07-02 — ran `cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`; exit 0; 157 unit, 137 CLI contract, 12 MCP contract, 47 memory fabric, 4 ignored benchmarks, clippy, and release build passed.

---

## 6. Decisions · LIVE (append-only)

Meaningful choices/concessions needing visibility. Scope impact must be `none`.

- 2026-07-02 — The user explicitly rejected backwards compatibility for `python-bootstrap`; remove the active subcommand, tests, script, and docs instead of hiding, aliasing, or deprecating it. Completed historical goal docs may retain past references because they are task history, not product surface. Scope impact: none.

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
