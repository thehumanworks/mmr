use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::TeleportStatus;
use super::bundle::{
    NATIVE_TRANSCRIPT_PATH, TeleportBundleFile, apply_path_remap, cache_dir,
    codex_native_destination_path, load_bundle_from_locator, remap_text, timestamp_is_newer_than,
    transcript_latest_timestamp, verify_artifact_hashes, write_bundle,
};
use super::error::TeleportFailure;
use super::manifest::{TeleportFidelity, TeleportManifest};

const SUPPORTED_SOURCE: &str = "codex";
const RESUME_STATUS_VISIBLE_BUT_NOT_RESUMABLE: &str = "visible_but_not_resumable";

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub bundle_path: PathBuf,
    pub project: Option<String>,
    pub dry_run: bool,
    pub force: bool,
    pub skip_store_import: bool,
}

#[derive(Debug, Serialize)]
pub struct ApplyNativeSummary {
    pub written: bool,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ApplyStoreSummary {
    pub imported_events: u64,
    pub skipped_events: u64,
}

#[derive(Debug, Serialize)]
pub struct ApplyResumeSummary {
    pub provider: String,
    pub documented_command: String,
    pub agent_resume: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ApplyResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub bundle_id: String,
    pub target_project: String,
    pub native: ApplyNativeSummary,
    pub store: ApplyStoreSummary,
    pub resume: ApplyResumeSummary,
    pub path_remap_applied: bool,
    pub dry_run: bool,
}

pub fn apply_bundle(options: ApplyOptions) -> Result<ApplyResponse, TeleportFailure> {
    let bundle = load_bundle_from_locator(&options.bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
    verify_artifact_hashes(&bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;

    if bundle.manifest.source != SUPPORTED_SOURCE {
        return Err(TeleportFailure::runtime(
            "teleport/apply",
            format!(
                "teleport apply supports native Codex bundles only; bundle source is {}",
                bundle.manifest.source
            ),
        ));
    }

    if bundle.manifest.fidelity != TeleportFidelity::Native {
        return Err(TeleportFailure::runtime(
            "teleport/apply",
            "teleport apply supports native Codex bundles only",
        ));
    }

    let target_project = resolve_target_project(&bundle.manifest.project.canonical_path, &options);
    let replacements = apply_path_remap(&bundle, &target_project);
    let path_remap_applied = path_remap_was_applied(&bundle, &target_project, &replacements);
    let resume = build_resume_summary(&bundle.manifest);

    let native_transcript = bundle.files.get(NATIVE_TRANSCRIPT_PATH).ok_or_else(|| {
        TeleportFailure::runtime("teleport/apply", "native transcript missing from bundle")
    })?;
    let remapped_transcript = remap_codex_native_transcript(
        native_transcript,
        &replacements,
        &target_project,
        &bundle.manifest.project.canonical_path,
        &bundle.manifest.project.aliases,
    );

    let cache_root = cache_dir(&bundle.manifest.bundle_id)
        .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
    let cache_marker = cache_root.join("applied.json");
    let cache_bundle = cache_root.join("bundle.mmr");

    let native_paths = vec![native_destination_path(&bundle)?];

    if !native_paths.is_empty()
        && native_paths.iter().all(|path| {
            path.exists()
                && fs::read_to_string(path).ok().as_deref() == Some(remapped_transcript.as_str())
        })
        && !options.force
    {
        if !options.dry_run {
            fs::create_dir_all(&cache_root)
                .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
            write_bundle(&cache_bundle, &bundle)
                .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
            fs::write(&cache_marker, &bundle.manifest.bundle_id)
                .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
        }
        return Ok(skipped_apply_response(
            &bundle,
            &target_project,
            path_remap_applied,
            options.dry_run,
            resume,
        ));
    }

    if options.dry_run {
        return Ok(ApplyResponse {
            command: "teleport/apply",
            status: TeleportStatus::Ok,
            bundle_id: bundle.manifest.bundle_id.clone(),
            target_project,
            native: ApplyNativeSummary {
                written: !native_paths.is_empty(),
                paths: native_paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect(),
            },
            store: ApplyStoreSummary {
                imported_events: 0,
                skipped_events: 0,
            },
            resume,
            path_remap_applied,
            dry_run: true,
        });
    }

    fs::create_dir_all(&cache_root)
        .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
    write_bundle(&cache_bundle, &bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;

    let mut written = false;
    for path in &native_paths {
        if path.exists() {
            let existing = fs::read_to_string(path)
                .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
            if existing == remapped_transcript {
                continue;
            }
            reject_newer_existing_transcript(
                &existing,
                &bundle.manifest.session.last_timestamp,
                path,
                options.force,
            )?;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
        }
        fs::write(path, &remapped_transcript)
            .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;
        written = true;
    }

    fs::write(&cache_marker, &bundle.manifest.bundle_id)
        .map_err(|error| TeleportFailure::runtime("teleport/apply", error.to_string()))?;

    Ok(ApplyResponse {
        command: "teleport/apply",
        status: TeleportStatus::Ok,
        bundle_id: bundle.manifest.bundle_id.clone(),
        target_project,
        native: ApplyNativeSummary {
            written,
            paths: native_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        },
        store: ApplyStoreSummary {
            imported_events: 0,
            skipped_events: if options.skip_store_import {
                bundle.manifest.session.message_count as u64
            } else {
                0
            },
        },
        resume,
        path_remap_applied,
        dry_run: false,
    })
}

fn resolve_target_project(default: &str, options: &ApplyOptions) -> String {
    options
        .project
        .clone()
        .unwrap_or_else(|| default.to_string())
}

fn native_destination_path(bundle: &TeleportBundleFile) -> Result<PathBuf, TeleportFailure> {
    let home = dirs::home_dir().ok_or_else(|| {
        TeleportFailure::runtime("teleport/apply", "resolve HOME for native apply")
    })?;
    Ok(codex_native_destination_path(
        &home,
        &bundle.metadata.native_source_file,
        &bundle.manifest.session.source_session_id,
    ))
}

fn path_remap_was_applied(
    bundle: &TeleportBundleFile,
    target_project: &str,
    replacements: &BTreeMap<String, String>,
) -> bool {
    !replacements.is_empty() && target_project != bundle.manifest.project.canonical_path
}

fn remap_codex_native_transcript(
    content: &str,
    replacements: &BTreeMap<String, String>,
    target_project: &str,
    source_canonical: &str,
    aliases: &[String],
) -> String {
    let mut lines = Vec::new();
    for line in remap_text(content, replacements).lines() {
        if line.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            lines.push(line.to_string());
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("session_meta")
            && let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut)
            && let Some(cwd) = payload.get("cwd").and_then(Value::as_str)
            && (cwd == source_canonical || aliases.iter().any(|alias| alias == cwd))
        {
            payload.insert("cwd".to_string(), Value::String(target_project.to_string()));
        }
        lines.push(value.to_string());
    }

    let mut updated = lines.join("\n");
    if content.ends_with('\n') && !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

fn reject_newer_existing_transcript(
    existing: &str,
    bundle_last_timestamp: &str,
    path: &Path,
    force: bool,
) -> Result<(), TeleportFailure> {
    if force {
        return Ok(());
    }

    let existing_timestamp = transcript_latest_timestamp(existing);
    if let Some(existing_timestamp) = &existing_timestamp
        && timestamp_is_newer_than(existing_timestamp, bundle_last_timestamp)
    {
        return Err(TeleportFailure::runtime(
            "teleport/apply",
            format!(
                "native session file at {} is newer than bundle (existing {}, bundle {}); pass --force to replace",
                path.display(),
                existing_timestamp,
                bundle_last_timestamp
            ),
        ));
    }

    if existing_timestamp.is_none() {
        return Err(TeleportFailure::runtime(
            "teleport/apply",
            format!(
                "native session file already exists at {}; pass --force to replace",
                path.display()
            ),
        ));
    }

    Ok(())
}

fn build_resume_summary(manifest: &TeleportManifest) -> ApplyResumeSummary {
    let status = if manifest.restore.agent_resume == "verified" {
        "resumable".to_string()
    } else {
        RESUME_STATUS_VISIBLE_BUT_NOT_RESUMABLE.to_string()
    };
    ApplyResumeSummary {
        provider: manifest.source.clone(),
        documented_command: manifest.restore.documented_command.clone(),
        agent_resume: manifest.restore.agent_resume.clone(),
        status,
    }
}

fn skipped_apply_response(
    bundle: &TeleportBundleFile,
    target_project: &str,
    path_remap_applied: bool,
    dry_run: bool,
    resume: ApplyResumeSummary,
) -> ApplyResponse {
    ApplyResponse {
        command: "teleport/apply",
        status: TeleportStatus::Skipped,
        bundle_id: bundle.manifest.bundle_id.clone(),
        target_project: target_project.to_string(),
        native: ApplyNativeSummary {
            written: false,
            paths: Vec::new(),
        },
        store: ApplyStoreSummary {
            imported_events: 0,
            skipped_events: 0,
        },
        resume,
        path_remap_applied,
        dry_run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn remap_codex_native_transcript_rewrites_session_meta_cwd() {
        let content = r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess-codex-1","cwd":"/Users/test/codex-proj","timestamp":"2025-01-02T00:00:00"}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:01","payload":{"type":"user_message","message":"hello"}}"#;
        let remapped = remap_codex_native_transcript(
            content,
            &BTreeMap::from([(
                "/Users/test/codex-proj".to_string(),
                "/Users/test/target-proj".to_string(),
            )]),
            "/Users/test/target-proj",
            "/Users/test/codex-proj",
            &["-Users-test-codex-proj".to_string()],
        );
        assert!(remapped.contains(r#""cwd":"/Users/test/target-proj""#));
        assert!(!remapped.contains(r#""cwd":"/Users/test/codex-proj""#));
    }

    #[test]
    fn build_resume_summary_reports_visible_but_not_resumable_for_best_effort() {
        let summary = build_resume_summary(&TeleportManifest {
            mmr_teleport_manifest_version: 1,
            bundle_id: "tp:v1:test".to_string(),
            created_at: String::new(),
            source_host: String::new(),
            mmr_version: String::new(),
            min_mmr_version: String::new(),
            source: "codex".to_string(),
            parser_version: String::new(),
            fidelity: TeleportFidelity::Native,
            session: super::super::manifest::ManifestSession {
                source_session_id: "sess-codex-1".to_string(),
                message_count: 1,
                first_timestamp: String::new(),
                last_timestamp: String::new(),
                partial_tail: false,
            },
            project: super::super::manifest::ManifestProject {
                canonical_path: String::new(),
                aliases: Vec::new(),
                path_remap: BTreeMap::new(),
            },
            artifacts: Vec::new(),
            capabilities: Vec::new(),
            restore: super::super::manifest::ManifestRestore {
                agent_resume: "best_effort".to_string(),
                documented_command: "codex exec resume sess-codex-1".to_string(),
                adapters: Vec::new(),
            },
        });
        assert_eq!(summary.status, RESUME_STATUS_VISIBLE_BUT_NOT_RESUMABLE);
        assert_eq!(summary.documented_command, "codex exec resume sess-codex-1");
    }
}
