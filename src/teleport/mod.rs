mod apply;
mod bundle;
mod error;
mod export;
mod file;
mod http;
mod inspect;
mod manifest;
mod pack;
mod provider;
pub use provider::profiles as providers;
mod read;
mod receive;
mod resume;
mod send;
mod ssh;

pub use apply::{ApplyOptions, apply_bundle};
pub use bundle::{
    BundleLocatorError, METADATA_PATH, load_bundle, load_bundle_from_locator, write_bundle,
};
pub use error::TeleportFailure;
pub use export::{ExportOptions, ExportResponse, export_bundle, parse_export_as};
pub use http::{ServeError, ServeOptions, fetch_and_cache_http_bundle, serve_session};
pub use inspect::{InspectOptions, inspect_bundle};
pub use manifest::{BundleMetadata, TeleportFidelity, TeleportManifest, project_aliases};
pub use pack::{PackOptions, pack_session};
pub use provider::{profile_for, supported_sources};
pub use read::{ReadOptions, ReadResponse, read_bundle, resolve_read_locator};
pub use receive::{ReceiveOptions, receive_bundle, resolve_receive_locator};
pub use resume::{
    ResumeAgentAs, ResumeOptions, ResumeResponse, parse_resume_agent_as, resolve_target_agent,
    resume_bundle,
};
pub use send::{SendOptions, SendTransport, send_session};

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeleportStatus {
    Ok,
    Skipped,
    Partial,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportOutputFormat {
    Json,
    Md,
}

impl TeleportOutputFormat {
    pub fn from_arg(value: &str) -> Result<Self, String> {
        match value {
            "json" => Ok(Self::Json),
            "md" => Ok(Self::Md),
            other => Err(other.to_string()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TeleportScanSummary {
    pub blocking_findings: usize,
    pub redacted_findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pii_coverage: Option<String>,
}

pub(crate) fn resolve_bundle_locator(
    positional: Option<PathBuf>,
    to: Option<PathBuf>,
    subcommand: &str,
) -> std::result::Result<PathBuf, BundleLocatorError> {
    match (positional, to) {
        (Some(_), Some(_)) => Err(BundleLocatorError::MultipleLocators {
            subcommand: subcommand.to_string(),
        }),
        (None, None) => Err(BundleLocatorError::MissingLocator {
            subcommand: subcommand.to_string(),
        }),
        (Some(path), None) | (None, Some(path)) => Ok(path),
    }
}
