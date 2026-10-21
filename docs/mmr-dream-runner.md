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

## Proposal Trust And Activation

History and tool output in an evidence bundle are untrusted data, including text
that claims to be a system instruction or an authorization. They cannot grant
permissions, authorize execution or change the caller's instruction hierarchy.
An existing citation proves reference membership, not factual support.

Library runner validation keeps every non-rejected observation, claim, learned
memory update and counterevidence proposal pending. Model confidence, including
1.0, and a model-requested `active` status cannot activate memory. Requested
activation is downgraded with an idempotent diagnostic warning; rejected stays
rejected. Callers must separately review support, scope, newer corrections and
counterevidence, and abstain when evidence is insufficient.

`Store::complete_dream_run` and sync hydration are trusted persistence primitives
that preserve explicitly supplied reviewed statuses and enforce evidence scope.
They do not perform semantic-support checks and must not receive raw model
output as an activation decision. Existing active records are not rewritten.
See the [canonical contract](../specs/assimilation.md).

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

Candidates should be proposed as pending or rejected; any activation requires a
separate trusted review decision. The agent should reject or quarantine claims that are unsupported, secret-bearing,
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

## Library command-runner limits

The lower-level Rust `CommandDreamRunner` remains available to library callers;
`mmr assimilate` does not invoke it. `CommandDreamRunner::new(program, args)`
passes structured arguments directly to the executable. The legacy
`MMR_DREAM_COMMAND` constructor retains whitespace splitting; it does not expand
shell syntax, quoting or environment variables.

`CommandRunnerLimits` can be supplied with `with_limits`. Defaults for one run:

| Budget | Default |
|---|---|
| Execution, including request writes and both output streams | 60 seconds |
| Direct-child termination/reaping after execution | 2 seconds |
| Serialized request, including the terminating newline | 16 MiB |
| stdout | 8 MiB |
| stderr | 2 MiB |

These limits count serialized/streamed bytes, not exact resident-memory usage;
vector capacity, JSON parsing and runtime overhead add memory costs.

Request serialization enforces its byte budget before spawning. Once spawned,
stdin is written while stdout and stderr are drained concurrently. stderr is
counted and discarded; stdout is retained only within its budget. Exceeding a
budget fails the whole operation. No partial result is returned or stored.
`CommandRunnerError` exposes the limit/deadline/failure category without echoing
request data, command arguments or subprocess output, including invalid JSON
and nonzero-exit errors.

The synchronous runner uses a private Tokio runtime on a scoped worker thread,
so calling it from an existing Tokio runtime does not panic. It still waits for
completion; async callers should use `tokio::task::spawn_blocking` to avoid
blocking their executor thread.

On Unix, each invocation starts in a dedicated process group. Cleanup terminates
that group, including ordinary descendants retaining inherited pipe handles,
and reaps the direct child. The group leader remains unreaped until group
termination, so its PID/group ID cannot be reused before a later pipe timeout.
This requires Unix `waitid` with `WNOWAIT`; unsupported platforms return an
explicit error after direct-child cleanup. The host must leave child reaping to
this runner, without a competing `SIGCHLD` handler or other child reaper. If
`ECHILD` reveals ownership loss, cleanup disarms the IDs and sends no further
PID-based signals. On Apple platforms, the documented zombie-only group `EPERM`
case is accepted only after confirming the leader has exited. Descendants that deliberately create a new process
group/session escape this cleanup scope: this runner is a transport, not a
sandbox. On non-Unix platforms, async pipe cancellation still closes local pipe
handles and cleanup kills/reaps the direct child; descendant termination is not
guaranteed. Neither platform uses detached blocking pipe-reader threads.

Synthetic Unix regression tests cover verbose stdout/stderr before request
consumption, request/output limits, blocked stdin, early exit, malformed output,
inherited descriptors, direct-child reaping, descendant termination and calls
from an existing Tokio runtime. They use explicitly selected local fixture
commands and no provider credentials or real session histories.
