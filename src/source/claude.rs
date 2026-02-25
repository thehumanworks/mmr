use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde_json::Value;

use crate::model::{MessageRecord, SourceKind};

use super::decode_project_name;

struct ClaudeFileTask {
    path: PathBuf,
    project_name: String,
    is_subagent: bool,
}

pub fn load_claude_messages(home: &Path) -> Result<Vec<MessageRecord>> {
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for project_entry in fs::read_dir(&projects_dir)
        .with_context(|| format!("failed to read {}", projects_dir.display()))?
    {
        let project_entry = match project_entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let is_dir = match project_entry.file_type() {
            Ok(file_type) => file_type.is_dir(),
            Err(_) => false,
        };
        if !is_dir {
            continue;
        }

        let project_name = project_entry.file_name().to_string_lossy().to_string();
        let project_dir = project_entry.path();

        for entry in fs::read_dir(&project_dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();

            if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                files.push(ClaudeFileTask {
                    path,
                    project_name: project_name.clone(),
                    is_subagent: false,
                });
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if !file_type.is_dir() {
                continue;
            }

            let subagents_dir = path.join("subagents");
            if !subagents_dir.exists() {
                continue;
            }

            for subentry in fs::read_dir(&subagents_dir)? {
                let subentry = match subentry {
                    Ok(subentry) => subentry,
                    Err(_) => continue,
                };
                let subpath = subentry.path();
                if subpath.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                    files.push(ClaudeFileTask {
                        path: subpath,
                        project_name: project_name.clone(),
                        is_subagent: true,
                    });
                }
            }
        }
    }

    let nested = files
        .par_iter()
        .map(parse_claude_file)
        .collect::<Result<Vec<_>>>()?;

    Ok(nested.into_iter().flatten().collect())
}

fn parse_claude_file(task: &ClaudeFileTask) -> Result<Vec<MessageRecord>> {
    let content = fs::read_to_string(&task.path)
        .with_context(|| format!("failed to read Claude session file {}", task.path.display()))?;

    let mut out = Vec::new();
    let fallback_project_path = decode_project_name(&task.project_name);
    let source_file = task.path.display().to_string();
    let mut detected_project_path = String::new();

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let msg_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        if msg_type != "user" && msg_type != "assistant" {
            continue;
        }

        let session_id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if session_id.is_empty() {
            continue;
        }

        let message = match value.get("message") {
            Some(message) => message,
            None => continue,
        };

        let content_text = message
            .get("content")
            .map(extract_text_content)
            .unwrap_or_default();
        if content_text.trim().is_empty() {
            continue;
        }

        let cwd = value
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if detected_project_path.is_empty() && !cwd.is_empty() {
            detected_project_path = cwd.clone();
        }

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or(msg_type)
            .to_string();
        let model = message
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (input_tokens, output_tokens) =
            message.get("usage").map(extract_usage).unwrap_or((0, 0));

        out.push(MessageRecord {
            source: SourceKind::Claude,
            project_name: task.project_name.clone(),
            project_path: if cwd.is_empty() {
                fallback_project_path.clone()
            } else {
                cwd
            },
            session_id,
            role,
            content: content_text,
            model,
            timestamp,
            is_subagent: task.is_subagent,
            msg_type: msg_type.to_string(),
            input_tokens,
            output_tokens,
            source_file: source_file.clone(),
            line_index,
        });
    }

    if !detected_project_path.is_empty() {
        for record in &mut out {
            if record.project_path == fallback_project_path {
                record.project_path = detected_project_path.clone();
            }
        }
    }

    Ok(out)
}

fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .map(extract_text_content)
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(inner) = map.get("content") {
                return extract_text_content(inner);
            }
            if let Some(inner) = map.get("parts") {
                return extract_text_content(inner);
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn extract_usage(usage: &Value) -> (i64, i64) {
    (
        usage_number(usage, "input_tokens"),
        usage_number(usage, "output_tokens"),
    )
}

fn usage_number(usage: &Value, key: &str) -> i64 {
    match usage.get(key) {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::Object(map)) => map
            .get("total")
            .and_then(Value::as_i64)
            .or_else(|| map.get("value").and_then(Value::as_i64))
            .unwrap_or(0),
        _ => 0,
    }
}
