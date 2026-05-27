use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use super::TeleportStatus;
use super::bundle::{load_bundle_from_locator, verify_artifact_hashes};
use super::error::TeleportFailure;
use super::manifest::TeleportFidelity;
use super::provider::{profile_for, resolve_bundle_native_path};
use super::resume::{ResumeAgentAs, parse_resume_agent_as, resolve_target_agent};

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub bundle_path: PathBuf,
    pub to: PathBuf,
    pub requested_as: ResumeAgentAs,
    pub requested_as_label: String,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct ExportResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub bundle_id: String,
    pub requested_as: String,
    pub target_format: String,
    pub to: String,
    pub bytes: u64,
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn parse_export_as(value: Option<&str>) -> Result<(ResumeAgentAs, String), TeleportFailure> {
    let label = value.unwrap_or("same");
    match label {
        "same" | "codex" | "claude" | "cursor" | "grok" | "pi" => {
            parse_resume_agent_as(Some(label))
        }
        "native" | "shared-safe" => Err(TeleportFailure::usage(
            "teleport/export",
            format!(
                "--as {label} is not valid for teleport export; allowed values: same, codex, claude, cursor, grok, pi"
            ),
        )),
        "json" | "md" => Err(TeleportFailure::usage(
            "teleport/export",
            "--as json is not valid for teleport export; use -O for output format",
        )),
        other => Err(TeleportFailure::usage(
            "teleport/export",
            format!(
                "unsupported --as value {other:?}; allowed values: same, codex, claude, cursor, grok, pi"
            ),
        )),
    }
}

pub fn export_bundle(options: ExportOptions) -> Result<ExportResponse, TeleportFailure> {
    let bundle = load_bundle_from_locator(&options.bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/export", error.to_string()))?;
    verify_artifact_hashes(&bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/export", error.to_string()))?;

    let bundle_id = bundle.manifest.bundle_id.clone();
    let target_format = resolve_target_agent(options.requested_as, &bundle.manifest.source);
    let to = options.to.display().to_string();
    let profile = profile_for(&bundle.manifest.source)?;

    if !profile.supports_resume_transform(&bundle.manifest.source, &target_format) {
        return Ok(unsupported_export_response(
            bundle_id,
            &options.requested_as_label,
            &target_format,
            &to,
            options.dry_run,
            unsupported_transform_message(&bundle.manifest.source, &target_format),
        ));
    }

    if bundle.manifest.fidelity != TeleportFidelity::Native {
        return Err(TeleportFailure::runtime(
            "teleport/export",
            "teleport export requires a native fidelity bundle",
        ));
    }

    let primary_path = profile.primary_native_path();
    let native_transcript =
        resolve_bundle_native_path(profile, &bundle, primary_path).ok_or_else(|| {
            TeleportFailure::runtime(
                "teleport/export",
                format!("native artifact missing from bundle at {primary_path}"),
            )
        })?;

    if profile.source_name() == "grok" {
        if looks_like_file_path(&options.to) {
            return Err(TeleportFailure::usage(
                "teleport/export",
                "teleport export for grok bundles requires --to <directory> to write summary.json and updates.jsonl",
            ));
        }
        return export_grok_directory(&bundle, profile, &options, bundle_id, target_format, to);
    }

    let bytes = native_transcript.len() as u64;

    if !options.dry_run {
        if let Some(parent) = options.to.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| TeleportFailure::runtime("teleport/export", error.to_string()))?;
        }
        fs::write(&options.to, &native_transcript)
            .map_err(|error| TeleportFailure::runtime("teleport/export", error.to_string()))?;
    }

    Ok(ExportResponse {
        command: "teleport/export",
        status: TeleportStatus::Ok,
        bundle_id,
        requested_as: options.requested_as_label,
        target_format,
        to,
        bytes,
        dry_run: options.dry_run,
        message: None,
    })
}

fn export_grok_directory(
    bundle: &super::bundle::TeleportBundleFile,
    profile: &dyn super::provider::TeleportProviderProfile,
    options: &ExportOptions,
    bundle_id: String,
    target_format: String,
    to: String,
) -> Result<ExportResponse, TeleportFailure> {
    use crate::teleport::provider::profiles::grok::{SUMMARY_PATH, UPDATES_PATH};

    let summary = resolve_bundle_native_path(profile, bundle, SUMMARY_PATH).ok_or_else(|| {
        TeleportFailure::runtime("teleport/export", "grok summary.json missing from bundle")
    })?;
    let updates = resolve_bundle_native_path(profile, bundle, UPDATES_PATH).ok_or_else(|| {
        TeleportFailure::runtime("teleport/export", "grok updates.jsonl missing from bundle")
    })?;
    let bytes = (summary.len() + updates.len()) as u64;

    if !options.dry_run {
        fs::create_dir_all(&options.to)
            .map_err(|error| TeleportFailure::runtime("teleport/export", error.to_string()))?;
        fs::write(options.to.join("summary.json"), summary)
            .map_err(|error| TeleportFailure::runtime("teleport/export", error.to_string()))?;
        fs::write(options.to.join("updates.jsonl"), updates)
            .map_err(|error| TeleportFailure::runtime("teleport/export", error.to_string()))?;
    }

    Ok(ExportResponse {
        command: "teleport/export",
        status: TeleportStatus::Ok,
        bundle_id,
        requested_as: options.requested_as_label.clone(),
        target_format,
        to,
        bytes,
        dry_run: options.dry_run,
        message: None,
    })
}

fn unsupported_transform_message(bundle_source: &str, target_format: &str) -> String {
    format!(
        "teleport export does not transform {bundle_source} bundles to {target_format}; cross-agent export is not supported"
    )
}

fn looks_like_file_path(path: &std::path::Path) -> bool {
    path.is_file() || (path.extension().is_some() && !path.is_dir())
}

fn unsupported_export_response(
    bundle_id: String,
    requested_as: &str,
    target_format: &str,
    to: &str,
    dry_run: bool,
    message: String,
) -> ExportResponse {
    ExportResponse {
        command: "teleport/export",
        status: TeleportStatus::Unsupported,
        bundle_id,
        requested_as: requested_as.to_string(),
        target_format: target_format.to_string(),
        to: to.to_string(),
        bytes: 0,
        dry_run,
        message: Some(message),
    }
}
