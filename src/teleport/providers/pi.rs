use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::teleport::bundle::{remap_text, transcript_latest_timestamp};
use crate::teleport::manifest::restore_hints_for_provider;
use crate::teleport::provider::{
    NativeArtifactSpec, TeleportProviderProfile, pi_project_dir, relative_after_marker,
    replace_first_relative_component,
};

#[derive(Debug)]
pub struct PiProfile;

pub const NATIVE_TRANSCRIPT: &str = "native/pi/transcript.jsonl";
pub const RESTORE_PATH: &str = "restore/pi.json";

pub static PI_PROFILE: PiProfile = PiProfile;

impl TeleportProviderProfile for PiProfile {
    fn source_name(&self) -> &'static str {
        "pi"
    }

    fn parser_version(&self) -> &'static str {
        "pi-agent-jsonl-v1"
    }

    fn capabilities(&self) -> Vec<String> {
        vec!["pi-native-apply".to_string()]
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
            "pi",
            session_id,
            "pi agent",
            "manual",
            vec![NATIVE_TRANSCRIPT],
        )
    }

    fn documented_resume_command(&self, session_id: &str) -> String {
        format!("pi agent resume {session_id}")
    }

    fn agent_resume_level(&self) -> &'static str {
        "manual"
    }

    fn restore_adapters(&self) -> Vec<String> {
        vec!["pi-native-apply".to_string()]
    }

    fn supports_native_apply(&self) -> bool {
        true
    }

    fn supports_resume_transform(&self, bundle_source: &str, target: &str) -> bool {
        bundle_source == "pi" && target == "pi"
    }

    fn native_destination_path(
        &self,
        home: &Path,
        native_source_file: &str,
        session_id: &str,
        target_project: &str,
        _source_canonical: &str,
    ) -> PathBuf {
        pi_native_destination_path(home, native_source_file, session_id, target_project)
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
        remap_pi_native_transcript(
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

pub fn pi_native_destination_path(
    home: &Path,
    native_source_file: &str,
    session_id: &str,
    target_project: &str,
) -> PathBuf {
    let target_dir = pi_project_dir(target_project);
    if let Some(relative) = relative_after_marker(
        native_source_file,
        &["/.pi/agent/sessions/", ".pi/agent/sessions/"],
    ) {
        return replace_first_relative_component(
            home.join(".pi").join("agent").join("sessions"),
            relative,
            &target_dir,
        );
    }
    home.join(".pi")
        .join("agent")
        .join("sessions")
        .join(target_dir)
        .join(format!("{session_id}.jsonl"))
}

#[allow(clippy::collapsible_if)]
pub fn remap_pi_native_transcript(
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
        if value.get("type").and_then(Value::as_str) == Some("session")
            && let Some(cwd) = value.get("cwd").and_then(Value::as_str)
            && (cwd == source_canonical || aliases.iter().any(|alias| alias == cwd))
            && let Some(obj) = value.as_object_mut()
        {
            obj.insert("cwd".to_string(), Value::String(target_project.to_string()));
        }
        lines.push(value.to_string());
    }
    let mut updated = lines.join("\n");
    if content.ends_with('\n') && !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

pub fn pi_conflict_timestamp(content: &str) -> Option<String> {
    transcript_latest_timestamp(content)
}
