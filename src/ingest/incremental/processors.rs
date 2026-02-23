use anyhow::{Context, Result};
use duckdb::{params, Connection};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use crate::ingest::source::claude::ClaudeJsonlLine;
use crate::ingest::source::common::{extract_text_content, extract_usage};

use super::state::{
    decide_file_refresh_mode, delete_messages_for_file, file_set_key, load_ingest_file_state,
    metadata_mtime_unix, path_to_string, upsert_ingest_file_state, upsert_ingest_project,
    ClaudeFileIngestOutcome, CodexFileIngestOutcome, CodexSessionMeta, FileRefreshMode,
    IncrementalRefreshStats, IngestFileState,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn ingest_claude_jsonl_file_from_offset(
    conn: &Connection,
    path: &Path,
    project_name: &str,
    project_path: &str,
    default_session_id: &str,
    is_subagent: bool,
    start_offset: u64,
    counter: &mut i64,
) -> Result<ClaudeFileIngestOutcome> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(start_offset))?;

    let source_file = path_to_string(path);
    let mut current_offset = start_offset;
    let mut line = String::new();
    let mut result = ClaudeFileIngestOutcome {
        session_id: default_session_id.to_string(),
        ..Default::default()
    };

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }
        let line_offset = current_offset;
        current_offset += bytes_read as u64;

        if line.trim().is_empty() {
            continue;
        }

        let parsed: ClaudeJsonlLine = match serde_json::from_str(&line) {
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

        if let Some(session_id) = parsed.session_id.as_deref() {
            if !session_id.is_empty() {
                result.session_id = session_id.to_string();
            }
        }

        *counter += 1;
        conn.execute(
            "INSERT INTO messages (
                id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid,
                msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version,
                input_tokens, output_tokens, source_file, source_offset
             ) VALUES (?, 'claude', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                *counter,
                project_name,
                project_path,
                &result.session_id,
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
                &source_file,
                line_offset as i64,
            ],
        )?;

        result.base.inserted_messages += 1;
        result.base.last_message_timestamp = parsed.timestamp.unwrap_or_default();
        result.base.last_message_key = parsed
            .uuid
            .unwrap_or_else(|| format!("{}:{}", result.session_id, line_offset));
    }

    result.base.final_offset = current_offset;
    Ok(result)
}

pub(crate) fn probe_codex_session_meta(path: &Path) -> Result<CodexSessionMeta> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut meta = CodexSessionMeta::default();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let val: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if val.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
            continue;
        }

        if let Some(payload) = val.get("payload") {
            meta.session_id = payload
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            meta.cwd = payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            meta.model_provider = payload
                .get("model_provider")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            meta.cli_version = payload
                .get("cli_version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(git) = payload.get("git") {
                meta.git_branch = git
                    .get("branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
        }
        break;
    }

    Ok(meta)
}

