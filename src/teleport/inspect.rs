use serde::Serialize;

use super::TeleportOutputFormat;
use super::TeleportScanSummary;
use super::TeleportStatus;
use super::bundle::{load_bundle_from_locator, verify_artifact_hashes};
use super::error::TeleportFailure;
use super::manifest::TeleportFidelity;
use super::provider::profile_for;

#[derive(Debug, Clone)]
pub struct InspectOptions {
    pub bundle_path: std::path::PathBuf,
    pub output_format: TeleportOutputFormat,
    pub verbose: bool,
}

#[derive(Debug, Serialize)]
pub struct InspectResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub bundle_id: String,
    pub manifest_version: u32,
    pub fidelity: TeleportFidelity,
    pub restore_ready: bool,
    pub apply_ready: bool,
    pub resume_ready: String,
    pub warnings: Vec<String>,
    pub artifacts: Vec<InspectArtifactSummary>,
    pub scan: TeleportScanSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InspectArtifactSummary {
    pub path: String,
    pub required: bool,
    pub sha256: String,
    pub verified: bool,
}

pub fn inspect_bundle(options: InspectOptions) -> Result<InspectResponse, TeleportFailure> {
    let bundle = load_bundle_from_locator(&options.bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/inspect", error.to_string()))?;
    let mut warnings = verify_artifact_hashes(&bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/inspect", error.to_string()))?;

    if options.verbose {
        warnings.push(format!(
            "manifest source_host={}",
            bundle.manifest.source_host
        ));
    }

    let profile = profile_for(&bundle.manifest.source).ok();
    let native_ready = bundle.manifest.fidelity == TeleportFidelity::Native
        && profile.is_some_and(|profile| profile.supports_native_apply())
        && bundle
            .manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.kind.contains("native"));
    let restore_ready = native_ready;
    let apply_ready = native_ready;

    let artifacts = bundle
        .manifest
        .artifacts
        .iter()
        .map(|artifact| InspectArtifactSummary {
            path: artifact.path.clone(),
            required: artifact.required,
            sha256: artifact.sha256.clone(),
            verified: true,
        })
        .collect::<Vec<_>>();

    let response = InspectResponse {
        command: "teleport/inspect",
        status: TeleportStatus::Ok,
        bundle_id: bundle.manifest.bundle_id.clone(),
        manifest_version: bundle.manifest.mmr_teleport_manifest_version,
        fidelity: bundle.manifest.fidelity,
        restore_ready,
        apply_ready,
        resume_ready: bundle.manifest.restore.agent_resume.clone(),
        warnings,
        artifacts,
        scan: TeleportScanSummary {
            blocking_findings: 0,
            redacted_findings: 0,
            pii_coverage: None,
        },
        text: None,
    };

    if options.output_format == TeleportOutputFormat::Md {
        Ok(InspectResponse {
            text: Some(inspect_markdown(&response, &bundle.manifest.source)),
            ..response
        })
    } else {
        Ok(response)
    }
}

fn inspect_markdown(response: &InspectResponse, source: &str) -> String {
    format!(
        "# Teleport inspect\n\n- bundle_id: {}\n- source: {}\n- fidelity: {:?}\n- restore_ready: {}\n- apply_ready: {}\n",
        response.bundle_id, source, response.fidelity, response.restore_ready, response.apply_ready
    )
}
