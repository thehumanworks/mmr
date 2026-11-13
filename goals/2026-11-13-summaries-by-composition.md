---
goal_id: "2026-11-13-summaries-by-composition"
title: "Leave summaries to the user's own model, and make mmr safe to pipe"
description: "mmr does not summarise. Document how to pipe its bounded JSON output to any model or headless agent the user already has, and stop mmr panicking when a pipe reader such as head closes early."
date: "2026-11-13"
status: "done"
created: "2026-11-13"
updated: "2026-11-13"
---

# Goal

Tomas, 2026-11-13: "We need to make session summarisation something flexible
so users can use the models and harnesses they want." Three options were put
to him: (A) a `summarize` command that pipes to a command the user
configures, (B) built-in HTTP providers, (C) no summariser in mmr, with a
documented pipe. His answer: "best course of action? Lean and composable?
Don't summarise, leave that up to an extension the user adds?" Decision: C.

mmr prints raw project context as JSON. Summaries are something the user
composes on top with a model or agent CLI they already have. mmr has no
summariser setting, provider code, model name or API key.

## Facts behind the decision

- `main` and the prepared v0.3.0 have no summariser. PR #34 removed
  `summarize`, `compact`, the config file and `reqwest`. On `main`,
  `MMR_SUMMARISER_MODEL` appears only in the v0.3.0 list of settings that
  mmr no longer reads (`CHANGELOG.md`) and in two closed goal docs.
- In v0.2.0, `--help` named `MMR_SUMMARISER_MODEL` and nothing else. The
  real contract (an OpenAI-compatible chat-completions call, `OPENAI_API_KEY`,
  default model `gpt-5.5`, no input limit, no timeout) was only in
  `AGENTS.md` and `docs/`.
- In 820 recorded eval runs on binaries that had `summarize`, no agent ran
  it.

## Is an input cap missing?

No new flag is needed. `--limit` already bounds the message count of
`read project`, `context project`, `read session` and `recall`. On a real,
tool-heavy Claude Code session (this thread's own history), messages were
about 2 KB on average and 13 KB at most, so `context project --limit 100` is
about 200 KB, or roughly 50k tokens. For a hard byte cap, the user appends
`head -c <bytes>`. `context project` lists sessions first and then messages
newest first, so `head -c` keeps the newest material.

That composition failed before this goal: when `head` closed the pipe, mmr
panicked with "failed printing to stdout: Broken pipe", a stack trace and
exit 101. Agents pipe mmr into `head` often (the eval transcripts have many
`mmr ... | head` calls); the eval corpus was small enough that the output fit
the pipe buffer, so it never showed there. Real histories are larger.

## Surface

- `src/main.rs`: write the JSON with an explicit write; on `BrokenPipe`, exit
  0 with nothing on stderr, as `rg` and other Unix tools do.
- `tests/cli_contract.rs`: a test that closes the reader before mmr writes
  and asserts exit 0 and no panic text.
- `README.md`: a short "Summarise with your own model" section with the pipe
  and examples for Claude Code, Codex, `llm` and Ollama.
- `CHANGELOG.md`: the v0.3.0 "Use instead" row for `summarize` points to
  that section, and a Fixed entry for the broken pipe.
- `docs/mmr-command-taxonomy.md`: the removed-commands row points to the same
  section.
- No change to the bundled skill or the agent instructions paragraph. The
  calling agent is a model and condenses what it reads itself; the eval shows
  that skill wording changes agent behaviour, so text that suggests calling
  another model is left out unless an eval shows it helps.

## Validation plan

1. The new test fails on `main` (exit 101, panic text) and passes after.
2. Reproduce by hand: `mmr context project --limit 64 | head -c 1000` on a
   real history before and after.
3. Run the README pipe with `claude -p` where Claude Code is signed in, if
   possible from this environment; otherwise check the flags in `--help` and
   say which examples were not run.
4. Full loop: `cargo fmt --check`, clippy `-D warnings`, `cargo test`,
   `cargo build --release`.

## Definition of Done

- [x] DoD-1: mmr exits 0 and prints nothing on stderr when its stdout reader
  closes early; test added.
- [x] DoD-2: README section and CHANGELOG entries written; nothing on `main`
  outside the changelog and closed goal docs names `MMR_SUMMARISER_MODEL`.
- [x] DoD-3: Verification loop passes.

## Evidence

- `closed_stdout_reader_exits_quietly` (a 256 KB message, larger than a pipe
  buffer, with the reader closed at once) failed before the fix with exit 101
  for `context project` and passes after, for `context project` and
  `read project`.
- By hand on this thread's own Claude Code history:
  `mmr context project --limit 64 | head -c 1000` printed "failed printing to
  stdout: Broken pipe" and a stack trace with exit 101 before, and exits 0
  with nothing on stderr after.
- The README pipe ran end to end with Claude Code 2.1.323:
  `mmr context project --limit 20 | head -c 20000 | claude -p
  --no-session-persistence --tools "" --model haiku "<instruction>"` returned
  a five-part brief in about 10 seconds, and no new session file appeared
  under `~/.claude/projects`. The Codex (`codex exec --ephemeral`, prompt plus
  stdin as context) and `llm` (`-s` with stdin) rows follow their official
  docs (developers.openai.com/codex/noninteractive, llm.datasette.io usage);
  they were not run here, because neither CLI is installed in this
  environment.
- Remaining `MMR_SUMMARISER_MODEL` mentions on the branch: the v0.3.0
  removal list in `CHANGELOG.md` and closed goal docs. `OPENAI_API_KEY`
  remains only in the secret scanner's patterns (`src/redaction.rs`).
- Loop: `cargo fmt --check`, clippy `-D warnings`, `cargo test` (all suites
  pass with `zsh` on PATH), release build, benchmarks, `nix build`.

## Exit conditions

- `SCOPE-CHANGE` if Tomas wants a summariser inside mmr after all (option A
  is written up in this file's history, commit 41fab64).
