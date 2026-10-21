# Completed-session evidence fences

This is a local Store prerequisite, not a background worker. Existing public
`assimilate project/source` remains stateless. No jobs, scheduler, runner,
configuration, provider calls or learned-memory writes are introduced.

## Acceptance boundary

A trusted application may explicitly attest that the current local session
revision is complete. Bounded snapshots include only such sealed revisions and
exclude locally revoked or physically deleted event IDs. Proposed output can be stored as a local
pending/rejected receipt only after current evidence is checked in the same
SQLite `BEGIN IMMEDIATE` transaction as receipt insertion. This is an evidence
fence, not a future job lease/cancellation/watermark fence and not an authenticity
or semantic-support proof.

## Public library API (module `mmr::store::evidence`)

- `EvidenceBudget { max_sessions, max_events, max_bytes }` with validated hard
  caps (64 sessions, 4096 source rows, 4 MiB semantic metadata/content). Smaller
  caller limits allowed; no unlimited setting.
- `Store::evidence_high_watermark() -> EvidenceWatermark { store_epoch, sequence }`.
- `Store::evidence_changes(project_id, after, through, budget)` returns a bounded
  page ordered by monotonic change sequence, including open/deleted/revoked
  sessions. Initial `through=None` captures a fixed high-water; subsequent pages
  use it. Completion/reopen/revocation gets a new sequence, so an earlier open
  session can be reconsidered after a worker advances its watermark.
- `Store::attest_session_completion(session_id, attestation, budget)` returns a
  seal of the current revision and full semantic digest. Attestation is an
  explicit bounded trusted-caller label, never a model statement or idle timer.
  Repeating the identical attestation at the identical revision returns the
  same durable seal and does not bump sequence.
- `Store::revoke_evidence_event(project_id, event_id, reason)` records a durable
  local tombstone idempotently. It may precede (re)import; no raw row is deleted. Tombstones key globally on event ID within this Store; project is provenance, not an exclusion boundary.
- `Store::create_evidence_snapshot(project_id, ordered_session_ids,
  projection_policy_version, budget)` requires distinct sealed current sessions
  and persists a deterministic immutable metadata snapshot. The response records
  store epoch, policy tag, ordered session completion revisions/control sequence,
  ordered event IDs and full semantic hashes; no timestamp-only cursor.
- `Store::read_current_evidence_snapshot(snapshot_id, budget)` validates current
  seals/revisions/tombstones and returns bounded local source evidence. The data
  is local raw evidence: a future runner must separately enforce shared-safe
  projection and provider authorization. Snapshot rows do not duplicate raw text.
- `Store::commit_snapshot_proposals(snapshot_id, DreamRunnerOutput, budget)`
  validates schema/citations against exactly selected evidence and normalizes
  model output through the existing pending/rejected guardrail. It persists an
  idempotent local proposal receipt, never a learned-memory row. Result envelope
  has an independent hard cap of 1 MiB. No job lease semantics are implied. `evidence_proposal_receipt(id)` reads a
  bounded historical receipt even after its evidence becomes stale.
- `pub(crate) validate_snapshot_on_tx(...)` is available for a future worker's
  same-transaction lease-generation/expiry/cancel check, proposal insertion and
  cursor acknowledgement; no generic public SQL callback API is added.

The standalone receipt is an evidence fence only. A future worker must combine
lease generation/expiry, cancellation and watermark acknowledgement with this
validation in the same immediate transaction.

## Schema v3

Five bounded-purpose tables inside the existing database:

1. `evidence_clock`: persistent random store epoch and monotonic control sequence.
2. `evidence_sessions`: persistent session/project identity, current revision,
   latest change sequence, source-row count/semantic-byte counters, presence and
   optional current completion seal (revision/digest/attestation/control seq).
   No cascading foreign key: revisions survive session/project deletion.
3. `evidence_revocations`: global local-Store `event_id` tombstones with attesting-project provenance, reason and control
   sequence, no cascading foreign key; same-ID reinsertion never clears them.
