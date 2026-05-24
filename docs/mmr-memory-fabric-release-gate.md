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

- `mmr link` creates the local store, links the project, imports Codex/Claude/
  Cursor fixture history, reconciles the fake remote, rebuilds search documents,
  and prints status JSON.
- `mmr note` adds safe and unsafe human-authored events.
- `projects`, `sessions`, `messages`, and `export` still retrieve raw transcript
  history.
- `mmr rg` and `mmr search` find stored evidence with stable `mmr://event/...`
  citations.
- `mmr summary` and `mmr remember` use the stateless continuity runner through a
  mock Gemini endpoint, with `remember` retained as a compatibility alias.
- `mmr redact scan` and `mmr sync --dry-run` block fake secrets from both
  imported source history and human notes without printing the secret, and sync
  redacts PII before remote payloads are written.
- `mmr dream` applies only evidence-linked learned memory.
- `mmr sync` uploads safe redacted projections and active learned memory while
  blocking unsafe content.
- A second empty local HOME/store can `mmr link` against the fake remote and
  hydrate usable events, search documents, learned memory, and evidence refs
  that still work in a fresh `mmr dream --dry-run`.

## Optional External Smoke

External provider checks are intentionally opt-in. The MVP release gate does not
require external network credentials.

Summary provider examples:

```bash
MMR_DEFAULT_REMEMBER_AGENT=gemini GOOGLE_API_KEY=<key> mmr summary --agent gemini
CURSOR_API_KEY=<key> mmr summary --agent cursor
mmr summary --agent codex
```

The automated optional Gemini smoke is gated by an explicit flag:

```bash
MMR_RUN_EXTERNAL_SUMMARY_SMOKE=1 GOOGLE_API_KEY=<key> \
  cargo test --test memory_fabric_contract optional_external_summary_provider_smoke_is_gated -- --nocapture
```

Dream command runner example:

```bash
export MMR_DEFAULT_DREAM_RUNNER=command
export MMR_DREAM_COMMAND="python ./dream_runner.py"
mmr dream --dry-run --pretty
```

The automated optional command-runner smoke is gated separately:

```bash
MMR_RUN_EXTERNAL_DREAM_SMOKE=1 MMR_DREAM_COMMAND="python ./dream_runner.py" \
  cargo test --test memory_fabric_contract optional_external_dream_command_smoke_is_gated -- --nocapture
```

The built-in release gate uses the deterministic mock dream runner and a local
mock Gemini server so CI and local development do not depend on third-party
accounts.

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
