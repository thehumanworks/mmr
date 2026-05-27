use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::capture::CodexAdapter;
use crate::teleport::bundle::{remap_text, transcript_latest_timestamp};
use crate::teleport::manifest::restore_hints_for_provider;
use crate::teleport::provider::{NativeArtifactSpec, TeleportProviderProfile};

#[derive(Debug)]
pub struct CodexProfile;

pub const NATIVE_TRANSCRIPT: &str = "native/codex/transcript.jsonl";
pub const RESTORE_PATH: &str = "restore/codex.json";

pub static CODEX_PROFILE: CodexProfile = CodexProfile;

impl TeleportProviderProfile for CodexProfile {
    fn source_name(&self) -> &'static str {
        "codex"
    }

    fn parser_version(&self) -> &'static str {
        CodexAdapter::PARSER_VERSION
    }

    fn capabilities(&self) -> Vec<String> {
        vec!["codex-native-apply".to_string(), "store-import".to_string()]
    }

    fn native_artifacts(&self) -> Vec<NativeArtifactSpec> {
        vec![NativeArtifactSpec {
            bundle_path: NATIVE_TRANSCRIPT,
            kind: "native_transcript",
            required: true,
        }]
    }

    fn normalized_transcript_path(&self) -> &'static str {
        "transcript.normalized.jsonl"
    }

    fn restore_hints_path(&self) -> &'static str {
        RESTORE_PATH
    }

    fn primary_native_path(&self) -> &'static str {
        NATIVE_TRANSCRIPT
    }

    fn build_restore_hints(&self, session_id: &str) -> Value {
        restore_hints_for_provider(
            "codex",
            session_id,
            "codex exec",
            "best_effort",
            vec![NATIVE_TRANSCRIPT],
        )
    }

    fn documented_resume_command(&self, session_id: &str) -> String {
        format!("codex exec resume {session_id}")
    }

    fn agent_resume_level(&self) -> &'static str {
        "best_effort"
    }

    fn restore_adapters(&self) -> Vec<String> {
        vec!["codex-native-apply".to_string()]
    }

    fn supports_native_apply(&self) -> bool {
        true
    }

    fn supports_resume_transform(&self, bundle_source: &str, target: &str) -> bool {
        bundle_source == "codex" && target == "codex"
    }

    fn native_destination_path(
        &self,
        home: &Path,
        native_source_file: &str,
        session_id: &str,
        _target_project: &str,
        _source_canonical: &str,
    ) -> PathBuf {
        codex_native_destination_path(home, native_source_file, session_id)
    }

    fn remap_native_content(
        &self,
        bundle_path: &str,
        content: &str,
        replacements: &BTreeMap<String, String>,
        target_project: &str,
        source_canonical: &str,
        aliases: &[String],
    ) -> String {
        let _ = bundle_path;
        remap_codex_native_transcript(
            content,
            replacements,
            target_project,
            source_canonical,
            aliases,
        )
    }

    fn conflict_check_path(
        &self,
        bundle: &crate::teleport::bundle::TeleportBundleFile,
    ) -> Option<String> {
        bundle
            .files
            .keys()
            .find(|path| *path == NATIVE_TRANSCRIPT || *path == "transcript.native.jsonl")
            .cloned()
    }
}

pub fn codex_native_destination_path(
    home: &Path,
    native_source_file: &str,
    session_id: &str,
) -> PathBuf {
    const MARKER: &str = "/.codex/";
    if let Some(index) = native_source_file.find(MARKER) {
        let relative = &native_source_file[index + MARKER.len()..];
        return home.join(".codex").join(relative);
    }

    const MARKER_NO_SLASH: &str = ".codex/";
    if let Some(index) = native_source_file.find(MARKER_NO_SLASH) {
        let relative = &native_source_file[index + MARKER_NO_SLASH.len()..];
        return home.join(".codex").join(relative);
    }

    home.join(".codex")
        .join("sessions")
        .join(format!("{session_id}.jsonl"))
}

pub fn remap_codex_native_transcript(
    content: &str,
    replacements: &BTreeMap<String, String>,
    target_project: &str,
    source_canonical: &str,
    aliases: &[String],
) -> String {
    use serde_json::Value;

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

pub fn codex_conflict_timestamp(content: &str) -> Option<String> {
    transcript_latest_timestamp(content)
}
