use super::*;
use crate::store::evidence::EvidenceBudget;
use crate::store::{MIGRATIONS, NewEvent};
use serde_json::json;

fn seed(store: &mut Store, path: &std::path::Path) -> (String, String, String) {
    let project = store.ensure_project_link(path).unwrap();
    let event = store
        .insert_event(
            &project.id,
            &NewEvent::new(
                "codex",
                "unit-job",
                "message",
                "user",
                "2026-01-01T00:00:00Z",
                "synthetic",
                "fixture:v1",
            ),
        )
        .unwrap();
    store
        .attest_session_completion(
            &event.session_id,
            "trusted fixture",
            EvidenceBudget::default(),
        )
        .unwrap();
    let snapshot = store
        .create_evidence_snapshot(
            &project.id,
            &[event.session_id],
            "policy:v1",
            EvidenceBudget::default(),
        )
        .unwrap();
    (project.id, event.id, snapshot.id)
}
fn policy() -> JobPolicy {
    JobPolicy {
        projection_policy_version: "policy:v1".into(),
        prompt_policy_version: "prompt:v1".into(),
        runner_fingerprint: content_hash("fakecommand"),
    }
}

#[test]
fn v3_existing_store_migrates_without_changing_snapshot_or_old_receipt_api() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;PRAGMA recursive_triggers=ON;CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY NOT NULL,name TEXT NOT NULL,applied_at TEXT NOT NULL,checksum TEXT NOT NULL);").unwrap();
    for migration in &MIGRATIONS[..3] {
        conn.execute_batch(migration.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2,'fixture',?3)",
            params![
                migration.version,
                migration.name,
                content_hash(migration.sql)
            ],
        )
        .unwrap();
    }
    let mut old = Store {
        conn,
        db_path: path.clone(),
    };
    let (project, event, snapshot) = seed(&mut old, dir.path());
    let before = old
        .read_current_evidence_snapshot(&snapshot, EvidenceBudget::default())
        .unwrap();
    let output:DreamRunnerOutput=serde_json::from_value(json!({"observations":[{"kind":"observation","claim":"synthetic","confidence":0.9,"evidence_refs":[format!("mmr://event/{event}")]}]})).unwrap();
    let receipt = old
        .commit_snapshot_proposals(&snapshot, output.clone(), EvidenceBudget::default())
        .unwrap();
    drop(old);
    let mut upgraded = Store::open_job_store(&path).unwrap();
    assert_eq!(upgraded.schema_version().unwrap(), 4);
    assert_eq!(
        upgraded
            .read_current_evidence_snapshot(&snapshot, EvidenceBudget::default())
            .unwrap(),
        before
    );
    assert_eq!(
        upgraded
            .commit_snapshot_proposals(&snapshot, output, EvidenceBudget::default())
            .unwrap(),
        receipt
    );
    assert!(upgraded.jobs(&project, 10).unwrap().is_empty());
    let job = upgraded
        .enqueue_job(&project, &snapshot, &policy(), JobLimits::default())
        .unwrap();
    drop(upgraded);
    let reopened = Store::open_job_store(&path).unwrap();
    assert_eq!(reopened.job(&project, &job.id).unwrap().summary, job);
    reopened
        .conn
        .execute(
            "UPDATE schema_migrations SET checksum='bad' WHERE version=4",
            [],
        )
        .unwrap();
    drop(reopened);
    assert!(matches!(
        Store::open_job_store(&path),
        Err(JobStoreError::Storage)
    ));
}