pub(crate) fn ingest_codex_jsonl_file_from_offset(
    conn: &Connection,
    path: &Path,
    start_offset: u64,
    seed_meta: &CodexSessionMeta,
    counter: &mut i64,
) -> Result<CodexFileIngestOutcome> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(start_offset))?;

    let source_file = path_to_string(path);
    let mut current_offset = start_offset;
    let mut line = String::new();
    let mut result = CodexFileIngestOutcome {
        meta: seed_meta.clone(),
        ..Default::default()
    };

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }
        let line_offset = current_offset;
        current_offset += bytes_read as u64;

        if line.trim().is_empty() {
            continue;
        }

        let val: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let line_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let timestamp = val.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");

        match line_type {
            "session_meta" => {
                if let Some(payload) = val.get("payload") {
                    let sid = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if !sid.is_empty() {
                        result.meta.session_id = sid.to_string();
                    }
                    let cwd = payload.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
                    if !cwd.is_empty() {
                        result.meta.cwd = cwd.to_string();
                    }
                    let provider = payload
                        .get("model_provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !provider.is_empty() {
                        result.meta.model_provider = provider.to_string();
                    }
                    let version = payload
                        .get("cli_version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !version.is_empty() {
                        result.meta.cli_version = version.to_string();
                    }
                    if let Some(git) = payload.get("git") {
                        let branch = git.get("branch").and_then(|v| v.as_str()).unwrap_or("");
                        if !branch.is_empty() {
                            result.meta.git_branch = branch.to_string();
                        }
                    }
                }
            }
            "event_msg" => {
                if let Some(payload) = val.get("payload") {
                    let evt_type = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if evt_type != "user_message" {
                        continue;
                    }
                    let text = payload
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("");
                    if text.trim().is_empty()
                        || result.meta.session_id.is_empty()
                        || result.meta.cwd.is_empty()
                    {
                        continue;
                    }

                    *counter += 1;
                    conn.execute(
                        "INSERT INTO messages (
                            id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid,
                            msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version,
                            input_tokens, output_tokens, source_file, source_offset
                         ) VALUES (?, 'codex', ?, ?, ?, FALSE, '', '', 'user', 'user', ?, ?, ?, ?, ?, '', ?, 0, 0, ?, ?)",
                        params![
                            *counter,
                            &result.meta.cwd,
                            &result.meta.cwd,
                            &result.meta.session_id,
                            text,
                            &result.meta.model_provider,
                            timestamp,
                            &result.meta.cwd,
                            &result.meta.git_branch,
                            &result.meta.cli_version,
                            &source_file,
                            line_offset as i64,
                        ],
                    )?;

                    result.base.inserted_messages += 1;
                    result.base.last_message_timestamp = timestamp.to_string();
                    result.base.last_message_key =
                        format!("{}:{line_offset}", result.meta.session_id);
                }
            }
            "response_item" => {
                if let Some(payload) = val.get("payload") {
                    if payload.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                        continue;
                    }
                    if result.meta.session_id.is_empty() || result.meta.cwd.is_empty() {
                        continue;
                    }
                    let Some(content_arr) = payload.get("content").and_then(|c| c.as_array())
                    else {
                        continue;
                    };

                    let mut parts = Vec::new();
                    for item in content_arr {
                        if let Some(obj) = item.as_object() {
                            if obj.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                                    parts.push(text.to_string());
                                }
                            }
                        }
                    }

                    let text = parts.join("\n");
                    if text.trim().is_empty() {
                        continue;
                    }

                    *counter += 1;
                    conn.execute(
                        "INSERT INTO messages (
                            id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid,
                            msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version,
                            input_tokens, output_tokens, source_file, source_offset
                         ) VALUES (?, 'codex', ?, ?, ?, FALSE, '', '', 'assistant', 'assistant', ?, ?, ?, ?, ?, '', ?, 0, 0, ?, ?)",
                        params![
                            *counter,
                            &result.meta.cwd,
                            &result.meta.cwd,
                            &result.meta.session_id,
                            text,
                            &result.meta.model_provider,
                            timestamp,
                            &result.meta.cwd,
                            &result.meta.git_branch,
                            &result.meta.cli_version,
                            &source_file,
                            line_offset as i64,
                        ],
                    )?;

                    result.base.inserted_messages += 1;
                    result.base.last_message_timestamp = timestamp.to_string();
                    result.base.last_message_key =
                        format!("{}:{line_offset}", result.meta.session_id);
                }
            }
            _ => {}
        }
    }

    result.base.final_offset = current_offset;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn process_claude_file_incremental(
    conn: &Connection,
    path: &Path,
    project_name: &str,
    project_path: &str,
    is_subagent: bool,
    id_counter: &mut i64,
    seen_files: &mut HashSet<String>,
    refresh_stats: &mut IncrementalRefreshStats,
) -> Result<()> {
    let source = "claude";
    let file_path = path_to_string(path);
    seen_files.insert(file_set_key(source, &file_path));

    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();
    let file_mtime = metadata_mtime_unix(&metadata);
    let existing = load_ingest_file_state(conn, source, &file_path)?;
    let mode = decide_file_refresh_mode(existing.as_ref(), file_size, file_mtime);

    let default_session_id = existing
        .as_ref()
        .map(|s| s.session_id.clone())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });

    match mode {
        FileRefreshMode::Skip => {
            refresh_stats.unchanged_files += 1;
            if !project_name.is_empty() {
                upsert_ingest_project(conn, source, project_name, project_path)?;
            }
            return Ok(());
        }
        FileRefreshMode::Append(start_offset) => {
            let mut seed_last_ts = existing
                .as_ref()
                .map(|s| s.last_message_timestamp.clone())
                .unwrap_or_default();
            let mut seed_last_key = existing
                .as_ref()
                .map(|s| s.last_message_key.clone())
                .unwrap_or_default();
            let outcome = ingest_claude_jsonl_file_from_offset(
                conn,
                path,
                project_name,
                project_path,
                &default_session_id,
                is_subagent,
                start_offset,
                id_counter,
            )?;
            if !outcome.base.last_message_timestamp.is_empty() {
                seed_last_ts = outcome.base.last_message_timestamp.clone();
            }
            if !outcome.base.last_message_key.is_empty() {
                seed_last_key = outcome.base.last_message_key.clone();
            }

            refresh_stats.inserted_messages += outcome.base.inserted_messages;
            if outcome.base.inserted_messages > 0 || existing.is_none() {
                refresh_stats.changed_files += 1;
            } else {
                refresh_stats.unchanged_files += 1;
            }

            let state = IngestFileState {
                project: project_name.to_string(),
                project_path: project_path.to_string(),
                session_id: outcome.session_id,
                is_subagent,
                last_offset: outcome.base.final_offset as i64,
                file_size: file_size as i64,
                file_mtime_unix: file_mtime,
                last_message_timestamp: seed_last_ts,
                last_message_key: seed_last_key,
                meta_model: String::new(),
                meta_git_branch: String::new(),
                meta_version: String::new(),
            };
            upsert_ingest_file_state(conn, source, &file_path, &state)?;
            if !project_name.is_empty() {
                upsert_ingest_project(conn, source, project_name, project_path)?;
            }
        }
        FileRefreshMode::Rewrite => {
            let removed = delete_messages_for_file(conn, source, &file_path)?;
            refresh_stats.removed_messages += removed;
            let outcome = ingest_claude_jsonl_file_from_offset(
                conn,
                path,
                project_name,
                project_path,
                &default_session_id,
                is_subagent,
                0,
                id_counter,
            )?;

            refresh_stats.inserted_messages += outcome.base.inserted_messages;
            refresh_stats.changed_files += 1;

            let state = IngestFileState {
                project: project_name.to_string(),
                project_path: project_path.to_string(),
                session_id: outcome.session_id,
                is_subagent,
                last_offset: outcome.base.final_offset as i64,
                file_size: file_size as i64,
                file_mtime_unix: file_mtime,
                last_message_timestamp: outcome.base.last_message_timestamp,
                last_message_key: outcome.base.last_message_key,
                meta_model: String::new(),
                meta_git_branch: String::new(),
                meta_version: String::new(),
            };
            upsert_ingest_file_state(conn, source, &file_path, &state)?;
            if !project_name.is_empty() {
                upsert_ingest_project(conn, source, project_name, project_path)?;
            }
        }
    }

    Ok(())
}

