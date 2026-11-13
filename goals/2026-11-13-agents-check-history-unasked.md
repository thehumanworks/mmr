---
goal_id: "2026-11-13-agents-check-history-unasked"
title: "Agents check earlier sessions unasked, in more than one harness"
description: "Close the gaps the first cross-harness evaluation left: measure a reader other than Claude Code, make agents look at history for requests that do not sound like history questions, and fix the remaining failing runs."
date: "2026-11-13"
status: "in-progress"
created: "2026-11-13"
updated: "2026-11-13"
---

# Goal

Follow-up to [2026-11-12-prove-cross-harness-context](2026-11-12-prove-cross-harness-context.md).
After PR #33, agents with the skill found earlier-session facts in 13-15 of 18
tasks, but the coordinator (2026-11-13, relaying Tomas's goal of proactive
fetching across harnesses) named three gaps:

1. Only Claude Code was measured as the reading agent. Measure at least Codex.
2. Agents with only the skill or MCP skipped history for requests that do not
   sound like history questions; only a Claude Code SessionStart hook helped.
   Close this without depending on one harness's hook, or make the mechanism
   the default install for each harness that supports it.
3. Fix the remaining failing runs. Sibling-project search is optional.

## Surface

- `evals/cross_harness/`: proactive tasks (requests to change code whose
  answer sits past the first prompt of an earlier session), an `instructions`
  condition, and a Codex runner.
- `mmr skill install` (`src/cli.rs`, `src/agent_instructions.md`): also adds
  one marked paragraph to `~/.claude/CLAUDE.md` and `~/.codex/AGENTS.md` for
  agents already set up on the machine; `--no-instructions` opts out.
- README section "Let Your Agents Use It".

## Validation plan

1. Proactive tasks with Claude Code: none, skill, hook, global-instructions
   paragraph, 2 repetitions each.
2. The winning mechanism on the original nine tasks, to check it does not hurt.
3. The same conditions with Codex as the reader, once a model and endpoint are
   approved. Proposed: Codex 0.167 with its default model (GPT-6.1-Sol) through
   the Vercel AI Gateway, with the model's entry from Codex's bundled catalog so
   Codex sends its native prompt and tools. The runner was checked offline
   against a local stand-in server: Codex sees the skill, the AGENTS.md
   paragraph, the hook output and the MCP server.
4. Contract tests for the install change; fmt, clippy, tests, release build,
   `nix build`.

## Definition of done

- Results for Claude Code and Codex readers committed with a report that
  states where mmr still fails.
- `mmr skill install` sets up the mechanism that the evaluation found works.
- PR merged after CI passes.

## Decisions

- Sibling-project search is left out: it is outside "the project's context",
  and searching every project by default would pull in unrelated projects
  (the eval's old prototype with the same directory name is one). The skill
  already documents `--all`.

## Exit conditions

- `BLOCKED-DEP`: no approved way to run Codex headless. Report Claude Code
  results and leave the Codex measurement as an open gap.

## Evidence so far

[evals/cross_harness/RESULTS.md](../evals/cross_harness/RESULTS.md), section
2026-11-13.

- Claude Code, build bbb65e5, 16 tasks x 2: `mmr skill install` (skill plus
  paragraph) 29/32 and 15/16 on change requests; skill alone 19/32 and 5/16;
  hook 23/32; MCP 26/32; no mmr 0/32.
- The first paragraph wording missed one-line changes (timeout 0/2). The
  revised wording, skill and MCP instructions pass it 2/2 and three tasks
  written afterwards 6/6.
- Still failing: the sibling-project task (out of scope, see Decisions) and
  one run where the agent read a rule and broke it.
- Codex runner built (`--agent codex`) and checked offline against a local
  stand-in server. Codex measurement waits on approval of model and endpoint;
  until then the goal stays in progress.
