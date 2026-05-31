---
title: "Breaking mmr command taxonomy redesign"
description: "Redesign the public mmr command surface around explicit recall, read, context, summarization, assimilation, setup, sync, search, and teleport intents, with no backwards-compatibility requirement."
date: 2026-05-31
status: done
---

# GOAL: Breaking `mmr` Command Taxonomy Redesign

## Outcome

Replace the current historically-grown command surface with a smaller, more
explicit CLI taxonomy. Breaking changes are acceptable and expected. Do not keep
compatibility aliases merely to avoid churn.

The new surface must support three first-class user interactions:

1. Project-wide context across all sources, so an AI agent or human can
   consolidate project-specific learnings regardless of which harness produced
   the evidence.
2. Source/harness-wide context across all projects, so an AI agent or human can
   consolidate learnings about a harness itself.
3. Previous-session recall, so an AI agent or human can retrieve only the
   previous stable session for immediate continuity.

## Surface Touched

- Public command names and goals.
- Removal of backwards-compatibility aliases and overloaded historical names.
- Separation of retrieval, recall, assimilation, setup, sync, and handoff.
- Help text, docs, specs, tests, and local skills that mention old names.

## Validation Plan

- Keep each command mapped to one clear user intent.
- Avoid compatibility aliases in the target list.
- Prefer explicit command names over overloaded flags.
- Assert removed top-level commands fail with clap usage errors.
- Assert the three primary behavior-driven workflows have direct command
  coverage.
