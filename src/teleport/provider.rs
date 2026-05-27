use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use std::fmt::Debug;
use std::fs;

use super::bundle::{TeleportBundleFile, hash_text};
use super::error::TeleportFailure;
use super::manifest::ManifestRestore;

#[path = "providers/mod.rs"]
pub mod profiles;

#[derive(Debug, Clone)]
pub struct NativeArtifactSpec {
    pub bundle_path: &'static str,
    pub kind: &'static str,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct PackedNativeFile {
    pub bundle_path: String,
    pub content: String,
    pub kind: String,
    pub required: bool,
}

pub trait TeleportProviderProfile: Debug + Send + Sync {
    fn source_name(&self) -> &'static str;
    fn parser_version(&self) -> &'static str;
    fn capabilities(&self) -> Vec<String>;
    fn native_artifacts(&self) -> Vec<NativeArtifactSpec>;
    fn normalized_transcript_path(&self) -> &'static str;
    fn restore_hints_path(&self) -> &'static str;
    fn primary_native_path(&self) -> &'static str;

    fn build_restore_hints(&self, session_id: &str) -> Value;
    fn documented_resume_command(&self, session_id: &str) -> String;
    fn agent_resume_level(&self) -> &'static str;
    fn restore_adapters(&self) -> Vec<String>;

    fn supports_native_apply(&self) -> bool;
    fn supports_resume_transform(&self, bundle_source: &str, target: &str) -> bool;

    fn native_destination_path(
        &self,
        home: &Path,
        native_source_file: &str,
        session_id: &str,
        target_project: &str,
        source_canonical: &str,
    ) -> PathBuf;

    fn remap_native_content(
        &self,
        bundle_path: &str,
        content: &str,
        replacements: &BTreeMap<String, String>,
        target_project: &str,
        source_canonical: &str,
        aliases: &[String],
    ) -> String;

    fn conflict_check_path(&self, bundle: &TeleportBundleFile) -> Option<String>;
}

pub fn profile_for(source: &str) -> Result<&'static dyn TeleportProviderProfile, TeleportFailure> {
    match source {
        "codex" => Ok(&profiles::CODEX_PROFILE),
        "claude" => Ok(&profiles::CLAUDE_PROFILE),
        "cursor" => Ok(&profiles::CURSOR_PROFILE),
        "grok" => Ok(&profiles::GROK_PROFILE),
        "pi" => Ok(&profiles::PI_PROFILE),
        other => Err(TeleportFailure::runtime(
            "teleport/provider",
            format!(
                "unsupported teleport provider {other:?}; supported: codex, claude, cursor, grok, pi"
            ),
        )),
    }
}

pub fn supported_sources() -> &'static [&'static str] {
    &["codex", "claude", "cursor", "grok", "pi"]
}

pub fn resolve_bundle_native_path(
    profile: &dyn TeleportProviderProfile,
    bundle: &TeleportBundleFile,
    bundle_path: &str,
) -> Option<String> {
    if bundle.files.contains_key(bundle_path) {
        return bundle.files.get(bundle_path).cloned();
    }
    for legacy in legacy_paths_for(bundle_path) {
        if let Some(content) = bundle.files.get(legacy) {
            return Some(content.clone());
        }
    }
    let _ = profile;
    None
}

pub fn legacy_paths_for(bundle_path: &str) -> Vec<&'static str> {
    match bundle_path {
        "native/codex/transcript.jsonl" => vec!["transcript.native.jsonl"],
        "native/claude/transcript.jsonl" => vec!["transcript.native.jsonl"],
        "native/cursor/transcript.jsonl" => vec!["transcript.native.jsonl"],
        "native/grok/updates.jsonl" => vec!["transcript.native.jsonl"],
        "native/pi/transcript.jsonl" => vec!["transcript.native.jsonl"],
        _ => Vec::new(),
    }
}

pub fn build_manifest_restore(
    profile: &dyn TeleportProviderProfile,
    session_id: &str,
) -> ManifestRestore {
    ManifestRestore {
        agent_resume: profile.agent_resume_level().to_string(),
        documented_command: profile.documented_resume_command(session_id),
        adapters: profile.restore_adapters(),
    }
}

pub fn artifact_entries_for_pack(
    metadata_hash: &str,
    packed_files: &[PackedNativeFile],
    normalized_hash: &str,
    restore_hash: Option<&str>,
    restore_path: &str,
) -> Vec<(String, String, String, bool)> {
    let mut entries = vec![(
        super::bundle::METADATA_PATH.to_string(),
        metadata_hash.to_string(),
        "metadata".to_string(),
        true,
    )];
    for file in packed_files {
        entries.push((
            file.bundle_path.clone(),
            hash_text(&file.content),
            file.kind.clone(),
            file.required,
        ));
    }
    entries.push((
        super::bundle::NORMALIZED_TRANSCRIPT_PATH.to_string(),
        normalized_hash.to_string(),
        "normalized_transcript".to_string(),
        false,
    ));
    if let Some(restore_hash) = restore_hash {
        entries.push((
            restore_path.to_string(),
            restore_hash.to_string(),
            "restore_hints".to_string(),
            false,
        ));
    }
    entries
}

