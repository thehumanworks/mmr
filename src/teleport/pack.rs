use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::bundle::{
    TeleportBundleFile, bundle_bytes, bundle_sha256, default_bundle_path, hash_text, write_bundle,
};
use super::error::TeleportFailure;
use super::manifest::{
    BundleMetadata, ManifestArtifact, ManifestProject, ManifestSession, TeleportFidelity,
    TeleportManifest, path_remap_for_project, project_aliases,
};
use super::provider::{
    artifact_entries_for_pack, build_manifest_restore, collect_native_files_for_pack, profile_for,
};
use super::{TeleportScanSummary, TeleportStatus};
use crate::messages::service::{MessageQueryOptions, QueryService};
use crate::redaction::{DeterministicPrivacyDetector, scan_text_with_detector};
use crate::types::{SortBy, SortOptions, SortOrder, SourceFilter};

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
            "teleport pack supports native bundles only; --as shared-safe is not supported",
        ));
    }

    let lookup_source_filter = if options.session_id.is_some() {
        options.source_filter
    } else {
        options.source_filter.or(Some(SourceFilter::Codex))
    };

    let context = service
        .resolve_teleport_session(
            options.session_id.as_deref(),
            options.project.as_deref(),
            lookup_source_filter,
        )
        .map_err(map_session_lookup_failure)?;

    let profile = profile_for(&context.session.source)?;
    let packed_native = collect_native_files_for_pack(profile, &context.source_file)?;

    let scan_content = packed_native
        .iter()
        .map(|file| file.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let scan = scan_native_transcript(&scan_content);

    let source_filter = source_filter_for(&context.session.source)?;

    let messages = service
        .messages(
            std::slice::from_ref(&context.session.session_id),
            Some(&context.session.project_name),
            Some(source_filter),
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
    let restore_json = profile
        .build_restore_hints(&context.session.session_id)
        .to_string();

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
        notes: Some(format!(
            "native {} bundle (provider profile)",
            profile.source_name()
        )),
    };
    let metadata_json = serde_json::to_string(&metadata).map_err(|error| {
        TeleportFailure::runtime("teleport/pack", format!("serialize metadata: {error}"))
    })?;
    let metadata_hash = hash_text(&metadata_json);
    let normalized_hash = hash_text(&normalized_transcript);
    let restore_hash = hash_text(&restore_json);

    let bundle_id = compute_content_bundle_id_for_profile(
        profile,
        &packed_native,
        &normalized_hash,
        Some(&restore_hash),
    );

    let artifact_entries = artifact_entries_for_pack(
        &metadata_hash,
        &packed_native,
        &normalized_hash,
        Some(&restore_hash),
        profile.restore_hints_path(),
    );
    let artifacts: Vec<ManifestArtifact> = artifact_entries
        .into_iter()
        .map(|(path, sha256, kind, required)| ManifestArtifact {
            path,
            required,
            sha256,
            kind,
        })
        .collect();

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
        source: profile.source_name().to_string(),
        parser_version: profile.parser_version().to_string(),
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
        capabilities: profile.capabilities(),
        restore: build_manifest_restore(profile, &context.session.session_id),
    };

    let mut files = BTreeMap::new();
    for packed in &packed_native {
        files.insert(packed.bundle_path.clone(), packed.content.clone());
    }
    files.insert(
        profile.normalized_transcript_path().to_string(),
        normalized_transcript,
    );
    files.insert(profile.restore_hints_path().to_string(), restore_json);

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

fn compute_content_bundle_id_for_profile(
    profile: &dyn super::provider::TeleportProviderProfile,
    packed_native: &[super::provider::PackedNativeFile],
    normalized_hash: &str,
    restore_hash: Option<&str>,
) -> String {
    let mut entries: Vec<(String, String)> = packed_native
        .iter()
        .map(|file| (file.bundle_path.clone(), hash_text(&file.content)))
        .collect();
    entries.push((
        profile.normalized_transcript_path().to_string(),
        normalized_hash.to_string(),
    ));
    if let Some(restore_hash) = restore_hash {
        entries.push((
            profile.restore_hints_path().to_string(),
            restore_hash.to_string(),
        ));
    }
    super::bundle::compute_bundle_id(&entries)
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

fn source_filter_for(source: &str) -> Result<SourceFilter, TeleportFailure> {
    Ok(match source {
        "codex" => SourceFilter::Codex,
        "claude" => SourceFilter::Claude,
        "cursor" => SourceFilter::Cursor,
        "grok" => SourceFilter::Grok,
        "pi" => SourceFilter::Pi,
        other => {
            return Err(TeleportFailure::runtime(
                "teleport/pack",
                format!("unknown source filter for {other}"),
            ));
        }
    })
}

fn source_host_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string())
}
