# mmr Assimilation Guide

`mmr assimilate` is a handoff command for AI agents. It prepares a shared-safe
evidence bundle and returns a system prompt, runbook, output contract, and
guardrails. The calling agent then performs memory deduplication, knowledge
assimilation, and generalisation in its own reasoning context.

## Boundary

`mmr assimilate` is stateless. It does not:

- run a mock, command, or provider-backed AI runner
- read legacy runner environment variables
- create run or candidate rows
- write learned memory

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

## Sync And Hydration

Active learned-memory rows created through store-level paths are still synced as
dedicated remote learned-memory payloads. During sync, evidence refs are remapped
from local event ids to redacted remote event ids that will exist on a fresh host.
Learned memory whose evidence cannot be synced is skipped rather than uploaded
with dangling refs.

Hydration replays remote events first, then learned-memory payloads, so hydrated
learned memory points at resolvable `mmr://event/...` citations. `mmr find` also
inspects active learned-memory rows directly.