pub(crate) fn process_codex_file_incremental(
    conn: &Connection,
    path: &Path,
    id_counter: &mut i64,
    seen_files: &mut HashSet<String>,
    refresh_stats: &mut IncrementalRefreshStats,
) -> Result<()> {
    let source = "codex";
    let file_path = path_to_string(path);
    seen_files.insert(file_set_key(source, &file_path));

    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();
    let file_mtime = metadata_mtime_unix(&metadata);
    let existing = load_ingest_file_state(conn, source, &file_path)?;
    let mode = decide_file_refresh_mode(existing.as_ref(), file_size, file_mtime);

    let mut seed_meta = if let Some(state) = existing.as_ref() {
        CodexSessionMeta {
            session_id: state.session_id.clone(),
            cwd: state.project_path.clone(),
            model_provider: state.meta_model.clone(),
            git_branch: state.meta_git_branch.clone(),
            cli_version: state.meta_version.clone(),
        }
    } else {
        probe_codex_session_meta(path)?
    };
    if seed_meta.session_id.is_empty() {
        seed_meta.session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
    }

    match mode {
        FileRefreshMode::Skip => {
            refresh_stats.unchanged_files += 1;
            if !seed_meta.cwd.is_empty() {
                upsert_ingest_project(conn, source, &seed_meta.cwd, &seed_meta.cwd)?;
            }
            return Ok(());
        }
        FileRefreshMode::Append(start_offset) => {
            let mut seed_last_ts = existing
                .as_ref()
                .map(|s| s.last_message_timestamp.clone())
                .unwrap_or_default();
            let mut seed_last_key = existing
                .as_ref()
                .map(|s| s.last_message_key.clone())
                .unwrap_or_default();
            let outcome = ingest_codex_jsonl_file_from_offset(
                conn,
                path,
                start_offset,
                &seed_meta,
                id_counter,
            )?;
            if !outcome.base.last_message_timestamp.is_empty() {
                seed_last_ts = outcome.base.last_message_timestamp.clone();
            }
            if !outcome.base.last_message_key.is_empty() {
                seed_last_key = outcome.base.last_message_key.clone();
            }

            refresh_stats.inserted_messages += outcome.base.inserted_messages;
            if outcome.base.inserted_messages > 0 || existing.is_none() {
                refresh_stats.changed_files += 1;
            } else {
                refresh_stats.unchanged_files += 1;
            }

            let project = if !outcome.meta.cwd.is_empty() {
                outcome.meta.cwd.clone()
            } else {
                existing
                    .as_ref()
                    .map(|s| s.project.clone())
                    .unwrap_or_default()
            };
            let state = IngestFileState {
                project: project.clone(),
                project_path: project.clone(),
                session_id: outcome.meta.session_id.clone(),
                is_subagent: false,
                last_offset: outcome.base.final_offset as i64,
                file_size: file_size as i64,
                file_mtime_unix: file_mtime,
                last_message_timestamp: seed_last_ts,
                last_message_key: seed_last_key,
                meta_model: outcome.meta.model_provider.clone(),
                meta_git_branch: outcome.meta.git_branch.clone(),
                meta_version: outcome.meta.cli_version.clone(),
            };
            upsert_ingest_file_state(conn, source, &file_path, &state)?;
            if !project.is_empty() {
                upsert_ingest_project(conn, source, &project, &project)?;
            }
        }
        FileRefreshMode::Rewrite => {
            let removed = delete_messages_for_file(conn, source, &file_path)?;
            refresh_stats.removed_messages += removed;
            let outcome =
                ingest_codex_jsonl_file_from_offset(conn, path, 0, &seed_meta, id_counter)?;

            refresh_stats.inserted_messages += outcome.base.inserted_messages;
            refresh_stats.changed_files += 1;

            let project = if !outcome.meta.cwd.is_empty() {
                outcome.meta.cwd.clone()
            } else {
                existing
                    .as_ref()
                    .map(|s| s.project.clone())
                    .unwrap_or_default()
            };
            let state = IngestFileState {
                project: project.clone(),
                project_path: project.clone(),
                session_id: outcome.meta.session_id.clone(),
                is_subagent: false,
                last_offset: outcome.base.final_offset as i64,
                file_size: file_size as i64,
                file_mtime_unix: file_mtime,
                last_message_timestamp: outcome.base.last_message_timestamp,
                last_message_key: outcome.base.last_message_key,
                meta_model: outcome.meta.model_provider.clone(),
                meta_git_branch: outcome.meta.git_branch.clone(),
                meta_version: outcome.meta.cli_version.clone(),
            };
            upsert_ingest_file_state(conn, source, &file_path, &state)?;
            if !project.is_empty() {
                upsert_ingest_project(conn, source, &project, &project)?;
            }
        }
    }

    Ok(())
}

pub(crate) fn cleanup_removed_files(
    conn: &Connection,
    seen_files: &HashSet<String>,
    refresh_stats: &mut IncrementalRefreshStats,
) -> Result<()> {
    let mut stmt = conn.prepare("SELECT source, file_path FROM ingest_files")?;
    let existing_rows: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (source, file_path) in existing_rows {
        let key = file_set_key(&source, &file_path);
        if seen_files.contains(&key) {
            continue;
        }
        let removed = delete_messages_for_file(conn, &source, &file_path)?;
        refresh_stats.removed_messages += removed;
        refresh_stats.changed_files += 1;
        conn.execute(
            "DELETE FROM ingest_files WHERE source = ? AND file_path = ?",
            params![source, file_path],
        )?;
    }

    Ok(())
}
