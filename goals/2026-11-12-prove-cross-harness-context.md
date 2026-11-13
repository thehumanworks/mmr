---
goal_id: "2026-11-12-prove-cross-harness-context"
title: "Prove mmr gives a new agent the project's earlier context, whichever harness produced it"
description: "Build a repeatable evaluation in which a fresh agent answers questions about, or resumes, work done earlier in the same project by other agent harnesses; measure success, cost and stumbles; fix what blocks it; report before and after."
date: "2026-11-12"
status: "done"
confidence_floor: 80
created: "2026-11-12"
updated: "2026-11-13"
---

# Goal

Tomas (2026-11-12): "Own this codebase as yours. This has to be proven useful
for an agent looking to be aware of previous actions and context, capable of
proactively fetching that across agent harnesses because what matters is the
project context, not which agent ran."

"Proven" means evidence: an agent that knows nothing about a project starts a
session in it, and either answers a question whose answer exists only in
earlier sessions run by other harnesses (Codex, Claude Code, Cursor, Grok, Pi),
or resumes the work. We measure whether it succeeds, how many tool calls and
tokens it spends, and where it stumbles. Then we fix what the evaluation shows
is in the way, re-run, and report the before and after, including where mmr
still fails.

## Surface

- New: `evals/cross_harness/` — synthetic project, native-format history
  writers for each harness, task set with deterministic rubrics, a runner that
  drives a headless agent (Claude Code `claude -p`) in an isolated `HOME`, and a
  report generator. Results are committed under `evals/cross_harness/results/`.
- Fixes the evaluation points to. Ownership is split with the parallel
  "remove unnecessary complexity" work: this goal owns `evals/`, project
  resolution in `src/messages/service.rs` and the `src/source/*.rs` loaders;
  changes to `src/cli.rs`, `src/mcp.rs`, `README.md`, `docs/` and
  `.agents/skills/mmr/**` are coordinated with that work first.
- Out of scope: `--remote` (being removed), `summarize`/`compact` (need
  external model keys and are not part of the context fetch).

## Evaluation design

**Scenario.** One project (`ledger-svc`, a small webhook-delivery service in a
git repo) with about three weeks of history across all five harnesses, plus
noise: unrelated sessions in the same project, a sibling project with similar
vocabulary, and an older prototype with the same directory basename. Each key
fact lives in exactly one earlier session and is not recoverable from the repo
files or git log. Some sessions ran in a subdirectory or a Claude Code worktree
of the project, as real sessions do.

**Tasks.** Natural requests a user would make at the start of a session, none
of which mention mmr, history or harnesses: why a constant has its value,
resume the last session, constraints the user set earlier, whether an approach
was already tried, a detail mentioned once, and how a policy evolved. Each task
has a rubric of required facts checked by regular expressions on the final
answer, so grading is deterministic and repeatable.

**Conditions.**

| id | what the agent has |
|----|--------------------|
| `none` | no mmr (control: what the repo alone gives) |
| `path` | mmr on PATH, nothing else |
| `shipped-skill` | mmr on PATH and `mmr skill install` run, as the docs say |
| `skill` | mmr skill placed where Claude Code loads skills |
| `told` | the task prompt says mmr exists (measures use once discovered) |
| `mcp` | mmr MCP server configured, unprompted |
| `hook` | skill plus a SessionStart hook that lists recent sessions |

**Metrics per run.** Pass/fail per rubric item, tool calls (total and mmr),
the mmr commands used, input/output/cache tokens, cost, turns, wall time, and
errors or wrong turns seen in the transcript.

## Validation plan

1. Corpus self-check: `mmr` on the corpus lists the project's sessions from all
   five harnesses for the project path, and each fact is in exactly one session
   (script asserts this before any agent run).
2. Baseline: run every condition × task with 2 repetitions on the current
   `main` binary; commit raw results and the report.
3. Analyse transcripts; write findings (what blocks success, what wastes
   tokens). Share command-usage findings with the surface-cut work.
4. Fix, with tests for any code change (`cargo test`, clippy, fmt).
5. Re-run the same matrix on the fixed binary; report before/after side by
   side.

## Definition of done

- `python3 evals/cross_harness/run.py` reproduces the evaluation from a clean
  checkout given a built `mmr` and an authenticated `claude` CLI.
- Baseline and after results are committed with a report that states plainly
  where mmr passes and fails.
- Code changes pass the full verification loop.
- A PR is open for Tomas's review, not merged.

## Exit conditions

- `BLOCKED-DEP`: the agent CLI cannot run headless in the container.
- `SCOPE-CHANGE`: a needed fix collides with the surface-cut work; hand it over
  as a finding instead.

## Evidence

[evals/cross_harness/RESULTS.md](../evals/cross_harness/RESULTS.md) has the
before/after tables, the failures that remain, and the cause and fix for each
problem the evaluation found. Raw results for each run are in
`evals/cross_harness/results/<label>/results.jsonl`.

- Baseline, `main` 83f0fb5: skill 2/18, MCP 6/18, best condition 6/18, no mmr 0/18.
- After this goal and PR #32 (10383c3): skill 13/18, `mmr skill install` 14/18,
  SessionStart hook 15/18, MCP 14/18, no mmr 0/18. Haiku: skill 7/9, hook 8/9.
- Probe from the project directory: 5/10 sessions and 2/9 facts on `main`;
  10/10 and 9/9 after, with the sibling project and the old prototype still
  out of scope.
- Still failing, stated in RESULTS.md: generic requests without the hook,
  facts in a sibling project, and only Claude Code measured as the reader.
- Overfitting check: the skill's search example first used words from the
  tasks; it now uses a placeholder, and three held-out tasks were added after
  the skill text was settled.

