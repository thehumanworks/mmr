use anyhow::{Context, Result};
use duckdb::{params, Connection};
use serde::Deserialize;
use std::path::Path;

use super::common::{extract_project_path_from_sessions, extract_text_content, extract_usage};

#[derive(Deserialize, Debug)]
pub(crate) struct ClaudeJsonlLine {
    #[serde(rename = "type")]
    pub(crate) msg_type: Option<String>,
    #[serde(rename = "sessionId")]
    pub(crate) session_id: Option<String>,
    pub(crate) message: Option<ClaudeMessagePayload>,
    pub(crate) timestamp: Option<String>,
    pub(crate) uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    pub(crate) parent_uuid: Option<String>,
    pub(crate) cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    pub(crate) git_branch: Option<String>,
    pub(crate) slug: Option<String>,
    pub(crate) version: Option<String>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct ClaudeMessagePayload {
    pub(crate) role: Option<String>,
    pub(crate) content: Option<serde_json::Value>,
    pub(crate) model: Option<String>,
    pub(crate) usage: Option<serde_json::Value>,
}

pub(crate) fn ingest_claude_jsonl_file(
    conn: &Connection,
    path: &Path,
    project_name: &str,
    project_path: &str,
    is_subagent: bool,
    counter: &mut i64,
) -> Result<usize> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let mut count = 0;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: ClaudeJsonlLine = match serde_json::from_str(line) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let msg_type = parsed.msg_type.as_deref().unwrap_or("");

        if msg_type != "user" && msg_type != "assistant" {
            continue;
        }

        let (role, content_text, model, input_tokens, output_tokens) =
            if let Some(ref msg) = parsed.message {
                let role = msg.role.as_deref().unwrap_or("").to_string();
                let text = msg
                    .content
                    .as_ref()
                    .map(extract_text_content)
                    .unwrap_or_default();

                if text.trim().is_empty() {
                    continue;
                }

                let model = msg.model.as_deref().unwrap_or("").to_string();
                let (input_t, output_t) = msg.usage.as_ref().map(extract_usage).unwrap_or((0, 0));
                (role, text, model, input_t, output_t)
            } else {
                continue;
            };

        *counter += 1;
        conn.execute(
            "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (?, 'claude', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                *counter,
                project_name,
                project_path,
                parsed.session_id.as_deref().unwrap_or(""),
                is_subagent,
                parsed.uuid.as_deref().unwrap_or(""),
                parsed.parent_uuid.as_deref().unwrap_or(""),
                msg_type,
                role,
                content_text,
                model,
                parsed.timestamp.as_deref().unwrap_or(""),
                parsed.cwd.as_deref().unwrap_or(""),
                parsed.git_branch.as_deref().unwrap_or(""),
                parsed.slug.as_deref().unwrap_or(""),
                parsed.version.as_deref().unwrap_or(""),
                input_tokens,
                output_tokens,
            ],
        )?;
        count += 1;
    }

    Ok(count)
}

pub(crate) fn ingest_claude(
    conn: &Connection,
    id_counter: &mut i64,
) -> Result<(usize, usize, usize)> {
    let claude_dir = dirs::home_dir()
        .context("No home directory")?
        .join(".claude")
        .join("projects");

    if !claude_dir.exists() {
        tracing::warn!(
            "Claude projects directory not found at {}",
            claude_dir.display()
        );
        return Ok((0, 0, 0));
    }

    let mut total_messages = 0usize;
    let mut total_sessions = 0usize;
    let mut total_projects = 0usize;

    for entry in std::fs::read_dir(&claude_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let project_dir_name = entry.file_name().to_string_lossy().to_string();
        let project_path = extract_project_path_from_sessions(&entry.path())
            .unwrap_or_else(|| super::common::decode_project_name(&project_dir_name));
        let mut project_sessions = 0;
        let mut project_messages = 0;

        for session_entry in std::fs::read_dir(entry.path())? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();

            if session_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let session_id = session_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                let msg_count = ingest_claude_jsonl_file(
                    conn,
                    &session_path,
                    &project_dir_name,
                    &project_path,
                    false,
                    id_counter,
                )?;

                if msg_count > 0 {
                    project_sessions += 1;
                    project_messages += msg_count;
                    total_sessions += 1;
                }

                let subagents_dir = entry.path().join(&session_id).join("subagents");
                if subagents_dir.exists() {
                    for sub_entry in std::fs::read_dir(&subagents_dir)? {
                        let sub_entry = sub_entry?;
                        let sub_path = sub_entry.path();
                        if sub_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                            let sub_count = ingest_claude_jsonl_file(
                                conn,
                                &sub_path,
                                &project_dir_name,
                                &project_path,
                                true,
                                id_counter,
                            )?;
                            project_messages += sub_count;
                        }
                    }
                }
            }
        }

        if project_messages > 0 {
            total_messages += project_messages;
            total_projects += 1;
            conn.execute(
                "INSERT INTO projects (name, source, original_path, session_count, message_count) VALUES (?, 'claude', ?, ?, ?)",
                params![project_dir_name, project_path, project_sessions, project_messages],
            )?;
        }
    }

    Ok((total_projects, total_sessions, total_messages))
}