- Run the full repo verification loop before completion:
  - `cargo fmt`
  - `cargo test`
  - `cargo test --test cli_benchmark -- --ignored --nocapture`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo build --release`

## Definition of Done

The public CLI exposes the target command surface below, the old top-level
commands are removed, docs/tests/skills are updated, and the verification loop
passes.

## Design Principles

- Commands should name the user's intent, not an implementation detail.
- Scope should be explicit in the command path when it changes the mental model:
  `project`, `source`, and `session` are different products.
- `cwd` project scoping remains a good default for project commands.
- Source/harness commands require `--source`; otherwise the command has no
  clear subject.
- Previous-session recall must exclude the newest assumed-live session by
  default.
- Raw transcript queries remain available, but are not the primary agent-facing
  UX.
- No compatibility aliases. If a name is wrong, remove it.

## Target Command Surface

### Setup, Store, And Sync

| Command | Goal |
| --- | --- |
| `mmr init` | Set up or repair the local mmr store for the current project and import available source history. |
| `mmr status` | Report store, project link, source ingest, redaction, sync, and provider readiness. |
| `mmr sync` | Reconcile redacted project memory with the configured remote store. |
| `mmr import` | Ingest or re-ingest conversation history from one or more harness sources into normalized mmr events. |
| `mmr note` | Add a human-authored project-scoped evidence event. |

### Discovery And Search

| Command | Goal |
| --- | --- |
| `mmr list projects` | List known projects with source coverage and recency metadata. |
| `mmr list sessions` | List sessions in a scope, defaulting to cwd project unless source/global scope is explicit. |
| `mmr find` | Search normalized events and learned memory with structured filters; `--format {json,line}`, JSON default. |

### Raw Reading

| Command | Goal |
| --- | --- |
| `mmr recall` | Retrieve the previous stable session for immediate continuity; default is cwd project, all sources, previous session. |
| `mmr read session` | Read one explicit session by ID as a transcript/evidence payload; `--format {json,tree}`. |
| `mmr read project` | Read project-scoped history across all sources; default project is cwd; `--format {json,tree}`. |
| `mmr read source` | Read source-scoped history across all projects for one harness; `--format {json,tree}`. |

### Context And Summaries

| Command | Goal |
| --- | --- |
| `mmr context project` | Produce project-specific context across all sources for improving future agents working in that project. |
| `mmr context source` | Produce harness-specific context across all projects for improving how a given source/harness behaves. |
| `mmr summarize project` | Run a stateless summary over project-scoped history. |
| `mmr summarize source` | Run a stateless summary over all history from one harness/source. |
| `mmr summarize session` | Run a stateless summary over one explicit session. |

### Assimilation

| Command | Goal |
| --- | --- |
| `mmr assimilate project` | Return the prompt, runbook, output contract, and evidence bundle for project-specific memory deduplication and generalisation. |
| `mmr assimilate source` | Return the prompt, runbook, output contract, and evidence bundle for harness-wide learning across projects. |

### Privacy

| Command | Goal |
| --- | --- |
| `mmr redact scan` | Run local privacy and secret checks over syncable evidence. |
| `mmr redact explain` | Explain why a specific event was redacted or blocked. |

### Session Handoff

| Command | Goal |
| --- | --- |
| `mmr teleport pack` | Package one native harness session for handoff to another machine. |
| `mmr teleport read` | Read a teleport bundle without applying it. |
| `mmr teleport apply` | Install a native teleport bundle into local harness storage. |
| `mmr teleport send` | Transfer a session bundle to another machine or inbox. |
| `mmr teleport receive` | Receive and optionally apply a transferred bundle. |
| `mmr teleport serve` | Serve one session bundle over a one-shot local transfer URL. |
| `mmr teleport inspect` | Validate and inspect a bundle manifest. |
| `mmr teleport export` | Export native artifacts from a teleport bundle. |
| `mmr teleport resume` | Report or perform same-provider resume guidance from a bundle. |

## Removed Top-Level Commands

Remove these as public top-level commands:

- `projects`
- `sessions`
- `messages`
- `export`
- `prev`
- `summary`
- `remember`
- `dream`
- `search`
- `rg`
- `link`

Replacement mapping:

| Remove | Replace with |
| --- | --- |
| `projects` | `list projects` |
| `sessions` | `list sessions` |
| `messages --session-back 1` / `prev` | `recall` |
| `messages --session <id>` | `read session <id>` |
| bare `messages` project window | `read project` |
| `messages --all --source <source>` | `read source --source <source>` |
| `export` | `read project --format json` or `read project --format tree` |
| `summary` | `summarize project/source/session` |
| `remember` | removed, no replacement alias |
| `dream` | `assimilate project` |
| `search` / `rg` | `find` |
| `link` | `init` |

## Required Workflow Examples

Project-specific consolidation across all sources:

```sh
mmr context project
mmr assimilate project
```

Project-specific consolidation for an explicit project:

```sh
mmr context project --project /path/to/project
mmr assimilate project --project /path/to/project
```

Harness/source-wide consolidation across all projects:

```sh
mmr context source --source codex
mmr assimilate source --source codex
```

Immediate previous-session recall:

```sh
mmr recall
mmr recall 2
mmr recall --project /path/to/project
```

Raw session read:

```sh
mmr read session <session-id>
```

Raw project read:

```sh
mmr read project
mmr read project --project /path/to/project
```

Raw source read:

```sh
mmr read source --source claude
```

## Resolved Design Decisions

All four open questions are resolved below. Each records the options weighed, the
chosen decision, the rationale, and the concrete implementation surface so a
later agent can pick up the work without re-deriving the call sites.

### Decision 1 — `mmr init` imports immediately by default

Options considered:

1. Import immediately on `init` (preserve today's `link` behavior).
2. Link-only `init` that reports suggested `mmr import` commands and leaves the
   store empty until the user runs them.
3. Import by default with an explicit opt-out flag for staged automation.

Decision: **Import immediately by default, with a `--link-only` opt-out.**

Rationale: the entire value of the tool is "give me context," and all three
primary workflows (project context, source context, previous-session recall)
return nothing until events are ingested. A link-only first run produces an
empty `mmr recall` / `mmr context project`, which is a footgun for an
agent-facing CLI. Importing on `init` also matches the current `link` behavior,
so this is the lowest-risk mapping. The opt-out keeps deterministic, staged
ingestion possible for automation that wants to control when imports happen.

Implementation:

- `init` reuses today's `link_response` path in `src/cli.rs:1662` essentially
  verbatim: open/repair the store, link cwd to a project, then call
  `reconcile_default_sources` (`src/cli.rs:1682`) to import discovered Codex,
  Claude, and Cursor history, rebuild search documents, and run sync if a remote
  is configured.
- Honor the existing `--source` filter so `init --source codex` links and
  imports only that harness.
- Add `--link-only` to skip `reconcile_default_sources`; in that mode the JSON
  response still lists per-source `suggested_import_commands` (one
  `mmr import --source <s>` per discovered-but-unimported source) so the
  follow-up is obvious.
- stdout stays machine-readable JSON: reuse the `link_response` report shape
  (store schema, project link, per-source import statuses, sync status).

### Decision 2 — line output survives as an explicit `find --format` mode

Options considered:

1. Carry the legacy `rg`-only `--line` boolean forward onto `find`.
2. Remove line output entirely; `find` is JSON-only.
3. Keep line output but expose it as an explicit `--format line` mode alongside
   the default JSON.

Decision: **Keep line output, but as `find --format {json,line}` (JSON default),
not as the inherited `--line` boolean.**

Rationale: the TSV shape
(`citation\tline_number\tsource\tsnippet`, `src/cli.rs:2627`) is genuinely
useful for agent pipelines (`cut`/`awk`) and human scanning, and the rendering
code already exists, so removing it loses a real capability for no gain. But the
taxonomy's principle is "prefer explicit modes over overloaded flags," and the
project-wide constraint is JSON-by-default with explicitly human/stream-oriented
exceptions. A bare boolean that only ever worked on one of the two merged
commands (`rg`) is exactly the overloaded-flag smell we are removing, so it
becomes a named format value instead. This also unifies `find`, `read`, and the
tree manifest on a single `--format` vocabulary.

Implementation:

- Replace the `line: bool` field in `SearchTextArgs` (`src/cli.rs:376`) with
  `#[arg(long, value_enum, default_value = "json")] format: FindFormat`, where
  `FindFormat` is `{ Json, Line }`.
