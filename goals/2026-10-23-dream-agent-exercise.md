---
title: "Exercise codification-first assimilation with a fresh agent"
description: "Evaluate lesson selection and an authored check on synthetic history, beyond JSON validation."
date: "2026-10-23"
status: "in-progress"
confidence_floor: 90
---

# Outcome and scope

Give a fresh agent the actual assimilation handoff from synthetic native history,
without an expected proposal or the earlier fixture-authored answer. Include a
recurring deterministic failure, a contextual judgement and conflicting evidence.
Have it choose codification/retention/rejection, use the real review path, author
and inspect a tiny harmless check, and exercise failing/passing synthetic inputs.
Keep product proposals `proposed`; developer execution is not product installation
or a model benchmark. No live histories, network, secret use, new runner or tools.

# Definition of done

- [x] Fresh agent receives actual handoff/source history without an expected plan.
- [x] Independently chosen plan passes actual review; authored check distinguishes
  failing/passing fixtures; source evidence and native files remain unchanged.
- [x] Evaluate codification bias and conflicting/contextual evidence handling.
  Fix only a concrete flaw if found; otherwise record concise reproducible evidence.
- [ ] Applicable checks and independent review pass; update draft PR26 without
  readiness/merge changes, preserving exact head and verification limitations.

# Evidence

- 2026-10-23 — PR26 is draft and unmerged at
  `d632acb275e58b3005563fb182083c314324a042`; clean local checkout matches it.
  Existing product checks passed in the parent goal. This exercise adds evidence,
  not a new runtime or execution-status feature.
- 2026-10-23 — Fresh agent `independent_dream_exercise` received only the actual
  handoff and synthetic source. It chose a manifest-order proposal, retained an
  audience judgement, and marked blanket array sorting superseded. It authored
  and inspected the check before execution. The two fixtures returned 1 and 0
  respectively. Details and reproducible replay:
  `docs/mmr-dream-agent-exercise.md`; original local artifacts:
  `../dream-agent-exercise/{proposal.json,decision-rationale.md,code-inspection.md,verification-results.json}`.
- 2026-10-23 — Replayed `mmr assimilate project --review proposal.json --pretty`
  using absolute binary/proposal paths, synthetic project cwd and
  environment containing only HOME/PATH. Exit 0; three observations, codification
  `proposed`, full handoff/evidence equal to the original and native SHA-256
  unchanged. Exact original argv/cwd/output and hashes are in
  `../dream-agent-exercise/verification-results.json`. This is developer
  verification, not product execution, installation or a model benchmark.
- 2026-10-23 — Executed the Python block in `docs/mmr-dream-agent-exercise.md`
  directly from its text with `/usr/bin/python3 -B`: expected fail/pass output,
  exit 1/0, unchanged fixtures, removed temporary HOME. Copied checker SHA-256
  matches the independently authored original. Clarified the existing confidence
  `status` versus superseded `kind` meaning in the guide; no runtime change.
- 2026-10-23 — `python3 -B examples/dream_codification.py target/release/mmr`
  passed; `bash ../cargo-check.sh test --locked --test assimilation_review_contract`
  passed all four tests; `git diff --check` passed. Host mise cache-write warnings
  occurred when invoking the PATH Python shim; direct `/usr/bin/python3` replay
  passed without those warnings. No host configuration was changed.
- 2026-10-23 — `bash ../verify.sh dream-exercise` passed format, all tests,
  364 tests, four ignored benchmarks, strict Clippy and release build. Logs:
  `../evidence/dream-exercise-{fmt,test,benchmark,clippy,release}.log`.
  These checks ran on the final example/guide working tree; later changes only
  record evidence in this goal.
- 2026-10-23 — Independent reviewer `review_exercise_evidence` found no actionable
  issues; reproduced the documented replay and verified original/copied checker
  hash, native hash, complete preserved handoff, references/scope and proposed
  state. Review cannot independently establish the original agent's context
  isolation or inspection chronology from saved artifacts; those are recorded
  process claims. Earlier reviewer launches failed transiently with model-capacity
  errors; the later review completed. Publication remains pending.
- 2026-10-23 — Refreshed remote refs before publication: main remains
  `2d19dc1fe8bc75c565a6b50b2d74fb2dc024cd67`, draft26 head remains
  `d632acb275e58b3005563fb182083c314324a042`; no concurrent changes to overwrite.

# Exit conditions

- DONE: exercise, evidence review and draft update complete.
- BLOCKED-DEP: required isolated execution or publication unavailable.
- SCOPE-CHANGE: findings require a broader product redesign.
