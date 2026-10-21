use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::projection::project_snapshot;
use super::{JobConfig, JobWorkerError, JobsAction, JobsResponse};
use crate::dream::{
    CommandCancellation, CommandDreamRunner, CommandRunnerError, CommandRunnerLimits,
};
use crate::store::Store;
use crate::store::content_hash;
use crate::store::jobs::{
    JobClaim, JobControl, JobDiagnostic, JobFinish, JobLease, JobStoreError, JobSummary,
};

fn response(command: &'static str, status: &'static str) -> JobsResponse {
    JobsResponse {
        command,
        status,
        jobs: vec![],
        job: None,
        receipt: None,
        detail: None,
    }
}

pub fn execute(
    config: JobConfig,
    action: JobsAction,
    started: Instant,
) -> Result<JobsResponse, JobWorkerError> {
    let command = match &action {
        JobsAction::Enqueue { .. } => "enqueue",
        JobsAction::Run { .. } => "run",
        JobsAction::Status { .. } => "status",
        JobsAction::Show { .. } => "show",
        JobsAction::Cancel { .. } => "cancel",
    };
    if !config.enabled && matches!(action, JobsAction::Enqueue { .. } | JobsAction::Run { .. }) {
        return Ok(response(command, "disabled"));
    }
    config.validate(true)?;
    // This opener never creates a database, and uses a bounded busy timeout.
    let mut store = Store::open_job_store(
        config
            .database_path
            .as_ref()
            .ok_or(JobWorkerError::InvalidConfig)?,
    )?;
    let project = config
        .project_id
        .as_deref()
        .ok_or(JobWorkerError::InvalidConfig)?;
    let mut result = response(command, "ok");
    match action {
        JobsAction::Enqueue {
            session_ids,
            attest_complete,
        } => {
            if session_ids.is_empty()
                || session_ids.len() > config.budgets.max_sessions
                || session_ids
                    .iter()
                    .any(|id| id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control))
                || session_ids.iter().collect::<BTreeSet<_>>().len() != session_ids.len()
            {
                return Err(JobWorkerError::InvalidAction);
            }
            let limits = config.budgets.job_limits()?;
            let policy = config.job_policy()?;
            if let Some(attestation) = attest_complete {
                if attestation.trim().is_empty()
                    || attestation.len() > 128
                    || attestation.chars().any(char::is_control)
                {
                    return Err(JobWorkerError::InvalidAction);
                }
                // The scoped Store helper checks project membership while
                // holding the same immediate transaction as completion seals.
                store.attest_job_sessions(project, &session_ids, &attestation, limits)?;
            }
            let snapshot = store
                .create_evidence_snapshot(
                    project,
                    &session_ids,
                    &policy.projection_policy_version,
                    limits.evidence_budget(),
                )
                .map_err(|_| JobWorkerError::Projection)?;
            result.job = Some(store.enqueue_job(project, &snapshot.id, &policy, limits)?);
        }
        JobsAction::Run { max_jobs } => return run_jobs(config, store, max_jobs, started),
        JobsAction::Status { limit } => {
            result.jobs = store.jobs(project, limit)?;
        }
        JobsAction::Show { job_id } => {
            let job = store.job(project, &job_id)?;
            if let Some(id) = &job.summary.receipt_id {
                result.receipt = Some(
                    store
                        .evidence_proposal_receipt(id)
                        .map_err(|_| JobWorkerError::Store)?,
                );
            }
            result.job = Some(job.summary.clone());
            result.detail = Some(job);
        }
        JobsAction::Cancel { job_id } => {
            result.job = Some(store.cancel_job(project, &job_id)?);
        }
    }
    Ok(result)
}

fn enough_time(started: Instant, config: &JobConfig) -> bool {
    let required = Duration::from_millis(
        config.budgets.execution_timeout_ms + config.budgets.cleanup_timeout_ms + 1000,
    );
    Duration::from_millis(config.budgets.invocation_timeout_ms).saturating_sub(started.elapsed())
        >= required
}