- `--format json` (default) returns the structured result set
  (`src/cli.rs:2596`); `--format line` emits the existing TSV writer
  (`src/cli.rs:2627`) unchanged.
- The line writer's output is still tab-delimited and therefore
  machine-parseable, so it satisfies the JSON-default constraint as an explicit
  stream mode.

### Decision 3 — `read --format tree` fully replaces `export tree`; no `export` namespace survives

Options considered:

1. `read project --format tree` replaces `export --format tree` exactly; remove
   the top-level `export` command.
2. Retain a dedicated `mmr export tree` namespace because tree output writes
   files rather than emitting to stdout.
3. Drop tree output altogether and keep only stdout serializations on `read`.

Decision: **`read <scope> --format tree` replaces `export tree` exactly; no
`export` namespace survives.** Tree becomes a valid format for every `read`
scope (`session`, `project`, `source`), not just the old export path.

Rationale: tree was never a stdout serialization — `export_tree_response`
(`src/cli.rs:2820`) materializes a directory of per-event markdown files under
`--output-dir` and returns a manifest. That side-effect-plus-manifest model is
coherent under `read`: `read` chooses a representation of the selected events,
and `tree` is the on-disk representation whose stdout payload is the manifest.
Resurrecting an `export` namespace would reintroduce exactly the
historically-grown command the redesign removes, and would split a single
"render these events" intent across two verbs. Making tree available on all
read scopes is strictly more general than today, where it was export-only.

Implementation:

- Give `read` a `--format {json,tree}` option (JSON default). `json` emits the
  event payload to stdout as today; `tree` invokes the existing
  `export_tree_response` writer (`src/cli.rs:2820`) and prints its manifest JSON
  (`format`, `output_dir`, `total_files`, `files[].{path,event_id,citation}`) to
  stdout.
- `--output-dir` applies only to `--format tree`; default to the existing
  `mmr-tree-<content_hash>` directory naming when unset, and have json/text
  formats ignore it.
- Wire the same format handling for `read session` and `read source` so the
  manifest's `files[]` layout (`{source}/{session_id}/{event_id}.md`) is
  consistent across scopes.
- Remove the top-level `Export` clap variant (`src/cli.rs:195`) and its
  `--format` enum; the replacement mapping table above already points `export`
  at `read project --format {json,tree}`.

### Decision 4 — `assimilate source` reuses the per-event projection but adds per-project selection windows

Options considered:

1. Reuse `build_evidence_bundle` as-is, just swapping the project query for a
   source-wide query that dumps every event for the source across all projects.
2. Fork a separate source projection with its own redaction logic.
3. Reuse the per-event projection verbatim but replace the *selection* layer
   with a source-wide gather that applies a per-project window and labels
   evidence by originating project.

Decision: **Reuse the per-event evidence projection unchanged; add a new
source-wide selection layer with per-project dedup/recency windows.** The answer
to "same projection or source-wide windows" is *both, separated by concern*:
same projection, new selection.

