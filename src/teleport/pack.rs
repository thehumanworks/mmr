use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::bundle::{
    METADATA_PATH, NATIVE_TRANSCRIPT_PATH, NORMALIZED_TRANSCRIPT_PATH, RESTORE_CODEX_PATH,
    TeleportBundleFile, bundle_bytes, bundle_sha256, compute_content_bundle_id,
    default_bundle_path, hash_text, write_bundle,
};
use super::error::TeleportFailure;
use super::manifest::{
    BundleMetadata, ManifestArtifact, ManifestProject, ManifestRestore, ManifestSession,
    TeleportFidelity, TeleportManifest, path_remap_for_project, project_aliases,
    restore_hints_for_codex,
};
use super::{TeleportScanSummary, TeleportStatus};
use crate::capture::CodexAdapter;
use crate::messages::service::{MessageQueryOptions, QueryService};
use crate::redaction::{DeterministicPrivacyDetector, scan_text_with_detector};
use crate::types::{SortBy, SortOptions, SortOrder, SourceFilter};

const SUPPORTED_SOURCE: &str = "codex";

#[derive(Debug, Clone)]
pub struct PackOptions {
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub source_filter: Option<SourceFilter>,
    pub output_path: Option<PathBuf>,
    pub fidelity: TeleportFidelity,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct PackArtifactSummary {
    pub path: String,
    pub sha256: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackSessionSummary {
    pub source: String,
    pub source_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PackResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub bundle_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub fidelity: TeleportFidelity,
    pub session: PackSessionSummary,
    pub artifacts: Vec<PackArtifactSummary>,
    pub scan: TeleportScanSummary,
    pub dry_run: bool,
}

pub fn pack_session(
    service: &QueryService,
    options: PackOptions,
) -> Result<PackResponse, TeleportFailure> {
    if options.fidelity == TeleportFidelity::SharedSafe {
        return Err(TeleportFailure::runtime(
            "teleport/pack",
            "teleport pack supports native Codex bundles only; --as shared-safe is not supported",
        ));
    }

    if let Some(source_filter) = options.source_filter
        && source_filter != SourceFilter::Codex
    {
        return Err(TeleportFailure::runtime(
            "teleport/pack",
            "teleport pack supports native Codex bundles only; omit --source or pass --source codex",
        ));
    }

    let context = service
        .resolve_teleport_session(
            options.session_id.as_deref(),
            options.project.as_deref(),
            Some(SourceFilter::Codex),
        )
        .map_err(map_session_lookup_failure)?;

    if context.session.source != SUPPORTED_SOURCE {
        return Err(TeleportFailure::runtime(
            "teleport/pack",
            format!(
                "teleport pack supports native Codex bundles only; session source is {}",
                context.session.source
            ),
        ));
    }

    let native_transcript = fs::read_to_string(&context.source_file).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/pack",
            format!(
                "read native transcript {}: {error}",
                context.source_file.display()
            ),
        )
    })?;

    let scan = scan_native_transcript(&native_transcript);

    let messages = service
        .messages(
            Some(&context.session.session_id),
            Some(&context.session.project_name),
            Some(SourceFilter::Codex),
            MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
        )
        .map_err(|error| {
            TeleportFailure::runtime(
                "teleport/pack",
                format!("load normalized transcript messages: {error}"),
            )
        })?;
    let normalized_lines = messages
        .messages
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            TeleportFailure::runtime(
                "teleport/pack",
                format!("serialize normalized transcript: {error}"),
            )
        })?;
    let normalized_transcript = normalized_lines.join("\n");
    let restore_json = restore_hints_for_codex(&context.session.session_id).to_string();

    let created_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| {
            TeleportFailure::runtime(
                "teleport/pack",
                format!("format created_at timestamp: {error}"),
            )
        })?;
    let metadata = BundleMetadata {
        source: context.session.source.clone(),
        source_session_id: context.session.session_id.clone(),
        project_name: context.session.project_name.clone(),
        project_path: context.session.project_path.clone(),
        native_source_file: context.source_file.display().to_string(),
        packed_at: created_at.clone(),
        notes: Some("native Codex bundle (NHL-322)".to_string()),
    };
    let metadata_json = serde_json::to_string(&metadata).map_err(|error| {
        TeleportFailure::runtime("teleport/pack", format!("serialize metadata: {error}"))
    })?;
    let metadata_hash = hash_text(&metadata_json);
    let native_hash = hash_text(&native_transcript);
    let normalized_hash = hash_text(&normalized_transcript);
    let restore_hash = hash_text(&restore_json);

    let bundle_id = compute_content_bundle_id(&native_hash, &normalized_hash, Some(&restore_hash));

    let artifacts = vec![
        ManifestArtifact {
            path: METADATA_PATH.to_string(),
            required: true,
            sha256: metadata_hash,
            kind: "metadata".to_string(),
        },
        ManifestArtifact {
            path: NATIVE_TRANSCRIPT_PATH.to_string(),
            required: true,
            sha256: native_hash,
            kind: "native_transcript".to_string(),
        },
        ManifestArtifact {
            path: NORMALIZED_TRANSCRIPT_PATH.to_string(),
            required: false,
            sha256: normalized_hash,
            kind: "normalized_transcript".to_string(),
        },
        ManifestArtifact {
            path: RESTORE_CODEX_PATH.to_string(),
            required: false,
            sha256: restore_hash,
            kind: "restore_hints".to_string(),
        },
    ];

    let canonical_path = if context.session.project_path.is_empty() {
        context.session.project_name.clone()
    } else {
        context.session.project_path.clone()
    };
    let manifest = TeleportManifest {
        mmr_teleport_manifest_version: 1,
        bundle_id: bundle_id.clone(),
        created_at,
        source_host: source_host_name(),
        mmr_version: env!("CARGO_PKG_VERSION").to_string(),
        min_mmr_version: env!("CARGO_PKG_VERSION").to_string(),
        source: SUPPORTED_SOURCE.to_string(),
        parser_version: CodexAdapter::PARSER_VERSION.to_string(),
        fidelity: TeleportFidelity::Native,
        session: ManifestSession {
            source_session_id: context.session.session_id.clone(),
            message_count: context.session.message_count.max(0) as u32,
            first_timestamp: context.session.first_timestamp.clone(),
            last_timestamp: context.session.last_timestamp.clone(),
            partial_tail: false,
        },
        project: ManifestProject {
            canonical_path: canonical_path.clone(),
            aliases: project_aliases(&canonical_path),
            path_remap: path_remap_for_project(&canonical_path),
        },
        artifacts: artifacts.clone(),
        capabilities: vec!["codex-native-apply".to_string(), "store-import".to_string()],
        restore: ManifestRestore {
            agent_resume: "best_effort".to_string(),
            documented_command: format!("codex exec resume {}", context.session.session_id),
            adapters: vec!["codex-native-apply".to_string()],
        },
    };

    let mut files = BTreeMap::new();
    files.insert(NATIVE_TRANSCRIPT_PATH.to_string(), native_transcript);
    files.insert(
        NORMALIZED_TRANSCRIPT_PATH.to_string(),
        normalized_transcript,
    );
    files.insert(RESTORE_CODEX_PATH.to_string(), restore_json);

    let bundle = TeleportBundleFile {
        mmr_teleport_bundle_version: super::bundle::BUNDLE_FORMAT_VERSION,
        manifest,
        metadata,
        files,
    };

    let output_path = match options.output_path {
        Some(path) => path,
        None => default_bundle_path(&bundle_id)
            .map_err(|error| TeleportFailure::runtime("teleport/pack", error.to_string()))?,
    };

    if !options.dry_run {
        write_bundle(&output_path, &bundle)
            .map_err(|error| TeleportFailure::runtime("teleport/pack", error.to_string()))?;
        eprintln!("teleport: native bundle may contain secrets");
    }

    let artifact_summaries = artifacts
        .iter()
        .map(|artifact| PackArtifactSummary {
            path: artifact.path.clone(),
            sha256: artifact.sha256.clone(),
            required: artifact.required,
        })
        .collect();

    Ok(PackResponse {
        command: "teleport/pack",
        status: TeleportStatus::Ok,
        bundle_id,
        bundle_path: Some(output_path.display().to_string()),
        bytes: if options.dry_run {
            None
        } else {
            Some(
                bundle_bytes(&output_path).map_err(|error| {
                    TeleportFailure::runtime("teleport/pack", error.to_string())
                })?,
            )
        },
        sha256: if options.dry_run {
            None
        } else {
            Some(
                bundle_sha256(&output_path).map_err(|error| {
                    TeleportFailure::runtime("teleport/pack", error.to_string())
                })?,
            )
        },
        fidelity: TeleportFidelity::Native,
        session: PackSessionSummary {
            source: context.session.source,
            source_session_id: context.session.session_id,
            project_name: Some(context.session.project_name),
        },
        artifacts: artifact_summaries,
        scan,
        dry_run: options.dry_run,
    })
}

fn map_session_lookup_failure(error: anyhow::Error) -> TeleportFailure {
    let message = error.to_string();
    if message.contains("not found in scope")
        || message.contains("no sessions found")
        || message.contains("multiple projects matched alias")
        || message.contains("multiple sessions matched")
    {
        TeleportFailure::usage("teleport/pack", message)
    } else {
        TeleportFailure::runtime("teleport/pack", message)
    }
}

fn scan_native_transcript(content: &str) -> TeleportScanSummary {
    let detector = DeterministicPrivacyDetector;
    let outcome = scan_text_with_detector(content, &detector);
    TeleportScanSummary {
        blocking_findings: outcome
            .findings
            .iter()
            .filter(|finding| finding.blocks_sync)
            .count(),
        redacted_findings: outcome.findings.len(),
        pii_coverage: Some(format!("{:?}", outcome.pii_coverage.status).to_ascii_lowercase()),
    }
}

fn source_host_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string())
}