fn run_jobs(
    config: JobConfig,
    mut store: Store,
    max_jobs: u8,
    started: Instant,
) -> Result<JobsResponse, JobWorkerError> {
    if !(1..=8).contains(&max_jobs) {
        return Err(JobWorkerError::InvalidAction);
    }
    let project = config
        .project_id
        .as_deref()
        .ok_or(JobWorkerError::InvalidConfig)?;
    let limits = config.budgets.job_limits()?;
    let fingerprint = config.job_policy()?.fingerprint(&limits)?;
    store.recover_jobs(project)?;
    static NEXT_OWNER: AtomicU64 = AtomicU64::new(0);
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JobWorkerError::Clock)?
        .as_nanos();
    let owner = content_hash(&format!(
        "{wall}:{}:{}",
        std::process::id(),
        NEXT_OWNER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut result = response("run", "ok");
    for _ in 0..max_jobs {
        if !enough_time(started, &config) {
            result.status = "budget_exhausted";
            break;
        }
        let Some(claim) = store.claim_job(project, &fingerprint, &owner)? else {
            break;
        };
        result
            .jobs
            .push(run_claim(&config, &mut store, claim, started)?);
    }
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControllerExit {
    Finished,
    CancelRequested,
    Lost,
    Expired,
    Unavailable,
}

fn run_claim(
    config: &JobConfig,
    store: &mut Store,
    claim: JobClaim,
    started: Instant,
) -> Result<JobSummary, JobWorkerError> {
    let project = config
        .project_id
        .as_deref()
        .ok_or(JobWorkerError::InvalidConfig)?;
    let current = match store.read_current_evidence_snapshot(
        &claim.job.summary.snapshot_id,
        claim.job.limits.evidence_budget(),
    ) {
        Ok(current) => current,
        Err(_) => {
            return finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::EvidenceStale),
            );
        }
    };
    let projected = match project_snapshot(&current, config.budgets) {
        Ok(projected) => projected,
        Err(_) => {
            return finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::ProjectionFailed),
            );
        }
    };
    if projected.request.evidence.is_empty() {
        return finish_or_observe(
            store,
            project,
            &claim.lease,
            JobFinish::Failed(JobDiagnostic::NoEligibleEvidence),
        );
    }
    if !enough_time(started, config) {
        return finish_or_observe(
            store,
            project,
            &claim.lease,
            JobFinish::Failed(JobDiagnostic::BudgetExceeded),
        );
    }
    let controller_store = match Store::open_job_store(
        config
            .database_path
            .as_ref()
            .ok_or(JobWorkerError::InvalidConfig)?,
    ) {
        Ok(store) => store,
        Err(_) => {
            return finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::RunnerFailed),
            );
        }
    };
    let command = config
        .command
        .as_ref()
        .ok_or(JobWorkerError::InvalidConfig)?;
    let runner = CommandDreamRunner::new(command.program.as_os_str(), command.args.iter())
        .with_clean_environment()
        .with_limits(CommandRunnerLimits {
            execution_timeout: Duration::from_millis(claim.job.limits.timeout_ms),
            cleanup_timeout: Duration::from_millis(claim.job.limits.cleanup_ms),
            max_request_bytes: claim.job.limits.max_request_bytes,
            max_stdout_bytes: claim.job.limits.max_output_bytes,
            max_stderr_bytes: claim.job.limits.max_stderr_bytes,
        })
        .map_err(|_| JobWorkerError::InvalidConfig)?;
    // A successful single-use barrier is necessary before starting the runner.
    if let Err(error) = store.begin_job_execution(project, &claim.lease, &projected.projection) {
        return match error {
            JobStoreError::Cancelled => {
                finish_or_observe(store, project, &claim.lease, JobFinish::CancelledClean)
            }
            JobStoreError::StaleEvidence => finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::EvidenceStale),
            ),
            _ => finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::RunnerFailed),
            ),
        };
    }
    let cancellation = CommandCancellation::default();
    let (output, control) = thread::scope(|scope| {
        let (stop, stopped) = mpsc::channel();
        let cancel = cancellation.clone();
        let lease = &claim.lease;
        let poll = Duration::from_millis(config.budgets.poll_interval_ms);
        let control = thread::Builder::new()
            .name("mmr-job-control".into())
            .spawn_scoped(scope, move || {
                loop {
                    let exit = match controller_store.job_control(project, lease) {
                        Ok(JobControl::Continue) => None,
                        Ok(JobControl::CancelRequested) => Some(ControllerExit::CancelRequested),
                        Ok(JobControl::Lost) => Some(ControllerExit::Lost),
                        Ok(JobControl::Expired) => Some(ControllerExit::Expired),
                        Err(_) => Some(ControllerExit::Unavailable),
                    };
                    if let Some(exit) = exit {
                        cancel.request_cancel();
                        return exit;
                    }
                    match stopped.recv_timeout(poll) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                            return ControllerExit::Finished;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                }
            });
        let Ok(control) = control else {
            return (None, ControllerExit::Unavailable);
        };
        let output = runner.run_with_cancellation(&projected.request, &cancellation);
        let _ = stop.send(());
        let control = control.join().unwrap_or(ControllerExit::Unavailable);
        (Some(output), control)
    });
    let Some(output) = output else {
        // Control-thread creation failed; no runner was invoked.
        return finish_or_observe(
            store,
            project,
            &claim.lease,
            JobFinish::Failed(JobDiagnostic::RunnerFailed),
        );
    };
    if let Err(error) = &output {
        if !cleanup_confirmed(error) {
            return finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Unknown(JobDiagnostic::CleanupUnknown),
            );
        }
    }
    if control == ControllerExit::CancelRequested {
        return finish_or_observe(store, project, &claim.lease, JobFinish::CancelledClean);
    }
    if control != ControllerExit::Finished {
        return finish_or_observe(
            store,
            project,
            &claim.lease,
            JobFinish::Unknown(JobDiagnostic::OwnerLost),
        );
    }
    match output {
        Ok(output) => match store.complete_job(project, &claim.lease, output) {
            Ok(completion) => Ok(completion.job),
            Err(JobStoreError::Cancelled) => {
                finish_or_observe(store, project, &claim.lease, JobFinish::CancelledClean)
            }
            Err(JobStoreError::StaleEvidence) => finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::EvidenceStale),
            ),
            Err(JobStoreError::InvalidOutput) => finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Failed(JobDiagnostic::InvalidOutput),
            ),
            Err(_) => finish_or_observe(
                store,
                project,
                &claim.lease,
                JobFinish::Unknown(JobDiagnostic::OwnerLost),
            ),
        },
        Err(error) => {
            let diagnostic = match error.downcast_ref::<CommandRunnerError>() {
                Some(CommandRunnerError::Deadline) => JobDiagnostic::RunnerTimedOut,
                Some(CommandRunnerError::InvalidOutput) => JobDiagnostic::InvalidOutput,
                Some(
                    CommandRunnerError::RequestLimit { .. }
                    | CommandRunnerError::OutputLimit { .. },
                ) => JobDiagnostic::BudgetExceeded,
                _ => JobDiagnostic::RunnerFailed,
            };
            finish_or_observe(store, project, &claim.lease, JobFinish::Failed(diagnostic))
        }
    }
}

