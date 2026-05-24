# mmr Dream Runner

NHL-278 adds the provider-neutral runner layer used by the later `mmr dream`
assimilation workflow.

## Boundary

Dreaming is stateful learned-memory assimilation, not summarization. The runner
layer only prepares evidence, routes it to a provider, parses structured output,
and validates evidence refs. Durable learned-memory writes remain owned by the
assimilation workflow.

## Runner Config

Runner selection resolves in this order:

1. CLI override (`--runner` once `mmr dream` is wired)
2. project default passed by the future `mmr dream` command
3. user default (`MMR_DEFAULT_DREAM_RUNNER`)
4. built-in default (`mock`)

Supported runner kinds:

- `mock`: deterministic test/local runner
- `command`: local command adapter. It reads a JSON `DreamRequest` on stdin and
  must print structured dream output JSON on stdout. Configure with
  `MMR_DREAM_COMMAND`, for example `MMR_DREAM_COMMAND="python runner.py"`.

Retry and best-of-N fields are part of the config shape for later high-value
jobs. NHL-278 rejects non-default values until NHL-279 or a later ticket gives
them explicit execution semantics.

Command runners are bounded by a local timeout. A command that exits non-zero,
hangs, or emits invalid JSON fails the run before any downstream memory write.

## Evidence Privacy

Remote/API-style runners receive shared-safe evidence by default. Shared-safe
bundles:

- redact deterministic local PII such as private email addresses
- omit events blocked by deterministic secret findings
- include only `mmr://event/<id>` citations, normalized metadata, and redacted
  content

Raw local evidence requires an explicit local-only opt-in and is rejected for
non-mock runners.

Final validation is scoped to the exact evidence refs included in the runner
request. A provider cannot cite local events that were omitted from the
shared-safe bundle.

## Structured Output

Runner output must be strict JSON with known top-level fields:

- `observations`
- `claims`
- `patterns`
- `open_loops`
- `learned_memory_updates`
- `counterevidence`
- `recommended_actions`
- `diagnostics`
- `usage`

Every observation or learned-memory update must include:

- `kind`
- `claim`
- `confidence` between `0` and `1`
- at least one resolvable `evidence_refs` item

Validation rejects missing evidence refs before any downstream memory write can
occur. Evidence refs are checked against the submitted evidence bundle, not
against every event in the project. Confidence below `0.5` is classified as
pending rather than active.
If `learned_memory_updates` is absent, validated `claims` are used as the
candidate learned-memory source before falling back to `observations`.
