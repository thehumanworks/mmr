use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::teleport::bundle::{remap_text, transcript_latest_timestamp};
use crate::teleport::manifest::restore_hints_for_provider;
use crate::teleport::provider::{
    NativeArtifactSpec, TeleportProviderProfile, percent_encoded_project_dir,
    relative_after_marker, replace_first_relative_component,
};

#[derive(Debug)]
pub struct GrokProfile;

pub const SUMMARY_PATH: &str = "native/grok/summary.json";
pub const UPDATES_PATH: &str = "native/grok/updates.jsonl";
pub const RESTORE_PATH: &str = "restore/grok.json";

pub static GROK_PROFILE: GrokProfile = GrokProfile;

impl TeleportProviderProfile for GrokProfile {
    fn source_name(&self) -> &'static str {
        "grok"
    }

    fn parser_version(&self) -> &'static str {
        "grok-session-v1"
    }

    fn capabilities(&self) -> Vec<String> {
        vec!["grok-native-apply".to_string()]
    }

    fn native_artifacts(&self) -> Vec<NativeArtifactSpec> {
        vec![
            NativeArtifactSpec {
                bundle_path: SUMMARY_PATH,
                kind: "native_summary",
                required: true,
            },
            NativeArtifactSpec {
                bundle_path: UPDATES_PATH,
                kind: "native_transcript",
                required: true,
            },
        ]
    }

    fn normalized_transcript_path(&self) -> &'static str {
        "transcript.normalized.jsonl"
    }

    fn restore_hints_path(&self) -> &'static str {
        RESTORE_PATH
    }

    fn primary_native_path(&self) -> &'static str {
        UPDATES_PATH
    }

    fn build_restore_hints(&self, session_id: &str) -> Value {
        restore_hints_for_provider(
            "grok",
            session_id,
            "grok agent",
            "best_effort",
            vec![SUMMARY_PATH, UPDATES_PATH],
        )
    }

    fn documented_resume_command(&self, session_id: &str) -> String {
        format!("grok agent resume {session_id}")
    }

    fn agent_resume_level(&self) -> &'static str {
        "best_effort"
    }

    fn restore_adapters(&self) -> Vec<String> {
        vec!["grok-native-apply".to_string()]
    }

    fn supports_native_apply(&self) -> bool {
        true
    }

    fn supports_resume_transform(&self, bundle_source: &str, target: &str) -> bool {
        bundle_source == "grok" && target == "grok"
    }

    fn native_destination_path(
        &self,
        home: &Path,
        native_source_file: &str,
        session_id: &str,
        target_project: &str,
        _source_canonical: &str,
    ) -> PathBuf {
        grok_native_destination_path(home, native_source_file, session_id, target_project)
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
        match bundle_path {
            SUMMARY_PATH => remap_grok_summary(
                content,
                replacements,
                target_project,
                source_canonical,
                aliases,
            ),
            UPDATES_PATH => remap_text(content, replacements),
            _ => remap_text(content, replacements),
        }
    }

    fn conflict_check_path(
        &self,
        bundle: &crate::teleport::bundle::TeleportBundleFile,
    ) -> Option<String> {
        if bundle.files.contains_key(UPDATES_PATH) {
            Some(UPDATES_PATH.to_string())
        } else if bundle.files.contains_key("transcript.native.jsonl") {
            Some("transcript.native.jsonl".to_string())
        } else {
            None
        }
    }
}

pub fn grok_native_destination_path(
    home: &Path,
    native_source_file: &str,
    session_id: &str,
    target_project: &str,
) -> PathBuf {
    let target_dir = percent_encoded_project_dir(target_project);
    if let Some(relative) =
        relative_after_marker(native_source_file, &["/.grok/sessions/", ".grok/sessions/"])
    {
        let base = replace_first_relative_component(
            home.join(".grok").join("sessions"),
            relative,
            &target_dir,
        );
        if native_source_file.ends_with("summary.json") {
            return base;
        }
        return base;
    }
    home.join(".grok")
        .join("sessions")
        .join(target_dir)
        .join(session_id)
        .join("updates.jsonl")
}

fn remap_grok_summary(
    content: &str,
    replacements: &BTreeMap<String, String>,
    target_project: &str,
    source_canonical: &str,
    aliases: &[String],
) -> String {
    let remapped = remap_text(content, replacements);
    let Ok(mut value) = serde_json::from_str::<Value>(&remapped) else {
        return remapped;
    };
    if let Some(info) = value.get_mut("info").and_then(Value::as_object_mut)
        && let Some(cwd) = info.get("cwd").and_then(Value::as_str)
        && (cwd == source_canonical || aliases.iter().any(|alias| alias == cwd))
    {
        info.insert("cwd".to_string(), Value::String(target_project.to_string()));
    }
    value.to_string()
}

pub fn grok_conflict_timestamp(
    bundle: &crate::teleport::bundle::TeleportBundleFile,
) -> Option<String> {
    let updates = bundle
        .files
        .get(UPDATES_PATH)
        .or_else(|| bundle.files.get("transcript.native.jsonl"))?;
    transcript_latest_timestamp(updates)
}
