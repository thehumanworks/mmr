# Cross-harness context eval: results

## 2026-11-13: agents check earlier sessions without being asked

**Result.** `mmr skill install` now also adds one paragraph to the global
instructions of Claude Code (`~/.claude/CLAUDE.md`) and Codex
(`~/.codex/AGENTS.md`). With it, a Claude Code agent found the earlier-session
fact in 29 of 32 runs, and in 15 of 16 runs of requests to change code, which
never sound like questions about the past. With the skill alone it found 5 of
those 16; without mmr, none. Every run with the install called mmr.

The reader is still only Claude Code. A Codex runner is built and checked
offline, but Codex has not run against a real model yet: the model and endpoint
wait on approval.

| condition | what the agent has | all 16 tasks | 8 change requests | 9 tasks of round 1 |
|---|---|---|---|---|
| `none` | no mmr | 0/32 | 0/16 | 0/18 |
| `skill` | the skill in `~/.claude/skills/mmr` | 19/32 | 5/16 | 14/18 |
| `hook` | skill plus a SessionStart hook running `mmr list sessions --limit 8` | 23/32 | 11/16 | 14/18 |
| `mcp` | mmr's MCP server | 26/32 | 12/16 | 16/18 |
| `shipped-skill` | `mmr skill install`: skill plus the paragraph | **29/32** | **15/16** | **16/18** |

Model `claude-sonnet-5-5`, Claude Code 2.1.323, 2 repetitions per task, build
bbb65e5. In round 1 (below) the same install found 14/18 on the first nine
tasks.

**Where mmr still fails.**

- **Other projects.** "How many times does the ledger dashboard retry?" is
  answered in the sibling project `ledger-ui`. No condition looks there (0 of
  2 everywhere). This is left out on purpose: the job is the current project's
  context, and searching every project by default would pull in unrelated work,
  such as this eval's old prototype with the same directory name. The skill
  documents `--all` for when the user names another project.
- **Reading a rule is not following it.** In one `add-dep` run the agent read
  the session where the user said "no new runtime dependencies without asking
  me first", kept the other rule from the same message, and added `tenacity`
  anyway. mmr returned the rule; the agent did not act on it.
- **Without the paragraph, change requests mostly miss.** The skill alone
  checks history for 5 of 16 change requests, the hook for 11, MCP for 12. A
  client that reads neither `CLAUDE.md` nor `AGENTS.md` should use MCP, whose
  instructions now name code changes.
- **Only Claude Code reads.** See above. The Codex runner (`--agent codex`)
  puts the skill, hook, paragraph and MCP server where Codex 0.167 reads them;
  a local stand-in server confirmed that each reaches Codex's model request.
- **One synthetic project.** Real histories are longer and noisier.

### What changed this round

| problem | evidence | fix |
|---|---|---|
| Asked to change code, agents with the skill did not check history | 5 change requests x 2: skill 1/10, hook 8/10, a paragraph in `CLAUDE.md` 10/10 ([proactive-claude](results/proactive-claude/report.md)) | `mmr skill install` writes that paragraph to `~/.claude/CLAUDE.md` and `~/.codex/AGENTS.md` for agents set up on the machine, between markers, replacing it on reinstall; `--no-instructions` opts out |
| One-line changes skipped history even with the paragraph | "Set TIMEOUT_S back to 10": 0/2 for the install, and 3 of 4 runs made no mmr call ([claude-paragraph-v1](results/claude-paragraph-v1/report.md)) | the paragraph, the skill and the MCP instructions name one-line changes and say to `mmr find` a name from the task: timeout 2/2, and 6/6 on three new tasks written after the change |
| MCP instructions named only risky steps such as deploys | mcp 1/10 on the five first change requests | they now name code changes: 6/10 on the same five, 6/6 on the new three |

The paragraph, as installed:

> This machine keeps the history of earlier AI coding sessions (Claude Code,
> Codex, Cursor, Grok, Pi), readable with `mmr` from a project's directory.
> They hold decisions and their reasons, rules the user gave, and work left
> unfinished. Check them before you start a task in a project, even a one-line
> change: run `mmr find "<name>"` with a distinctive name from the task (a
> constant, function or file), or `mmr list sessions` for recent work, then
> `mmr read session <id>` for a session that matters. If an earlier session
> gives a reason against the change, or a rule it must follow, tell the user.

### Cost

With the install, a run makes a median of 3 tool calls and 2 mmr calls, reads
12k characters of mmr output, and costs $0.06 in 9 seconds. Without mmr: 2 tool
calls, $0.04, 8 seconds. In 13 of the 14 runs of the seven new change
requests, the agent with the install did not make the change: it quoted the
earlier session and asked the user first. The 14th is the `add-dep` run above.

### Tasks added this round

Eight of the 16 tasks ask for a change (see [README.md](README.md#tasks)):
`deploy` from round 1, four written for this round, and three (`replay-delete`,
`log-payload`, `backoff`) written after the paragraph's final wording, to check
that it was not tuned to the first ones. In the new tasks the fact sits past
the start of its session, so the preview in `mmr list sessions` does not show
it.

Full reports: [claude-paragraph-v2](results/claude-paragraph-v2/report.md)
(final), [claude-paragraph-v1](results/claude-paragraph-v1/report.md) (first
paragraph wording, 13 tasks), [proactive-claude](results/proactive-claude/report.md)
and [instructions-claude](results/instructions-claude/report.md) (the paragraph
before it shipped).

## 2026-11-12: round 1, finding earlier work at all

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

### Pass rate

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

### Cost

With the skill, a run makes a median of 3 tool calls, 1 mmr call and 6k
characters of mmr output, and costs $0.06 in 11 seconds. Without mmr: 2 tool
calls, $0.04. On `main`, the `told` condition, where the agent knew mmr existed
and kept trying, took a median of 6 tool calls and $0.10, with 10 failed mmr
calls across its runs.

### What was in the way, and what changed

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

### Method

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