fn cleanup_confirmed(error: &anyhow::Error) -> bool {
    match error.downcast_ref::<CommandRunnerError>() {
        Some(
            CommandRunnerError::InvalidLimits
            | CommandRunnerError::RequestLimit { .. }
            | CommandRunnerError::OutputLimit { .. }
            | CommandRunnerError::Deadline
            | CommandRunnerError::Cancelled
            | CommandRunnerError::UnsuccessfulExit { .. }
            | CommandRunnerError::InvalidOutput,
        ) => true,
        Some(CommandRunnerError::Io { operation }) => matches!(
            *operation,
            "spawn child"
                | "create runtime"
                | "start worker"
                | "write request"
                | "close stdin"
                | "read output"
        ),
        _ => false,
    }
}

fn finish_or_observe(
    store: &mut Store,
    project: &str,
    lease: &JobLease,
    finish: JobFinish,
) -> Result<JobSummary, JobWorkerError> {
    match store.finish_job(project, lease, finish) {
        Ok(job) => Ok(job),
        Err(
            JobStoreError::LostLease
            | JobStoreError::LeaseExpired
            | JobStoreError::InvalidState
            | JobStoreError::Cancelled,
        ) => {
            store.recover_jobs(project)?;
            Ok(store.job(project, &lease.job_id)?.summary)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_worker_cleanup_classification_preserves_uncertainty() {
        for error in [
            CommandRunnerError::CleanupDeadline,
            CommandRunnerError::ChildOwnershipLost,
            CommandRunnerError::UnsupportedExitInspection,
            CommandRunnerError::Io {
                operation: "terminate process group",
            },
            CommandRunnerError::Io {
                operation: "reap child",
            },
            CommandRunnerError::Io {
                operation: "inspect child exit",
            },
            CommandRunnerError::Io {
                operation: "join worker",
            },
            CommandRunnerError::Io {
                operation: "open stdout",
            },
        ] {
            assert!(!cleanup_confirmed(
                &anyhow::Error::new(error).context("synthetic wrapper")
            ));
        }
        assert!(cleanup_confirmed(&CommandRunnerError::Cancelled.into()));
        assert!(cleanup_confirmed(&CommandRunnerError::Deadline.into()));
        assert!(!cleanup_confirmed(&anyhow::anyhow!(
            "arbitrary command-like text: cancelled"
        )));
    }
}
