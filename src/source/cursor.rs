use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde_json::Value;

use crate::types::{MessageRecord, SourceKind};

use super::decode_project_name;

struct CursorFileTask {
    path: std::path::PathBuf,
    project_name: String,
    session_id: String,
}

pub fn load_cursor_messages(home: &Path) -> Result<Vec<MessageRecord>> {
    let projects_dir = home.join(".cursor").join("projects");
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
        let transcripts_dir = project_entry.path().join("agent-transcripts");
        if !transcripts_dir.exists() {
            continue;
        }

        for session_entry in fs::read_dir(&transcripts_dir)
            .with_context(|| format!("failed to read {}", transcripts_dir.display()))?
        {
            let session_entry = match session_entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let is_dir = match session_entry.file_type() {
                Ok(file_type) => file_type.is_dir(),
                Err(_) => false,
            };
            if !is_dir {
                continue;
            }

            let session_id = session_entry.file_name().to_string_lossy().to_string();
            let session_dir = session_entry.path();

            for file_entry in fs::read_dir(&session_dir)
                .with_context(|| format!("failed to read {}", session_dir.display()))?
            {
                let file_entry = match file_entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };
                let path = file_entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                    files.push(CursorFileTask {
                        path,
                        project_name: project_name.clone(),
                        session_id: session_id.clone(),
                    });
                }
            }
        }
    }

    let nested = files
        .par_iter()
        .map(parse_cursor_file)
        .collect::<Result<Vec<_>>>()?;

    Ok(nested.into_iter().flatten().collect())
}

fn parse_cursor_file(task: &CursorFileTask) -> Result<Vec<MessageRecord>> {
    let content = fs::read_to_string(&task.path).with_context(|| {
        format!(
            "failed to read Cursor transcript file {}",
            task.path.display()
        )
    })?;

    let mut out = Vec::new();
    let source_file = task.path.display().to_string();
    let project_path = decode_project_name(&task.project_name);

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let role = value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if role != "user" && role != "assistant" {
            continue;
        }

        let content_text = value
            .get("message")
            .and_then(|m| m.get("content"))
            .map(extract_text_content)
            .unwrap_or_default();
        if content_text.trim().is_empty() {
            continue;
        }

        let model = value
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        out.push(MessageRecord {
            source: SourceKind::Cursor,
            project_name: task.project_name.clone(),
            project_path: project_path.clone(),
            session_id: task.session_id.clone(),
            role: role.clone(),
            content: content_text,
            model,
            timestamp,
            is_subagent: false,
            msg_type: role,
            input_tokens: 0,
            output_tokens: 0,
            source_file: source_file.clone(),
            line_index,
        });
    }

    Ok(out)
}

fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let obj = item.as_object()?;
                let item_type = obj.get("type").and_then(Value::as_str).unwrap_or("");
                if item_type == "text" {
                    obj.get("text").and_then(Value::as_str).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(inner) = map.get("content") {
                return extract_text_content(inner);
            }
            String::new()
        }
        _ => String::new(),
    }
}
