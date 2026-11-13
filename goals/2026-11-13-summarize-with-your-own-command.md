---
goal_id: "2026-11-13-summarize-with-your-own-command"
title: "Summarise sessions with a model or harness the user chooses"
description: "Bring back session summaries as an opt-in command that pipes a bounded transcript to a command the user configures (a signed-in agent CLI, an API client or a local model), with no provider code, API key handling or new dependency in mmr."
date: "2026-11-13"
status: "draft"
created: "2026-11-13"
updated: "2026-11-13"
---

# Goal

Tomas, 2026-11-13: "We need to make session summarisation something flexible
so users can use the models and harnesses they want. How do you propose we
change this program to allow for that? Currently there's a
MMR_SUMMARISER_MODEL, no indication of what provider or harness it expects,
limitations or any other info."

This is a proposal. Nothing is built until Tomas approves an option.

## Facts

- `main` and the prepared v0.3.0 have no summariser. PR #34 (merged
  2026-11-13) removed `summarize` and `compact`, their MCP tools, the
  `~/.config/mmr/config.json` file and the `reqwest` dependency. On `main`,
  `MMR_SUMMARISER_MODEL` appears only in the v0.3.0 "Settings that mmr no
  longer reads" list in `CHANGELOG.md` and in two closed goal docs. The v0.3.0
  tag is not pushed yet, so the newest release is still v0.2.0.
- In v0.2.0, `mmr summarize --help` names `MMR_SUMMARISER_MODEL` and nothing
  else. What it really expected was only in `AGENTS.md` and `docs/`: an
  OpenAI-compatible `POST {OPENAI_BASE_URL}/chat/completions` call (default
  base URL `https://api.openai.com/v1`, default model `gpt-5.5`),
  `OPENAI_API_KEY` or a config-file key, and OpenRouter as the documented
  alternative. The Nix module example set OpenRouter and `openai/gpt-5.5`.
  There was no input limit (without `--limit` it sent the whole project
  history in one request), no timeout and no chunking. Tomas's description is
  accurate for v0.2.0.
- History: before 2026-05-31, summaries ran through harness SDKs (Codex
  app-server SDK, Cursor, Gemini). That was replaced by the HTTP call because
  the provider SDKs and CLIs changed too fast
  ([2026-05-31-api-compatible-summarizer](2026-05-31-api-compatible-summarizer.md)).
- Usage evidence: 820 recorded eval runs used binaries that still had
  `summarize`. No agent ran `summarize` or `compact`. Two agents loaded the
  MCP tool schema and did not call it. Caveats: the eval set no API key, and
  its tasks ask for specific facts, not a brief.

## What summaries are for

mmr helps an agent learn what earlier sessions did in this project, whichever
harness ran them. Raw reads stay the default. A summary helps in two cases:

1. The history is too long to read. A cheap model condenses many sessions
   into a resume brief, so the calling agent spends a few thousand tokens of
   its own context instead of tens of thousands.
2. Something without a model wants a brief: a hook, a script or a person.

## Options

### A (recommended): pipe to a command the user chooses

`MMR_SUMMARIZE_CMD` holds one shell command that reads a prompt on stdin and
prints the answer on stdout. mmr builds the prompt (instructions plus the
newest messages that fit a budget), runs the command, and returns its output.
mmr has no provider, model, base URL or key of its own. The user picks all of
that in the command.

Examples the docs will give, each run end to end before release:

| Want | `MMR_SUMMARIZE_CMD` |
| --- | --- |
| Claude Code sign-in or key | `claude -p --no-session-persistence --tools "" --model <model>` |
| Codex sign-in | `codex exec --skip-git-repo-check -` plus its no-persistence option, to be confirmed |
| Any HTTP API (OpenAI, Anthropic, Gemini, OpenRouter) | `llm -m <model>` (the `llm` CLI and its plugins) |
| Local model, nothing leaves the machine | `ollama run <model>` |

The `claude` flags exist in Claude Code 2.1.323 (`claude --help`). The other
rows are not verified yet.

Surface:

