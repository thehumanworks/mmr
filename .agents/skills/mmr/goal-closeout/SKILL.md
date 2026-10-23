---
name: goal-closeout
description: >-
  Closes mmr goal execution with targeted tests, the full verification loop,
  goal evidence updates, diff checks, and scoped commits when appropriate.
  Use when finishing a goals/*.md delivery, marking status done, or before
  commit/push after goal work.
---

# mmr Goal Closeout

Finish goal-driven work on `mmr` with evidence the repo can trust.

## When to Use

- A `goals/*.md` task or full goal is ready to close.
- Implementation is done and you need the mandatory verification sequence.
- You must decide whether to commit, and keep the goal doc truthful.

## Closeout Checklist

```
Progress:
- [ ] Targeted tests for the changed surface pass
- [ ] Full verification loop passes
- [ ] Goal evidence and status updated
- [ ] git diff --check clean
- [ ] Scoped commit (only if user asked or repo policy allows)
```

## 1. Run Targeted Tests First

Match tests to the touched surface before the full loop:

```bash
# CLI contract changes
cargo test --test cli_contract <test_prefix>_ -- --nocapture

# Memory fabric / retrieval
cargo test --test memory_fabric_contract <test_prefix>_ -- --nocapture

# MCP surface
cargo test --test mcp_contract <test_prefix>_ -- --nocapture
```

Fix failures before proceeding.

## 2. Full Verification Loop

Required sequence (see `.cursor/rules/verification-loop.mdc`):

```bash
cargo fmt --check
cargo test --locked
cargo test --locked --test cli_benchmark -- --ignored --nocapture
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --locked --release
```

Report concrete command output on failure; do not claim success without running it.

## 3. Update Goal Evidence

Append-only evidence under the closing task:

- Date (`YYYY-MM-DD`)
- Command(s) run (inline code)
- Outcome (exit code, pass counts, artifact path)

Tick DoD items only after their `verify by:` commands pass. Tick tasks only at Confidence ≥ `confidence_floor`.

### Evidence checklist

Before setting `status: done`, review the goal against this checklist:

- Frontmatter has `title`, `description`, `date`, and a valid `status`.
- Outcome, scope, validation plan and definition of done are explicit. Each
  completed task/DoD item has dated evidence with the exact command, result and
  relevant log or artifact; do not infer success from a checked box.
- Product verification above and any goal-specific checks passed on the final
  code. Record the tested commit or working tree and any later documentation-only
  changes. Record failures and resolutions; list skipped/unavailable checks and
  unresolved issues explicitly. Required product checks cannot be waived by a
  documentation review.
- Confidence meets the goal's floor when specified; requested independent review
  is complete. Record publication/merge state separately from local verification.
- No unchecked required work or unresolved blocker remains. Then update `status`
  and `updated`, and cite the evidence in the handoff.

This is an evidence review using repository tools, not an automatic eligibility
calculation. A global goal skill or host-installed checker is not required.

### Runnable verification example

After the full loop above, run this from the repository root (Python 3 standard
library only). It reuses the portable example's isolated temporary homes and
cleared provider environment; no real histories or credentials are needed:

```bash
python3 examples/portable_read_only.py target/release/mmr
git diff --check
```

The example must exit 0 and print the retry-limit conversation, followed by
`Source and destination native histories unchanged (including a colliding session).`,
`Bundle cached for reading; no native apply requested.`, and
`Temporary homes removed.` The diff check should produce no output and exit 0.
Record both outcomes in the goal. This example demonstrates executable evidence;
it supplements the full loop and checks specific to the change.

## 4. Diff Check

```bash
git diff --check
git status
git diff --stat
```

Ensure only goal-scoped files changed. Unrelated edits stay unstaged.

## 5. Scoped Commit

Commit only when:

- The user explicitly asked, or
- Repo policy says commit/push after a verified goal (`AGENTS.md`), and
- The full verification loop passed with high confidence.

Use an imperative message focused on **why**. Never commit secrets or unrelated worktree changes.

## Anti-Patterns

- Ticking DoD/tasks before evidence exists.
- Running only `cargo test --test cli_contract` and skipping clippy/benchmark/release.
- Marking `done` without reviewing the evidence checklist.
- Committing when the user said not to.

## Related

- Parent: `.agents/skills/mmr/SKILL.md`
- `AGENTS.md` — repository-local goal structure and approval model
- `.cursor/rules/verification-loop.mdc`
- Example closeout: `goals/2026-07-02-remove-python-bootstrap-mcp.md`
