# Completed-session evidence for library consumers

The `mmr::store::evidence` module provides local, bounded evidence selection for
applications preparing assimilation proposals. It adds no worker, runner, CLI
command, scheduler or provider call. `mmr assimilate project/source` continues to
return its stateless prompt/runbook/evidence response.

A trusted application explicitly attests a current session revision, then seals
a bounded selection of completed sessions. Idle time, imported role labels and
model claims cannot establish completion, permissions or authority. Snapshot
reads return **local raw history**; the caller must separately authorize any
projection, model transmission or execution. Blob metadata is fenced, but this
API does not read or verify external attachment files behind a storage reference.

```rust,ignore
use mmr::store::evidence::EvidenceBudget;

let budget = EvidenceBudget::default(); // <=64 sessions,4096 rows,4 MiB
store.attest_session_completion(&session_id, "trusted:lifecycle-complete", budget)?;
let snapshot = store.create_evidence_snapshot(
    &project_id, &[session_id], "my-projection-policy:v1", budget,
)?;
let local = store.read_current_evidence_snapshot(&snapshot.id, budget)?;
// Interpret local.events only as untrusted data. No runner is called here.
let receipt = store.commit_snapshot_proposals(&snapshot.id, proposed_output, budget)?;
```

Selection fails when a session is open, evidence is stale, or any configured
budget is exceeded. All source rows in selected sessions count toward the row
budget, including revoked rows. String sizes use UTF-8 bytes, including related
project/session/source/blob/redaction-policy metadata. Semantic JSON
serialization is independently bounded. Typed proposal input is already owned by
the caller; the 1 MiB result cap bounds further serialization/validation work, not
memory the caller already allocated.

Receipt creation validates the snapshot and inserts pending/rejected proposals
within one SQLite immediate transaction. Every observation collection uses the
existing schema/citation validator. A valid citation identifies a selected row;
it does not prove support, truth or semantic entailment. A receipt never writes
learned memory or activates a model claim. Review unsupported inferences,
contradictions, scope, updates and abstention separately. Historical receipts
remain inspectable using `evidence_proposal_receipt`, but retrying a receipt
against stale evidence fails, even when its output is identical.

For incremental consumers, `evidence_changes` uses a bounded latest-state feed:

1. Start with the previously acknowledged `EvidenceCursor { watermark,
   session_id: None }`, or `None` for the first pass. Capture the returned
   `through` watermark.
2. Follow each returned `next` cursor, always passing the same `through`.
   The cursor is `(sequence, session_id)` because shared metadata changes and
   migration backfills can give several sessions the same sequence.
3. Once `next` is `None`, the complete window may be acknowledged by persisting
   `EvidenceCursor { watermark: through, session_id: None }`. A row moving past
   `through` between pages is returned in the next window. Open sessions get a
   new sequence when later completed. This feed is not an immutable audit log.

`revoke_evidence_event(project_id, event_id, reason)` retains the raw row and
records an immutable Store-wide event-ID exclusion. The project is attesting
provenance, not an exclusion boundary. Revocation may precede import. Direct SQL
physical event deletion also records a tombstone, including session/project
cascades. Deleting/reinserting the same ID, even under another project, cannot
make it eligible again. This is local snapshot exclusion; it does not physically
erase history, retract existing memory, or delete remote copies.

Store-owned handles enable both `PRAGMA foreign_keys=ON` and
`PRAGMA recursive_triggers=ON`. External SQL writers must set **both on each
connection before mutation** (foreign-key configuration must precede a
transaction). Default SQLite connections are outside this fence contract.
Recursive deletion triggers are required for `REPLACE`, including event-ID and
row-ID replacement, parent cascades and policy dependency replacement. Detected
count mismatches fail closed, but cannot compensate for an unconfigured writer.

Configured raw semantic edits, appends, deletion and dependency changes invalidate current
completion and snapshots transactionally, including equal timestamps. Duplicate
Store replay and operational sync-status updates do not invalidate them. The
fences cover configured Store/SQLite source mutations; they are not an access
control barrier against an actor replacing the DB or disabling its triggers.
Source/session identity renames are rejected; migration does not infer completion
from legacy `ended_at` values.

A future job worker must combine snapshot validation with its lease generation,
lease expiry, cancellation flag, proposal insert and cursor acknowledgement in
one immediate transaction. The crate-internal snapshot validator exists for that
integration. The standalone receipt has no job lease or cancellation semantics.

Run the synthetic contract with `cargo test --test evidence_contract`. The opt-in
measurement entry point is
`cargo test --test evidence_contract benchmark_completed_evidence_selection_and_receipt -- --ignored --nocapture`.
It reports import, seal/snapshot, current-read and receipt times for 200 selected
events out of 2,000 total; it asserts preservation/selection counts, not a timing
threshold or a speedup claim. Schema-v2 backfill, lock/rollback and bounded stored
metadata tests are in `src/store/evidence/tests.rs`.

Contract: [completed evidence fences](../specs/assimilation-evidence.md).
