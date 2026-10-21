//! Synthetic durable queue contracts. All raw SQL is isolated fixture control.
use mmr::dream::{DreamObservationStatus, DreamRunnerOutput};
use mmr::store::evidence::EvidenceSnapshot;
use mmr::store::jobs::*;
use mmr::store::{NewEvent, Store, content_hash};
use rusqlite::{Connection, params};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
fn limits() -> JobLimits {
    JobLimits::default()
}
fn policy() -> JobPolicy {
    JobPolicy {
        projection_policy_version: "synthetic-shared-safe:v1".into(),
        prompt_policy_version: "synthetic-prompt:v1".into(),
        runner_fingerprint: content_hash("synthetic explicit command"),
    }
}
fn millis() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}
struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    project: String,
    store: Store,
    session: String,
    event_ids: Vec<String>,
    snapshot: EvidenceSnapshot,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let project = store.ensure_project_link(dir.path()).unwrap().id;
        let mut ids = Vec::new();
        let mut session = String::new();
        for text in ["synthetic included narrative", "synthetic omitted row"] {
            let event = store
                .insert_event(
                    &project,
                    &NewEvent::new(
                        "codex",
                        "session",
                        "message",
                        "user",
                        "2026-01-01T00:00:00Z",
                        text,
                        "fixture:v1",
                    ),
                )
                .unwrap();
            session = event.session_id;
            ids.push(event.id);
        }
        store
            .attest_job_sessions(
                &project,
                std::slice::from_ref(&session),
                "trusted fixture completion",
                limits(),
            )
            .unwrap();
        let snapshot = store
            .create_evidence_snapshot(
                &project,
                std::slice::from_ref(&session),
                &policy().projection_policy_version,
                limits().evidence_budget(),
            )
            .unwrap();
        Self {
            _dir: dir,
            path,
            project,
            store,
            session,
            event_ids: ids,
            snapshot,
        }
    }
    fn sql(&self) -> Connection {
        let db = Connection::open(&self.path).unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA recursive_triggers=ON;")
            .unwrap();
        db
    }
    fn enqueue(&mut self) -> JobSummary {
        self.store
            .enqueue_job(&self.project, &self.snapshot.id, &policy(), limits())
            .unwrap()
    }
    fn claim(&mut self) -> JobClaim {
        self.store
            .claim_job(
                &self.project,
                &policy().fingerprint(&limits()).unwrap(),
                "fixture-owner",
            )
            .unwrap()
            .unwrap()
    }
    fn projection(&self) -> JobProjection {
        JobProjection {
            policy_version: policy().projection_policy_version,
            evidence_hash: content_hash("bounded synthetic shared-safe request"),
            included_event_ids: vec![self.event_ids[0].clone()],
            omissions: vec![JobOmission {
                event_id: self.event_ids[1].clone(),
                reason: JobOmissionReason::PolicyBlocked,
            }],
        }
    }
    fn launch(&mut self) -> JobClaim {
        self.enqueue();
        let claim = self.claim();
        let projection = self.projection();
        self.store
            .begin_job_execution(&self.project, &claim.lease, &projection)
            .unwrap();
        claim
    }
    fn receipt_count(&self) -> i64 {
        self.sql()
            .query_row("SELECT count(*) FROM evidence_proposal_receipts", [], |r| {
                r.get(0)
            })
            .unwrap()
    }
}
fn output(id: &str) -> DreamRunnerOutput {
    let item = json!({"kind":"observation","claim":"Synthetic scoped proposal","confidence":0.99,"evidence_refs":[format!("mmr://event/{id}")],"status":"active"});
    serde_json::from_value(json!({"observations":[item.clone()],"claims":[item.clone()],"learned_memory_updates":[item.clone()],"counterevidence":[item]})).unwrap()
}
fn expire(f: &Fixture, lease: &mut JobLease) {
    f.sql()
        .execute(
            "UPDATE assimilation_jobs SET lease_expires_at_ms=0 WHERE id=?1",
            [&lease.job_id],
        )
        .unwrap();
    lease.lease_expires_at_ms = 0;
}

