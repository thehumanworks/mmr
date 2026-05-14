use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde_json::Value;
use walkdir::WalkDir;

use crate::types::{MessageRecord, SourceKind};

struct PiFileTask {
    path: PathBuf,
    project_name: String,
}

pub fn load_pi_messages(home: &Path) -> Result<Vec<MessageRecord>> {
    let sessions_dir = home.join(".pi").join("agent").join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(&sessions_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let project_name = entry
            .path()
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();

        files.push(PiFileTask {
            path: entry.path().to_path_buf(),
            project_name,
        });
    }

    let nested = files
        .par_iter()
        .map(parse_pi_file)
        .collect::<Result<Vec<_>>>()?;

    Ok(nested.into_iter().flatten().collect())
}

fn parse_pi_file(task: &PiFileTask) -> Result<Vec<MessageRecord>> {
    let content = fs::read_to_string(&task.path)
        .with_context(|| format!("failed to read Pi session file {}", task.path.display()))?;

    let mut out = Vec::new();
    let mut session_id = session_id_from_path(&task.path);
    let mut cwd = String::new();
    let mut current_model = String::new();
    let source_file = task.path.display().to_string();

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let entry_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        match entry_type {
            "session" => {
                if let Some(id) = value.get("id").and_then(Value::as_str)
                    && !id.is_empty()
                {
                    session_id = id.to_string();
                }
                cwd = value
                    .get("cwd")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
            }
            "model_change" => {
                current_model = extract_model_change(&value);
            }
            "message" => {
                let message = match value.get("message") {
                    Some(message) => message,
                    None => continue,
                };
                let role = message.get("role").and_then(Value::as_str).unwrap_or("");
                if role != "user" && role != "assistant" {
                    continue;
                }

                let content_text = message
                    .get("content")
                    .map(extract_text_content)
                    .unwrap_or_default();
                if content_text.trim().is_empty() {
                    continue;
                }

                let timestamp = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let model = message
                    .get("model")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(|| current_model.clone());
                let (input_tokens, output_tokens) =
                    message.get("usage").map(extract_usage).unwrap_or((0, 0));

                push_message(
                    &mut out,
                    &source_file,
                    line_index,
                    &task.project_name,
                    &session_id,
                    &cwd,
                    role,
                    content_text,
                    model,
                    timestamp,
                    input_tokens,
                    output_tokens,
                );
            }
            _ => {}
        }
    }

    Ok(out)
}

fn session_id_from_path(path: &Path) -> String {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return String::new();
    };
    stem.rsplit_once('_')
        .map(|(_, id)| id.to_string())
        .unwrap_or_else(|| stem.to_string())
}

fn extract_model_change(value: &Value) -> String {
    let provider = value.get("provider").and_then(Value::as_str).unwrap_or("");
    let model = value.get("modelId").and_then(Value::as_str).unwrap_or("");
    match (provider.is_empty(), model.is_empty()) {
        (true, true) => String::new(),
        (true, false) => model.to_string(),
        (false, true) => provider.to_string(),
        (false, false) => format!("{provider}/{model}"),
    }
}

fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(extract_text_item)
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => extract_text_item(content).unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_text_item(item: &Value) -> Option<String> {
    let obj = item.as_object()?;
    match obj.get("type").and_then(Value::as_str) {
        Some("text") => obj.get("text").and_then(Value::as_str).map(String::from),
        _ => None,
    }
}

fn extract_usage(usage: &Value) -> (i64, i64) {
    (
        usage_number(usage, "input")
            + usage_number(usage, "input_tokens")
            + usage_number(usage, "inputTokens")
            + usage_number(usage, "prompt_tokens"),
        usage_number(usage, "output")
            + usage_number(usage, "output_tokens")
            + usage_number(usage, "outputTokens")
            + usage_number(usage, "completion_tokens"),
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

#[allow(clippy::too_many_arguments)]
fn push_message(
    out: &mut Vec<MessageRecord>,
    source_file: &str,
    line_index: usize,
    project_name: &str,
    session_id: &str,
    cwd: &str,
    role: &str,
    content: String,
    model: String,
    timestamp: String,
    input_tokens: i64,
    output_tokens: i64,
) {
    if project_name.is_empty() || session_id.is_empty() || cwd.is_empty() {
        return;
    }

    out.push(MessageRecord {
        source: SourceKind::Pi,
        project_name: project_name.to_string(),
        project_path: cwd.to_string(),
        session_id: session_id.to_string(),
        role: role.to_string(),
        content,
        model,
        timestamp,
        is_subagent: false,
        msg_type: role.to_string(),
        input_tokens,
        output_tokens,
        source_file: source_file.to_string(),
        line_index,
    });
}
