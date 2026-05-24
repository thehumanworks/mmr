# mmr Dream Runner

NHL-278 adds the provider-neutral runner layer. NHL-279 wires that layer into
the public `mmr dream` assimilation workflow.

## Boundary

Dreaming is stateful learned-memory assimilation, not summarization. The runner
layer prepares evidence, routes it to a provider, parses structured output, and
validates evidence refs. The `mmr dream` command owns durable writes to
`dream_runs`, `dream_candidates`, and `learned_memory`.

## Command Workflow

`mmr dream` analyzes the linked current project by default. Use `--project` to
target another linked project path.

Useful flags:

- `--dry-run`: validate proposed learned-memory changes without writing a dream
  run, candidates, or learned memory.
- `--review`: return the same non-mutating proposal shape with review status.
- `--runner mock|command`: choose the runner. The mock runner reads
  `MMR_DREAM_MOCK_OUTPUT` when set and otherwise emits a deterministic pending
  diagnostic proposal. `MMR_DREAM_MOCK_FAILURE` forces a mock failure in tests.
- `--model`: record the provider model identifier on non-dry-run dream runs.
- `--evidence-mode shared-safe|local-raw`: shared-safe is the default. Raw
  evidence requires `--allow-raw-evidence` and is available only for mock/local
  experiments.

High-confidence, non-sensitive, counterevidence-free learned-memory updates are
written as active learned memory. Low-confidence or counterevidenced updates are
queued as internal candidates. Sensitive, identity-affecting, or PII-bearing
claims are rejected rather than applied.

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
jobs. The MVP rejects non-default values until a later ticket gives them explicit
execution semantics.

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
against every event in the project. Confidence below `0.8` is classified as
pending rather than active.
If `learned_memory_updates` is absent, validated `claims` are used as the
candidate learned-memory source. Plain `observations` are retained as internal
candidates/audit material and are not promoted directly to active learned
memory.

## Sync And Hydration

Active learned-memory rows are synced as dedicated remote learned-memory
payloads. During sync, evidence refs are remapped from local event ids to the
redacted remote event ids that will exist on a fresh host. Learned memory whose
evidence cannot be synced is skipped rather than uploaded with dangling refs.

Hydration replays remote events first, then learned-memory payloads, so hydrated
learned memory points at resolvable `mmr://event/...` citations. `mmr search`
also inspects active learned-memory rows directly, so learned memory is
discoverable without adding a public context or candidate command.