- `mmr summarize project [--project P]` and `mmr summarize session <id>`,
  the same shape as `read`. Flags: `--max-chars` (default 200000, about 50k
  tokens), `--limit` (optional message cap), `--instructions <text>` (replace
  the default brief with a question, for example "what did we decide about
  retries?"), `--timeout <seconds>` (default 300).
- MCP tools `mmr_summarize_project` and `mmr_summarize_session`, listed only
  when `MMR_SUMMARIZE_CMD` is set when the server starts. Tool arguments can
  never choose the command.
- Output is JSON like every other command: `summary`, `program` (the first
  word of the command only, so a key inside the command is never printed),
  `sessions`, `messages_included`, `messages_available`, `truncated`,
  `input_chars`, `elapsed_ms`.
- The default prompt is the v0.2.0 resume brief (status, what was done,
  decisions, open items, files, resume instructions), shortened.

Behaviour that a hand-written pipe gets wrong, and mmr owns:

- The command runs in a new empty temp directory, which mmr deletes after.
  Otherwise an agent CLI started in the project folder saves a new session
  in that project's history, which holds the whole transcript as its prompt,
  and every later `find` matches it.
- The child gets `MMR_SUMMARIZING=1`. A nested `mmr summarize` refuses to run,
  so a summariser agent cannot start another one.
- Input is the newest messages that fit `--max-chars`. Older messages are cut,
  not an error, and the output says how many were left out.

Failure modes, each with a test:

| Case | Result |
| --- | --- |
| `MMR_SUMMARIZE_CMD` not set | Exit 1. The message names the variable, the stdin/stdout contract and the four examples. If `MMR_SUMMARISER_MODEL` or `OPENAI_API_KEY` is set, it says mmr no longer reads them and shows the `llm -m <model>` equivalent. |
| Command missing or exits non-zero | Exit 1 with the exit code and the last 2 KB of its stderr. |
| Timeout | mmr kills the command, exit 1, and suggests `--timeout` or a lower `--max-chars`. |
| Empty output | Exit 1. |
| Nested call | Exit 1 at once, no command started. |
| Every other command | Unchanged. Nothing reads `MMR_SUMMARIZE_CMD` except `summarize` and the MCP tool list. |

Docs: a README section "Summaries with your own model" and the long
`summarize --help` both state the contract (stdin text in, stdout text out),
the budget and timeout, the cost (one model call per summary, at most about
50k input tokens by default, billed by the user's provider or plan), and
privacy (history leaves the machine unless the command runs a local model;
there is no secret scan, unlike `share session`). `docs/mmr-nix.md` notes
that the MCP service runs under systemd or launchd, so the command must be on
that service's `PATH` and able to sign in there.

Cost: one new module, CLI and MCP wiring, about 400 lines of Rust plus tests.
No new crate (only `std::process` and `std::thread`), so PR #34's "no runtime
dependency compiles C" stays true. No config file. Nothing changes for a user
who does not set the variable.

### B: built-in HTTP providers

`MMR_SUMMARIZE_MODEL=<provider>:<model>` with code for OpenAI-compatible,
Anthropic Messages and Ollama endpoints. This brings back `reqwest` and TLS
(C or assembly through `ring` or `aws-lc-rs`), key handling and one request
shape per provider for mmr to keep current. It cannot use a Claude Code or
Codex sign-in, only API keys. Not recommended.

### C: keep it removed and let the calling agent summarise

No code. The README and skill give a pipe recipe, for example
`mmr read project --limit 200 | claude -p --no-session-persistence --tools "" "Write a resume brief of these sessions"`.
This is competitive: no agent used the old command, and the calling agent is
itself a model. It loses the parts listed under A: users who forget the
no-persistence flag pollute the project history, there is no input budget,
MCP-only agents cannot pipe, and every user writes their own prompt.

## Validation plan (option A)

1. Tests first, with fake commands: `cat` (assert the prompt and budget
   reach stdin), `sleep` (timeout), `exit 3` (stderr and code), empty output,
   nested guard, temp-dir cwd, MCP `tools/list` with and without the variable.
2. Differential check against `main` with the PR #31 matrix: every existing
   case unchanged except top-level help.
3. Run each README example end to end on a machine that has that CLI.
4. One eval round with a `summarize` condition (skill plus a configured
   cheap-model command) against `skill`: does the agent call it, and does it
   keep accuracy with fewer tokens? Report the result either way.
5. Full loop: fmt, clippy `-D warnings`, `cargo test`, benchmarks, release
   build, `nix build`, Nix module checks, `cargo tree -i cc`.

## Definition of Done

- [ ] DoD-1: `summarize project|session` and the two MCP tools work through
  `MMR_SUMMARIZE_CMD`, with the failure modes above tested.
- [ ] DoD-2: README, `--help`, MCP docs and Nix docs state provider, contract,
  limits, cost and privacy.
- [ ] DoD-3: Differential check explained; no new crate; no C dependency.
- [ ] DoD-4: Eval round recorded in `evals/cross_harness/RESULTS.md`.
- [ ] DoD-5: CHANGELOG entry for the next release.

## Exit conditions

- `SCOPE-CHANGE` if Tomas picks B or C, or wants a different contract.
- `SCOPE-CHANGE` if an example CLI cannot run without saving a session in
  the temp project; then mmr must also skip its own temp projects in `list`
  and `find`.
