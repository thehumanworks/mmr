# Cross-harness context eval: results, 2026-11-12

**Result.** On `main` before this work, an agent that had mmr's bundled skill
found the answer in 2 of 18 tasks whose answers live only in earlier sessions.
With this round's fixes (this PR and #32) it finds 13 of 18, 14 of 18 when the
skill comes from `mmr skill install`, and 15 of 18 with a SessionStart hook.
The MCP server alone goes from 6 to 14. Without mmr an agent finds none. A
smaller model (Haiku, one repetition per task) finds 7 of 9 with the skill and
8 of 9 with the hook.

**Where mmr still fails.**

- **Requests that do not sound like history questions.** "I'm about to deploy
  this, anything to watch out for?" The answer (no deploys 17:00-19:00 CET, the
  partner's settlement batch) is in an earlier Codex session. With the skill
  or MCP, the agent reviewed the diff and never checked history (0 of 2). Only
  the SessionStart hook, which puts the session list in front of the agent,
  made it look (2 of 2).
- **Other projects.** "How many times does the ledger dashboard retry?" The
  answer is in a sibling project, `ledger-ui`. No condition found it (0 of 2
  everywhere): agents answer from the current repo and do not search beyond
  the project.
- **MCP depends on the server's instructions.** Claude Code loads MCP tools on
  demand. With the earlier instructions (build 467e46e), an agent asked "why is
  MAX_RETRIES 3?" answered from git and never called mmr (0 of 2). PR #32's
  instructions say when to check earlier sessions, and the same question now
  passes (2 of 2). A client that ignores server instructions will likely
  behave like the old build.
- **Only Claude Code reads.** The histories come from five harnesses, but the
  reading agent is always Claude Code. Whether Codex or Cursor agents reach for
  mmr unprompted is not measured.
- **One synthetic project.** Real histories are longer and noisier. The probe
  numbers say a fact is reachable, not how hard it is to find in a year of
  sessions.

## Pass rate

Each cell counts tasks where every rubric item was in the final answer, over 9
tasks x 2 repetitions. Model `claude-sonnet-5-5`, Claude Code 2.1.323.

| condition | what the agent has | main (83f0fb5) | this round |
|---|---|---|---|
| `none` | no mmr | 0/18 | 0/18 |
| `path` | mmr on PATH, nothing else | 1/18 | 0/18 |
| `shipped-skill` | `mmr skill install`, as the README now says | 1/18 | 14/18 |
| `skill` | the skill in `~/.claude/skills/mmr` | 2/18 | 13/18 |
| `told` | the prompt says mmr exists | 5/18 | 14/18 |
| `mcp` | mmr's MCP server, unprompted | 6/18 | 14/18 |
| `hook` | skill plus a SessionStart hook running `mmr list sessions --limit 8` | 4/18 | 15/18 |

Per task, this round (10383c3, which is `main` with PR #32 merged plus this PR):

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | all |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **0/18** |
| path | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **0/18** |
| shipped-skill | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **14/18** |
| skill | 1/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **13/18** |
| told | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **14/18** |
| mcp | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **14/18** |
| hook | 1/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | **15/18** |

Full reports: [baseline-main](results/baseline-main/report.md),
[merged](results/merged/report.md),
[merged, Haiku](results/merged-haiku/report.md). Each `results.jsonl` row
holds the final answer and every tool step.

## Cost

With the skill, a run makes a median of 3 tool calls, 1 mmr call and 6k
characters of mmr output, and costs $0.06 in 11 seconds. Without mmr: 2 tool
calls, $0.04. On `main`, the `told` condition, where the agent knew mmr existed
and kept trying, took a median of 6 tool calls and $0.10, with 10 failed mmr
calls across its runs.

## What was in the way, and what changed

The probe runs mmr from the project directory with no agent. On `main` it
listed 5 of the project's 10 sessions and returned 2 of its 9 facts. Now it
lists 10 of 10 and returns 9 of 9. The sibling project and an old prototype
with the same directory name stay out of scope in both.

| problem on `main` | evidence | fix |
|---|---|---|
| Sessions from a Claude Code worktree (`.claude/worktrees/x`), a Grok session started in `scripts/`, and Cursor sessions were not in the project | probe: 5/10 sessions | project scope includes nested directories and worktrees; Cursor's folder name (no leading dash) is decoded to the project path |
| Codex 0.164 writes user prompts as `item_completed` `UserMessage` events, which mmr skipped; archived Codex sessions (`.jsonl.zst`) were not read | the user's own rules in Codex sessions were invisible | both read; zstd through the pure-Rust `ruzstd` crate |
| Cursor transcripts have no timestamps; the time is a `<timestamp>` tag inside the user text, which is also wrapped in `<user_query>` | Cursor sessions sorted first and quoted tags | time taken from the tag, inherited by the turn's other messages; tags stripped |
| `find` needed `mmr init` and a store; agents called it first and it failed every time | 43 `find` calls on `main`; every one that searched failed with "project is not linked; run `mmr init`" | `find` reads raw history with no setup (PR #32) |
| `mmr skill install` wrote only `~/.agents/skills`, where Claude Code does not look, and the README never mentioned it | `shipped-skill`: 1/18 | installs to `~/.claude/skills` too; README section "Let Your Agents Use It" |
| The skill described commands, not when to use them | `skill`: 2/18, agent made no mmr call in 10 of 18 runs | description names the triggers: starting or resuming work, "why is it like this?", "did we try X?" |
| `read session` with a shortened id returned an empty page and exit 0 | an agent read `1ee02b7c` and told the user the session "has no recorded messages" | a unique prefix of 4+ characters resolves; an unknown id is an error |

## Method

`python3 evals/cross_harness/run.py` builds a throwaway `HOME` with a git repo
and three weeks of sessions written in each harness's on-disk format, as found
on a real machine in November 2026: Codex (including an archived, compressed
session), Claude Code (including a worktree session), Cursor, Grok (started in
a subdirectory) and Pi. Noise sessions, a sibling project with similar words
and an old prototype with the same directory name are mixed in. Each key fact
is in exactly one session and nowhere in the repo or git log; `--selfcheck`
asserts this. Claude Code then runs headless in the project with one natural
request per task, and regular expressions grade the final answer. The three
held-out tasks (deploy, flaky, dashboard) were written after the skill text was
settled. See [README.md](README.md) for the conditions and how to rerun.
