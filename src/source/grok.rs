use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::types::{MessageRecord, SourceKind};

struct GrokSessionTask {
    session_dir: PathBuf,
    fallback_project_name: String,
    fallback_session_id: String,
}

#[derive(Default)]
struct GrokSummary {
    session_id: String,
    cwd: String,
    model: String,
    created_at: String,
}

struct PendingAssistantMessage {
    content: String,
    timestamp: String,
    line_index: usize,
    model: String,
}

pub fn load_grok_messages(home: &Path) -> Result<Vec<MessageRecord>> {
    let sessions_dir = home.join(".grok").join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();
    for project_entry in fs::read_dir(&sessions_dir)
        .with_context(|| format!("failed to read {}", sessions_dir.display()))?
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

        let fallback_project_name = project_entry
            .file_name()
            .to_str()
            .map(decode_percent_path)
            .unwrap_or_default();
        let project_dir = project_entry.path();

        for session_entry in fs::read_dir(&project_dir)
            .with_context(|| format!("failed to read {}", project_dir.display()))?
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

            let session_dir = session_entry.path();
            if !session_dir.join("updates.jsonl").exists() {
                continue;
            }

            tasks.push(GrokSessionTask {
                session_dir,
                fallback_project_name: fallback_project_name.clone(),
                fallback_session_id: session_entry.file_name().to_string_lossy().to_string(),
            });
        }
    }

    let nested = tasks
        .par_iter()
        .map(parse_grok_session)
        .collect::<Result<Vec<_>>>()?;

    Ok(nested.into_iter().flatten().collect())
}

fn parse_grok_session(task: &GrokSessionTask) -> Result<Vec<MessageRecord>> {
    let summary = read_summary(&task.session_dir);
    let session_id = first_non_empty(&summary.session_id, &task.fallback_session_id);
    let project_name = first_non_empty(&summary.cwd, &task.fallback_project_name);
    let mut current_model = summary.model.clone();
    let updates_path = task.session_dir.join("updates.jsonl");
    let content = fs::read_to_string(&updates_path).with_context(|| {
        format!(
            "failed to read Grok updates file {}",
            updates_path.display()
        )
    })?;

    let mut out = Vec::new();
    let mut pending_assistant: Option<PendingAssistantMessage> = None;
    let source_file = updates_path.display().to_string();

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let update = match value.get("params").and_then(|params| params.get("update")) {
            Some(update) => update,
            None => continue,
        };
        let update_type = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or("");

        match update_type {
            "user_message_chunk" => {
                flush_pending_assistant(
                    &mut out,
                    pending_assistant.take(),
                    &source_file,
                    &session_id,
                    &project_name,
                );

                if let Some(model) = update
                    .get("_meta")
                    .and_then(|meta| meta.get("modelId"))
                    .and_then(Value::as_str)
                    && !model.is_empty()
                {
                    current_model = model.to_string();
                }

                let text = update
                    .get("content")
                    .map(extract_text_content)
                    .unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }

                let timestamp = timestamp_from_update(&value)
                    .unwrap_or_else(|| first_non_empty(&summary.created_at, ""));
                push_message(
                    &mut out,
                    &source_file,
                    line_index,
                    &session_id,
                    &project_name,
                    "user",
                    text,
                    current_model.clone(),
                    timestamp,
                );
            }
            "agent_message_chunk" => {
                let text = update
                    .get("content")
                    .map(extract_text_content)
                    .unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }

                let timestamp = timestamp_from_update(&value)
                    .unwrap_or_else(|| first_non_empty(&summary.created_at, ""));
                match &mut pending_assistant {
                    Some(pending) => pending.content.push_str(&text),
                    None => {
                        pending_assistant = Some(PendingAssistantMessage {
                            content: text,
                            timestamp,
                            line_index,
                            model: current_model.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    flush_pending_assistant(
        &mut out,
        pending_assistant.take(),
        &source_file,
        &session_id,
        &project_name,
    );

    Ok(out)
}

fn read_summary(session_dir: &Path) -> GrokSummary {
    let summary_path = session_dir.join("summary.json");
    let Ok(content) = fs::read_to_string(summary_path) else {
        return GrokSummary::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return GrokSummary::default();
    };

    GrokSummary {
        session_id: value
            .get("info")
            .and_then(|info| info.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        cwd: value
            .get("info")
            .and_then(|info| info.get("cwd"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        model: value
            .get("current_model_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        created_at: value
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    }
}

fn flush_pending_assistant(
    out: &mut Vec<MessageRecord>,
    pending: Option<PendingAssistantMessage>,
    source_file: &str,
    session_id: &str,
    project_name: &str,
) {
    let Some(pending) = pending else {
        return;
    };

    push_message(
        out,
        source_file,
        pending.line_index,
        session_id,
        project_name,
        "assistant",
        pending.content,
        pending.model,
        pending.timestamp,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_message(
    out: &mut Vec<MessageRecord>,
    source_file: &str,
    line_index: usize,
    session_id: &str,
    project_name: &str,
    role: &str,
    content: String,
    model: String,
    timestamp: String,
) {
    if session_id.is_empty() || project_name.is_empty() || content.trim().is_empty() {
        return;
    }

    out.push(MessageRecord {
        source: SourceKind::Grok,
        project_name: project_name.to_string(),
        project_path: project_name.to_string(),
        session_id: session_id.to_string(),
        role: role.to_string(),
        content,
        model,
        timestamp,
        is_subagent: false,
        msg_type: role.to_string(),
        input_tokens: 0,
        output_tokens: 0,
        source_file: source_file.to_string(),
        line_index,
    });
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
            String::new()
        }
        _ => String::new(),
    }
}

fn timestamp_from_update(value: &Value) -> Option<String> {
    if let Some(milliseconds) = value
        .get("params")
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get("agentTimestampMs"))
        .and_then(value_to_i64)
    {
        return format_unix_timestamp_millis(milliseconds);
    }

    match value.get("timestamp") {
        Some(Value::String(timestamp)) if !timestamp.is_empty() => Some(timestamp.clone()),
        Some(number) => value_to_i64(number).and_then(|seconds| {
            seconds
                .checked_mul(1000)
                .and_then(format_unix_timestamp_millis)
        }),
        None => None,
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
        _ => None,
    }
}

fn format_unix_timestamp_millis(milliseconds: i64) -> Option<String> {
    let nanoseconds = i128::from(milliseconds).checked_mul(1_000_000)?;
    OffsetDateTime::from_unix_timestamp_nanos(nanoseconds)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

fn first_non_empty(first: &str, fallback: &str) -> String {
    if first.is_empty() {
        fallback.to_string()
    } else {
        first.to_string()
    }
}

fn decode_percent_path(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
