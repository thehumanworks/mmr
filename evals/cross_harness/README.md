# Cross-harness context evaluation

Latest results and what they changed: [RESULTS.md](RESULTS.md).

Does mmr let an agent that knows nothing about a project find out what earlier
agents did there, whichever agent harness they ran in? This evaluation answers
that with headless agent runs instead of argument.

## What it does

1. **Builds a world** in a throwaway `HOME`: a small git repo (`src/ledger-svc`,
   a webhook-delivery service) and three weeks of earlier sessions written in
   each harness's real on-disk format (Codex 0.164 including a zstd-archived
   session, Claude Code 2.1 including a worktree session, Cursor, Grok from a
   subdirectory, Pi), plus noise: unrelated sessions in the same project, a
   sibling project with similar words, and an old prototype with the same
   directory name. Formats follow what these harnesses wrote on a real machine
   in November 2026 (`writers.py` documents them).
2. **Checks the corpus** (`--selfcheck`): each key fact is in exactly one
   session and nowhere in the repo or git history; then probes, without any
   agent, which sessions and facts mmr returns from the project directory.
3. **Runs an agent** (Claude Code `claude -p`, or Codex `codex exec` with
   `--agent codex`, in an isolated environment) in the project with one
   natural request per task. No prompt mentions mmr, history or other agents,
   except in the `told` condition.
4. **Grades** the final answer with regular expressions for facts that only the
   earlier sessions hold (`scenario.TASKS`), and records tool calls, mmr calls
   and errors, tokens, cost and time from the transcript.

## Tasks

| id | request | the answer lives in |
|----|---------|---------------------|
| `why-retries` | Why is MAX_RETRIES 3? | Claude Code session |
| `resume` | Pick up where we left off | Pi session, the newest before the agent's own |
| `constraints` | Did I set rules for delivery.py? | Codex session, user message |
| `redis` | Would Redis fit? Did we look at it? | Claude Code session in `.claude/worktrees/` |
| `staging-url` | Which staging endpoint did we use? | Grok session started in `scripts/` |
| `retry-history` | How did the retry policy change, and why? | archived Codex + Claude Code + Cursor |
| `deploy` | About to deploy, anything to watch out for? | Codex session, an aside in a user message (held out) |
| `flaky` | Are any tests known to be flaky? | Cursor session (held out) |
| `dashboard` | How many times does the ledger dashboard retry? | sibling project `ledger-ui`, Codex session (held out) |
| `signature` | Make deliver() take a config dataclass | Codex session: billing-svc imports deliver(), keep its signature (proactive) |
| `add-dep` | Swap the retry loop for the tenacity library | Codex session: no new dependencies without asking (proactive) |
| `redis-move` | Move the dedupe keys to Redis | Claude Code worktree session: on-prem installs have no Redis (proactive) |
| `timeout` | Set TIMEOUT_S back to 10 | Claude Code session: 15 s was set after duplicate charges at Acme (proactive) |

The held-out tasks were written after the skill text was settled, to check
that it was not tuned to the first six. The proactive tasks (and `deploy`)
ask for a change and do not sound like a question about the past. In the four
new ones, the fact that should change the agent's plan sits past the start of
its session, so the preview in `mmr list sessions` does not show it.

## Conditions

| id | the agent has |
|----|---------------|
| `none` | no mmr (control) |
| `path` | mmr on PATH only |
| `shipped-skill` | mmr on PATH, then `mmr skill install` as the README says |
| `skill` | the bundled skill in `~/.claude/skills/mmr` |
| `told` | the prompt says mmr exists |
| `mcp` | mmr's MCP server configured |
| `hook` | skill, plus a SessionStart hook that prints `mmr list sessions --limit 8` |
| `instructions` | skill, plus the paragraph from `src/agent_instructions.md` in the agent's global instructions (`~/.claude/CLAUDE.md` or `~/.codex/AGENTS.md`) |

`shipped-skill` installs whatever the binary's `mmr skill install` installs.
Since the paragraph became part of it, `shipped-skill` is `instructions` set up
by mmr itself.

With `--agent codex`, the skill goes to `~/.agents/skills`, the hook to
`~/.codex/hooks.json` (run with `--dangerously-bypass-hook-trust`), the
paragraph to `~/.codex/AGENTS.md` and the MCP server to `~/.codex/config.toml`.

## Run it

Needs a built mmr, an authenticated `claude` CLI, `git`, Python 3, and zstd
(the `zstd` CLI or `pip install zstandard`) for the archived Codex session.

```bash
cargo build --release
python3 evals/cross_harness/run.py --selfcheck            # corpus check + mmr probe, no agent
python3 evals/cross_harness/run.py --label mine --reps 2  # all tasks x conditions
python3 evals/cross_harness/run.py --label quick --conditions skill,told --tasks resume --reps 1
python3 evals/cross_harness/run.py --regrade evals/cross_harness/results/mine  # after editing rubrics
```

`--model` picks the agent model (default `claude-sonnet-5-5`), `--jobs` the
parallelism, `--keep` keeps sandboxes. A Claude Code run of 16 tasks x 5
conditions x 2 repetitions (160 runs) takes about 25 minutes at 7 jobs and costs
about $10 with the default model. Results land in `results/<label>/`:
`results.jsonl` (one row per run, with the final answer and every tool step),
`report.md`, `probe.json` and every transcript (transcripts are not committed).

### Codex as the reader

`--agent codex` runs `codex exec` (tested with Codex 0.167) in the same
throwaway `HOME`, with `CODEX_HOME` inside it. Subagents, web search,
analytics, and the apps, plugins and browser tools a signed-in account brings
are off. Each row records the model Codex actually used, read from its own
session file. There are two ways to reach a model:

```bash
# With your own Codex sign-in (ChatGPT plan), on a real machine:
python3 evals/cross_harness/run.py --agent codex --codex-login --codex-sandbox \
  --conditions none,skill,shipped-skill,mcp --label codex --jobs 4

# Through an OpenAI-compatible gateway (default: Vercel AI Gateway):
AI_GATEWAY_API_KEY=... python3 evals/cross_harness/run.py --agent codex --label codex-gw
```

- `--codex-login` links `~/.codex/auth.json` into each throwaway `CODEX_HOME`
  and leaves the model to Codex's default. If Codex refreshes the tokens by
  replacing the link, the runner copies them back. Codex writes its sessions
  and settings to the throwaway `CODEX_HOME`, not to your `~/.codex`.
- `--codex-sandbox` keeps Codex's `workspace-write` sandbox, so the agent's
  commands can write only inside the throwaway project (and temp folders).
  Without it the runner bypasses the sandbox, which is what works inside a
  container.
- Through a gateway, the default model is `openai/gpt-6.1-sol`. Codex knows
  models only by its own names, so the runner copies the model's entry from
  Codex's bundled catalog (`codex debug models --bundled`) under the gateway's
  name; Codex then sends its native prompt and tools. `--base-url` picks
  another endpoint.

## Limits

- Two readers are measured, Claude Code and Codex. Cursor, Grok and Pi agents
  are not. Codex runs used a ChatGPT plan, so they report no cost.
- One synthetic project. Real histories are longer and noisier; the probe
  numbers say whether a fact is reachable, not how hard it is to find in a
  year of sessions.
- Rubrics check for facts in the answer, not the quality of the reasoning, and
  a model can sometimes guess a generic reason; the `none` control shows how
  often that happens.
