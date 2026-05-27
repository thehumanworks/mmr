use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::manifest::{BundleMetadata, TeleportManifest};
use crate::types::api::ApiMessage;

pub const BUNDLE_FORMAT_VERSION: u32 = 1;
pub const METADATA_PATH: &str = "metadata.json";
pub const NORMALIZED_TRANSCRIPT_PATH: &str = "transcript.normalized.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeleportBundleFile {
    pub mmr_teleport_bundle_version: u32,
    pub manifest: TeleportManifest,
    pub metadata: BundleMetadata,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
}

#[derive(Debug)]
pub enum BundleLocatorError {
    MultipleLocators { subcommand: String },
    MissingLocator { subcommand: String },
}

impl std::fmt::Display for BundleLocatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MultipleLocators { subcommand } => write!(
                f,
                "teleport {subcommand}: only one bundle locator is allowed; use either a positional path or --to, not both"
            ),
            Self::MissingLocator { subcommand } => write!(
                f,
                "teleport {subcommand}: bundle path is required; pass a positional path or --to"
            ),
        }
    }
}

impl std::error::Error for BundleLocatorError {}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hash_hex(bytes))
}

pub fn hash_text(text: &str) -> String {
    hash_bytes(text.as_bytes())
}

pub fn compute_bundle_id(artifact_hashes: &[(String, String)]) -> String {
    let mut pairs = artifact_hashes.to_vec();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let identity = pairs
        .iter()
        .map(|(path, hash)| format!("{path}:{hash}"))
        .collect::<Vec<_>>()
        .join("|");
    format!("tp:v1:{}", hash_hex(identity.as_bytes()))
}

pub fn manifest_content_identity(manifest: &TeleportManifest) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for artifact in &manifest.artifacts {
        if artifact.path == METADATA_PATH {
            continue;
        }
        entries.push((artifact.path.clone(), artifact.sha256.clone()));
    }
    entries
}

pub fn default_bundle_dir(bundle_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("resolve HOME for teleport bundle output")?;
    Ok(home
        .join(".mmr")
        .join("teleport")
        .join("bundles")
        .join(bundle_id))
}

pub fn default_bundle_path(bundle_id: &str) -> Result<PathBuf> {
    Ok(default_bundle_dir(bundle_id)?.join("bundle.mmr"))
}

pub fn cache_dir(bundle_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("resolve HOME for teleport cache")?;
    Ok(home
        .join(".mmr")
        .join("teleport")
        .join("cache")
        .join(bundle_id))
}

pub fn write_bundle(path: &Path, bundle: &TeleportBundleFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create bundle parent directory {}", parent.display()))?;
    }
    let json = serde_json::to_string(bundle).context("serialize teleport bundle")?;
    fs::write(path, json).with_context(|| format!("write bundle {}", path.display()))?;
    Ok(())
}

pub fn load_bundle_from_locator(locator: &Path) -> Result<TeleportBundleFile> {
    if locator.as_os_str() == "-" {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("read teleport bundle from stdin")?;
        parse_bundle_json(&buffer)
    } else {
        load_bundle(locator)
    }
}

pub fn load_bundle(path: &Path) -> Result<TeleportBundleFile> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read teleport bundle {}", path.display()))?;
    parse_bundle_json(&content)
}

fn parse_bundle_json(content: &str) -> Result<TeleportBundleFile> {
    let bundle: TeleportBundleFile =
        serde_json::from_str(content).context("parse teleport bundle JSON")?;
    if bundle.mmr_teleport_bundle_version != BUNDLE_FORMAT_VERSION {
        bail!(
            "unsupported teleport bundle version {}; expected {}",
            bundle.mmr_teleport_bundle_version,
            BUNDLE_FORMAT_VERSION
        );
    }
    Ok(bundle)
}

pub fn artifact_content<'a>(bundle: &'a TeleportBundleFile, path: &str) -> Result<Option<&'a str>> {
    if path == METADATA_PATH {
        return Ok(None);
    }
    if let Some(content) = bundle.files.get(path) {
        return Ok(Some(content.as_str()));
    }
    Ok(legacy_artifact_paths(path)
        .iter()
        .find_map(|legacy| bundle.files.get(*legacy).map(String::as_str)))
}

pub fn normalized_transcript_content(bundle: &TeleportBundleFile) -> Option<&str> {
    artifact_content(bundle, NORMALIZED_TRANSCRIPT_PATH)
        .ok()
        .flatten()
        .or_else(|| {
            bundle
                .manifest
                .artifacts
                .iter()
                .find(|artifact| artifact.kind == "normalized_transcript")
                .and_then(|artifact| artifact_content(bundle, &artifact.path).ok().flatten())
        })
}