4. `evidence_snapshots`: deterministic id, project/epoch/policy, immutable
   bounded metadata JSON and digest. JSON stores ordered seals/refs/hashes,
   not a raw-history copy. Metadata length checked before loading from SQLite.
5. `evidence_proposal_receipts`: deterministic snapshot/output-hash identity,
   bounded normalized proposal JSON, created time; duplicate replay returns the
   same receipt. Pending/rejected proposals only, no automatic activation.

Indexes support `(project_id, changed_seq, session_id)` feed selection and
session event lookup. One-time migration backfills scalar session counters and
revisions; snapshot creation does not rescan an unbounded project.

SQLite triggers fence relevant event insertion, semantic update and physical
deletion (including configured direct SQL/concurrent handles), session deletion and semantic
metadata change, and revocation. Persistent ledger revisions are monotonic;
delete/reinsert cannot recreate an old completion generation. Event count/byte
counters allow preflight rejection before source-string allocation. Conservative
budgets count all source rows in selected sessions, including revoked rows.
Related blob/redaction-policy changes affecting evidence must invalidate seals.
Operational updated-at/sync-status-only changes do not affect semantic evidence.

Idempotent Store event replay leaves session semantic metadata unchanged. The
existing append-only event identity no-op now precedes incidental session
upserts, so duplicate batches do not reopen completed sessions.
Source/session identity renames are rejected. Event row-order edits are semantic
changes because snapshots use explicit session order and event insertion order.

## Semantic identity and ordering

Hash the store epoch, projection policy tag, project/session/source identities,
completion revisions and ordered IDs together with complete event semantics:
role/type, exact timestamp (including equal values), content/hash, parent,
parser version, raw provenance, blob identity/semantic metadata and registered
redaction-policy id/version. Use unambiguous versioned serialization, not string
concatenation or model output. Operational sync status/time does not grant
completion. Missing/deleted/revoked/modified evidence makes prior snapshots stale.

## Revocation and durability

Revocation means Store-wide exclusion by event ID from these local completed-evidence snapshots. It
retains raw event history and durable tombstones; direct physical event deletion
also records an immutable ID tombstone, including cascades and later reimport
into another project. It does not promise physical or global remote deletion,
rewrite existing memories or retract remote copies. Such
work requires an explicit later retention/deletion workflow. Completion and
snapshot records are local attestations, not signed provider lifecycle proofs.

## Required tests

Crash/reopen and duplicate seal/snapshot/receipt; append/update with equal
timestamps; completion invalidation; revocation before/after snapshot and before
reimport; physical event/session/project deletion and reinsertion; two Store
handles racing snapshot vs commit; bounded counts/metadata/content/result bytes;
parser/redaction-policy/blob hash sensitivity; schema-v2 migration, checksum
replay and old fixture suite. Public assimilation must remain provider-free and
stateless throughout.

## Resource and trust boundaries

- Global local-Store event-ID tombstones include original project provenance. Event DELETE triggers tombstone IDs too; physical delete/reinsert (including cross-project or cascades) never re-enables the deleted ID.
- Change cursors use `(sequence, session_id)` within a fixed through watermark, because one dependency change can affect several sessions at the same sequence. Completed/open/deleted states are reconsidered after newer changes.
- All preflight sizes use UTF-8 bytes, including project/session/source/blob/policy metadata. Typed proposal input is already caller-owned; serialization limits bound further work/allocation, not allocation already performed by its caller.
- Existing receipts remain inspectable after staleness, but even duplicate commit requests must validate current evidence before returning success.

Store handles enable `PRAGMA foreign_keys=ON` and `PRAGMA recursive_triggers=ON`.
External SQL writers must enable **both on each connection before mutations**;
SQLite defaults are insufficient. Supported `REPLACE` deletion/reinsertion must
fire recursive deletion triggers, including policy dependency invalidation.
Unconfigured external handles are outside this guarantee; detectable source-count
mismatches fail closed but do not replace the connection requirement.

Fences cover configured database source mutations, not replacement of the database
or deliberate removal of its triggers/control records. External attachment bytes
are not read; blob identity/hash/storage metadata is included as recorded. See
[the library guide](../docs/mmr-evidence-fences.md) for paging and trust boundaries.