#[test]
fn exact_expiry_boundary_and_invalid_limits_are_rejected() {
    let summary = JobSummary {
        id: "j".into(),
        project_id: "p".into(),
        snapshot_id: "s".into(),
        snapshot_digest: content_hash("s"),
        policy_fingerprint: content_hash("p"),
        state: JobState::Executing,
        attempts: 1,
        fence: 1,
        cancel_requested: false,
        owner_id: Some("o".into()),
        lease_expires_at_ms: Some(100),
        diagnostic: None,
        receipt_id: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        execution_intent_at_ms: Some(1),
        may_have_executed: true,
        cleanup_confirmed: false,
    };
    let lease = JobLease {
        job_id: "j".into(),
        owner_id: "o".into(),
        fence: 1,
        lease_expires_at_ms: 100,
    };
    assert!(token(&summary, &lease, 99).is_ok());
    assert_eq!(
        token(&summary, &lease, 100),
        Err(JobStoreError::LeaseExpired)
    );
    assert_eq!(
        token(&summary, &lease, 101),
        Err(JobStoreError::LeaseExpired)
    );
    for limit in [
        JobLimits {
            timeout_ms: u64::MAX,
            ..JobLimits::default()
        },
        JobLimits {
            cleanup_ms: u64::MAX,
            ..JobLimits::default()
        },
        JobLimits {
            lease_ms: u64::MAX,
            ..JobLimits::default()
        },
        JobLimits {
            max_events: 2001,
            ..JobLimits::default()
        },
        JobLimits {
            max_claim_attempts: 9,
            ..JobLimits::default()
        },
    ] {
        assert_eq!(limit.validate(), Err(JobStoreError::InvalidInput));
    }
    let body=json!({"policy_version":"p","evidence_hash":content_hash("request"),"included_event_ids":vec!["e";2001],"omissions":[]}).to_string();
    assert!(body.len() < MAX_PROJECTION_BYTES);
    assert!(
        serde_json::from_str::<JobProjection>(&body)
            .unwrap_err()
            .to_string()
            .contains("count bound")
    );
}

#[test]
fn expiry_after_staged_receipt_rolls_back_every_publication_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollback.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let (project, event, snapshot) = seed(&mut store, dir.path());
    let limits = JobLimits::default();
    store
        .enqueue_job(&project, &snapshot, &policy(), limits)
        .unwrap();
    let mut claim = store
        .claim_job(&project, &policy().fingerprint(&limits).unwrap(), "owner")
        .unwrap()
        .unwrap();
    let projection = JobProjection {
        policy_version: "policy:v1".into(),
        evidence_hash: content_hash("request"),
        included_event_ids: vec![event.clone()],
        omissions: vec![],
    };
    store
        .begin_job_execution(&project, &claim.lease, &projection)
        .unwrap();
    let deadline = now().unwrap() + 300;
    store
        .conn
        .execute(
            "UPDATE assimilation_jobs SET lease_expires_at_ms=?2 WHERE id=?1",
            params![claim.lease.job_id, deadline],
        )
        .unwrap();
    claim.lease.lease_expires_at_ms = deadline;
    PRECOMMIT_DELAY_MS.with(|delay| delay.set(400));
    let output=serde_json::from_value(json!({"observations":[{"kind":"observation","claim":"synthetic","confidence":0.9,"status":"active","evidence_refs":[format!("mmr://event/{event}")]}]})).unwrap();
    assert_eq!(
        store
            .complete_job(&project, &claim.lease, output)
            .unwrap_err(),
        JobStoreError::LeaseExpired
    );
    PRECOMMIT_DELAY_MS
        .with(|delay| assert_eq!(delay.get(), 0, "did not reach staged-receipt pause"));
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM evidence_proposal_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .job(&project, &claim.lease.job_id)
            .unwrap()
            .summary
            .state,
        JobState::Executing
    );
    assert_eq!(store.recover_jobs(&project).unwrap().unknown, 1);
}

#[test]
fn global_admission_cap_is_checked_before_materializing_other_queue_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capacity.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let (project, _, snapshot) = seed(&mut store, dir.path());
    let original = store
        .enqueue_job(&project, &snapshot, &policy(), JobLimits::default())
        .unwrap();
    // Scalar synthetic capacity seed: rows reference valid snapshot/policy JSON.
    // Control-table capacity is independent of source text/real execution.
    store.conn.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO assimilation_jobs(id,project_id,store_epoch,snapshot_id,snapshot_digest,policy_fingerprint,policy_json,limits_json,state,created_at_ms,updated_at_ms) SELECT 'capacity:'||x,'capacity-project:'||(x/256),store_epoch,snapshot_id,snapshot_digest,policy_fingerprint,policy_json,limits_json,'queued',created_at_ms,updated_at_ms FROM n,assimilation_jobs WHERE id=?1",[&original.id]).unwrap();
    let mut changed = policy();
    changed.prompt_policy_version = "another prompt".into();
    assert_eq!(
        store
            .enqueue_job(&project, &snapshot, &changed, JobLimits::default())
            .unwrap_err(),
        JobStoreError::QueueFull
    );
    assert_eq!(
        store
            .enqueue_job(&project, &snapshot, &policy(), JobLimits::default())
            .unwrap(),
        original
    );
}
