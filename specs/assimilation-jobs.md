# Bounded local assimilation jobs — frozen behavioral boundary

This extends the existing Store, evidence seals/receipts and command transport.
It is a project-specific queue and finite invocation, not a daemon, scheduler or
agent framework. Existing `assimilate project/source` remains stateless.

## Explicit configuration and input

The new namespace is `mmr assimilate jobs`. It reads only an explicitly supplied
bounded JSON config file. No default history/config discovery or ambient dream,
summarize, Morph, endpoint or credential environment selection occurs. Missing
config or `enabled=false` makes enqueue/run return `disabled` before Store open
or runner creation. Inspection/cancellation with an explicit disabled config is
still allowed so execution can be turned off without losing control.

Config contains a version, enabled flag, explicit database path and project ID,
policy version, structured absolute executable/argv and runner version, and
validated budgets. Unknown fields, excessive file/field/argument sizes and invalid
budgets fail with sanitized errors. Program/argv and arbitrary command diagnostics
are not copied into job rows/logs. Store a stable policy fingerprint instead.
Config enabling an explicit command authorizes that program for shared-safe
evidence; the program is trusted code with the operator's privileges, not a
network/ filesystem sandbox. The worker clears inherited environment; this does
not prevent an executable reading files or networking. Actual experiments use
synthetic histories and fake local commands only.

Enqueue selects an explicit ordered set of session IDs. They must be currently
sealed; `--attest-complete LABEL` is an optional explicit trusted caller decision
to seal them first. No idle-time/model inference. Snapshot creation uses the
existing bounded evidence API. No automatic discovery cursor or session ingest
is introduced in this first slice.

## Queue states and identity

Use the existing SQLite database and one small jobs table. State:
`queued -> claimed -> executing -> proposed | failed | stale | cancelled | unknown`.
An execution-intent barrier is committed before any process launch. `unknown`
means execution/cleanup may have happened and requires reconciliation; there is
no automatic retry or generic retry/reconciliation framework in this slice.

The unique key binds format version, Store epoch/project, snapshot ID/digest,
projection/prompt policy and explicit command policy fingerprint. Persist effective
bounded execution settings/fingerprint, attempts/fence, lease/owner, cancellation,
terminal diagnostic code and receipt ID. No second result blob or unbounded journal.
Enqueue of an existing key returns its existing state, including terminal/unknown.
Bound queue admissions and inspection rows; caps and exact types must be aligned
before source changes and documented alongside the implementation.

## Claims, recovery, cancellation and commits

Claims use short `BEGIN IMMEDIATE` transactions with a fresh owner and monotonic
fence. Only queued or expired **pre-launch claimed** work can be claimed. Attempts
are capped. Reclaim invalidates every older token. An executing job is never
claimed again automatically: after owner/lease loss it becomes unknown, even if
the owner may have died between the intent barrier and actual spawn. This false
positive is safer than replaying uncertain external execution.

`begin_execution` verifies current token, lease, cancellation, policy and evidence
before committing the launch barrier. Proposal completion again verifies token,
unexpired lease, executing state, no cancellation and current snapshot in the
**same immediate transaction** as normalized receipt insertion and proposed-state
update. Reuse the evidence receipt implementation through a dedicated internal
transaction helper; do not expose generic SQL callbacks. Stale evidence cannot
produce a fresh receipt. No model-active or learned-memory writes.

Cancel is durable/sticky. Queued/pre-launch cancellation can become terminal
without launch. Executing cancellation remains a request until owned cleanup is
joined; cleanup uncertainty becomes unknown, not a false stopped acknowledgement.
Expired/lost leases prevent publication. A finite controller polls only indexed
job control and requests the existing cancellation token on cancel/lost control.
It always joins; no detached blocking tasks/readers/controllers remain.

Known failed executions are not automatically retried either. Diagnostics are
bounded categorical codes, never captured command output/arguments. Status/show
distinguish cancellation request, cleanup acknowledgement and unknown outcome.
Pending receipts remain inspectable. Publishing/activating memory is out of scope.

## Finite worker and CLI

Commands: `enqueue`, `run`, `status`, `show JOB_ID`, `cancel JOB_ID`, each with
explicit `--config`; enqueue accepts repeated `--session` and optional completion
attestation. `run --max-jobs N` defaults to1 and supports a small bounded drain.
Inspection is bounded and scoped to config project; proposed output is an explicit
show operation, not repeated in queue listings. Early CLI dispatch precedes history
loading. Existing assimilation/continuation behavior is unchanged.

Use conservative defaults below the evidence hard caps: <=8 sessions,2000 events,
2 MiB evidence/request,256 KiB result,64 KiB stderr; <=60s execution,2s cleanup,
120s invocation. Lease exceeds execution+cleanup+control margin. A new job starts
only with enough remaining invocation time to cover its configured execution and
cleanup. Default max-jobs1, hard cap8. No launch of arbitrary imported tool calls.

The worker projects selected snapshot rows through existing deterministic privacy
rules, records omissions/policy identity and supplies only shared-safe evidence.
No silent source truncation; no eligible evidence produces an explicit terminal
disposition. Whole-project unbounded evidence builders are not used.

## Required proof and honest limits

Tests must distinguish crash before launch (safe bounded reclaim) from crash
during/after external execution (unknown, no retry), including an owned real
process fixture. Leases fence database writes; they do not stop an orphan command
or provide exactly-once external model execution. Keep that limitation explicit
unless a small tested platform-specific containment mechanism is implemented.
Do not kill a PID read from the database.

Also test enqueue dedup, concurrent claimers, expired tokens, cancellation races,
stale/deleted evidence before receipt, model-active normalization, budgets/drain
deadline, disabled/ambient-poison zero launch, safe errors, restart with identical
receipt identity, and all previous preservation/Store/native/CLI contracts.

## Aligned implementation caps and modules

- One `assimilation_jobs` table in Store schema4. Hard admission256 rows/project,
  4096 rows/store, including terminal rows; duplicate lookup precedes cap checks.
  No automatic deletion, GC or retry framework.
- Default claim attempts3, hard cap8. Lease default65s, hard cap90s, at least
  execution+cleanup+1s. No heartbeat/renewal. Production clock checks occur after
  SQLite transaction lock acquisition and again before final receipt/state commit.
- Policy/settings JSON <=4 KiB. Launch projection metadata <=256 KiB and partitions
  the exact snapshot IDs into included IDs and categorical omissions. Receipt
  citations are restricted to included IDs. Listings <=100 rows/1 MiB and omit
  projection/model output; explicit show may include the bounded pending receipt.
- Explicit config <=32 KiB, IDs/policy/runner versions <=128 bytes, DB/program paths
  <=4096 bytes, <=32 arguments of<=1024 bytes and<=16 KiB total, no NUL. Structured
  absolute executable, clean inherited environment plus PATH=/usr/bin:/bin.
- Public orchestration entry: `assimilation_jobs::execute(Option<&Path>, JobsAction)`
  returns a typed bounded response. CLI only maps arguments and serializes it.
  Explicit inapplicable `--source` is a usage error before execution; ambient default
  source config is ignored. Missing/disabled config with valid job arguments is disabled.
- Store module: `store::jobs`; orchestration module: `assimilation_jobs`. The evidence
  receipt extraction remains internal and preserves the old public evidence API.
