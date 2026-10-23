# mmr Assimilation Guide

`mmr assimilate` is a handoff command for AI agents. It prepares a shared-safe
evidence bundle and returns a system prompt, runbook, output contract, and
guardrails. The calling agent then performs memory deduplication, knowledge
assimilation, and generalisation in its own reasoning context.

Consolidation should first consider a small deterministic test, check, script or
tool for a recurring lesson. Keep the lesson and its provenance even when code
is proposed. Where judgement or context cannot be encoded reliably, keep one
short scoped lesson and explain why. Identify redundant/obsolete instructions
with cited replacement evidence; do not delete raw history or silently rewrite
instructions.

## Boundary

`mmr assimilate` is stateless. It does not:

- run a mock, command, or provider-backed AI runner
- read legacy runner environment variables
- create run or candidate rows
- write learned memory
- execute, verify, or install a proposed check

The local store still contains historical run and learned-memory tables for
sync, hydration, and lower-level store contracts, but the public command no
longer mutates those tables.

## Project Workflow

`mmr assimilate project` analyzes the linked current project by default. Use
`--project` to target another linked project path.

```bash
mmr assimilate project --pretty
mmr assimilate project --project /path/to/project --pretty
```

Useful flags:

- `--project <path>`: choose the linked project whose evidence should be
  included.
- `--evidence-mode shared-safe|local-raw`: shared-safe is the default. Raw
  evidence requires `--allow-raw-evidence`.
- `--allow-raw-evidence`: explicit local-only opt-in for raw evidence.

## Source Workflow

`mmr assimilate source` prepares a harness-wide bundle across projects. It
requires an explicit global source filter so the subject is unambiguous.

```bash
mmr --source codex assimilate source --pretty
```

Useful flags:

- `--per-project-limit <N>`: bound the evidence retained from each project before
  projection.
- `--since <rfc3339>`: keep only events at or after a timestamp.
- `--evidence-mode shared-safe|local-raw` and `--allow-raw-evidence`: same
  privacy controls as project assimilation.

## Response

The JSON response includes:

- `mode: "prompt_runbook"`
- `system_prompt`
- `runbook`
- `output_contract`
- `guardrails`
- `suggested_commands`
- `evidence.events` with `mmr://event/...` refs and projected content
- `evidence.omitted` for events omitted by the privacy boundary

## Evidence Privacy

Shared-safe bundles:

- redact deterministic local PII such as private email addresses
- omit events blocked by deterministic secret findings
- include `mmr://event/<id>` citations, normalized metadata, project identity,
  and redacted content

The calling agent must not treat omitted evidence as reviewed and must not infer
private facts from redacted placeholders.

## Agent Output Contract

The returned output contract asks the calling agent to produce:

- evidence reviewed
- deduplication groups
- memory candidates
- counterevidence or rejections
- application plan

Each memory candidate should include:

- `kind`
- `claim`
- `scope`
- `status`
- `confidence`
- `evidence_refs`
- `counterevidence_refs`
- `target_surface`

The agent should reject or quarantine claims that are unsupported, secret-bearing,
identity-affecting, too narrow to reuse, contradicted by newer evidence, or better
represented as a transient task than durable memory.

## Review a codification proposal

After inspecting the handoff, the calling agent can write JSON and ask MMR to
review it against the same project or source evidence window:

```bash
mmr assimilate project --review proposal.json
mmr --source codex assimilate source --review proposal.json
```

Without `--review`, the handoff is unchanged except for codification guidance.
With it, the response additionally includes `reviewed_output` in the existing
dream observation shape. Minimum input:

The earlier report sections and `target_surface` describe an agent's narrative
handoff; they are not fields in review JSON. Use the observation shape below.

```json
{
  "observations": [{
    "kind": "workflow",
    "claim": "Retry counts must never exceed three.",
    "confidence": 0.9,
    "scope": "<project_id from supplied evidence>",
    "evidence_refs": ["<first supplied ref>", "<second supplied ref>"],
    "codification": {
      "state": "proposed",
      "target": "tests/retry_limit.rs",
      "expected_behavior": "A fourth retry is rejected.",
      "validation": "Run the retry-limit test with both three and four attempts."
    }
  }]
}
```

The review accepts at most 64 KiB JSON and 16 total observations (including
claims/updates/counterevidence in the existing format). Claims, scope, rationale,
target, expected behavior and validation text are bounded to 2,000 characters.
Every observation needs a project-ID scope matching all cited supporting and
counterevidence events in the current projected bundle. Even a source-wide
review produces project-scoped candidates; wider application needs separate
review in each repository. Codification requires at least two distinct supplied
event refs (recurrence is a claim for the agent to assess, not proven by counting
refs). The target is a nonempty project-relative path without traversal.

For non-codifiable lessons omit `codification`, use `kind: "judgment"`, and give
`recommended_action` explaining why judgement remains necessary. `duplicate`
and `superseded` kinds likewise require a rationale and counterevidence refs
identifying the supporting replacement/correction. These are recommendations;
no instruction or lesson is removed.

`state` must be `proposed`; claiming `verified` or `installed` is rejected.
Observation `status: active` describes confidence in the lesson, never test or
installation status. For `kind: superseded`, that is confidence in the assessment
that an old instruction is superseded, not a direction to activate that instruction.
Validation text is inert: review confirms structure and
evidence membership, not semantic truth, test success or safe code. Only a
separately reviewed implementation and actual check results can establish those.
Review fails nonzero without returning a partially accepted plan and never
executes commands, creates targets, or writes memory. Keep the returned proposal
and evidence with that implementation's review if pursuing it.

Try it with synthetic history (Python 3 standard library, temporary HOME):

```bash
cargo build --locked
python3 examples/dream_codification.py
```

Expected output:

```text
Recurring lesson: retry cap, 2 supporting events retained.
Check: tests/retry_limit.py — proposed, not verified or installed.
Judgment lesson retained: adapt explanations to the audience.
No check executed or installed; evidence and native histories unchanged.
Temporary home removed.
```

The proposal in this example is authored by the fixture, not by a live model.
For an independent agent's choices from conflicting synthetic history and a
runnable fail/pass check, see the [agent exercise](mmr-dream-agent-exercise.md).
It records developer verification; product codification remains `proposed`.
The new file-review option is CLI-only; existing MCP/HTTP handoff tools receive
the updated guidance but do not expose file-reading review arguments.

## Sync And Hydration

Active learned-memory rows created through store-level paths are still synced as
dedicated remote learned-memory payloads. During sync, evidence refs are remapped
from local event ids to redacted remote event ids that will exist on a fresh host.
Learned memory whose evidence cannot be synced is skipped rather than uploaded
with dangling refs.

Hydration replays remote events first, then learned-memory payloads, so hydrated
learned memory points at resolvable `mmr://event/...` citations. `mmr find` also
inspects active learned-memory rows directly.
