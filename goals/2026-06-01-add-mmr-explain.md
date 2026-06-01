---
title: "Add deterministic mmr explain"
description: "Add a local explain surface for sessions, projects, and source scopes that summarizes what happened without requiring an AI call."
date: 2026-06-01
status: proposed
---

# GOAL: Add Deterministic `mmr explain`

## Outcome

Add a deterministic `mmr explain` surface for understanding local AI work
without requiring a summarizer API call.

Suggested shape:

```bash
mmr explain session <session-id>
mmr explain project --project /path/to/project
mmr --source codex explain source
```

Default output must remain machine-readable JSON. Add `-O md` only as an
explicit human-readable rendering.

## Why

Entire's `checkpoint explain` is useful because it ties transcript evidence to
prompts, responses, files touched, tokens, commits, and generated summaries.
`mmr` has raw reads and stateless `summarize`, but it lacks a cheap deterministic
middle layer that answers "what happened here?" without model cost or prompt
variance.

## Surface Touched

- `src/cli.rs` command surface.
- Query/session aggregation in `src/messages/service.rs` or a new explain module.
- API response types in `src/types/`.
- Store-backed event metadata where imported events provide richer facts.
- Docs/specs/tests.

## Required Output Contract

For a selected session or scope, report:

- scope and resolved selectors
- sources and projects involved
- first/last timestamps
- user prompt count, assistant response count, tool/result counts when known
- token totals when known
- models seen
- top files touched when provider/import data includes them
- compaction/lifecycle events when known
- redaction/sync status for store-backed events
- evidence refs and equivalent `mmr read ...` commands

## Non-Goals

- No AI-generated summary in the base command.
- No Git checkpoint/commit reconstruction.
- No mutation of the store or source files.
- No replacement for `mmr summarize`; explain is deterministic evidence, not a
  continuity brief.

## Validation Plan

- Add fixture sessions with multiple sources, token usage, tool events, and
  known file metadata where available.
- Assert JSON schema stability and absence of raw transcript bytes unless the
  caller explicitly asks for a read command.
- Assert Markdown rendering is generated from the same response type.
- Assert `summarize` behavior is unchanged.
- Run full repo verification.

## Definition of Done

- [ ] `mmr explain session`, `project`, and `source` produce deterministic JSON.
- [ ] Every claim in the response is backed by source/session/event identifiers.
- [ ] Output is useful without an API key.
- [ ] Docs clarify when to use `read`, `explain`, `summarize`, and `assimilate`.
- [ ] Full verification loop passes.
