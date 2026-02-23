use anyhow::Result;
use std::path::Path;

use super::claude::ClaudeJsonlLine;

pub(crate) fn extract_text_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(obj) = item.as_object() {
                    match obj.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                                parts.push(text.to_string());
                            }
                        }
                        Some("thinking") => {
                            if let Some(text) = obj.get("thinking").and_then(|t| t.as_str()) {
                                parts.push(format!("[thinking] {}", text));
                            }
                        }
                        Some("tool_use") => {
                            let name = obj
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("unknown");
                            let input = obj.get("input").map(|i| i.to_string()).unwrap_or_default();
                            parts.push(format!("[tool_use: {}] {}", name, input));
                        }
                        Some("tool_result") => {
                            if let Some(text) = obj.get("content").and_then(|c| c.as_str()) {
                                parts.push(format!("[tool_result] {}", text));
                            }
                        }
                        _ => {}
                    }
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

pub(crate) fn extract_usage(usage: &serde_json::Value) -> (i64, i64) {
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    (input + cache_read, output)
}

pub fn decode_project_name(dir_name: &str) -> String {
    dir_name.to_string()
}

/// Extract the actual project path from JSONL session files by reading the `cwd`
/// field from the first parseable line that has one.
///
/// Claude Code's encoding (`replace(/[^a-zA-Z0-9]/g, "-")`) is lossy: `/`, `.`,
/// `-`, `_`, and spaces all map to `-`, making decoding from the dir name alone
/// impossible. Instead we read the ground-truth `cwd` from session data.
pub(crate) fn extract_project_path_from_sessions(project_dir: &Path) -> Option<String> {
    let mut entries: Vec<_> = std::fs::read_dir(project_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));

    for entry in entries {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<ClaudeJsonlLine>(line) {
                    if let Some(cwd) = parsed.cwd.as_deref() {
                        if !cwd.is_empty() {
                            return Some(cwd.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn collect_jsonl_recursive(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_recursive(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}