pub fn messages_from_bundle(bundle: &TeleportBundleFile) -> Result<Vec<ApiMessage>, String> {
    let content = normalized_transcript_content(bundle)
        .ok_or_else(|| "bundle missing normalized transcript".to_string())?;
    parse_normalized_messages(content)
}

fn parse_normalized_messages(content: &str) -> Result<Vec<ApiMessage>, String> {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|error| format!("parse normalized transcript line as ApiMessage: {error}"))
        })
        .collect()
}

pub fn metadata_json(bundle: &TeleportBundleFile) -> Result<String> {
    serde_json::to_string(&bundle.metadata).context("serialize bundle metadata")
}

pub fn verify_artifact_hashes(bundle: &TeleportBundleFile) -> Result<Vec<String>> {
    let metadata_hash = hash_text(&metadata_json(bundle)?);
    let warnings = Vec::new();

    for artifact in &bundle.manifest.artifacts {
        let actual = if artifact.path == METADATA_PATH {
            metadata_hash.clone()
        } else if let Some(content) = artifact_content(bundle, &artifact.path)? {
            hash_text(content)
        } else if artifact.required {
            bail!("missing required bundle artifact {}", artifact.path);
        } else {
            continue;
        };

        if actual != artifact.sha256 {
            bail!(
                "artifact hash mismatch for {}: expected {}, got {}",
                artifact.path,
                artifact.sha256,
                actual
            );
        }
    }

    if bundle.manifest.bundle_id != compute_bundle_id(&manifest_content_identity(&bundle.manifest))
    {
        bail!("manifest bundle_id does not match content-addressed artifact identity");
    }

    Ok(warnings)
}

pub fn bundle_bytes(path: &Path) -> Result<u64> {
    Ok(fs::metadata(path)
        .with_context(|| format!("stat bundle {}", path.display()))?
        .len())
}

pub fn bundle_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read bundle {}", path.display()))?;
    Ok(hash_bytes(&bytes))
}

pub fn remap_text(content: &str, replacements: &BTreeMap<String, String>) -> String {
    let mut updated = content.to_string();
    for (from, to) in replacements {
        updated = updated.replace(from, to);
    }
    updated
}

pub fn transcript_latest_timestamp(content: &str) -> Option<String> {
    let mut latest: Option<String> = None;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(timestamp) = value.get("timestamp").and_then(|entry| entry.as_str()) {
            latest = Some(max_timestamp(latest.as_deref(), timestamp).to_string());
        }
        if value.get("type").and_then(|entry| entry.as_str()) == Some("session_meta")
            && let Some(timestamp) = value
                .pointer("/payload/timestamp")
                .and_then(|entry| entry.as_str())
        {
            latest = Some(max_timestamp(latest.as_deref(), timestamp).to_string());
        }
    }
    latest
}

pub fn timestamp_is_newer_than(candidate: &str, baseline: &str) -> bool {
    max_timestamp(Some(baseline), candidate) == candidate && candidate != baseline
}

fn max_timestamp<'a>(left: Option<&'a str>, right: &'a str) -> &'a str {
    match left {
        Some(left) if left > right => left,
        _ => right,
    }
}

pub fn apply_path_remap(
    bundle: &TeleportBundleFile,
    target_project: &str,
) -> BTreeMap<String, String> {
    let mut replacements = BTreeMap::new();
    for (from, to) in &bundle.manifest.project.path_remap {
        let mapped = if to == "${TARGET_PROJECT}" {
            target_project.to_string()
        } else {
            to.clone()
        };
        replacements.insert(from.clone(), mapped);
    }
    replacements
}

