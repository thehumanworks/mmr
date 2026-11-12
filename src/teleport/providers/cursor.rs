use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::teleport::bundle::remap_text;
use crate::teleport::manifest::restore_hints_for_provider;
use crate::teleport::provider::{
    NativeArtifactSpec, TeleportProviderProfile, relative_after_marker,
    replace_first_relative_component, slash_hyphen_project_dir,
};

#[derive(Debug)]
pub struct CursorProfile;

pub const NATIVE_TRANSCRIPT: &str = "native/cursor/transcript.jsonl";
pub const RESTORE_PATH: &str = "restore/cursor.json";

pub static CURSOR_PROFILE: CursorProfile = CursorProfile;

impl TeleportProviderProfile for CursorProfile {
    fn source_name(&self) -> &'static str {
        "cursor"
    }

    fn parser_version(&self) -> &'static str {
        "cursor-agent-jsonl-v1"
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            "cursor-native-apply".to_string(),
            "cursor-pack-read".to_string(),
        ]
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

    fn build_restore_hints(&self, session_id: &str) -> Value {
        restore_hints_for_provider(
            "cursor",
            session_id,
            "Cursor IDE",
            "manual",
            vec![NATIVE_TRANSCRIPT],
        )
    }

    fn documented_resume_command(&self, session_id: &str) -> String {
        format!("Open Cursor agent transcript for session {session_id} in the target project")
    }

    fn agent_resume_level(&self) -> &'static str {
        "manual"
    }

    fn restore_adapters(&self) -> Vec<String> {
        vec!["cursor-native-apply".to_string()]
    }

    fn supports_native_apply(&self) -> bool {
        true
    }

    fn native_destination_path(
        &self,
        home: &Path,
        native_source_file: &str,
        session_id: &str,
        target_project: &str,
        _source_canonical: &str,
    ) -> PathBuf {
        cursor_native_destination_path(home, native_source_file, session_id, target_project)
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
        remap_cursor_native_transcript(
            content,
            replacements,
            target_project,
            source_canonical,
            aliases,
        )
    }
}

pub fn cursor_native_destination_path(
    home: &Path,
    native_source_file: &str,
    session_id: &str,
    target_project: &str,
) -> PathBuf {
    let target_dir = slash_hyphen_project_dir(target_project);
    if let Some(relative) = relative_after_marker(
        native_source_file,
        &["/.cursor/projects/", ".cursor/projects/"],
    ) {
        return replace_first_relative_component(
            home.join(".cursor").join("projects"),
            relative,
            &target_dir,
        );
    }
    home.join(".cursor")
        .join("projects")
        .join(target_dir)
        .join("agent-transcripts")
        .join(session_id)
        .join(format!("{session_id}.jsonl"))
}

pub fn remap_cursor_native_transcript(
    content: &str,
    replacements: &BTreeMap<String, String>,
    _target_project: &str,
    _source_canonical: &str,
    _aliases: &[String],
) -> String {
    remap_text(content, replacements)
}
