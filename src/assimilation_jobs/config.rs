use std::fmt;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::JobWorkerError;
use crate::store::content_hash;
use crate::store::jobs::{JobLimits, JobPolicy};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const CONFIG_BYTES: usize = 32 * 1024;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobConfig {
    pub version: u32,
    pub enabled: bool,
    pub database_path: Option<PathBuf>,
    pub project_id: Option<String>,
    pub policy_version: String,
    pub command: Option<CommandPolicy>,
    pub budgets: JobBudgets,
}
impl Default for JobConfig {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: false,
            database_path: None,
            project_id: None,
            policy_version: "assimilation-jobs/v1".into(),
            command: None,
            budgets: JobBudgets::default(),
        }
    }
}
// Neither paths nor command arguments belong in errors/logs.
impl fmt::Debug for JobConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobConfig")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandPolicy {
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    pub version: String,
}
impl fmt::Debug for CommandPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CommandPolicy { .. }")
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct JobBudgets {
    pub max_sessions: usize,
    pub max_events: usize,
    pub max_evidence_bytes: usize,
    pub max_request_bytes: usize,
    pub max_output_bytes: usize,
    pub max_stderr_bytes: usize,
    pub execution_timeout_ms: u64,
    pub cleanup_timeout_ms: u64,
    pub invocation_timeout_ms: u64,
    pub poll_interval_ms: u64,
    pub lease_ms: u64,
    pub max_claim_attempts: usize,
}
impl Default for JobBudgets {
    fn default() -> Self {
        Self {
            max_sessions: 8,
            max_events: 2000,
            max_evidence_bytes: 2 * 1024 * 1024,
            max_request_bytes: 2 * 1024 * 1024,
            max_output_bytes: 256 * 1024,
            max_stderr_bytes: 64 * 1024,
            execution_timeout_ms: 60_000,
            cleanup_timeout_ms: 2000,
            invocation_timeout_ms: 120_000,
            poll_interval_ms: 25,
            lease_ms: 65_000,
            max_claim_attempts: 3,
        }
    }
}
fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn path(value: &Path) -> bool {
    value.is_absolute()
        && value
            .to_str()
            .is_some_and(|v| v.len() <= 4096 && !v.contains('\0'))
}
impl JobBudgets {
    pub fn job_limits(self) -> Result<JobLimits, JobWorkerError> {
        self.validate()?;
        Ok(JobLimits {
            max_sessions: self.max_sessions,
            max_events: self.max_events,
            max_evidence_bytes: self.max_evidence_bytes,
            max_request_bytes: self.max_request_bytes,
            max_output_bytes: self.max_output_bytes,
            max_stderr_bytes: self.max_stderr_bytes,
            timeout_ms: self.execution_timeout_ms,
            cleanup_ms: self.cleanup_timeout_ms,
            lease_ms: self.lease_ms,
            max_claim_attempts: self.max_claim_attempts as u32,
        })
    }
    pub fn validate(self) -> Result<(), JobWorkerError> {
        let cap = Self::default();
        if self.max_sessions == 0
            || self.max_sessions > cap.max_sessions
            || self.max_events == 0
            || self.max_events > cap.max_events
            || self.max_evidence_bytes == 0
            || self.max_evidence_bytes > cap.max_evidence_bytes
            || self.max_request_bytes == 0
            || self.max_request_bytes > cap.max_request_bytes
            || self.max_output_bytes == 0
            || self.max_output_bytes > cap.max_output_bytes
            || self.max_stderr_bytes == 0
            || self.max_stderr_bytes > cap.max_stderr_bytes
            || self.execution_timeout_ms == 0
            || self.execution_timeout_ms > cap.execution_timeout_ms
            || self.cleanup_timeout_ms == 0
            || self.cleanup_timeout_ms > cap.cleanup_timeout_ms
            || self.invocation_timeout_ms == 0
            || self.invocation_timeout_ms > cap.invocation_timeout_ms
            || !(10..=250).contains(&self.poll_interval_ms)
            || self.lease_ms < self.execution_timeout_ms + self.cleanup_timeout_ms + 1000
            || self.lease_ms > 90_000
            || !(1..=8).contains(&self.max_claim_attempts)
        {
            return Err(JobWorkerError::InvalidConfig);
        }
        Ok(())
    }
}
impl JobConfig {
    pub fn load(path: Option<&Path>) -> Result<Self, JobWorkerError> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let mut bytes = Vec::new();
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        // Refuse streams/devices: a byte cap alone cannot bound a FIFO read.
        if !path
            .metadata()
            .map_err(|_| JobWorkerError::ConfigRead)?
            .is_file()
        {
            return Err(JobWorkerError::InvalidConfig);
        }
        let file = options.open(path).map_err(|_| JobWorkerError::ConfigRead)?;
        if !file
            .metadata()
            .map_err(|_| JobWorkerError::ConfigRead)?
            .is_file()
        {
            return Err(JobWorkerError::InvalidConfig);
        }
        file.take((CONFIG_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| JobWorkerError::ConfigRead)?;
        if bytes.len() > CONFIG_BYTES {
            return Err(JobWorkerError::InvalidConfig);
        }
        let config: Self =
            serde_json::from_slice(&bytes).map_err(|_| JobWorkerError::InvalidConfig)?;
        config.validate(false)?;
        Ok(config)
    }

    pub fn validate(&self, require_store: bool) -> Result<(), JobWorkerError> {
        let invalid = JobWorkerError::InvalidConfig;
        self.budgets.validate()?;
        if self.version != 1
            || !label(&self.policy_version)
            || self.database_path.as_ref().is_some_and(|p| !path(p))
            || self.project_id.as_deref().is_some_and(|p| !label(p))
            || (require_store && (self.database_path.is_none() || self.project_id.is_none()))
            || (self.enabled && self.command.is_none())
        {
            return Err(invalid);
        }
        if let Some(command) = &self.command {
            if !path(&command.program)
                || !label(&command.version)
                || command.args.len() > 32
                || command
                    .args
                    .iter()
                    .any(|a| a.len() > 1024 || a.contains('\0'))
                || command.args.iter().map(String::len).sum::<usize>() > 16 * 1024
            {
                return Err(invalid);
            }
        }
        if self.enabled && (self.database_path.is_none() || self.project_id.is_none()) {
            return Err(invalid);
        }
        Ok(())
    }

    pub fn policy_hash(&self) -> Result<String, JobWorkerError> {
        self.validate(true)?;
        let material = serde_json::to_string(&(
            "assimilation-job-policy/v1",
            &self.policy_version,
            "deterministic-shared-safe-narrative/v1",
            &self.command,
            self.budgets,
        ))
        .map_err(|_| JobWorkerError::InvalidConfig)?;
        Ok(content_hash(&material))
    }

    pub fn job_policy(&self) -> Result<JobPolicy, JobWorkerError> {
        Ok(JobPolicy {
            projection_policy_version: super::projection::PROJECTION_POLICY.into(),
            prompt_policy_version: self.policy_version.clone(),
            runner_fingerprint: self.policy_hash()?,
        })
    }
}