#[test]
fn queue_existing_only_open_dedup_reopen_and_policy_identity() {
    let mut f = Fixture::new();
    let missing = f._dir.path().join("missing.sqlite3");
    assert!(Store::open_job_store(&missing).is_err());
    assert!(!missing.exists());
    let empty = f._dir.path().join("empty.sqlite3");
    std::fs::write(&empty, []).unwrap();
    assert!(Store::open_job_store(&empty).is_err());
    assert_eq!(std::fs::metadata(&empty).unwrap().len(), 0);
    let original = f.enqueue();
    let repeated = f.enqueue();
    assert_eq!(original, repeated);
    let mut reopened = Store::open_job_store(&f.path).unwrap();
    assert_eq!(
        reopened.job(&f.project, &original.id).unwrap().summary,
        original
    );
    assert_eq!(
        reopened
            .enqueue_job(&f.project, &f.snapshot.id, &policy(), limits())
            .unwrap(),
        original
    );
    let mut other = policy();
    other.prompt_policy_version = "different:v1".into();
    let new = reopened
        .enqueue_job(&f.project, &f.snapshot.id, &other, limits())
        .unwrap();
    assert_ne!(original.id, new.id);
    let mut other_limits = limits();
    other_limits.max_output_bytes = 4096;
    assert_ne!(
        reopened
            .enqueue_job(&f.project, &f.snapshot.id, &policy(), other_limits)
            .unwrap()
            .id,
        original.id
    );
    assert_eq!(reopened.jobs(&f.project, 100).unwrap().len(), 3);
}

#[test]
fn queue_two_claimers_get_one_fenced_owner() {
    let mut f = Fixture::new();
    let job = f.enqueue();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for owner in ["owner-a", "owner-b"] {
        let mut store = Store::open_job_store(&f.path).unwrap();
        let project = f.project.clone();
        let fingerprint = policy().fingerprint(&limits()).unwrap();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            store.claim_job(&project, &fingerprint, owner).unwrap()
        }));
    }
    barrier.wait();
    let results = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].lease.job_id, job.id);
    assert_eq!(results[0].lease.fence, 1);
    assert_eq!(results[0].job.summary.attempts, 1);
}

#[test]
fn queue_prelaunch_reclaim_fences_old_owner_and_attempts_are_bounded() {
    let mut f = Fixture::new();
    let job = f.enqueue();
    let mut first = f.claim();
    expire(&f, &mut first.lease);
    assert_eq!(
        f.store.job_control(&f.project, &first.lease).unwrap(),
        JobControl::Expired
    );
    let second = f.claim();
    assert_eq!(second.lease.fence, 2);
    assert_eq!(second.job.summary.attempts, 2);
    assert_eq!(
        f.store
            .begin_job_execution(&f.project, &first.lease, &f.projection())
            .unwrap_err(),
        JobStoreError::LostLease
    );
    assert_eq!(
        f.store
            .finish_job(
                &f.project,
                &first.lease,
                JobFinish::Failed(JobDiagnostic::ProjectionFailed)
            )
            .unwrap_err(),
        JobStoreError::LostLease
    );
    let mut second = second;
    expire(&f, &mut second.lease);
    let mut third = f.claim();
    assert_eq!(third.job.summary.attempts, 3);
    expire(&f, &mut third.lease);
    let recovery = f.store.recover_jobs(&f.project).unwrap();
    assert_eq!(recovery.exhausted, 1);
    assert!(
        f.store
            .claim_job(
                &f.project,
                &policy().fingerprint(&limits()).unwrap(),
                "owner-d"
            )
            .unwrap()
            .is_none()
    );
    let failed = f.store.job(&f.project, &job.id).unwrap().summary;
    assert_eq!(failed.state, JobState::Failed);
    assert!(!failed.may_have_executed);
    assert!(failed.cleanup_confirmed);
    assert_eq!(f.enqueue(), failed);
}

#[test]
fn queue_execution_barrier_is_single_use_and_expired_execution_stays_unknown() {
    let mut f = Fixture::new();
    let mut claim = f.launch();
    let executing = f.store.job(&f.project, &claim.lease.job_id).unwrap();
    assert!(executing.summary.may_have_executed);
    assert!(!executing.summary.cleanup_confirmed);
    assert!(executing.summary.execution_intent_at_ms.is_some());
    assert_eq!(
        f.store
            .begin_job_execution(&f.project, &claim.lease, &f.projection())
            .unwrap_err(),
        JobStoreError::InvalidState
    );
    expire(&f, &mut claim.lease);
    drop(f.store);
    f.store = Store::open_job_store(&f.path).unwrap();
    assert_eq!(f.store.recover_jobs(&f.project).unwrap().unknown, 1);
    assert!(
        f.store
            .claim_job(
                &f.project,
                &policy().fingerprint(&limits()).unwrap(),
                "retry-owner"
            )
            .unwrap()
            .is_none()
    );
    let unknown = f
        .store
        .job(&f.project, &claim.lease.job_id)
        .unwrap()
        .summary;
    assert_eq!(unknown.state, JobState::Unknown);
    assert!(unknown.may_have_executed);
    assert!(!unknown.cleanup_confirmed);
    assert_eq!(f.enqueue(), unknown);
    assert_eq!(f.receipt_count(), 0);
}

