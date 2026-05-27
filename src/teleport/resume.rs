use std::path::PathBuf;

use serde::Serialize;

use super::TeleportStatus;
use super::apply::{ApplyOptions, ApplyResponse, apply_bundle};
use super::bundle::{load_bundle_from_locator, verify_artifact_hashes};
use super::error::TeleportFailure;
use super::manifest::TeleportFidelity;
use super::provider::profile_for;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeAgentAs {
    Same,
    Codex,
    Claude,
    Cursor,
    Grok,
    Pi,
}

#[derive(Debug, Clone)]
pub struct ResumeOptions {
    pub bundle_path: PathBuf,
    pub project: Option<String>,
    pub dry_run: bool,
    pub force: bool,
    pub no_agent_exec: bool,
    pub requested_as: ResumeAgentAs,
    pub requested_as_label: String,
}

#[derive(Debug, Serialize)]
pub struct ResumeAgentSummary {
    pub provider: String,
    pub requested_as: String,
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub manual_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ResumeResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub bundle_id: String,
    pub requested_as: String,
    pub target_agent: String,
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply: Option<ApplyResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<ResumeAgentSummary>,
}

pub fn parse_resume_agent_as(
    value: Option<&str>,
) -> Result<(ResumeAgentAs, String), TeleportFailure> {
    let label = value.unwrap_or("same");
    match label {
        "same" => Ok((ResumeAgentAs::Same, label.to_string())),
        "codex" => Ok((ResumeAgentAs::Codex, label.to_string())),
        "claude" => Ok((ResumeAgentAs::Claude, label.to_string())),
        "cursor" => Ok((ResumeAgentAs::Cursor, label.to_string())),
        "grok" => Ok((ResumeAgentAs::Grok, label.to_string())),
        "pi" => Ok((ResumeAgentAs::Pi, label.to_string())),
        "native" | "shared-safe" => Err(TeleportFailure::usage(
            "teleport/resume",
            format!(
                "--as {label} is not valid for teleport resume; allowed values: same, codex, claude, cursor, grok, pi"
            ),
        )),
        "json" | "md" => Err(TeleportFailure::usage(
            "teleport/resume",
            "--as json is not valid for teleport resume; use -O for output format",
        )),
        other => Err(TeleportFailure::usage(
            "teleport/resume",
            format!(
                "unsupported --as value {other:?}; allowed values: same, codex, claude, cursor, grok, pi"
            ),
        )),
    }
}

pub fn resolve_target_agent(requested: ResumeAgentAs, bundle_source: &str) -> String {
    match requested {
        ResumeAgentAs::Same => bundle_source.to_string(),
        ResumeAgentAs::Codex => "codex".to_string(),
        ResumeAgentAs::Claude => "claude".to_string(),
        ResumeAgentAs::Cursor => "cursor".to_string(),
        ResumeAgentAs::Grok => "grok".to_string(),
        ResumeAgentAs::Pi => "pi".to_string(),
    }
}

pub fn resume_bundle(options: ResumeOptions) -> Result<ResumeResponse, TeleportFailure> {
    let bundle = load_bundle_from_locator(&options.bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/resume", error.to_string()))?;
    verify_artifact_hashes(&bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/resume", error.to_string()))?;

    let bundle_id = bundle.manifest.bundle_id.clone();
    let target_agent = resolve_target_agent(options.requested_as, &bundle.manifest.source);
    let profile = profile_for(&bundle.manifest.source)?;

    if !profile.supports_resume_transform(&bundle.manifest.source, &target_agent) {
        return Ok(unsupported_resume_response(
            bundle_id,
            &options.requested_as_label,
            &target_agent,
            options.dry_run,
            unsupported_transform_message(&bundle.manifest.source, &target_agent),
        ));
    }

    if bundle.manifest.fidelity != TeleportFidelity::Native {
        return Err(TeleportFailure::usage(
            "teleport/resume",
            "teleport resume requires a native fidelity bundle",
        ));
    }

    let apply = apply_bundle(ApplyOptions {
        bundle_path: options.bundle_path,
        project: options.project.clone(),
        dry_run: options.dry_run,
        force: options.force,
        skip_store_import: false,
    })
    .map_err(map_apply_failure)?;

    let agent = build_agent_summary(
        &apply,
        options.requested_as_label.as_str(),
        options.no_agent_exec,
    );

    Ok(ResumeResponse {
        command: "teleport/resume",
        status: apply.status,
        bundle_id: apply.bundle_id.clone(),
        requested_as: options.requested_as_label,
        target_agent,
        dry_run: options.dry_run,
        message: None,
        apply: Some(apply),
        agent: Some(agent),
    })
}

fn unsupported_transform_message(bundle_source: &str, target_agent: &str) -> String {
    format!(
        "teleport resume does not transform {bundle_source} bundles to {target_agent}; cross-agent resume is not supported"
    )
}

fn unsupported_resume_response(
    bundle_id: String,
    requested_as: &str,
    target_agent: &str,
    dry_run: bool,
    message: String,
) -> ResumeResponse {
    ResumeResponse {
        command: "teleport/resume",
        status: TeleportStatus::Unsupported,
        bundle_id,
        requested_as: requested_as.to_string(),
        target_agent: target_agent.to_string(),
        dry_run,
        message: Some(message),
        apply: None,
        agent: None,
    }
}

fn build_agent_summary(
    apply: &ApplyResponse,
    requested_as: &str,
    no_agent_exec: bool,
) -> ResumeAgentSummary {
    let _ = no_agent_exec;
    let documented_command = apply.resume.documented_command.clone();
    let manual_steps = vec![format!("Run manually when ready: {documented_command}")];
    ResumeAgentSummary {
        provider: apply.resume.provider.clone(),
        requested_as: requested_as.to_string(),
        executed: false,
        command: Some(documented_command),
        status: Some(apply.resume.status.clone()),
        manual_steps,
    }
}

fn map_apply_failure(failure: TeleportFailure) -> TeleportFailure {
    let mut mapped = TeleportFailure::runtime("teleport/resume", failure.message);
    mapped.exit_code = failure.exit_code;
    mapped.error_kind = failure.error_kind;
    mapped
}
