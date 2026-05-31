# mmr Memory Fabric Release Gate

Status: implemented through NHL-281.

This document records the local release gate for the lean Memory Fabric MVP.
It complements the quickstart by naming the exact fixture-backed proof expected
before handoff.

## Local Gate

The required local gate is fully offline and uses temp `HOME`, temp
`XDG_DATA_HOME`, and the file-backed fake GitHub remote.

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

The end-to-end release scenario is covered by
`mvp_release_gate_e2e_fixture_scenario` in
`tests/memory_fabric_contract.rs`.

It proves, from a clean non-Git temp directory, that:

- `mmr init` creates the local store, links the project, imports Codex/Claude/
  Cursor fixture history, reconciles the fake remote, rebuilds search documents,
  and prints status JSON.
- `mmr note` adds safe and unsafe human-authored events.
- `mmr list projects`, `mmr list sessions`, `mmr read project`,
  `mmr read session`, and `mmr read source` retrieve raw transcript history.
- `mmr find` finds stored evidence with stable `mmr://event/...`
  citations.
- `mmr summarize project`, `mmr summarize source`, and `mmr summarize session`
  use the stateless continuity runner through a mock OpenAI-compatible Chat
  Completions endpoint.
- `mmr redact scan` and `mmr sync --dry-run` block fake secrets from both
  imported source history and human notes without printing the secret, and sync
  redacts PII before remote payloads are written.
- `mmr assimilate project` returns a prompt/runbook handoff with evidence-linked refs and
  does not run a provider or write learned memory.
- `mmr sync` uploads safe redacted projections and any active learned memory
  created through store-level paths while blocking unsafe content.
- A second empty local HOME/store can `mmr init` against the fake remote and
  hydrate usable events, search documents, and evidence refs that still work in
  a fresh `mmr assimilate project`.

## Optional External Smoke

External provider checks are intentionally opt-in. The MVP release gate does not
require external network credentials.

Summary provider examples:

```bash
OPENAI_API_KEY=<key> MMR_SUMMARISER_MODEL=gpt-4o-mini mmr summarize project
OPENAI_API_KEY=<key> OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
  MMR_SUMMARISER_MODEL=openai/gpt-4o-mini mmr summarize project
```

The automated optional external summary smoke is gated by an explicit flag:

```bash
MMR_RUN_EXTERNAL_SUMMARY_SMOKE=1 OPENAI_API_KEY=<key> \
  cargo test --test memory_fabric_contract optional_external_summary_provider_smoke_is_gated -- --nocapture
```

Dream handoff example:

```bash
mmr assimilate project --pretty
```

The automated optional assimilation handoff smoke is gated separately:

```bash
MMR_RUN_EXTERNAL_DREAM_SMOKE=1 \
  cargo test --test memory_fabric_contract optional_external_dream_command_smoke_is_gated -- --nocapture
```

The built-in release gate uses local assimilation handoff generation and a local mock
Gemini server so CI and local development do not depend on third-party accounts.

## Known Limitations

- The GitHub-shaped remote is file-backed for deterministic MVP verification;
  live GitHub API transport remains future hardening.
- The optional `openai/privacy-filter` runtime is not bundled. Deterministic
  secret and coarse PII blocking still run before sync.
- Grok and Pi remain raw retrieval sources for the MVP; Memory Fabric importers
  currently cover Codex, Claude, and Cursor.
- There is no public `init`, `store`, `learn`, `context`, `candidates`,
  `knowledge`, `promote`, or `reject` command.
- The MVP happy path does not support GitHub organizations.
- The MVP happy path does not expose an explicit remote repository argument; it
  uses `github:<user>/mmr-store`.
- Link/sync do not perform destructive cleanup by default.
