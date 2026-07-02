---
name: command-surface-removal
description: >-
  Removes public mmr CLI commands safely: records no-backwards-compat in a goal,
  deletes clap/tests/docs/scripts surface, adds rejection contract tests, greps
  active references, cleans dead code, and runs verification. Use when deleting
  or consolidating commands like context, python-bootstrap, or deprecated aliases.
---

# mmr Command Surface Removal

Remove public CLI commands cleanly — no hidden compatibility shims unless the user explicitly requests them.

## When to Use

- A command duplicates another surface (`context` vs `read`/`summarize`/`assimilate`)
- User chose no backwards compatibility
- Deprecation period is over and the command must disappear from help

## Removal Checklist

```
Progress:
- [ ] Goal records no-backwards-compat decision
- [ ] clap surface removed from src/cli.rs (and src/mcp.rs if exposed)
- [ ] Rejection contract test added
- [ ] Active docs/scripts/tests scrubbed
- [ ] grep confirms no live product references
- [ ] Dead code and unused imports removed
- [ ] Full verification loop
```

## 1. Goal with No-Backwards-Compat

Create `goals/<date>-remove-<command>.md` recording no alias/deprecate/hide, active vs historical surface, and DoD for help omission, clap rejection, grep clean, verification. See `goals/2026-07-02-remove-python-bootstrap-mcp.md`. Remove when the command duplicates `read`/`summarize`/`assimilate` (audit: `goals/2026-06-01-consolidate-context-command-noise.md`).

## 2. Remove clap / Tests / Docs / Scripts

Typical touch points:

| Layer | Location |
|-------|----------|
| CLI parser | `src/cli.rs`, `src/mcp.rs` |
| Handlers | command-specific modules |
| Contract tests | `tests/cli_contract.rs`, `tests/mcp_contract.rs` |
| Docs | `docs/`, `README.md`, `AGENTS.md`, skills, OpenAPI if applicable |
| Scripts | `scripts/` helpers that invoke the removed command |

Delete implementation code, not just hide from help.

## 3. Rejection Contract Test

Assert clap rejects the removed subcommand (stderr: unknown/unrecognized subcommand):

```bash
cargo test --test cli_contract <command>_subcommand_is_removed -- --nocapture
cargo test --test mcp_contract mcp_<command>_subcommand_is_removed -- --nocapture
cargo run -- --help && cargo run -- <parent> --help
```

## 4. Grep Active Surface

Search live product files; **exclude** completed historical goals and intentional rejection-test strings:

```bash
rg -n "<command-symbol>|<OldArgsType>|<script-name>" \
  src tests scripts docs/ README.md Cargo.toml AGENTS.md .agents/skills
```

Re-run excluding known-safe hits:

- `goals/` entries documenting past work (task history, not product surface)
- Test names/assertions that prove rejection (`*_subcommand_is_removed`)

Active hits must go to zero before closeout.

## 5. Dead Code + Verification

Drop unused types, MCP registrations, and orphaned fixtures in `tests/common/mod.rs`. Then:

```bash
cargo test --test cli_contract <removal_test> -- --nocapture
cargo fmt && cargo test && cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings && cargo build --release
```

Close with `goal-closeout` (`gdd_status.py` DONE gate).

## Anti-Patterns

- Hidden compatibility shims; scrubbing historical goal docs; MCP/docs left behind; no rejection tests.

## Related

- `.agents/skills/mmr/SKILL.md`, `goal-closeout`, `docs-first-contract-change`
- `goals/2026-07-02-remove-python-bootstrap-mcp.md`
