use std::path::{Path, PathBuf};

use serde::Serialize;

use super::TeleportStatus;
use super::apply::{ApplyOptions, ApplyResponse, apply_bundle};
use super::bundle::bundle_sha256;
use super::error::TeleportFailure;
use super::file::{
    BUNDLE_FILENAME, InboxEntryState, ReceiveLocatorKind, SHA256_FILENAME,
    classify_receive_locator, inbox_entry_state, parse_locator, verify_ready_inbox_bundle,
};
use super::http::{is_http_locator, parse_http_locator, receive_http_bundle};

#[derive(Debug, Clone)]
pub struct ReceiveOptions {
    pub locator: String,
    pub dry_run: bool,
    pub project: Option<String>,
    pub force: bool,
}

#[derive(Debug, Serialize)]
pub struct ReceiveStagedSummary {
    pub bundle_id: String,
    pub inbox_path: String,
    pub bundle_path: String,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct ReceiveResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub transport: &'static str,
    pub locator: String,
    pub staged: Vec<ReceiveStagedSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply: Option<ApplyResponse>,
    pub dry_run: bool,
}

pub fn receive_bundle(options: ReceiveOptions) -> Result<ReceiveResponse, TeleportFailure> {
    if is_http_locator(&options.locator) {
        return receive_http(&options);
    }

    let path = PathBuf::from(&options.locator);
    let kind = classify_receive_locator(&path)?;

    match kind {
        ReceiveLocatorKind::DirectBundle(bundle_path) => receive_direct_bundle(
            &options.locator,
            bundle_path,
            options.dry_run,
            options.project,
            options.force,
        ),
        ReceiveLocatorKind::InboxEntry(entry_dir) => {
            receive_inbox_entry(&options.locator, &entry_dir, &options)
        }
    }
}

pub fn resolve_receive_locator(
    positional: Option<String>,
    to: Option<String>,
) -> Result<String, TeleportFailure> {
    resolve_teleport_locator("teleport/receive", "receive", positional, to)
}

pub fn resolve_teleport_locator(
    command: &'static str,
    subcommand: &str,
    positional: Option<String>,
    to: Option<String>,
) -> Result<String, TeleportFailure> {
    match (positional, to) {
        (Some(_), Some(_)) => Err(TeleportFailure::usage(
            command,
            format!(
                "teleport {subcommand}: only one bundle locator is allowed; use either a positional path or --to, not both"
            ),
        )),
        (None, None) => Err(TeleportFailure::usage(
            command,
            format!(
                "teleport {subcommand}: bundle path is required; pass a positional path or --to"
            ),
        )),
        (Some(path), None) | (None, Some(path)) => normalize_teleport_locator(command, &path),
    }
}

fn normalize_teleport_locator(
    command: &'static str,
    value: &str,
) -> Result<String, TeleportFailure> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(TeleportFailure::usage(command, "locator path is required"));
    }
    if is_http_locator(trimmed) {
        return Ok(trimmed.to_string());
    }
    parse_locator(trimmed).map(|path| path.display().to_string())
}

fn receive_http(options: &ReceiveOptions) -> Result<ReceiveResponse, TeleportFailure> {
    let target = parse_http_locator(&options.locator)?;
    let (staged, apply) = receive_http_bundle(
        &target,
        options.dry_run,
        options.project.clone(),
        options.force,
    )?;
    let status = apply
        .as_ref()
        .map(|response| response.status)
        .unwrap_or(TeleportStatus::Ok);
    Ok(ReceiveResponse {
        command: "teleport/receive",
        status,
        transport: "http",
        locator: options.locator.clone(),
        staged,
        apply,
        dry_run: options.dry_run,
    })
}

fn receive_direct_bundle(
    locator: &str,
    bundle_path: PathBuf,
    dry_run: bool,
    project: Option<String>,
    force: bool,
) -> Result<ReceiveResponse, TeleportFailure> {
    let sha256 = bundle_sha256(&bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/receive", error.to_string()))?;
    let apply = apply_bundle(ApplyOptions {
        bundle_path: bundle_path.clone(),
        project,
        dry_run,
        force,
        skip_store_import: true,
    })
    .map_err(map_apply_failure)?;
    let staged = vec![ReceiveStagedSummary {
        bundle_id: apply.bundle_id.clone(),
        inbox_path: bundle_path
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        bundle_path: bundle_path.display().to_string(),
        sha256,
    }];
    Ok(ReceiveResponse {
        command: "teleport/receive",
        status: apply.status,
        transport: "file",
        locator: locator.to_string(),
        staged,
        apply: Some(apply),
        dry_run,
    })
}

fn receive_inbox_entry(
    locator: &str,
    entry_dir: &Path,
    options: &ReceiveOptions,
) -> Result<ReceiveResponse, TeleportFailure> {
    match inbox_entry_state(entry_dir) {
        InboxEntryState::Waiting => Ok(ReceiveResponse {
            command: "teleport/receive",
            status: TeleportStatus::Ok,
            transport: "file",
            locator: locator.to_string(),
            staged: Vec::new(),
            apply: None,
            dry_run: options.dry_run,
        }),
        InboxEntryState::Ready => {
            let bundle_path = entry_dir.join(BUNDLE_FILENAME);
            let sha256_path = entry_dir.join(SHA256_FILENAME);
            verify_ready_inbox_bundle(&bundle_path, &sha256_path)?;
            let sha256 = bundle_sha256(&bundle_path)
                .map_err(|error| TeleportFailure::runtime("teleport/receive", error.to_string()))?;
            let apply = apply_bundle(ApplyOptions {
                bundle_path: bundle_path.clone(),
                project: options.project.clone(),
                dry_run: options.dry_run,
                force: options.force,
                skip_store_import: true,
            })
            .map_err(map_apply_failure)?;
            Ok(ReceiveResponse {
                command: "teleport/receive",
                status: apply.status,
                transport: "file",
                locator: locator.to_string(),
                staged: vec![ReceiveStagedSummary {
                    bundle_id: apply.bundle_id.clone(),
                    inbox_path: entry_dir.display().to_string(),
                    bundle_path: bundle_path.display().to_string(),
                    sha256,
                }],
                apply: Some(apply),
                dry_run: options.dry_run,
            })
        }
    }
}

fn map_apply_failure(failure: TeleportFailure) -> TeleportFailure {
    let mut mapped = TeleportFailure::runtime("teleport/receive", failure.message);
    mapped.exit_code = failure.exit_code;
    mapped.error_kind = failure.error_kind;
    mapped
}
