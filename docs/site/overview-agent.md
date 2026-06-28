# mmr Retrieval Docs Overview

view: Agent

Use this site as the implementation contract before editing product code.

Read order:

1. `.well-known/agents.json`
2. `/agent/retrieve.md`
3. `specs/retrieval.md`
4. `goals/2026-06-28-retrieve-all-scope-flags.md`
5. Project instructions in `AGENTS.md` and `.cursor/rules/`

Implementation rules:

- If code and docs disagree, patch docs first, then code.
- If `specs/retrieval.md` contradicts a goal, update docs first rather than
  silently choosing one.
- Keep first-class MCP tooling out of v1; `mmr retrieve` is a CLI contract.
- Preserve existing `find` behavior and JSON stdout conventions.

Primary proof:

```bash
cargo test --test memory_fabric_contract retrieve_ -- --nocapture
cargo test --test cli_contract retrieve_ -- --nocapture
cargo test --test memory_fabric_contract rg_cli_contract_is_implemented -- --exact
cargo test --test memory_fabric_contract search_cli_contract_is_implemented -- --exact
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
