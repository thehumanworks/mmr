use anyhow::{Context, Result};
use duckdb::{params, Connection};

use super::common::collect_jsonl_recursive;

/// Parse a single Codex session JSONL file.
/// Returns (session_id, cwd, message_count).
pub(crate) fn ingest_codex_jsonl_file(
    conn: &Connection,
    path: &std::path::Path,
    counter: &mut i64,
) -> Result<Option<(String, String, usize)>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let mut session_id = String::new();
    let mut cwd = String::new();
    let mut git_branch = String::new();
    let mut cli_version = String::new();
    let mut model_provider = String::new();
    let mut session_timestamp = String::new();
    let mut count = 0usize;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let val: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let line_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let timestamp = val
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        match line_type {
            "session_meta" => {
                if let Some(payload) = val.get("payload") {
                    session_id = payload
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    cwd = payload
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    cli_version = payload
                        .get("cli_version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    model_provider = payload
                        .get("model_provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    session_timestamp = payload
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(git) = payload.get("git") {
                        git_branch = git
                            .get("branch")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                    }
                }
            }
            "event_msg" => {
                if let Some(payload) = val.get("payload") {
                    let evt_type = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if evt_type == "user_message" {
                        let text = payload
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("");
                        if text.trim().is_empty() {
                            continue;
                        }

                        let project_name = &cwd;
                        *counter += 1;
                        conn.execute(
                            "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (?, 'codex', ?, ?, ?, FALSE, '', '', 'user', 'user', ?, ?, ?, ?, ?, '', ?, 0, 0)",
                            params![
                                *counter,
                                project_name,
                                &cwd,
                                &session_id,
                                text,
                                &model_provider,
                                &timestamp,
                                &cwd,
                                &git_branch,
                                &cli_version,
                            ],
                        )?;
                        count += 1;
                    }
                }
            }
            "response_item" => {
                if let Some(payload) = val.get("payload") {
                    let role = payload.get("role").and_then(|r| r.as_str()).unwrap_or("");

                    if role == "assistant" {
                        if let Some(content_arr) = payload.get("content").and_then(|c| c.as_array())
                        {
                            let mut parts = Vec::new();
                            for item in content_arr {
                                if let Some(obj) = item.as_object() {
                                    let ct = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    if ct == "output_text" {
                                        if let Some(text) = obj.get("text").and_then(|t| t.as_str())
                                        {
                                            parts.push(text.to_string());
                                        }
                                    }
                                }
                            }
                            let text = parts.join("\n");
                            if text.trim().is_empty() {
                                continue;
                            }

                            let project_name = &cwd;
                            *counter += 1;
                            conn.execute(
                                "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (?, 'codex', ?, ?, ?, FALSE, '', '', 'assistant', 'assistant', ?, ?, ?, ?, ?, '', ?, 0, 0)",
                                params![
                                    *counter,
                                    project_name,
                                    &cwd,
                                    &session_id,
                                    text,
                                    &model_provider,
                                    &timestamp,
                                    &cwd,
                                    &git_branch,
                                    &cli_version,
                                ],
                            )?;
                            count += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if session_id.is_empty() || count == 0 {
        return Ok(None);
    }
    let _ = session_timestamp;
    Ok(Some((session_id, cwd, count)))
}

pub(crate) fn ingest_codex(
    conn: &Connection,
    id_counter: &mut i64,
) -> Result<(usize, usize, usize)> {
    let home = dirs::home_dir().context("No home directory")?;
    let codex_dir = home.join(".codex");

    if !codex_dir.exists() {
        tracing::warn!("Codex directory not found at {}", codex_dir.display());
        return Ok((0, 0, 0));
    }

    let mut jsonl_files = Vec::new();

    let sessions_dir = codex_dir.join("sessions");
    if sessions_dir.exists() {
        collect_jsonl_recursive(&sessions_dir, &mut jsonl_files)?;
    }

    let archived_dir = codex_dir.join("archived_sessions");
    if archived_dir.exists() {
        collect_jsonl_recursive(&archived_dir, &mut jsonl_files)?;
    }

    let mut project_stats: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    let mut total_sessions = 0usize;
    let mut total_messages = 0usize;

    for file_path in &jsonl_files {
        match ingest_codex_jsonl_file(conn, file_path, id_counter) {
            Ok(Some((_sid, cwd, msg_count))) => {
                let entry = project_stats.entry(cwd).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += msg_count;
                total_sessions += 1;
                total_messages += msg_count;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Failed to ingest codex file {}: {}", file_path.display(), e);
            }
        }
    }

    let total_projects = project_stats.len();

    for (cwd, (session_count, message_count)) in &project_stats {
        conn.execute(
            "INSERT INTO projects (name, source, original_path, session_count, message_count) VALUES (?, 'codex', ?, ?, ?)",
            params![cwd, cwd, session_count, message_count],
        )?;
    }

    Ok((total_projects, total_sessions, total_messages))
}
