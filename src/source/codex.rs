use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde_json::Value;
use walkdir::WalkDir;

use crate::types::{MessageRecord, SourceKind};

pub fn load_codex_messages(home: &Path) -> Result<Vec<MessageRecord>> {
    let codex_root = home.join(".codex");
    if !codex_root.exists() {
        return Ok(Vec::new());
    }

    let mut jsonl_files = Vec::new();
    collect_jsonl_recursive(&codex_root.join("sessions"), &mut jsonl_files);
    collect_jsonl_recursive(&codex_root.join("archived_sessions"), &mut jsonl_files);

    let nested = jsonl_files
        .par_iter()
        .map(|path| parse_codex_file(path))
        .collect::<Result<Vec<_>>>()?;

    Ok(nested.into_iter().flatten().collect())
}

fn collect_jsonl_recursive(root: &Path, out: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }

        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(entry.path().to_path_buf());
        }
    }
}

fn parse_codex_file(path: &Path) -> Result<Vec<MessageRecord>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read Codex session file {}", path.display()))?;

    let mut out = Vec::new();
    let mut session_id = String::new();
    let mut cwd = String::new();
    let mut model_provider = String::new();
    let source_file = path.display().to_string();

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let entry_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        match entry_type {
            "session_meta" => {
                if let Some(payload) = value.get("payload") {
                    session_id = payload
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    cwd = payload
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    model_provider = payload
                        .get("model_provider")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                }
            }
            "event_msg" => {
                let payload = match value.get("payload") {
                    Some(payload) => payload,
                    None => continue,
                };
                if payload.get("type").and_then(Value::as_str) != Some("user_message") {
                    continue;
                }
                let text = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();

                push_message(
                    &mut out,
                    &source_file,
                    line_index,
                    &session_id,
                    &cwd,
                    "user",
                    "user",
                    text,
                    &model_provider,
                    timestamp,
                );
            }
            "response_item" => {
                let payload = match value.get("payload") {
                    Some(payload) => payload,
                    None => continue,
                };
                if payload.get("role").and_then(Value::as_str) != Some("assistant") {
                    continue;
                }

                let text = payload
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.as_object())
                    .filter(|obj| obj.get("type").and_then(Value::as_str) == Some("output_text"))
                    .filter_map(|obj| obj.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n");

                push_message(
                    &mut out,
                    &source_file,
                    line_index,
                    &session_id,
                    &cwd,
                    "assistant",
                    "assistant",
                    text,
                    &model_provider,
                    timestamp,
                );
            }
            _ => {}
        }
    }

    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn push_message(
    out: &mut Vec<MessageRecord>,
    source_file: &str,
    line_index: usize,
    session_id: &str,
    cwd: &str,
    msg_type: &str,
    role: &str,
    content: String,
    model: &str,
    timestamp: String,
) {
    if session_id.is_empty() || cwd.is_empty() || content.trim().is_empty() {
        return;
    }

    out.push(MessageRecord {
        source: SourceKind::Codex,
        project_name: cwd.to_string(),
        project_path: cwd.to_string(),
        session_id: session_id.to_string(),
        role: role.to_string(),
        content,
        model: model.to_string(),
        timestamp,
        is_subagent: false,
        msg_type: msg_type.to_string(),
        input_tokens: 0,
        output_tokens: 0,
        source_file: source_file.to_string(),
        line_index,
    });
}
