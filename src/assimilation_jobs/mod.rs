//! Explicit, finite local command jobs. No automatic discovery or scheduler.
use std::fmt;
use std::path::Path;

use crate::store::evidence::ProposalReceipt;
use crate::store::jobs::{JobRecord, JobStoreError, JobSummary};
use serde::Serialize;

mod config;
mod projection;
mod worker;
pub use config::{CommandPolicy, JobBudgets, JobConfig};

#[derive(Debug, Clone)]
pub enum JobsAction {
    Enqueue {
        session_ids: Vec<String>,
        attest_complete: Option<String>,
    },
    Run {
        max_jobs: u8,
    },
    Status {
        limit: usize,
    },
    Show {
        job_id: String,
    },
    Cancel {
        job_id: String,
    },
}

#[derive(Debug, Serialize)]
pub struct JobsResponse {
    pub command: &'static str,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<JobSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<JobSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ProposalReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<JobRecord>,
}

pub fn execute(config: Option<&Path>, action: JobsAction) -> Result<JobsResponse, JobWorkerError> {
    let started = std::time::Instant::now();
    worker::execute(JobConfig::load(config)?, action, started)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobWorkerError {
    InvalidConfig,
    ConfigRead,
    Store,
    InvalidAction,
    Clock,
    Control,
    Projection,
    JobStore(JobStoreError),
}
impl fmt::Display for JobWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::JobStore(error) = self {
            return fmt::Display::fmt(error, f);
        }
        f.write_str(match self {
            Self::InvalidConfig => "invalid assimilation job configuration",
            Self::ConfigRead => "cannot read assimilation job configuration",
            Self::Store => "assimilation job store operation failed",
            Self::InvalidAction => "invalid assimilation job action",
            Self::Clock => "assimilation job clock unavailable",
            Self::Control => "assimilation job control unavailable",
            Self::Projection => "assimilation job privacy projection failed",
            Self::JobStore(_) => unreachable!(),
        })
    }
}
impl std::error::Error for JobWorkerError {}
impl From<JobStoreError> for JobWorkerError {
    fn from(error: JobStoreError) -> Self {
        Self::JobStore(error)
    }
}
