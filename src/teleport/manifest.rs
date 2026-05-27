use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TeleportFidelity {
    Native,
    #[serde(rename = "shared-safe")]
    SharedSafe,
}

impl TeleportFidelity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::SharedSafe => "shared-safe",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeleportManifest {
    pub mmr_teleport_manifest_version: u32,
    pub bundle_id: String,
    pub created_at: String,
    pub source_host: String,
    pub mmr_version: String,
    pub min_mmr_version: String,
    pub source: String,
    pub parser_version: String,
    pub fidelity: TeleportFidelity,
    pub session: ManifestSession,
    pub project: ManifestProject,
    pub artifacts: Vec<ManifestArtifact>,
    pub capabilities: Vec<String>,
    pub restore: ManifestRestore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSession {
    pub source_session_id: String,
    pub message_count: u32,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub partial_tail: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestProject {
    pub canonical_path: String,
    pub aliases: Vec<String>,
    pub path_remap: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestArtifact {
    pub path: String,
    pub required: bool,
    pub sha256: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestRestore {
    pub agent_resume: String,
    pub documented_command: String,
    pub adapters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleMetadata {
    pub source: String,
    pub source_session_id: String,
    pub project_name: String,
    pub project_path: String,
    pub native_source_file: String,
    pub packed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

pub fn project_aliases(canonical_path: &str) -> Vec<String> {
    let alias = if canonical_path == "/" {
        "-".to_string()
    } else {
        format!(
            "-{}",
            canonical_path.trim_start_matches('/').replace('/', "-")
        )
    };
    vec![alias]
}

pub fn path_remap_for_project(canonical_path: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    map.insert(canonical_path.to_string(), "${TARGET_PROJECT}".to_string());
    map
}

pub fn restore_hints_for_provider(
    provider: &str,
    session_id: &str,
    command_prefix: &str,
    agent_resume: &str,
    artifact_paths: Vec<&str>,
) -> Value {
    serde_json::json!({
        "provider": provider,
        "session_id": session_id,
        "artifact_paths": artifact_paths,
        "documented_command": format!("{command_prefix} resume {session_id}"),
        "agent_resume": agent_resume,
    })
}
