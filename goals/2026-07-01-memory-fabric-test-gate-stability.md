---
goal_id: "2026-07-01-memory-fabric-test-gate-stability"
title: "Stabilize memory fabric test gate"
status: "active"
confidence_floor: 90
created: "2026-07-01"
updated: "2026-07-01"
---

# Goal: The default `cargo test` suite completes without external API-key dependencies or hanging mock HTTP servers.

## 1. Invariants · the rules that must not break

This file is the only state for this delivery subgoal. The full procedure lives
in the **goal-driven-development** skill.

- Do not require real OpenAI, proxy, or external network credentials for default tests.
- Keep optional external-provider smoke tests gated behind explicit env vars.
- Do not disable release-gate coverage; replace flaky external assumptions with deterministic local mocks.
- Full `cargo test` is the primary proof for this goal.

---

## 2. References

- `goals/2026-07-01-deep-project-review.md` — live verification evidence from the review.
- `tests/memory_fabric_contract.rs:1390` — `mvp_release_gate_e2e_fixture_scenario`.
- `tests/memory_fabric_contract.rs:1694` — release-gate summarize command that failed with host config requiring `CLI_PROXY_API_KEY`.
- `tests/memory_fabric_contract.rs:3936` — `summarize_config_api_key_contract_is_implemented` mock server setup.
- `tests/memory_fabric_contract.rs:3942` — mock server `read_to_end` can deadlock before response.
- `tests/cli_contract.rs` — working summarize mock patterns and config-isolation examples.
- `src/agent/chat_completions.rs` — HTTP client behavior and request/response expectations.
- `.cursor/rules/verification-loop.mdc` — default verification suite expectations.

---

## 3. Definition of Done · INVARIANT

- [ ] **DoD-1** — `mvp_release_gate_e2e_fixture_scenario` passes without `CLI_PROXY_API_KEY`, `OPENAI_API_KEY`, or host summarize config — *verify by:* `env -u CLI_PROXY_API_KEY -u OPENAI_API_KEY -u MMR_CONFIG_FILE cargo test --test memory_fabric_contract mvp_release_gate_e2e_fixture_scenario -- --exact --nocapture`
- [ ] **DoD-2** — `summarize_config_api_key_contract_is_implemented` completes without hanging and validates `summarize.apiKeyEnv` against its local mock — *verify by:* `cargo test --test memory_fabric_contract summarize_config_api_key_contract_is_implemented -- --exact --nocapture`
- [ ] **DoD-3** — Default `cargo test` completes and passes on this repo without external credentials — *verify by:* `env -u CLI_PROXY_API_KEY -u OPENAI_API_KEY cargo test`
- [ ] **DoD-4** — Optional external summary smoke remains gated and still requires explicit opt-in — *verify by:* `cargo test --test memory_fabric_contract optional_external_summary_provider_smoke_is_gated -- --exact --nocapture`
- [ ] **DoD-5** — Full repo verification loop is green — *verify by:* `cargo fmt --check && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`

---

## 4. Exit Conditions

- **`DONE`** — all §3 items ticked and all §5 tasks >= confidence floor. *(primary)*
- **`BLOCKED-DEP`** — Rust/Cargo, localhost TCP bind, or the test binary fixture harness is unavailable after one retry.
- **`SCOPE-CHANGE`** — preserving release-gate coverage requires changing summarize config semantics or introducing a new test-only config boundary.
- **`CONFIDENCE-STALL`** — the summarize mock still hangs after 3 focused fixes with request-body evidence.
- **`BUDGET`** — more than 3 full `cargo test` attempts after the two targeted tests pass.

---

## 5. Tasks · INVARIANT

### T1 · Reproduce both gate failures in isolation · [ ]

**Steps**
- [ ] Run the release-gate scenario with external key env vars removed.
- [ ] Run the summarize config test with `--nocapture` and a bounded timeout if needed.
- [ ] Identify whether failure comes from leaked host config, local mock behavior, or client request handling.

**Verification Contract**
- *Check:* the current failure/hang is captured with exact commands and no external ambiguity.
- *Method:* `env -u CLI_PROXY_API_KEY -u OPENAI_API_KEY -u MMR_CONFIG_FILE cargo test --test memory_fabric_contract mvp_release_gate_e2e_fixture_scenario -- --exact --nocapture; cargo test --test memory_fabric_contract summarize_config_api_key_contract_is_implemented -- --exact --nocapture`
- *Expected:* before the fix, at least one command reproduces the reviewed failure; after the fix, both pass.
- *BDD scenarios covered:* Given default test env, release-gate and summarize config tests must not require user credentials or hang.

**Confidence:** 0 / 90 · **Depends on:** none · **Closes:** DoD-1, DoD-2

**Evidence (required before tick; append-only)**
- *(none yet)*

### T2 · Isolate summarize config and fix mock HTTP reads · [ ]

**Steps**
- [ ] Ensure release-gate summarize calls use deterministic mock config/env and do not inherit host `summarize.apiKeyEnv`.
- [ ] Replace `read_to_end` mock-server reads with request parsing that stops at headers/body `Content-Length`.
- [ ] Keep optional external-provider smoke gated behind its explicit env var.
- [ ] Reuse existing mock helpers if possible.

**Verification Contract**
- *Check:* targeted release-gate and summarize-config tests pass without external credentials and without hangs.
- *Method:* `env -u CLI_PROXY_API_KEY -u OPENAI_API_KEY -u MMR_CONFIG_FILE cargo test --test memory_fabric_contract mvp_release_gate_e2e_fixture_scenario -- --exact --nocapture && cargo test --test memory_fabric_contract summarize_config_api_key_contract_is_implemented -- --exact --nocapture`
- *Expected:* exit 0.
- *BDD scenarios covered:* Given no external API key, mocked summarize still succeeds; given a mock server, the test responds after one request body instead of waiting for client disconnect.

**Confidence:** 0 / 90 · **Depends on:** T1 · **Closes:** DoD-1, DoD-2, DoD-4

**Evidence (required before tick; append-only)**
- *(none yet)*

### T3 · Restore the default test gate · [ ]

**Steps**
- [ ] Run default `cargo test` with external key env vars removed.
- [ ] Run the benchmark, clippy, and release build gates.
- [ ] Update any docs/goal notes that mention this blocker if they become stale.

**Verification Contract**
- *Check:* default and full verification gates are useful again for downstream goals.
- *Method:* `env -u CLI_PROXY_API_KEY -u OPENAI_API_KEY cargo test && cargo fmt --check && cargo test --test cli_benchmark -- --ignored --nocapture && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release`
- *Expected:* exit 0 for all commands.
- *BDD scenarios covered:* Given a fresh agent without private API keys, when it runs the documented verification loop, then the suite completes.

**Confidence:** 0 / 90 · **Depends on:** T2 · **Closes:** DoD-3, DoD-5

**Evidence (required before tick; append-only)**
- *(none yet)*

---

## 6. Decisions · LIVE (append-only)

- 2026-07-01 — Adversarial self-review: this goal must not paper over release-gate coverage by ignoring the scenario. DoD requires the scenario to pass without credentials. Scope impact: none.

---

## 7. Learnings · LIVE (append-only)

*(none yet)*

---

## 8. Skills · LIVE (append-only)

*(none yet)*
