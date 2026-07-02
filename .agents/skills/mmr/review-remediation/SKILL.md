---
name: review-remediation
description: >-
  Turns deep mmr code reviews into GDD goal backlogs, implements fixes in
  parallel git worktrees per concern, merges to main, and verifies on merged
  main. Use after a deep project review, when executing review-finding goals,
  or when batch-remediating P0–P2 findings with isolated branches.
---

# mmr Review Remediation

Repeatable pipeline from review findings to verified fixes on local `main`.
Pattern from session `019f1c29` (deep review → goal backlog → parallel worktree execution).

## When to Use

- A deep review produced actionable P0–P2 findings.
- Multiple independent defects should ship without one giant branch.
- You need merge-safe integration with post-merge fixture fixes.

## Pipeline Checklist

```
Progress:
- [ ] Deep review complete with evidence-backed findings
- [ ] Findings converted to executable goal docs
- [ ] One branch/worktree per concern
- [ ] Subagents implement with targeted tests first
- [ ] Branches merged to local main
- [ ] Post-merge fixture/integration fixes applied
- [ ] Full verification loop on merged main
```

## 1. Deep Review (Read-Only)

Produce findings with severity, file/line evidence, fix direction, and validation plan.
Do not fix in the same pass unless the user asks.

Reference: `goals/2026-07-01-deep-project-review.md`

## 2. Findings → Goal Docs

Convert each final finding into one `goals/<date>-<slug>.md` GDD contract:

- Atomic DoD with `verify by:` commands
- Ordered tasks with verification contracts
- Exact source files and tests referenced

Validate structure:

```bash
GDD_SKILL="$HOME/.agents/skills/goal-driven-development"
for f in goals/<batch>-*.md; do
  python "$GDD_SKILL/scripts/gdd_status.py" --author "$f" >/tmp/gdd-check.json || exit 1
done
```

Record recommended execution order (test-gate stability first, then security/data, then contract/perf).

Reference: `goals/2026-07-01-review-findings-goal-backlog.md`

## 3. Parallel Worktrees per Concern

One isolated slice per goal or concern area:

```bash
git worktree add ../mmr-review-<slice> -b review/<slice>
```

Assign bounded goals to specialized subagents (see `.codex/agents/mmr_*.toml`):

- Work only in the assigned worktree
- Add fixture-driven tests before or with the fix
- Smallest change in `src/cli.rs`, `src/sync.rs`, `src/teleport/**`, etc.
- Return branch, changed files, verification outcomes, merge notes

## 4. Branch Implement → Merge to Main

Per slice:

1. Targeted tests for the goal's surface
2. Implement minimal fix
3. Run goal verification contracts
4. Merge completed branch into local `main`

Parent goal tracks integration: `goals/2026-07-01-execute-review-findings.md`

## 5. Post-Merge Fixture Fixes

After merges, expect cross-cutting test fallout:

- Shared fixtures in `tests/common/mod.rs`
- CLI contract tests that assumed pre-fix behavior
- Shell-execution tests for `next_command` continuations

Fix on `main`, not in stale worktrees.

## 6. Merged-Main Verification

On local `main` after all merges:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Append evidence to the execute-review goal; mark child goals `done` via `goal-closeout`.

## Anti-Patterns

- One mega-branch for unrelated findings; merging without targeted tests in the worktree.
- Leaving worktrees open after merge; skipping post-merge fixture fixes on `main`.

## Related

- `.agents/skills/mmr/SKILL.md`, `goal-closeout`, `.codex/agents/mmr_*_fixer.toml`
