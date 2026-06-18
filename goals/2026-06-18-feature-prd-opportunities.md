---
title: "Generate feature PRD opportunities for mmr"
description: "Research mmr's current product surface and adjacent AI-session tooling patterns, then produce ranked high-value feature PRDs with measurable definitions of done."
date: 2026-06-18
status: done
---

# GOAL: Generate feature PRD opportunities for `mmr`

## Outcome

Produce a researched, evidence-grounded set of high-value feature opportunities
for `mmr`, ranked by value versus effort and written as Markdown PRDs that a
downstream implementation agent can decompose into work.

## Surface Touched

- Repository documentation and command/spec surfaces.
- Existing goals, ADRs, and tests used as product evidence.
- External market/product research for adjacent agent memory, coding assistant,
  session sharing, observability, and CLI workflow tools.

## Validation Plan

- Read the applicable repo guidance and current product docs.
- Use subagents for bounded parallel exploration of feature opportunities.
- Use market/product research from primary or official sources where possible.
- Score candidates with the feature PRD prioritization rubric.
- Check the final PRDs against the feature PRD quality checklist.

## Definition of Done

- [x] Context summary covers purpose, target users, current capabilities,
      constraints, assumptions, and evidence gaps.
- [x] At least five feature candidates are ranked with concrete value and effort
      rationale.
- [x] The top three high-value/reasonable-effort ideas and one ambitious idea
      are identified.
- [x] Selected PRDs include measurable pass/fail definitions of done.
- [x] Goal status is updated to `done` or `blocked` with the smallest missing
      fact.

# Feature Opportunities and PRDs for `mmr`

## Executive summary

`mmr` is already a credible source-neutral memory fabric for local AI coding
history: it can ingest provider history, search/read/recall sessions, summarize
and compact scoped transcripts, produce assimilation handoffs, sync redacted
memory, and expose MCP tools. The best next opportunities should improve the
real agent loop: faster recovery of the right context, evidence-backed updates
to durable agent guidance, safer multi-machine activation, and privacy-safe
use of high-signal tool evidence. I used four read-only subagents for parallel
exploration and then narrowed their candidates against repo evidence and current
market/product documentation.

## Context used

- Repository: `README.md`, `AGENTS.md`, command taxonomy, Memory Fabric docs,
  search/redaction/session-sharing docs, MCP contract tests, and current CLI
  help.
- Subagents: four bounded read-only explorations covering core retrieval,
  Memory Fabric/privacy, remote sharing/adoption, and adjacent market patterns.
- Market research scope: official or primary docs checked on 2026-06-18 for
  OpenAI Codex customization/session/MCP patterns, Claude Code memory/hooks/OTel,
  Cursor rules/privacy/MCP, Continue context/MCP, Aider repo-map/test loops,
  LangSmith observability/evals, Atuin doctor/sync, Tailscale SSH, GitHub push
  protection, Syncthing, and Morph Compact.

## Assumptions and gaps

- Assumption: primary users are AI coding agents and power users who need local,
  cross-provider, evidence-linked continuity across Codex, Claude, Cursor, Grok,
  and Pi.
- Assumption: near-term features should preserve `mmr`'s JSON stdout contract,
  intent-first command taxonomy, no legacy aliases, and local-first privacy
  posture.
- Gap: no user telemetry, issue tracker data, or interviews were available, so
  value estimates are reasoned from repo evidence, current docs, and category
  patterns.
- Gap: external product surfaces change quickly; product claims should be
  rechecked before implementation planning.

## Ranked feature opportunities

