use crate::store::evidence::EvidenceBudget;
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::{error::Error, fmt};

pub const MAX_PROJECT_JOBS: usize = 256;
pub const MAX_STORE_JOBS: usize = 4096;
pub const MAX_INSPECTION_ROWS: usize = 100;
pub const MAX_INSPECTION_BYTES: usize = 1024 * 1024;
pub const MAX_PROJECTION_BYTES: usize = 256 * 1024;
pub(super) const MAX_POLICY_BYTES: usize = 4096;
pub(super) const MAX_LABEL_BYTES: usize = 128;
pub(super) const CONTROL_MARGIN_MS: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobLimits {
    pub max_sessions: usize,
    pub max_events: usize,
    pub max_evidence_bytes: usize,
    pub max_request_bytes: usize,
    pub max_output_bytes: usize,
    pub max_stderr_bytes: usize,
    pub timeout_ms: u64,
    pub cleanup_ms: u64,
    pub lease_ms: u64,
    pub max_claim_attempts: u32,
}
impl Default for JobLimits {
    fn default() -> Self {
        Self {
            max_sessions: 8,
            max_events: 2000,
            max_evidence_bytes: 2 * 1024 * 1024,
            max_request_bytes: 2 * 1024 * 1024,
            max_output_bytes: 256 * 1024,
            max_stderr_bytes: 64 * 1024,
            timeout_ms: 60_000,
            cleanup_ms: 2000,
            lease_ms: 65_000,
            max_claim_attempts: 3,
        }
    }
}
impl JobLimits {
    pub fn validate(self) -> Result<Self, JobStoreError> {
        if self.max_sessions == 0
            || self.max_sessions > 8
            || self.max_events == 0
            || self.max_events > 2000
            || self.max_evidence_bytes == 0
            || self.max_evidence_bytes > 2 * 1024 * 1024
            || self.max_request_bytes == 0
            || self.max_request_bytes > 2 * 1024 * 1024
            || self.max_output_bytes == 0
            || self.max_output_bytes > 256 * 1024
            || self.max_stderr_bytes == 0
            || self.max_stderr_bytes > 64 * 1024
            || self.timeout_ms == 0
            || self.timeout_ms > 60_000
            || self.cleanup_ms == 0
            || self.cleanup_ms > 2000
            || self.lease_ms > 90_000
            || self.lease_ms < self.timeout_ms + self.cleanup_ms + CONTROL_MARGIN_MS
            || self.max_claim_attempts == 0
            || self.max_claim_attempts > 8
        {
            return Err(JobStoreError::InvalidInput);
        }
        Ok(self)
    }
    pub fn evidence_budget(self) -> EvidenceBudget {
        EvidenceBudget {
            max_sessions: self.max_sessions,
            max_events: self.max_events,
            max_bytes: self.max_evidence_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobPolicy {
    pub projection_policy_version: String,
    pub prompt_policy_version: String,
    /// Hash of the explicitly selected command and effective configuration; never argv.
    pub runner_fingerprint: String,
}
impl JobPolicy {
    pub fn fingerprint(&self, limits: &JobLimits) -> Result<String, JobStoreError> {
        super::policy_fingerprint(self, *limits)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Claimed,
    Executing,
    Proposed,
    Failed,
    Stale,
    Cancelled,
    Unknown,
}
impl JobState {
    pub(super) fn terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Claimed | Self::Executing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobDiagnostic {
    ClaimAttemptsExhausted,
    LeaseExpired,
    OwnerLost,
    Cancelled,
    NoEligibleEvidence,
    ProjectionFailed,
    BudgetExceeded,
    RunnerFailed,
    RunnerTimedOut,
    InvalidOutput,
    CleanupUnknown,
    LeaseWindowExhausted,
    EvidenceStale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobLease {
    pub job_id: String,
    pub owner_id: String,
    pub fence: i64,
    pub lease_expires_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOmissionReason {
    PolicyBlocked,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobOmission {
    pub event_id: String,
    pub reason: JobOmissionReason,
}

fn bounded<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Entries<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Entries<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most 2000 projection entries")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while values.len() < 2000 {
                let Some(value) = sequence.next_element()? else {
                    return Ok(values);
                };
                values.push(value);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom("projection count bound"));
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Entries(std::marker::PhantomData))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobProjection {
    pub policy_version: String,
    /// Hash of the actual bounded shared-safe request, supplied by the trusted worker.
    pub evidence_hash: String,
    #[serde(deserialize_with = "bounded")]
    pub included_event_ids: Vec<String>,
    #[serde(deserialize_with = "bounded")]
    pub omissions: Vec<JobOmission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSummary {
    pub id: String,
    pub project_id: String,
    pub snapshot_id: String,
    pub snapshot_digest: String,
    pub policy_fingerprint: String,
    pub state: JobState,
    pub attempts: u32,
    pub fence: i64,
    pub cancel_requested: bool,
    pub execution_intent_at_ms: Option<i64>,
    pub may_have_executed: bool,
    pub cleanup_confirmed: bool,
    pub owner_id: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
    pub diagnostic: Option<JobDiagnostic>,
    pub receipt_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRecord {
    pub summary: JobSummary,
    pub policy: JobPolicy,
    pub limits: JobLimits,
    pub projection: Option<JobProjection>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobClaim {
    pub job: JobRecord,
    pub lease: JobLease,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobCompletion {
    pub job: JobSummary,
    pub receipt: crate::store::evidence::ProposalReceipt,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobControl {
    Continue,
    CancelRequested,
    Lost,
    Expired,
}
/// Failed/CancelledClean attest all owned execution and cleanup has joined.
/// Uncertain launch/cleanup MUST use Unknown. No disposition causes a retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobFinish {
    Failed(JobDiagnostic),
    CancelledClean,
    Unknown(JobDiagnostic),
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct JobRecovery {
    pub unknown: u32,
    pub exhausted: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStoreError {
    InvalidInput,
    QueueFull,
    NotFound,
    PolicyMismatch,
    LostLease,
    LeaseExpired,
    Cancelled,
    StaleEvidence,
    InvalidState,
    InvalidOutput,
    Storage,
    Clock,
}
impl fmt::Display for JobStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid_job_input",
            Self::QueueFull => "job_queue_full",
            Self::NotFound => "job_not_found",
            Self::PolicyMismatch => "job_policy_mismatch",
            Self::LostLease => "job_lease_lost",
            Self::LeaseExpired => "job_lease_expired",
            Self::Cancelled => "job_cancel_requested",
            Self::StaleEvidence => "job_evidence_stale",
            Self::InvalidState => "invalid_job_state",
            Self::InvalidOutput => "invalid_job_output",
            Self::Storage => "job_storage_error",
            Self::Clock => "job_clock_error",
        })
    }
}
impl Error for JobStoreError {}