pub fn collect_native_files_for_pack(
    profile: &dyn TeleportProviderProfile,
    source_file: &Path,
) -> Result<Vec<PackedNativeFile>, TeleportFailure> {
    let mut files = Vec::new();
    for spec in profile.native_artifacts() {
        let path = native_file_path_for_pack(profile.source_name(), source_file, spec.bundle_path)?;
        let content = fs::read_to_string(&path).map_err(|error| {
            TeleportFailure::runtime(
                "teleport/pack",
                format!("read native artifact {}: {error}", path.display()),
            )
        })?;
        files.push(PackedNativeFile {
            bundle_path: spec.bundle_path.to_string(),
            content,
            kind: spec.kind.to_string(),
            required: spec.required,
        });
    }
    Ok(files)
}

fn native_file_path_for_pack(
    source: &str,
    source_file: &Path,
    bundle_path: &str,
) -> Result<PathBuf, TeleportFailure> {
    if source == "grok" {
        let session_dir = source_file.parent().ok_or_else(|| {
            TeleportFailure::runtime("teleport/pack", "grok session directory missing")
        })?;
        return Ok(match bundle_path {
            "native/grok/summary.json" => session_dir.join("summary.json"),
            "native/grok/updates.jsonl" => session_dir.join("updates.jsonl"),
            _ => source_file.to_path_buf(),
        });
    }
    Ok(source_file.to_path_buf())
}

pub fn native_write_targets(
    profile: &dyn TeleportProviderProfile,
    bundle: &TeleportBundleFile,
    home: &Path,
    target_project: &str,
) -> Result<Vec<(String, PathBuf)>, TeleportFailure> {
    let session_id = bundle.manifest.session.source_session_id.as_str();
    let native_source = bundle.metadata.native_source_file.as_str();
    let source_canonical = bundle.manifest.project.canonical_path.as_str();
    let mut targets = Vec::new();
    for spec in profile.native_artifacts() {
        let bundle_path = spec.bundle_path.to_string();
        let destination = match profile.source_name() {
            "grok" => grok_destination_for_artifact(
                native_source,
                home,
                &bundle_path,
                session_id,
                target_project,
                source_canonical,
            ),
            _ => profile.native_destination_path(
                home,
                native_source,
                session_id,
                target_project,
                source_canonical,
            ),
        };
        targets.push((bundle_path, destination));
    }
    Ok(targets)
}

fn grok_destination_for_artifact(
    native_source_file: &str,
    home: &Path,
    bundle_path: &str,
    session_id: &str,
    target_project: &str,
    source_canonical: &str,
) -> PathBuf {
    let updates = profiles::GROK_PROFILE.native_destination_path(
        home,
        native_source_file,
        session_id,
        target_project,
        source_canonical,
    );
    let session_dir = updates.parent().unwrap_or(&updates).to_path_buf();
    match bundle_path {
        "native/grok/summary.json" => session_dir.join("summary.json"),
        "native/grok/updates.jsonl" => session_dir.join("updates.jsonl"),
        _ => updates,
    }
}

pub(crate) fn slash_hyphen_project_dir(project: &str) -> String {
    if project.starts_with('-') && !project.contains('/') {
        return project.to_string();
    }
    let trimmed = project.trim().trim_start_matches('/').trim_end_matches('/');
    if trimmed.is_empty() {
        "-".to_string()
    } else {
        format!("-{}", trimmed.replace('/', "-"))
    }
}

pub(crate) fn pi_project_dir(project: &str) -> String {
    let trimmed = project.trim();
    if trimmed.starts_with("--") && trimmed.ends_with("--") {
        return trimmed.to_string();
    }
    let path = trimmed.trim_start_matches('/').trim_end_matches('/');
    if path.is_empty() {
        "----".to_string()
    } else {
        format!("--{}--", path.replace('/', "-"))
    }
}

pub(crate) fn percent_encoded_project_dir(project: &str) -> String {
    let trimmed = project.trim();
    if !trimmed.contains('/') && trimmed.contains('%') {
        return trimmed.to_string();
    }

    let mut encoded = String::new();
    for byte in trimmed.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn relative_after_marker<'a>(
    native_source_file: &'a str,
    markers: &[&str],
) -> Option<&'a str> {
    markers.iter().find_map(|marker| {
        native_source_file
            .find(marker)
            .map(|index| &native_source_file[index + marker.len()..])
    })
}

pub(crate) fn replace_first_relative_component(
    root: PathBuf,
    relative: &str,
    first_component: &str,
) -> PathBuf {
    let mut out = root.join(first_component);
    for component in relative
        .split('/')
        .filter(|component| !component.is_empty())
        .skip(1)
    {
        out = out.join(component);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_for_codex_returns_profile() {
        let profile = profile_for("codex").expect("codex profile");
        assert_eq!(profile.source_name(), "codex");
        assert!(profile.supports_native_apply());
    }

    #[test]
    fn profile_for_unknown_returns_structured_error() {
        match profile_for("not-a-provider") {
            Err(err) => assert!(err.message.contains("unsupported teleport provider")),
            Ok(_) => panic!("expected unknown provider to fail"),
        }
    }
}