| Rank | Feature | Target user/use case | Value | Effort | Ranking rationale | Evidence summary |
| --- | --- | --- | ---: | ---: | --- | --- |
| 1 | Search-to-read retrieval pipeline | Agent recovering relevant prior work from a fuzzy clue | 4.5 | 2.5 | Converts a repeated multi-command workflow into one bounded context artifact using existing search metadata. | `mmr find` returns citations/session ids, and MCP already has a find-then-read prompt; Codex/Claude docs emphasize resume/session continuity. |
| 2 | Evidence-linked guidance compiler | Maintainer turning repeated agent lessons into AGENTS/CLAUDE/Cursor/Continue guidance | 4.5 | 2.5 | Strong fit with `assimilate` evidence bundles and a large current market pattern around persistent agent rules/memory. | `assimilate` already returns evidence/runbook; Codex, Claude Code, Cursor, and Continue all expose durable guidance/rules surfaces. |
| 3 | Remote readiness doctor and bootstrap plan | User setting up `--remote`, `share`, or `import` on another host | 4.0 | 2.0 | High activation leverage with low product risk because it packages existing peer/status checks into a public preflight. | `--remote` and share/import are broad but explicit; install requires Rust/toolchain; Atuin and Tailscale show preflight/remote setup expectations. |
| 4 | Sync integrity doctor and restore drill | User trusting mmr-store before switching machines | 4.0 | 2.5 | Builds confidence in sync/hydration using existing manifests and hash checks. | Status/sync manifests/hydration exist; corrupted remote payload tests already prove hash mismatch detection. |
| 5 | Budgeted selectors and dry-run estimates for summarize/compact | Agent avoiding huge/costly model-backed context calls | 4.0 | 3.0 | Practical cost and reliability improvement, but lower differentiation than retrieval/guidance. | Some summarize/compact paths can select broad history; Morph Compact quality depends on input shape. |
| 6 | Redaction decision ledger and scoped unblock workflow | User resolving false positives without weakening sync safety | 4.5 | 3.5 | High trust value, but override UX and stale-policy safety require care. | Redaction blocks secrets by default and defers allowlists; GitHub push protection uses bypass reasons/audit. |
| 7 | Learned memory review and lifecycle console | User auditing what `mmr` believes and why | 4.5 | 3.5 | Strong trust feature, but must not violate current public-command non-goals. | Learned-memory rows/evidence refs exist internally; provider memory tools expose manage/edit/audit patterns. |
| 8 | Shared-safe tool evidence projections and bundles | User sharing the useful part of tool calls/results without raw provider files | 4.5 | 4.0 | Ambitious because privacy correctness and provider drift are hard, but it unlocks high-signal evidence now blocked from sync/share. | Docs explicitly block tool calls/results/unknown raw events until safe projection exists; native bundles warn about private content. |

## Top 3 high-value / reasonable-effort ideas

- Search-to-read retrieval pipeline: best direct productivity win for agents,
  with existing citations/session metadata and MCP workflow precedent.
- Evidence-linked guidance compiler: turns `mmr`'s evidence advantage into
  durable improvements across the agent configuration surfaces users already use.
- Remote readiness doctor and bootstrap plan: reduces first-run and multi-host
  friction without changing remote read/share semantics.

## Ambitious idea

Shared-safe tool evidence projections and bundles are the ambitious bet. Tool
calls, test failures, build logs, and raw tool results are often the most useful
continuity evidence, but they are also the riskiest content to sync or share.
A safe projection pipeline would materially improve `mmr`'s value, but it needs
provider-specific parsing, redaction, leakage tests, and clear fidelity labels.

## PRDs

### PRD 1: Search-to-read retrieval pipeline

#### Problem statement

`mmr find` can locate matching events, but an agent must manually group matches,
copy session ids, call `read session`, and trim the transcript. This creates
extra turns, larger context payloads, and a real risk of reading the wrong or
too-broad session.

#### Target user or use case

An AI coding agent or maintainer knows a prior phrase, decision, error string,
or file path and needs the smallest useful prior-session packet for the current
task.

#### Proposed feature

Add an intent-first retrieval surface, for example `mmr retrieve <query>` or
`mmr find <query> --read`, that returns ranked match evidence plus bounded
message windows from the top matching sessions. Defaults should be conservative:
top 3 sessions, small context windows, explicit `next_command`, and JSON stdout.

#### Why this matters

Context recovery is the core product loop. The repo already provides search,
read, recall, and MCP prompts; this feature makes the common composition a
first-class CLI contract instead of relying on prompt discipline.

#### Evidence from project context

- `mmr find` returns event citations, source, session id, role, timestamp, and
  snippets.
- MCP tests expose an `mmr_find_then_read` prompt, which is direct evidence that
  this workflow exists but is currently prompt-composed.
- `mmr` keeps JSON stdout and explicit line-mode exceptions; the retrieval
  response can preserve that contract.

#### Evidence from market/product research

