---
name: docs-first-contract-change
description: >-
  Changes mmr CLI contracts docs-first: update specs/*.md, then src/cli.rs,
  then cli_contract and memory_fabric_contract tests, with live smoke and
  preserved next_command continuation semantics. Use when adding flags,
  changing JSON response shapes, or shipping retrieve/read/recall contract work.
---

# mmr Docs-First Contract Change

Ship CLI behavior from a frozen spec, not ad-hoc code edits.

## When to Use

- Adding or changing public CLI flags or JSON fields
- Retrieve, read, recall, find, or messages contract work
- Any change agents/scripts depend on via stdout JSON

## Workflow Checklist

```
Progress:
- [ ] Goal doc records frozen contract
- [ ] specs/*.md updated first
- [ ] src/cli.rs implements spec
- [ ] tests/cli_contract.rs updated
- [ ] tests/memory_fabric_contract.rs updated (if applicable)
- [ ] Live smoke on real/fixture history
- [ ] next_command continuation preserved
- [ ] Full verification loop
```

## 1. Freeze Contract in Goal + Spec

Write or update `goals/<date>-<slug>.md` with:

- Default vs opt-in output fields
- Flag names and JSON shape
- Pagination/continuation rules
- Non-goals (what must not change)

Then update the canonical spec **before** code:

| Surface | Spec file |
|---------|-----------|
| Retrieve | `specs/retrieval.md` |
| Messages/read/recall | `specs/messages.md` |
| Session sharing | `specs/session-sharing.md` |

Reference: `goals/2026-06-28-docs-first-retrieve-implementation.md`

## 2. Implement in src/cli.rs

- Use clap derive patterns (`mmr-clap-colored-cli` skill)
- Keep stdout JSON-only; diagnostics on stderr
- Wire response builders to match the spec field-for-field

## 3. Lock with Contract Tests

Primary gates:

```bash
cargo test --test cli_contract <prefix>_ -- --nocapture
cargo test --test memory_fabric_contract <prefix>_ -- --nocapture
```

Add fixture-driven tests covering defaults, opt-in flags, filters, pagination, and errors.

## 4. Preserve next_command Continuation

Paged commands must emit executable continuations that preserve caller intent:

- `--limit` / `--offset` or pinned session identity
- Scope flags (`--project`, `--source`, `--remote`, etc.)
- Mode flags the caller opted into

### Example: retrieve output modes

From `goals/2026-06-28-retrieve-debug-and-snippet-output.md`:

- Default JSON: concise matches/snippets; no debug metadata; no full `messages[]`
- `--debug`: include execution metadata (searched projects, scope details)
- `--full-message-history`: include provider message windows in `selected_sessions[].messages`
- `next_command` must carry `--debug` and `--full-message-history` through pagination

Tests to preserve:

```bash
cargo test --test cli_contract retrieve_next_command_preserves_debug_and_full_message_history -- --exact --nocapture
cargo test --test cli_contract retrieve_pinned_next_command_executes_as_printed_and_freezes_sessions -- --exact --nocapture
```

## 5. Live Smoke + Verification

```bash
cargo run -- retrieve "test query" --limit 5
cargo run -- retrieve "test query" --debug --full-message-history --limit 1
cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings && cargo build --release
```

Confirm JSON parses, `next_command` executes, then run the full loop.

## Anti-Patterns

- Code-first drift from `specs/*.md`; dropping opt-in flags on page-2 `next_command`.
- Testing only defaults; changing `find` while implementing `retrieve` unless spec says so.

## Related

- `.agents/skills/mmr/SKILL.md`, `mmr-clap-colored-cli`, `.cursor/rules/cli-contract.mdc`
- `specs/retrieval.md`, `goals/2026-06-28-retrieve-debug-and-snippet-output.md`