Rationale: the per-event projection (`event_to_evidence`, `src/dream.rs:629`) is
purely about turning one event into safe, cited evidence — privacy redaction in
shared-safe mode, raw text in local-raw mode. It is already source-agnostic and
orthogonal to scope, so forking it would only duplicate redaction logic and risk
divergence. The selection layer is where project and source assimilation genuinely
differ. `build_evidence_bundle` (`src/dream.rs:450`) calls
`events_for_project(project.id, None, None)` — one project, unbounded — and the
store has **no** cross-project source query (`events_for_project`,
`src/store.rs:1138`, can filter by source only *within* a project). A naive
source-wide dump would (a) blow context limits and (b) let one high-volume
project dominate the harness-wide signal. Per-project windows bound each
project's contribution so cross-project patterns are what surface.

Implementation:

- Add a source-wide store query, e.g.
  `events_for_source(&self, source: &str, since: Option<&str>, limit_per_project:
  Option<usize>) -> Result<Vec<EventRecord>>`, ordered by `project_id` then
  `timestamp ASC, id ASC`. (Either a dedicated SQL query filtered on `source`,
  or iterate `list_projects` calling `events_for_project(pid, Some(source),
  None)` per project — prefer the dedicated query for efficiency.)
- Add `build_source_evidence_bundle(store, source, mode, window)` that gathers
  events per project for the source, applies a per-project window
  (most-recent-N events or most-recent session(s) per project, and/or a
  `--since` cutoff), and projects each retained event through the existing
  `event_to_evidence` — no change to redaction, `DreamEvidence` shape, or
  `evidence_hash`.
- Surface the window as flags on `assimilate source`: `--per-project-limit`
  (default to a tractable recent window) and `--since`. Default to a recent
  window rather than unbounded so the bundle stays within model context.
- Evidence already carries `source`; ensure each `DreamEvidence` also exposes its
  originating `project_id` (present on `EventRecord`) so the runner can dedupe
  and generalize across projects. Evidence refs stay `mmr://event/<id>` (globally
  unique), so `validate_observation` (`src/dream.rs:570`) needs no change.
- `assimilate source` ships a distinct system prompt/runbook from `assimilate
  project`: instruct cross-project generalization (harness-wide behavior
  patterns, not project specifics), mirroring how the project runbook keeps
  memories project-scoped.

## Implementation Notes

- Start by changing clap command structure and help snapshots/contracts.
- Keep service-layer query functions where possible; most work should be command
  routing and response naming.
- Use new integration tests to lock old command removal before implementing
  replacements.
- Update `.agents/skills/mmr/` and `.agents/skills/mmr/session-mining/` so
  future agents learn `recall`, `context`, and `assimilate` instead of old
  names.

## Implementation Progress

Final implementation status:

- `src/cli.rs` now exposes the new top-level command tree:
  `init`, `list`, `find`, `recall`, `read`, `context`, `summarize`,
  `assimilate`, `import`, `note`, `redact`, `sync`, `status`, and `teleport`.
- Removed historical top-level commands are no longer accepted by clap.
- `init --link-only` was added; default `init` reuses the old first-run import
  path.
- `find --format {json,line}` replaces the old `search`/`rg --line` split.
- `read session`, `read project`, and `read source --source <source>` were added;
  `read --format tree` uses the store-backed event tree writer.
- `context project` and `context source --source <source>` return scoped
  machine-readable context bundles.
- `summarize project`, `summarize session`, and `summarize source --source
  <source>` route through the stateless summary runner.
- `assimilate project` replaces `dream`; `assimilate source --source <source>`
  uses a new source-wide evidence selection path with per-project limits.
- `DreamEvidence` now carries `project_id` so source-wide assimilation can dedupe
  and generalize across projects.
- New documentation and skill updates landed in `docs/mmr-command-taxonomy.md`,
  `specs/README.md`, and `.agents/skills/mmr/**`.
- CLI and memory-fabric contract tests cover old command rejection, the main
  replacement paths, project/source context, source/project assimilation, and
  previous-session recall.

Verification:

- `cargo fmt`: passed.
- `cargo test`: passed.
- `cargo test --test cli_benchmark -- --ignored --nocapture`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo build --release`: passed.
- Stale public-command search across docs, specs, skills, src, and tests: no
  matches outside `docs/mmr-command-taxonomy.md` replacement history.
- `cargo run -- --help`: top-level help shows only the target command surface.

Completion bar:

- Old top-level commands fail with clap usage errors.
- The three required workflows have direct passing contract coverage:
  project-wide context across all sources, source-wide context across all
  projects for one harness, and previous stable session recall.
- Docs, specs, and skills teach only the new command surface, with old names
  appearing only in explicit removal/replacement tables.
- The full verification loop passes without relying on real local history.
