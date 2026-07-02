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
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Report concrete command output on failure; do not claim success without running it.

## 3. Update Goal Evidence

Append-only evidence under the closing task:

- Date (`YYYY-MM-DD`)
- Command(s) run (inline code)
- Outcome (exit code, pass counts, artifact path)

Tick DoD items only after their `verify by:` commands pass. Tick tasks only at Confidence ≥ `confidence_floor`.

### gdd_status.py gate

Before setting frontmatter `status: done`, confirm DONE eligibility:

```bash
GDD_SKILL="$HOME/.agents/skills/goal-driven-development"
python "$GDD_SKILL/scripts/gdd_status.py" goals/<goal-file>.md | jq '.done_eligible, .violations'
```

Use `--author` when validating newly authored goal structure. Do not mark `done` while `done_eligible` is false or violations remain.

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
- Marking `done` without checking `gdd_status.py`.
- Committing when the user said not to.

## Related

- Parent: `.agents/skills/mmr/SKILL.md`
- `goal-driven-development` — goal doc structure and `gdd_status.py`
- `.cursor/rules/verification-loop.mdc`
- Example closeout: `goals/2026-07-02-remove-python-bootstrap-mcp.md`