fn hash_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn legacy_artifact_paths(path: &str) -> &'static [&'static str] {
    match path {
        "native/codex/transcript.jsonl"
        | "native/claude/transcript.jsonl"
        | "native/cursor/transcript.jsonl"
        | "native/grok/updates.jsonl"
        | "native/pi/transcript.jsonl" => &["transcript.native.jsonl"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teleport::manifest::{
        BundleMetadata, ManifestArtifact, ManifestProject, ManifestRestore, ManifestSession,
        TeleportFidelity, TeleportManifest,
    };
    use crate::teleport::provider::profiles::codex::{
        NATIVE_TRANSCRIPT as CODEX_NATIVE_TRANSCRIPT_PATH, RESTORE_PATH as RESTORE_CODEX_PATH,
        codex_native_destination_path,
    };

    fn sample_manifest(artifact_hash: &str) -> TeleportManifest {
        TeleportManifest {
            mmr_teleport_manifest_version: 1,
            bundle_id: String::new(),
            created_at: "2026-05-26T12:00:00Z".to_string(),
            source_host: "test-host".to_string(),
            mmr_version: "0.1.0".to_string(),
            min_mmr_version: "0.1.0".to_string(),
            source: "codex".to_string(),
            parser_version: "codex-rollout-v1".to_string(),
            fidelity: TeleportFidelity::Native,
            session: ManifestSession {
                source_session_id: "sess-codex-1".to_string(),
                message_count: 2,
                first_timestamp: "2025-01-02T00:00:01".to_string(),
                last_timestamp: "2025-01-02T00:05:00".to_string(),
                partial_tail: false,
            },
            project: ManifestProject {
                canonical_path: "/Users/test/codex-proj".to_string(),
                aliases: vec!["-Users-test-codex-proj".to_string()],
                path_remap: BTreeMap::from([(
                    "/Users/test/codex-proj".to_string(),
                    "${TARGET_PROJECT}".to_string(),
                )]),
            },
            artifacts: vec![
                ManifestArtifact {
                    path: METADATA_PATH.to_string(),
                    required: true,
                    sha256: String::new(),
                    kind: "metadata".to_string(),
                },
                ManifestArtifact {
                    path: CODEX_NATIVE_TRANSCRIPT_PATH.to_string(),
                    required: true,
                    sha256: artifact_hash.to_string(),
                    kind: "native_transcript".to_string(),
                },
            ],
            capabilities: vec!["codex-native-apply".to_string(), "store-import".to_string()],
            restore: ManifestRestore {
                agent_resume: "best_effort".to_string(),
                documented_command: "codex exec resume sess-codex-1".to_string(),
                adapters: vec!["codex-native-apply".to_string()],
            },
        }
    }

    #[test]
    fn teleport_bundle_id_is_stable_for_same_artifacts() {
        let left = compute_bundle_id(&[
            ("a".to_string(), "sha256:1".to_string()),
            ("b".to_string(), "sha256:2".to_string()),
        ]);
        let right = compute_bundle_id(&[
            ("b".to_string(), "sha256:2".to_string()),
            ("a".to_string(), "sha256:1".to_string()),
        ]);
        assert_eq!(left, right);
    }

    #[test]
    fn teleport_bundle_verify_hashes_accepts_matching_bundle() {
        let native = "native transcript\n";
        let normalized = r#"{"session_id":"sess-codex-1"}"#;
        let restore = r#"{"session_file":"sess-codex-1.jsonl"}"#;
        let native_hash = hash_text(native);
        let normalized_hash = hash_text(normalized);
        let restore_hash = hash_text(restore);
        let metadata = BundleMetadata {
            source: "codex".to_string(),
            source_session_id: "sess-codex-1".to_string(),
            project_name: "/Users/test/codex-proj".to_string(),
            project_path: "/Users/test/codex-proj".to_string(),
            native_source_file: "/tmp/sess-codex-1.jsonl".to_string(),
            packed_at: "2026-05-26T12:00:00Z".to_string(),
            notes: None,
        };
        let metadata_hash = hash_text(&serde_json::to_string(&metadata).expect("metadata json"));
        let mut manifest = sample_manifest(&native_hash);
        manifest.artifacts[0].sha256 = metadata_hash;
        manifest.artifacts.push(ManifestArtifact {
            path: NORMALIZED_TRANSCRIPT_PATH.to_string(),
            required: false,
            sha256: normalized_hash.clone(),
            kind: "normalized_transcript".to_string(),
        });
        manifest.artifacts.push(ManifestArtifact {
            path: RESTORE_CODEX_PATH.to_string(),
            required: false,
            sha256: restore_hash.clone(),
            kind: "restore_hints".to_string(),
        });
        manifest.bundle_id = compute_bundle_id(&[
            (
                CODEX_NATIVE_TRANSCRIPT_PATH.to_string(),
                native_hash.clone(),
            ),
            (
                NORMALIZED_TRANSCRIPT_PATH.to_string(),
                normalized_hash.clone(),
            ),
            (RESTORE_CODEX_PATH.to_string(), restore_hash.clone()),
        ]);
        let files = BTreeMap::from([
            (CODEX_NATIVE_TRANSCRIPT_PATH.to_string(), native.to_string()),
            (
                NORMALIZED_TRANSCRIPT_PATH.to_string(),
                normalized.to_string(),
            ),
            (RESTORE_CODEX_PATH.to_string(), restore.to_string()),
        ]);
        let bundle = TeleportBundleFile {
            mmr_teleport_bundle_version: BUNDLE_FORMAT_VERSION,
            manifest,
            metadata,
            files,
        };
        verify_artifact_hashes(&bundle).expect("hashes should match");
    }

    #[test]
    fn teleport_bundle_id_ignores_metadata_timestamp() {
        let native_hash = hash_text("native");
        let normalized_hash = hash_text("normalized");
        let restore_hash = hash_text("restore");
        let entries = [
            (
                CODEX_NATIVE_TRANSCRIPT_PATH.to_string(),
                native_hash.clone(),
            ),
            (
                NORMALIZED_TRANSCRIPT_PATH.to_string(),
                normalized_hash.clone(),
            ),
            (RESTORE_CODEX_PATH.to_string(), restore_hash.clone()),
        ];
        let first = compute_bundle_id(&entries);
        let second = compute_bundle_id(&entries);
        assert_eq!(first, second);
    }

    #[test]
    fn codex_native_destination_path_preserves_relative_layout() {
        let home = PathBuf::from("/Users/test");
        let path = codex_native_destination_path(
            &home,
            "/Users/alice/.codex/sessions/2025/01/sess-abc.jsonl",
            "sess-abc",
        );
        assert_eq!(
            path,
            home.join(".codex")
                .join("sessions")
                .join("2025")
                .join("01")
                .join("sess-abc.jsonl")
        );
    }

    #[test]
    fn codex_native_destination_path_falls_back_to_session_id() {
        let home = PathBuf::from("/Users/test");
        let path = codex_native_destination_path(&home, "/tmp/rollout.jsonl", "sess-codex-1");
        assert_eq!(
            path,
            home.join(".codex")
                .join("sessions")
                .join("sess-codex-1.jsonl")
        );
    }

    #[test]
    fn transcript_latest_timestamp_prefers_latest_line() {
        let content = r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess","cwd":"/tmp","timestamp":"2025-01-02T00:00:00"}}
{"type":"event_msg","timestamp":"2025-01-02T00:05:00","payload":{"type":"user_message","message":"hi"}}"#;
        assert_eq!(
            transcript_latest_timestamp(content),
            Some("2025-01-02T00:05:00".to_string())
        );
    }

    #[test]
    fn timestamp_is_newer_than_compares_lexicographically() {
        assert!(timestamp_is_newer_than(
            "2025-01-03T00:00:00",
            "2025-01-02T00:05:00"
        ));
        assert!(!timestamp_is_newer_than(
            "2025-01-02T00:05:00",
            "2025-01-02T00:05:00"
        ));
    }

    #[test]
    fn messages_from_bundle_parses_normalized_transcript_lines() {
        let normalized = r#"{"session_id":"sess-codex-1","source":"codex","project_name":"/Users/test/codex-proj","role":"user","content":"hello","model":"","timestamp":"2025-01-01T00:00:00Z","is_subagent":false,"msg_type":"message","input_tokens":0,"output_tokens":0}
{"session_id":"sess-codex-1","source":"codex","project_name":"/Users/test/codex-proj","role":"assistant","content":"hi","model":"gpt","timestamp":"2025-01-01T00:00:01Z","is_subagent":false,"msg_type":"message","input_tokens":1,"output_tokens":2}"#;
        let mut files = BTreeMap::new();
        files.insert(
            NORMALIZED_TRANSCRIPT_PATH.to_string(),
            normalized.to_string(),
        );
        let bundle = TeleportBundleFile {
            mmr_teleport_bundle_version: BUNDLE_FORMAT_VERSION,
            manifest: sample_manifest("sha256:unused"),
            metadata: BundleMetadata {
                source: "codex".to_string(),
                source_session_id: "sess-codex-1".to_string(),
                project_name: "/Users/test/codex-proj".to_string(),
                project_path: "/Users/test/codex-proj".to_string(),
                native_source_file: "/Users/test/.codex/sessions/sess-codex-1.jsonl".to_string(),
                packed_at: "2025-01-01T00:00:00Z".to_string(),
                notes: None,
            },
            files,
        };
        let messages = messages_from_bundle(&bundle).expect("messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }
}
