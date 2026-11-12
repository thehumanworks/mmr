mod apply;
mod bundle;
mod error;
mod file;
mod http;
mod manifest;
mod pack;
mod provider;
mod read;
mod receive;
mod send;
mod ssh;

pub use apply::{ApplyOptions, apply_bundle};
pub(crate) use bundle::verify_artifact_hashes;
pub use bundle::{TeleportBundleFile, load_bundle, write_bundle};
pub use error::TeleportFailure;
pub use http::{ServeError, ServeOptions, serve_session};
pub use manifest::{BundleMetadata, TeleportFidelity, project_aliases};
pub use pack::{PackOptions, pack_session};
pub use provider::profile_for;
pub use read::{ReadOptions, read_bundle};
pub use receive::{ReceiveOptions, receive_bundle};
pub use send::{SendOptions, SendTransport, send_session};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeleportStatus {
    Ok,
    Skipped,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportOutputFormat {
    Json,
    Md,
}

#[derive(Debug, Serialize)]
pub struct TeleportScanSummary {
    pub blocking_findings: usize,
    pub redacted_findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pii_coverage: Option<String>,
}