#[test]
fn queue_exact_projection_partition_and_included_citations_only() {
    let mut f = Fixture::new();
    f.enqueue();
    let claim = f.claim();
    let projection = f.projection();
    let mut missing = projection.clone();
    missing.omissions.clear();
    assert_eq!(
        f.store
            .begin_job_execution(&f.project, &claim.lease, &missing)
            .unwrap_err(),
        JobStoreError::InvalidInput
    );
    let mut duplicate = projection.clone();
    duplicate.omissions[0].event_id = duplicate.included_event_ids[0].clone();
    assert_eq!(
        f.store
            .begin_job_execution(&f.project, &claim.lease, &duplicate)
            .unwrap_err(),
        JobStoreError::InvalidInput
    );
    f.store
        .begin_job_execution(&f.project, &claim.lease, &projection)
        .unwrap();
    assert_eq!(
        f.store
            .job(&f.project, &claim.lease.job_id)
            .unwrap()
            .projection,
        Some(projection)
    );
    let mut invalid = output(&f.event_ids[0]);
    invalid.counterevidence[0].evidence_refs = vec![format!("mmr://event/{}", f.event_ids[1])];
    assert_eq!(
        f.store
            .complete_job(&f.project, &claim.lease, invalid)
            .unwrap_err(),
        JobStoreError::InvalidOutput
    );
    assert_eq!(f.receipt_count(), 0);
    let completion = f
        .store
        .complete_job(&f.project, &claim.lease, output(&f.event_ids[0]))
        .unwrap();
    assert_eq!(completion.job.state, JobState::Proposed);
    assert!(completion.job.cleanup_confirmed);
    let normalized: DreamRunnerOutput =
        serde_json::from_str(&completion.receipt.proposal_json).unwrap();
    for item in normalized
        .observations
        .iter()
        .chain(&normalized.claims)
        .chain(&normalized.learned_memory_updates)
        .chain(&normalized.counterevidence)
    {
        assert_eq!(item.status, DreamObservationStatus::Pending)
    }
    assert_eq!(f.receipt_count(), 1);
    assert_eq!(f.enqueue(), completion.job);
    assert_eq!(
        f.store
            .complete_job(&f.project, &claim.lease, output(&f.event_ids[0]))
            .unwrap_err(),
        JobStoreError::InvalidState
    );
    let mut reopened = Store::open_job_store(&f.path).unwrap();
    assert_eq!(
        reopened
            .job(&f.project, &completion.job.id)
            .unwrap()
            .summary,
        completion.job
    );
    assert_eq!(
        reopened
            .evidence_proposal_receipt(&completion.receipt.id)
            .unwrap(),
        completion.receipt
    );
    assert!(
        reopened
            .claim_job(
                &f.project,
                &policy().fingerprint(&limits()).unwrap(),
                "new-owner"
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.sql()
            .query_row("SELECT count(*) FROM learned_memory", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn queue_cancel_is_sticky_and_cleanup_ack_is_truthful() {
    for before_launch in [true, false] {
        let mut f = Fixture::new();
        f.enqueue();
        let claim = f.claim();
        if !before_launch {
            f.store
                .begin_job_execution(&f.project, &claim.lease, &f.projection())
                .unwrap();
        }
        let cancelled = f.store.cancel_job(&f.project, &claim.lease.job_id).unwrap();
        assert!(cancelled.cancel_requested);
        if before_launch {
            assert_eq!(cancelled.state, JobState::Cancelled);
            assert!(cancelled.cleanup_confirmed);
            assert!(!cancelled.may_have_executed);
            assert!(
                f.store
                    .begin_job_execution(&f.project, &claim.lease, &f.projection())
                    .is_err()
            );
        } else {
            assert_eq!(cancelled.state, JobState::Executing);
            assert!(!cancelled.cleanup_confirmed);
            assert_eq!(
                f.store.job_control(&f.project, &claim.lease).unwrap(),
                JobControl::CancelRequested
            );
            assert_eq!(
                f.store
                    .complete_job(&f.project, &claim.lease, output(&f.event_ids[0]))
                    .unwrap_err(),
                JobStoreError::Cancelled
            );
            let ack = f
                .store
                .finish_job(&f.project, &claim.lease, JobFinish::CancelledClean)
                .unwrap();
            assert_eq!(ack.state, JobState::Cancelled);
            assert!(ack.cleanup_confirmed);
            assert!(ack.may_have_executed);
        }
        assert_eq!(f.receipt_count(), 0);
        assert!(
            f.store
                .claim_job(
                    &f.project,
                    &policy().fingerprint(&limits()).unwrap(),
                    "retry"
                )
                .unwrap()
                .is_none()
        );
    }
    let mut f = Fixture::new();
    let claim = f.launch();
    f.store.cancel_job(&f.project, &claim.lease.job_id).unwrap();
    let unknown = f
        .store
        .finish_job(
            &f.project,
            &claim.lease,
            JobFinish::Unknown(JobDiagnostic::CleanupUnknown),
        )
        .unwrap();
    assert_eq!(unknown.state, JobState::Unknown);
    assert!(!unknown.cleanup_confirmed);
    assert!(unknown.cancel_requested);
}

#[test]
fn queue_completion_first_is_not_relabelled_cancelled() {
    let mut f = Fixture::new();
    let claim = f.launch();
    let completion = f
        .store
        .complete_job(&f.project, &claim.lease, output(&f.event_ids[0]))
        .unwrap();
    assert_eq!(
        f.store.cancel_job(&f.project, &claim.lease.job_id).unwrap(),
        completion.job
    );
    assert_eq!(f.receipt_count(), 1);
}

#[test]
fn queue_stale_before_barrier_or_receipt_has_no_new_receipt() {
    for after_launch in [false, true] {
        let mut f = Fixture::new();
        f.enqueue();
        let claim = f.claim();
        if after_launch {
            f.store
                .begin_job_execution(&f.project, &claim.lease, &f.projection())
                .unwrap();
        }
        f.store
            .revoke_evidence_event(&f.project, &f.event_ids[0], "synthetic exclusion")
            .unwrap();
        let error = if after_launch {
            f.store
                .complete_job(&f.project, &claim.lease, output(&f.event_ids[0]))
                .unwrap_err()
        } else {
            f.store
                .begin_job_execution(&f.project, &claim.lease, &f.projection())
                .unwrap_err()
        };
        assert_eq!(error, JobStoreError::StaleEvidence);
        let stale = f
            .store
            .job(&f.project, &claim.lease.job_id)
            .unwrap()
            .summary;
        assert_eq!(stale.state, JobState::Stale);
        assert_eq!(stale.may_have_executed, after_launch);
        assert!(stale.cleanup_confirmed);
        assert_eq!(f.receipt_count(), 0);
        assert_eq!(f.enqueue(), stale);
    }
}

#[test]
fn queue_known_failure_is_terminal_and_uncertainty_cannot_claim_clean_failure() {
    let mut f = Fixture::new();
    let claim = f.launch();
    assert_eq!(
        f.store
            .finish_job(
                &f.project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::CleanupUnknown)
            )
            .unwrap_err(),
        JobStoreError::InvalidInput
    );
    let failed = f
        .store
        .finish_job(
            &f.project,
            &claim.lease,
            JobFinish::Failed(JobDiagnostic::RunnerFailed),
        )
        .unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert!(failed.may_have_executed && failed.cleanup_confirmed);
    assert!(
        f.store
            .claim_job(
                &f.project,
                &policy().fingerprint(&limits()).unwrap(),
                "retry"
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn queue_scoped_attestation_validates_entire_batch_without_partial_mutation() {
    let mut f = Fixture::new();
    let other_dir = f._dir.path().join("other");
    std::fs::create_dir(&other_dir).unwrap();
    let other = f.store.ensure_project_link(&other_dir).unwrap().id;
    let a = f
        .store
        .insert_event(
            &f.project,
            &NewEvent::new(
                "codex",
                "new-a",
                "message",
                "user",
                "2026-01-01T00:00:00Z",
                "a",
                "fixture:v1",
            ),
        )
        .unwrap();
    let b = f
        .store
        .insert_event(
            &other,
            &NewEvent::new(
                "codex",
                "new-b",
                "message",
                "user",
                "2026-01-01T00:00:00Z",
                "b",
                "fixture:v1",
            ),
        )
        .unwrap();
    assert_eq!(
        f.store
            .attest_job_sessions(
                &f.project,
                &[a.session_id.clone(), b.session_id.clone()],
                "explicit",
                limits()
            )
            .unwrap_err(),
        JobStoreError::InvalidInput
    );
    for id in [&a.session_id, &b.session_id] {
        assert!(
            f.sql()
                .query_row(
                    "SELECT sealed_revision IS NULL FROM evidence_sessions WHERE session_id=?1",
                    [id],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap()
        );
    }
    let mut too_small = limits();
    too_small.max_events = 1;
    assert!(
        f.store
            .attest_job_sessions(
                &f.project,
                std::slice::from_ref(&f.session),
                "new label",
                too_small
            )
            .is_err()
    );
    assert_eq!(
        f.store
            .read_current_evidence_snapshot(&f.snapshot.id, limits().evidence_budget())
            .unwrap()
            .snapshot,
        f.snapshot
    );
}

#[test]
fn queue_capacity_duplicates_and_inspection_limits_are_bounded() {
    let mut f = Fixture::new();
    let first = f.enqueue();
    for n in 1..MAX_PROJECT_JOBS {
        let mut p = policy();
        p.prompt_policy_version = format!("synthetic:{n}");
        f.store
            .enqueue_job(&f.project, &f.snapshot.id, &p, limits())
            .unwrap();
    }
    assert_eq!(f.enqueue(), first);
    let mut p = policy();
    p.prompt_policy_version = "one-too-many".into();
    assert_eq!(
        f.store
            .enqueue_job(&f.project, &f.snapshot.id, &p, limits())
            .unwrap_err(),
        JobStoreError::QueueFull
    );
    assert_eq!(
        f.store.jobs(&f.project, MAX_INSPECTION_ROWS).unwrap().len(),
        100
    );
    assert!(f.store.jobs(&f.project, 0).is_err());
    assert!(f.store.jobs(&f.project, 101).is_err());
    let db = f.sql();
    let mut stmt=db.prepare("EXPLAIN QUERY PLAN SELECT id FROM assimilation_jobs WHERE project_id=?1 AND policy_fingerprint=?2 AND state='queued' ORDER BY created_at_ms,id LIMIT 1").unwrap();
    let plan = stmt
        .query_map(params![f.project, first.policy_fingerprint], |r| {
            r.get::<_, String>(3)
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join(" ");
    assert!(plan.contains("idx_jobs_claim"), "{plan}");
}

#[test]
fn queue_lease_is_checked_after_real_lock_wait_not_before_it() {
    for executing in [false, true] {
        let mut f = Fixture::new();
        f.enqueue();
        let mut claim = f.claim();
        if executing {
            f.store
                .begin_job_execution(&f.project, &claim.lease, &f.projection())
                .unwrap();
        }
        // Fixture updates deadline and token together, then delays lock acquisition
        // across expiry. Production cannot provide or backdate the Store clock.
        let mut other = Store::open_job_store(&f.path).unwrap();
        let deadline = millis() + 35;
        f.sql()
            .execute(
                "UPDATE assimilation_jobs SET lease_expires_at_ms=?2 WHERE id=?1",
                params![claim.lease.job_id, deadline],
            )
            .unwrap();
        claim.lease.lease_expires_at_ms = deadline;
        let mut writer = f.sql();
        let tx = writer
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let project = f.project.clone();
        let lease = claim.lease.clone();
        let projection = f.projection();
        let proposal = output(&f.event_ids[0]);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            started_tx.send(()).unwrap();
            if executing {
                other.complete_job(&project, &lease, proposal).map(|_| ())
            } else {
                other
                    .begin_job_execution(&project, &lease, &projection)
                    .map(|_| ())
            }
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        while millis() <= deadline {
            thread::yield_now();
        }
        tx.commit().unwrap();
        let error = handle.join().unwrap().unwrap_err();
        assert!(
            matches!(error, JobStoreError::LeaseExpired | JobStoreError::Storage),
            "{error}"
        );
        assert_eq!(f.receipt_count(), 0);
        assert_eq!(
            f.store
                .job(&f.project, &claim.lease.job_id)
                .unwrap()
                .summary
                .state,
            if executing {
                JobState::Executing
            } else {
                JobState::Claimed
            }
        );
    }
}

#[test]
fn queue_real_launch_and_completion_cancel_races_have_only_serial_outcomes() {
    for already_executing in [false, true] {
        let mut f = Fixture::new();
        f.enqueue();
        let claim = f.claim();
        if already_executing {
            f.store
                .begin_job_execution(&f.project, &claim.lease, &f.projection())
                .unwrap();
        }
        let mut worker = Store::open_job_store(&f.path).unwrap();
        let mut canceller = Store::open_job_store(&f.path).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let worker_barrier = barrier.clone();
        let cancel_barrier = barrier.clone();
        let project = f.project.clone();
        let lease = claim.lease.clone();
        let projection = f.projection();
        let proposal = output(&f.event_ids[0]);
        let worker_thread = thread::spawn(move || {
            worker_barrier.wait();
            if already_executing {
                worker.complete_job(&project, &lease, proposal).map(|_| ())
            } else {
                worker
                    .begin_job_execution(&project, &lease, &projection)
                    .map(|_| ())
            }
        });
        let project = f.project.clone();
        let id = claim.lease.job_id.clone();
        let cancel_thread = thread::spawn(move || {
            cancel_barrier.wait();
            canceller.cancel_job(&project, &id)
        });
        barrier.wait();
        let worker_result = worker_thread.join().unwrap();
        cancel_thread.join().unwrap().unwrap();
        let summary = f
            .store
            .job(&f.project, &claim.lease.job_id)
            .unwrap()
            .summary;
        if already_executing && worker_result.is_ok() {
            assert_eq!(summary.state, JobState::Proposed);
            assert!(!summary.cancel_requested);
            assert!(summary.cleanup_confirmed);
            assert_eq!(f.receipt_count(), 1);
        } else if !already_executing && worker_result.is_err() {
            assert!(matches!(
                worker_result,
                Err(JobStoreError::InvalidState | JobStoreError::Cancelled)
            ));
            assert_eq!(summary.state, JobState::Cancelled);
            assert!(!summary.may_have_executed);
            assert!(summary.cleanup_confirmed);
            assert_eq!(f.receipt_count(), 0);
        } else {
            if already_executing {
                assert_eq!(worker_result.unwrap_err(), JobStoreError::Cancelled);
            } else {
                worker_result.unwrap();
            }
            assert_eq!(summary.state, JobState::Executing);
            assert!(summary.cancel_requested && summary.may_have_executed);
            assert!(!summary.cleanup_confirmed);
            assert_eq!(f.receipt_count(), 0);
            let joined = f
                .store
                .finish_job(&f.project, &claim.lease, JobFinish::CancelledClean)
                .unwrap();
            assert_eq!(joined.state, JobState::Cancelled);
            assert!(joined.cleanup_confirmed);
        }
    }
}

#[test]
fn queue_output_projection_caps_and_categorical_errors_are_explicit() {
    let mut f = Fixture::new();
    let mut small = limits();
    small.max_output_bytes = 1024;
    f.store
        .enqueue_job(&f.project, &f.snapshot.id, &policy(), small)
        .unwrap();
    let claim = f
        .store
        .claim_job(&f.project, &policy().fingerprint(&small).unwrap(), "owner")
        .unwrap()
        .unwrap();
    let mut too_many = f.projection();
    too_many.included_event_ids = vec!["synthetic-unselected".into(); 2001];
    assert_eq!(
        f.store
            .begin_job_execution(&f.project, &claim.lease, &too_many)
            .unwrap_err(),
        JobStoreError::InvalidInput
    );
    let canary = "SYNTHETIC_DIAGNOSTIC_CANARY";
    let mut bad = f.projection();
    bad.evidence_hash = canary.repeat(20000);
    let error = f
        .store
        .begin_job_execution(&f.project, &claim.lease, &bad)
        .unwrap_err();
    assert!(!error.to_string().contains(canary));
    f.store
        .begin_job_execution(&f.project, &claim.lease, &f.projection())
        .unwrap();
    let mut proposal = output(&f.event_ids[0]);
    proposal.observations[0].claim = canary.repeat(80);
    let error = f
        .store
        .complete_job(&f.project, &claim.lease, proposal)
        .unwrap_err();
    assert_eq!(error, JobStoreError::InvalidOutput);
    assert!(!error.to_string().contains(canary));
    assert_eq!(f.receipt_count(), 0);
    assert_eq!(
        f.store
            .job(&f.project, &claim.lease.job_id)
            .unwrap()
            .summary
            .state,
        JobState::Executing
    );
    let done = f
        .store
        .finish_job(
            &f.project,
            &claim.lease,
            JobFinish::Failed(JobDiagnostic::InvalidOutput),
        )
        .unwrap();
    assert_eq!(done.state, JobState::Failed);
    assert!(done.cleanup_confirmed);
}