- OpenAI Codex documents session resume flows, including cwd-scoped and all-session
  resume options: [Codex CLI features](https://developers.openai.com/codex/cli/features).
- Claude Code documents continuing or resuming prior work as a core workflow:
  [Claude Code common workflows](https://code.claude.com/docs/en/common-workflows).
- Continue positions MCP and context selection as the way to make agents aware
  of codebases/docs: [Continue codebase documentation awareness](https://docs.continue.dev/guides/codebase-documentation-awareness).

#### Expected user value

Reduce a typical context recovery loop from three or more commands to one
bounded command, while preserving exact citations and session ids for audit.

#### Estimated implementation effort

- Value score: 4.5 - frequent, core workflow with strong project evidence.
- Effort score: 2.5 - mostly command composition, ranking, response-shape tests,
  and output-size guardrails over existing find/read data.
- Main effort drivers: grouped-session ranking, JSON schema, transcript-window
  defaults, pagination commands, and fixture tests.

#### Risks, dependencies, and open questions

- Risk: overly generous defaults create huge outputs.
- Risk: exact lexical search may still miss conceptually relevant sessions.
- Open question: should this be a new `retrieve` command or an option on `find`
  while preserving `find` as exact search?

#### Non-goals

- No semantic/vector search in this PRD.
- No legacy `search` or `rg` alias restoration.
- No automatic model summarization of the retrieved sessions.

#### Definition of done

- [ ] A fixture query returning matches from at least two sessions produces JSON
      with ranked matches, selected sessions, bounded messages, and stable
      `mmr://event/...` citations.
- [ ] Default output includes no more than 3 sessions and no more than the
      documented message/window limit unless the user passes explicit limits.
- [ ] `next_command` pins concrete session ids so later sessions cannot shift a
      paged retrieval window.
- [ ] Empty-match output is a successful JSON response with zero selected
      sessions and a clear suggested next action.
- [ ] Contract tests prove JSON stdout, `--pretty`, source/project filters,
      and no raw-local-ref leakage.

### PRD 2: Evidence-linked guidance compiler

#### Problem statement

Agents repeatedly relearn project rules, user preferences, and workflow fixes.
`mmr assimilate` can produce evidence-backed memory handoffs, but there is no
focused artifact that proposes durable updates to AGENTS.md, CLAUDE.md, Cursor
rules, Continue rules, or mmr skills with citations and scope control.

#### Target user or use case

A maintainer wants to turn repeated session evidence into reviewable guidance
updates without letting a model blindly append stale or overbroad rules.

#### Proposed feature

Add a report-only guidance compiler, for example `mmr assimilate guidance`, that
returns proposed instruction changes grouped by target surface. Each proposal
must include evidence refs, confidence, target file/scope, replacement or delete
recommendation, bloat risk, and counterevidence. File writes should be explicit
and out of scope for the first version.

#### Why this matters

This is a high-fit differentiator: `mmr` is source-neutral and evidence-linked,
while modern coding agents increasingly rely on durable instruction, memory,
rules, skills, and MCP layers.

#### Evidence from project context

- `mmr assimilate` already returns a system prompt, runbook, output contract,
  guardrails, suggested commands, and shared-safe evidence refs.
- Active learned memory must carry resolvable evidence refs, and sensitive or
  contradicted claims are kept pending or rejected.
- The repo includes local skills and AGENTS guidance, making target surfaces
  concrete in this project.

#### Evidence from market/product research

- OpenAI describes AGENTS.md, memories, skills, and MCP as complementary Codex
  customization layers: [Codex customization](https://developers.openai.com/codex/concepts/customization).
- OpenAI documents layered AGENTS.md guidance: [Custom instructions with AGENTS.md](https://developers.openai.com/codex/guides/agents-md).
- Claude Code separates CLAUDE.md guidance and auto memory: [Claude Code memory](https://code.claude.com/docs/en/memory).
- Cursor documents Project, User, Team, and AGENTS.md rule surfaces: [Cursor rules](https://cursor.com/docs/rules).
- Continue recommends rules and MCP/custom context for agent awareness:
  [Continue codebase documentation awareness](https://docs.continue.dev/guides/codebase-documentation-awareness).

#### Expected user value

Convert real session evidence into concise, reviewable guidance updates that
reduce repeated steering, repeated bugs, and context bloat across agent tools.

#### Estimated implementation effort

- Value score: 4.5 - strong strategic fit and broad agent-workflow impact.
- Effort score: 2.5 - can start as a report over existing assimilation evidence,
  without mutating files.
- Main effort drivers: output schema, evidence grouping, stale/bloat heuristics,
  target-surface mapping, and tests for unsupported or contradictory claims.

#### Risks, dependencies, and open questions

- Risk: codifying stale or one-off observations into durable guidance.
- Risk: instruction files become too long and lower agent adherence.
- Open question: which target surfaces should ship first: AGENTS.md only, or
  AGENTS.md plus CLAUDE/Cursor/Continue suggestions?

#### Non-goals

- No automatic edits in the first version.
- No public `learn`, `promote`, `reject`, or candidate-management command.
- No generation of guidance from evidence that was omitted by privacy policy.

#### Definition of done

- [ ] A fixture assimilation bundle produces at least one guidance proposal with
      target surface, proposed text, action type, confidence, and evidence refs.
- [ ] Proposals with missing evidence, contradicted newer evidence, secrets, or
      identity-affecting claims are rejected or quarantined in JSON.
- [ ] Output distinguishes append, rewrite, tighten, and delete recommendations.
- [ ] Report includes a token/line-size estimate and flags proposals that would
      exceed configured guidance-size limits.
- [ ] Tests prove report-only behavior does not modify AGENTS.md, skills, or
      external tool config files.

### PRD 3: Remote readiness doctor and bootstrap plan

#### Problem statement

Remote reads and session sharing depend on SSH reachability, remote `mmr` on
PATH, compatible peer protocol, provider roots, and local install prerequisites.
Today users discover most failures by running a real remote read/share command
and interpreting structured errors.

#### Target user or use case

A user wants `mmr list/read/recall/summarize --remote mini`, `share session`, or
`import session` to work reliably across laptop, desktop, and remote hosts.

#### Proposed feature

Add a read-only preflight command, for example `mmr doctor remote --host <target>`,
that runs local checks, SSH batch reachability, remote `mmr --version`, hidden
peer status/protocol checks, provider-source availability, and command-surface
compatibility. It should return JSON plus exact recovery commands.

#### Why this matters

Remote continuity is a major `mmr` differentiator, but setup friction can block
activation before the user sees value. A doctor command improves confidence
without changing the explicit-peer model.

#### Evidence from project context

- Remote read/share/import surfaces are documented across list, read, context,
  recall, summarize, share, and import.
- Session sharing docs explicitly say the remote needs up-to-date `mmr` reachable
  through SSH.
- README install currently requires Rust 1.85+, a C compiler toolchain, and
  TLS/build prerequisites.
- Hidden peer diagnostics already expose protocol/capability information.

#### Evidence from market/product research

- Atuin documents `atuin doctor` for diagnosing common CLI/system issues:
  [Atuin doctor](https://docs.atuin.sh/cli/reference/doctor/).
- Tailscale SSH has policy, auth, host key, and check-mode complexity that
  benefits from clear preflight reporting: [Tailscale SSH](https://tailscale.com/docs/features/tailscale-ssh).
- Syncthing emphasizes device-to-device sync and user control, a relevant
  pattern for multi-machine local-first continuity: [Syncthing FAQ](https://docs.syncthing.net/users/faq.html).

#### Expected user value

Turn remote setup from trial-and-error into a deterministic checklist with
machine-readable failure kinds and copyable next commands.

#### Estimated implementation effort

- Value score: 4.0 - high activation value for a visible workflow.
- Effort score: 2.0 - mostly orchestration over existing status/peer checks and
  structured errors.
- Main effort drivers: command shape, SSH probe safety, capability schema,
  recovery action generation, and fake-SSH contract tests.

#### Risks, dependencies, and open questions

- Risk: remote install suggestions can become platform-specific noise.
- Risk: auto-bootstrap over SSH would be an irreversible external effect.
- Open question: should the command live under `status`, `doctor`, or `remote`
  without conflicting with existing taxonomy?

#### Non-goals

- No automatic remote installation in the first version.
- No host registry, peer discovery daemon, Tailscale scan, or stored peer config.
- No weakening of strict remote failure semantics for normal read/share commands.

#### Definition of done

- [ ] Against a fake reachable peer, doctor JSON reports local version, remote
      version, protocol compatibility, sources, and supported command families.
- [ ] Against fake SSH auth failure, missing remote `mmr`, and stale peer
      protocol, doctor exits successfully or with documented status JSON and
      returns exact recovery actions without mutating either host.
- [ ] Output includes install/build prerequisite hints only when the remote
      check proves they are relevant.
- [ ] Normal `--remote`, `share`, and `import` behavior remains unchanged.
- [ ] Tests cover JSON stdout, stderr diagnostics, `--pretty`, and no leaked
      credentials or private SSH material.

### PRD 4: Shared-safe tool evidence projections and bundles

#### Problem statement

Tool calls, tool results, build logs, test failures, and unknown raw events are
some of the most useful continuity evidence. They are also high-risk: they can
contain secrets, local paths, proprietary logs, and provider-specific raw blobs.
`mmr` currently blocks those event types from sync until safe projections exist,
and native teleport bundles warn that raw session content may be private.

#### Target user or use case

A user wants to share or sync enough tool evidence for another agent to continue
debugging without sharing raw provider files or unsafe transcript payloads.

#### Proposed feature

Create provider-neutral safe projections for tool calls/results and unknown raw
events, then use them in sync, find, assimilate evidence, and a `shared-safe`
session bundle/read mode. Projections should keep command category, exit status,
bounded redacted diagnostics, omitted-byte counts, and evidence refs while
removing raw local refs, secrets, and unsafe paths.

#### Why this matters

This unlocks high-signal evidence that is currently blocked, materially improving
cross-machine continuity and share-safe handoffs while preserving `mmr`'s
privacy-first contract.

#### Evidence from project context

- Memory Fabric progress explicitly says tool results and unknown raw events
  need a future dedicated safe projection before remote sync eligibility.
- Sync uploads only redacted safe projections and blocks unresolved secrets,
  tool-call/tool-result raw payloads, unknown raw event types, or degraded
  policy coverage.
- Native session sharing warns that bundles can contain private transcript
  content and local paths.

#### Evidence from market/product research

- Aider treats lint/test failures as first-class feedback for repair loops:
  [Aider linting and testing](https://aider.chat/docs/usage/lint-test.html).
- Claude Code hooks expose lifecycle JSON and deterministic automation points:
  [Claude Code hooks](https://code.claude.com/docs/en/hooks).
- Claude Code also exports tool/session usage through OpenTelemetry:
  [Claude Code monitoring](https://code.claude.com/docs/en/monitoring-usage).
- Cursor privacy documentation highlights zero-data-retention/privacy-mode
  expectations for coding-agent data: [Cursor data use](https://cursor.com/data-use).

#### Expected user value

Enable another host or agent to understand what commands failed, what checks
passed, and what diagnostic snippet mattered without receiving raw logs or
provider-native files.

#### Estimated implementation effort

- Value score: 4.5 - high-signal evidence unlock with strong privacy fit.
- Effort score: 4.0 - provider-specific parsing, sanitization, redaction, sync,
  and bundle semantics make this materially harder.
- Main effort drivers: projection schema, provider mappings, redaction tests,
  sync eligibility, bundle fidelity labels, and leakage audits.

#### Risks, dependencies, and open questions

- Risk: a missed sanitizer leaks a secret or private path.
- Risk: over-sanitization removes the exact failure text needed for continuity.
- Dependency: stable provider event parsing for Codex, Claude, Cursor, Grok,
  and Pi where supported.
- Open question: should shared-safe bundles be readable-only forever, or later
  support limited native apply/resume?

#### Non-goals

- No raw native-provider apply from shared-safe bundles.
- No bypass of redaction policy or degraded-policy sync blocks.
- No guarantee that all provider-specific tool data can be projected in v1.

#### Definition of done

- [ ] Fixture tool-call/tool-result events with API keys, private paths, large
      logs, and malformed/unknown payloads produce safe projections with no raw
      secret strings, no raw local refs, and documented omitted fields.
- [ ] Sync dry-run marks projected-safe tool events as eligible only when
      deterministic secret checks and policy coverage pass.
- [ ] Shared-safe bundle read returns messages/evidence refs and clear
      `fidelity: "shared-safe"` metadata, while apply/resume attempts return a
      structured unsupported-mode error.
- [ ] Search and assimilate can include projected tool evidence with bounded
      snippets and omitted-byte counts.
- [ ] Regression tests scan generated remote payloads and bundles for fixture
      secrets, raw provider paths, and unredacted tool output.

## Sources

- [OpenAI Codex customization](https://developers.openai.com/codex/concepts/customization)
- [OpenAI Codex AGENTS.md guide](https://developers.openai.com/codex/guides/agents-md)
- [OpenAI Codex CLI features](https://developers.openai.com/codex/cli/features)
- [Claude Code memory](https://code.claude.com/docs/en/memory)
- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- [Claude Code monitoring](https://code.claude.com/docs/en/monitoring-usage)
- [Cursor rules](https://cursor.com/docs/rules)
- [Cursor data use](https://cursor.com/data-use)
- [Continue codebase documentation awareness](https://docs.continue.dev/guides/codebase-documentation-awareness)
- [Aider repo map](https://aider.chat/docs/repomap.html)
- [Aider linting and testing](https://aider.chat/docs/usage/lint-test.html)
- [LangSmith observability](https://docs.langchain.com/langsmith/observability)
- [OpenAI agent improvement loop](https://developers.openai.com/cookbook/examples/agents_sdk/agent_improvement_loop)
- [Atuin doctor](https://docs.atuin.sh/cli/reference/doctor/)
- [Tailscale SSH](https://tailscale.com/docs/features/tailscale-ssh)
- [GitHub push protection](https://docs.github.com/en/code-security/concepts/secret-security/push-protection)
- [Syncthing FAQ](https://docs.syncthing.net/users/faq.html)
- [Morph Compact](https://docs.morphllm.com/sdk/components/compact)
