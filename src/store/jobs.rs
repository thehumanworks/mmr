//! Bounded durable assimilation claims. No runner, queue discovery, GC or retry
//! after execution intent. Public lease checks use Store-owned transaction time.
use super::evidence::{self, CurrentEvidenceSnapshot};
use super::{Store, content_hash, run_migrations};
use crate::dream::DreamRunnerOutput;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod model;
pub use model::*;
pub(super) const SCHEMA: &str = include_str!("jobs/migration.sql");
type Result<T> = std::result::Result<T, JobStoreError>;
#[cfg(test)]
thread_local! { static PRECOMMIT_DELAY_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
fn storage<T, E>(result: std::result::Result<T, E>) -> Result<T> {
    result.map_err(|_| JobStoreError::Storage)
}
fn label(s: &str) -> Result<()> {
    if s.trim().is_empty() || s.len() > MAX_LABEL_BYTES {
        Err(JobStoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn hash(s: &str) -> Result<()> {
    if s.len() != 71
        || !s.starts_with("sha256:")
        || !s[7..]
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        Err(JobStoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn encode<T: Serialize>(value: &T, max: usize) -> Result<String> {
    evidence::json(value, max).map_err(|_| JobStoreError::InvalidInput)
}
fn now() -> Result<i64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JobStoreError::Clock)?
        .as_millis();
    let ms = i64::try_from(ms).map_err(|_| JobStoreError::Clock)?;
    if ms > i64::MAX - 90_000 {
        return Err(JobStoreError::Clock);
    }
    Ok(ms)
}
fn policy_fingerprint(policy: &JobPolicy, limits: JobLimits) -> Result<String> {
    limits.validate()?;
    label(&policy.projection_policy_version)?;
    label(&policy.prompt_policy_version)?;
    hash(&policy.runner_fingerprint)?;
    Ok(content_hash(&encode(
        &("assimilation-policy:v1", policy, limits),
        MAX_POLICY_BYTES,
    )?))
}
fn enum_text<T: Serialize>(value: T) -> Result<String> {
    let text = storage(serde_json::to_string(&value))?;
    storage(serde_json::from_str(&text))
}
fn decode<T: DeserializeOwned>(value: &str) -> Result<T> {
    storage(serde_json::from_str(value))
}

const SUMMARY_COLUMNS: &str = "id,project_id,snapshot_id,snapshot_digest,policy_fingerprint,state,attempts,fence,cancel_requested,owner_id,lease_expires_at_ms,diagnostic_code,receipt_id,created_at_ms,updated_at_ms,execution_intent_at_ms,cleanup_confirmed";
fn summary_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<JobSummary> {
    let state: String = r.get(5)?;
    let diagnostic: Option<String> = r.get(11)?;
    let parse = |s: &str| {
        serde_json::from_value(serde_json::Value::String(s.into())).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })
    };
    let state: JobState = parse(&state)?;
    let diagnostic = diagnostic
        .map(|s| {
            serde_json::from_value::<JobDiagnostic>(serde_json::Value::String(s)).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .transpose()?;
    let intent: Option<i64> = r.get(15)?;
    Ok(JobSummary {
        id: r.get(0)?,
        project_id: r.get(1)?,
        snapshot_id: r.get(2)?,
        snapshot_digest: r.get(3)?,
        policy_fingerprint: r.get(4)?,
        state,
        attempts: r.get(6)?,
        fence: r.get(7)?,
        cancel_requested: r.get(8)?,
        owner_id: r.get(9)?,
        lease_expires_at_ms: r.get(10)?,
        diagnostic,
        receipt_id: r.get(12)?,
        created_at_ms: r.get(13)?,
        updated_at_ms: r.get(14)?,
        execution_intent_at_ms: intent,
        may_have_executed: intent.is_some(),
        cleanup_confirmed: r.get(16)?,
    })
}
fn summary(conn: &Connection, project: &str, id: &str) -> Result<JobSummary> {
    let sql =
        format!("SELECT {SUMMARY_COLUMNS} FROM assimilation_jobs WHERE project_id=?1 AND id=?2");
    storage(
        conn.query_row(&sql, params![project, id], summary_row)
            .optional(),
    )?
    .ok_or(JobStoreError::NotFound)
}
fn record(conn: &Connection, project: &str, id: &str) -> Result<JobRecord> {
    let sizes:Option<(i64,i64,i64)>=storage(conn.query_row("SELECT length(CAST(policy_json AS BLOB)),length(CAST(limits_json AS BLOB)),coalesce(length(CAST(projection_json AS BLOB)),0) FROM assimilation_jobs WHERE project_id=?1 AND id=?2",params![project,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
    let (policy_size, limits_size, projection_size) = sizes.ok_or(JobStoreError::NotFound)?;
    if policy_size < 0
        || limits_size < 0
        || projection_size < 0
        || policy_size > MAX_POLICY_BYTES as i64
        || limits_size > MAX_POLICY_BYTES as i64
        || projection_size > MAX_PROJECTION_BYTES as i64
    {
        return Err(JobStoreError::Storage);
    }
    let (policy,limits,projection):(String,String,Option<String>)=storage(conn.query_row("SELECT policy_json,limits_json,projection_json FROM assimilation_jobs WHERE project_id=?1 AND id=?2",params![project,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))))?;
    let policy: JobPolicy = decode(&policy)?;
    let limits: JobLimits = decode(&limits)?;
    limits.validate().map_err(|_| JobStoreError::Storage)?;
    let summary = summary(conn, project, id)?;
    if policy
        .fingerprint(&limits)
        .map_err(|_| JobStoreError::Storage)?
        != summary.policy_fingerprint
    {
        return Err(JobStoreError::Storage);
    }
    let projection = projection.as_deref().map(decode).transpose()?;
    Ok(JobRecord {
        summary,
        policy,
        limits,
        projection,
    })
}
fn token(summary: &JobSummary, lease: &JobLease, at: i64) -> Result<()> {
    label(&lease.job_id)?;
    label(&lease.owner_id)?;
    if summary.id != lease.job_id
        || summary.owner_id.as_deref() != Some(&lease.owner_id)
        || summary.fence != lease.fence
        || summary.lease_expires_at_ms != Some(lease.lease_expires_at_ms)
    {
        return Err(JobStoreError::LostLease);
    }
    if at >= lease.lease_expires_at_ms {
        return Err(JobStoreError::LeaseExpired);
    }
    Ok(())
}
fn live(summary: &JobSummary, lease: &JobLease, at: i64, states: &[JobState]) -> Result<()> {
    token(summary, lease, at)?;
    if !states.contains(&summary.state) {
        return Err(JobStoreError::InvalidState);
    }
    if summary.cancel_requested {
        return Err(JobStoreError::Cancelled);
    }
    Ok(())
}
fn terminal(
    tx: &Transaction<'_>,
    id: &str,
    state: JobState,
    diagnostic: JobDiagnostic,
    at: i64,
    clean: bool,
) -> Result<()> {
    storage(tx.execute("UPDATE assimilation_jobs SET state=?2,diagnostic_code=?3,updated_at_ms=?4,cleanup_confirmed=?5 WHERE id=?1",params![id,enum_text(state)?,enum_text(diagnostic)?,at,clean]))?;
    Ok(())
}
fn recover(tx: &Transaction<'_>, project: &str, at: i64) -> Result<JobRecovery> {
    let unknown=storage(tx.execute("UPDATE assimilation_jobs SET state='unknown',diagnostic_code='lease_expired',cleanup_confirmed=0,updated_at_ms=?2 WHERE project_id=?1 AND state='executing' AND lease_expires_at_ms<=?2",params![project,at]))?;
    // attempts cap is a typed property in bounded limits_json; inspect <=256 rows.
    let mut statement=storage(tx.prepare("SELECT id FROM assimilation_jobs WHERE project_id=?1 AND state='claimed' AND lease_expires_at_ms<=?2 ORDER BY lease_expires_at_ms,id LIMIT ?3"))?;
    let ids = storage(
        storage(
            statement.query_map(params![project, at, (MAX_PROJECT_JOBS + 1) as i64], |r| {
                r.get::<_, String>(0)
            }),
        )?
        .collect::<rusqlite::Result<Vec<_>>>(),
    )?;
    if ids.len() > MAX_PROJECT_JOBS {
        return Err(JobStoreError::Storage);
    }
    let mut exhausted = 0;
    for id in ids {
        let job = record(tx, project, &id)?;
        if job.summary.attempts >= job.limits.max_claim_attempts {
            terminal(
                tx,
                &id,
                JobState::Failed,
                JobDiagnostic::ClaimAttemptsExhausted,
                at,
                true,
            )?;
            exhausted += 1;
        }
    }
    Ok(JobRecovery {
        unknown: u32::try_from(unknown).map_err(|_| JobStoreError::Storage)?,
        exhausted,
    })
}
fn current(tx: &Transaction<'_>, job: &JobRecord) -> Result<CurrentEvidenceSnapshot> {
    let current = evidence::validate_snapshot_on_tx(
        tx,
        &job.summary.snapshot_id,
        job.limits.evidence_budget(),
    )
    .map_err(|_| JobStoreError::StaleEvidence)?;
    if current.snapshot.metadata.project.id != job.summary.project_id
        || current.snapshot.digest != job.summary.snapshot_digest
    {
        return Err(JobStoreError::StaleEvidence);
    }
    Ok(current)
}
fn projection_refs(
    projection: &JobProjection,
    job: &JobRecord,
    current: &CurrentEvidenceSnapshot,
) -> Result<BTreeSet<String>> {
    // Typed projection is caller-owned. Bound serialization before any set clones.
    encode(projection, MAX_PROJECTION_BYTES)?;
    label(&projection.policy_version)?;
    hash(&projection.evidence_hash)?;
    if projection.policy_version != job.policy.projection_policy_version
        || projection.included_event_ids.is_empty()
        || projection.included_event_ids.len() + projection.omissions.len() > job.limits.max_events
    {
        return Err(JobStoreError::InvalidInput);
    }
    let actual = current
        .events
        .iter()
        .map(|e| e.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut allowed = BTreeSet::new();
    for id in &projection.included_event_ids {
        label(id)?;
        if !actual.contains(id.as_str()) || !seen.insert(id.as_str()) {
            return Err(JobStoreError::InvalidInput);
        }
        allowed.insert(format!("mmr://event/{id}"));
    }
    for omission in &projection.omissions {
        label(&omission.event_id)?;
        if !actual.contains(omission.event_id.as_str()) || !seen.insert(&omission.event_id) {
            return Err(JobStoreError::InvalidInput);
        }
    }
    if seen != actual {
        return Err(JobStoreError::InvalidInput);
    }
    Ok(allowed)
}

impl Store {
    /// Opens an already-existing valid mmr DB; never creates a typo path/database.
    /// Job handles wait at most 50ms for SQLite locks. Controllers open once.
    pub fn open_job_store(path: impl Into<PathBuf>) -> Result<Self> {
        let db_path = path.into();
        let mut conn = storage(Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        ))?;
        storage(conn.busy_timeout(Duration::from_millis(50)))?;
        let existing:bool=storage(conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",[],|r|r.get(0)))?;
        if !existing {
            return Err(JobStoreError::Storage);
        }
        storage(run_migrations(&mut conn))?;
        Ok(Self { conn, db_path })
    }
    /// Explicit trusted completion for one bounded project-scoped batch. Scope,
    /// aggregate budgets and every seal share one immediate transaction.
    pub fn attest_job_sessions(
        &mut self,
        project: &str,
        ids: &[String],
        attestation: &str,
        limits: JobLimits,
    ) -> Result<Vec<evidence::CompletionSeal>> {
        label(project)?;
        label(attestation)?;
        limits.validate()?;
        if ids.is_empty() || ids.len() > limits.max_sessions {
            return Err(JobStoreError::InvalidInput);
        }
        let mut unique = BTreeSet::new();
        for id in ids {
            label(id)?;
            if !unique.insert(id) {
                return Err(JobStoreError::InvalidInput);
            }
        }
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        for id in ids {
            let scoped: bool = storage(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND project_id=?2)",
                params![id, project],
                |r| r.get(0),
            ))?;
            if !scoped {
                return Err(JobStoreError::InvalidInput);
            }
        }
        evidence::preflight_completion_sessions(&tx, project, ids, limits.evidence_budget())
            .map_err(|_| JobStoreError::InvalidInput)?;
        let seals = ids
            .iter()
            .map(|id| {
                evidence::attest_completion_on_tx(&tx, id, attestation, limits.evidence_budget())
                    .map_err(|_| JobStoreError::InvalidInput)
            })
            .collect::<Result<Vec<_>>>()?;
        storage(tx.commit())?;
        Ok(seals)
    }
    pub fn enqueue_job(
        &mut self,
        project: &str,
        snapshot_id: &str,
        policy: &JobPolicy,
        limits: JobLimits,
    ) -> Result<JobSummary> {
        label(project)?;
        label(snapshot_id)?;
        let fingerprint = policy.fingerprint(&limits)?;
        let policy_json = encode(policy, MAX_POLICY_BYTES)?;
        let limits_json = encode(&limits, MAX_POLICY_BYTES)?;
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        let at = now()?;
        // Snapshot identity lookup is bounded; existing keys return even if stale/capped.
        let identity:Option<(String,String,String)>=storage(tx.query_row("SELECT store_epoch,digest,project_id FROM evidence_snapshots WHERE id=?1 AND length(CAST(store_epoch AS BLOB))=32 AND length(CAST(digest AS BLOB))<=128 AND length(CAST(project_id AS BLOB))<=128",[snapshot_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
        let (epoch, digest, snapshot_project) = identity.ok_or(JobStoreError::StaleEvidence)?;
        if snapshot_project != project {
            return Err(JobStoreError::StaleEvidence);
        }
        let id = format!(
            "assimilation-job:v1:{}",
            content_hash(&encode(
                &(
                    "assimilation-job:v1",
                    &epoch,
                    project,
                    snapshot_id,
                    &digest,
                    &fingerprint
                ),
                MAX_POLICY_BYTES
            )?)[7..]
                .to_owned()
        );
        match summary(&tx, project, &id) {
            Ok(existing) => {
                storage(tx.commit())?;
                return Ok(existing);
            }
            Err(JobStoreError::NotFound) => {}
            Err(error) => return Err(error),
        }
        let project_count: i64 = storage(tx.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM assimilation_jobs WHERE project_id=?1 LIMIT ?2)",
            params![project, (MAX_PROJECT_JOBS + 1) as i64],
            |r| r.get(0),
        ))?;
        let global_count: i64 = storage(tx.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM assimilation_jobs LIMIT ?1)",
            [(MAX_STORE_JOBS + 1) as i64],
            |r| r.get(0),
        ))?;
        if project_count >= MAX_PROJECT_JOBS as i64 || global_count >= MAX_STORE_JOBS as i64 {
            return Err(JobStoreError::QueueFull);
        }
        let current = evidence::validate_snapshot_on_tx(&tx, snapshot_id, limits.evidence_budget())
            .map_err(|_| JobStoreError::StaleEvidence)?;
        if current.snapshot.metadata.project.id != project
            || current.snapshot.digest != digest
            || current.snapshot.metadata.sessions.len() > limits.max_sessions
        {
            return Err(JobStoreError::StaleEvidence);
        }
        storage(tx.execute("INSERT INTO assimilation_jobs(id,project_id,store_epoch,snapshot_id,snapshot_digest,policy_fingerprint,policy_json,limits_json,state,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'queued',?9,?9)",params![id,project,epoch,snapshot_id,digest,fingerprint,policy_json,limits_json,at]))?;
        let result = summary(&tx, project, &id)?;
        storage(tx.commit())?;
        Ok(result)
    }
    pub fn claim_job(
        &mut self,
        project: &str,
        policy_fingerprint: &str,
        owner_id: &str,
    ) -> Result<Option<JobClaim>> {
        label(project)?;
        hash(policy_fingerprint)?;
        label(owner_id)?;
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        let at = now()?;
        recover(&tx, project, at)?;
        let id:Option<String>=storage(tx.query_row("SELECT id FROM assimilation_jobs WHERE project_id=?1 AND policy_fingerprint=?2 AND (state='queued' OR (state='claimed' AND lease_expires_at_ms<=?3)) ORDER BY created_at_ms,id LIMIT 1",params![project,policy_fingerprint,at],|r|r.get(0)).optional())?;
        let Some(id) = id else {
            storage(tx.commit())?;
            return Ok(None);
        };
        let job = record(&tx, project, &id)?;
        let deadline = at
            .checked_add(i64::try_from(job.limits.lease_ms).map_err(|_| JobStoreError::Clock)?)
            .ok_or(JobStoreError::Clock)?;
        storage(tx.execute("UPDATE assimilation_jobs SET state='claimed',attempts=attempts+1,fence=fence+1,owner_id=?2,lease_expires_at_ms=?3,updated_at_ms=?4 WHERE id=?1",params![id,owner_id,deadline,at]))?;
        let job = record(&tx, project, &id)?;
        let lease = JobLease {
            job_id: id,
            owner_id: owner_id.to_owned(),
            fence: job.summary.fence,
            lease_expires_at_ms: deadline,
        };
        storage(tx.commit())?;
        Ok(Some(JobClaim { job, lease }))
    }
    pub fn begin_job_execution(
        &mut self,
        project: &str,
        lease: &JobLease,
        projection: &JobProjection,
    ) -> Result<JobRecord> {
        label(project)?;
        label(&lease.job_id)?;
        encode(projection, MAX_PROJECTION_BYTES)?;
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        let job = record(&tx, project, &lease.job_id)?;
        live(&job.summary, lease, now()?, &[JobState::Claimed])?;
        let current = match current(&tx, &job) {
            Ok(current) => current,
            Err(error) => {
                live(&job.summary, lease, now()?, &[JobState::Claimed])?;
                terminal(
                    &tx,
                    &lease.job_id,
                    JobState::Stale,
                    JobDiagnostic::EvidenceStale,
                    now()?,
                    true,
                )?;
                storage(tx.commit())?;
                return Err(error);
            }
        };
        projection_refs(projection, &job, &current)?;
        let body = encode(projection, MAX_PROJECTION_BYTES)?;
        let at = now()?;
        live(&job.summary, lease, at, &[JobState::Claimed])?;
        if lease.lease_expires_at_ms - at
            < i64::try_from(job.limits.timeout_ms + job.limits.cleanup_ms + CONTROL_MARGIN_MS)
                .map_err(|_| JobStoreError::Clock)?
        {
            terminal(
                &tx,
                &lease.job_id,
                JobState::Failed,
                JobDiagnostic::LeaseWindowExhausted,
                at,
                true,
            )?;
            storage(tx.commit())?;
            return Err(JobStoreError::LeaseExpired);
        }
        storage(tx.execute("UPDATE assimilation_jobs SET state='executing',projection_json=?2,execution_intent_at_ms=?3,updated_at_ms=?3 WHERE id=?1",params![lease.job_id,body,at]))?;
        let result = record(&tx, project, &lease.job_id)?;
        storage(tx.commit())?;
        Ok(result)
    }
    pub fn job_control(&self, project: &str, lease: &JobLease) -> Result<JobControl> {
        label(project)?;
        label(&lease.job_id)?;
        let summary = match summary(&self.conn, project, &lease.job_id) {
            Ok(s) => s,
            Err(JobStoreError::NotFound) => return Ok(JobControl::Lost),
            Err(e) => return Err(e),
        };
        match token(&summary, lease, now()?) {
            Err(JobStoreError::LeaseExpired) => return Ok(JobControl::Expired),
            Err(_) => return Ok(JobControl::Lost),
            Ok(()) => {}
        }
        if summary.state != JobState::Executing {
            return Ok(JobControl::Lost);
        }
        Ok(if summary.cancel_requested {
            JobControl::CancelRequested
        } else {
            JobControl::Continue
        })
    }
    /// Caller attests that execution and all transport/cleanup work has joined.
    /// Errors never authorize another launch; inspect/recover durable state.
    pub fn complete_job(
        &mut self,
        project: &str,
        lease: &JobLease,
        output: DreamRunnerOutput,
    ) -> Result<JobCompletion> {
        label(project)?;
        label(&lease.job_id)?;
        encode(&output, 256 * 1024).map_err(|_| JobStoreError::InvalidOutput)?;
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        let job = record(&tx, project, &lease.job_id)?;
        live(&job.summary, lease, now()?, &[JobState::Executing])?;
        let current = match current(&tx, &job) {
            Ok(c) => c,
            Err(e) => {
                live(&job.summary, lease, now()?, &[JobState::Executing])?;
                terminal(
                    &tx,
                    &lease.job_id,
                    JobState::Stale,
                    JobDiagnostic::EvidenceStale,
                    now()?,
                    true,
                )?;
                storage(tx.commit())?;
                return Err(e);
            }
        };
        let projection = job.projection.as_ref().ok_or(JobStoreError::Storage)?;
        let refs = projection_refs(projection, &job, &current)?;
        let receipt = evidence::commit_proposals_on_tx(
            &tx,
            &job.summary.snapshot_id,
            output,
            job.limits.evidence_budget(),
            Some(&refs),
            job.limits.max_output_bytes,
        )
        .map_err(|_| JobStoreError::InvalidOutput)?;
        // Test-only controlled pause proves rollback after a staged receipt.
        #[cfg(test)]
        PRECOMMIT_DELAY_MS
            .with(|delay| std::thread::sleep(Duration::from_millis(delay.replace(0))));
        // This check is AFTER potentially expensive validation/normalization and
        // staged receipt insertion. Returning drops tx and rolls the receipt back.
        let at = now()?;
        live(&job.summary, lease, at, &[JobState::Executing])?;
        storage(tx.execute("UPDATE assimilation_jobs SET state='proposed',receipt_id=?2,cleanup_confirmed=1,updated_at_ms=?3 WHERE id=?1",params![lease.job_id,receipt.id,at]))?;
        let job = summary(&tx, project, &lease.job_id)?;
        storage(tx.commit())?;
        Ok(JobCompletion { job, receipt })
    }
    pub fn finish_job(
        &mut self,
        project: &str,
        lease: &JobLease,
        finish: JobFinish,
    ) -> Result<JobSummary> {
        label(project)?;
        label(&lease.job_id)?;
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        let job = record(&tx, project, &lease.job_id)?;
        let at = now()?;
        token(&job.summary, lease, at)?;
        if !matches!(job.summary.state, JobState::Claimed | JobState::Executing) {
            return Err(JobStoreError::InvalidState);
        }
        let (state, diagnostic, clean) = match finish {
            JobFinish::Unknown(code) if job.summary.state == JobState::Executing => {
                (JobState::Unknown, code, false)
            }
            JobFinish::Unknown(_) => return Err(JobStoreError::InvalidState),
            JobFinish::CancelledClean if job.summary.cancel_requested => {
                (JobState::Cancelled, JobDiagnostic::Cancelled, true)
            }
            JobFinish::CancelledClean => return Err(JobStoreError::InvalidState),
            JobFinish::Failed(
                JobDiagnostic::CleanupUnknown
                | JobDiagnostic::OwnerLost
                | JobDiagnostic::LeaseExpired,
            ) => return Err(JobStoreError::InvalidInput),
            JobFinish::Failed(_) if job.summary.cancel_requested => {
                (JobState::Cancelled, JobDiagnostic::Cancelled, true)
            }
            JobFinish::Failed(JobDiagnostic::EvidenceStale) => {
                (JobState::Stale, JobDiagnostic::EvidenceStale, true)
            }
            JobFinish::Failed(code) => (JobState::Failed, code, true),
        };
        terminal(&tx, &lease.job_id, state, diagnostic, at, clean)?;
        let result = summary(&tx, project, &lease.job_id)?;
        storage(tx.commit())?;
        Ok(result)
    }
    pub fn cancel_job(&mut self, project: &str, id: &str) -> Result<JobSummary> {
        label(project)?;
        label(id)?;
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        let job = summary(&tx, project, id)?;
        let at = now()?;
        if !job.state.terminal() {
            storage(tx.execute(
                "UPDATE assimilation_jobs SET cancel_requested=1,updated_at_ms=?2 WHERE id=?1",
                params![id, at],
            ))?;
            if job.state != JobState::Executing {
                terminal(
                    &tx,
                    id,
                    JobState::Cancelled,
                    JobDiagnostic::Cancelled,
                    at,
                    true,
                )?;
            }
        }
        let result = summary(&tx, project, id)?;
        storage(tx.commit())?;
        Ok(result)
    }
    pub fn recover_jobs(&mut self, project: &str) -> Result<JobRecovery> {
        label(project)?;
        let tx = storage(
            self.conn
                .transaction_with_behavior(TransactionBehavior::Immediate),
        )?;
        let result = recover(&tx, project, now()?)?;
        storage(tx.commit())?;
        Ok(result)
    }
    pub fn job(&self, project: &str, id: &str) -> Result<JobRecord> {
        label(project)?;
        label(id)?;
        let tx = storage(self.conn.unchecked_transaction())?;
        let result = record(&tx, project, id)?;
        storage(tx.commit())?;
        Ok(result)
    }
    pub fn jobs(&self, project: &str, limit: usize) -> Result<Vec<JobSummary>> {
        label(project)?;
        if limit == 0 || limit > MAX_INSPECTION_ROWS {
            return Err(JobStoreError::InvalidInput);
        }
        let size:i64=storage(self.conn.query_row("SELECT coalesce(sum(n),0) FROM (SELECT length(CAST(id AS BLOB))+length(CAST(project_id AS BLOB))+length(CAST(snapshot_id AS BLOB))+length(CAST(snapshot_digest AS BLOB))+length(CAST(policy_fingerprint AS BLOB))+coalesce(length(CAST(owner_id AS BLOB)),0)+coalesce(length(CAST(receipt_id AS BLOB)),0)+512 n FROM assimilation_jobs WHERE project_id=?1 ORDER BY created_at_ms DESC,id DESC LIMIT ?2)",params![project,limit as i64],|r|r.get(0)))?;
        if size < 0 || size > MAX_INSPECTION_BYTES as i64 {
            return Err(JobStoreError::Storage);
        }
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM assimilation_jobs WHERE project_id=?1 ORDER BY created_at_ms DESC,id DESC LIMIT ?2"
        );
        let mut statement = storage(self.conn.prepare(&sql))?;
        storage(
            storage(statement.query_map(params![project, limit as i64], summary_row))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )
    }
}

#[cfg(test)]
mod tests;
