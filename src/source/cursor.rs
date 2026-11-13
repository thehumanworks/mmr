use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde_json::Value;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

use crate::types::{MessageRecord, SourceKind};

struct CursorFileTask {
    path: std::path::PathBuf,
    project_name: String,
    project_path: String,
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
        let project_path =
            decode_cursor_project_dir(&project_name).unwrap_or_else(|| project_name.clone());

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
                        project_path: project_path.clone(),
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
    let project_path = task.project_path.clone();
    // Cursor records carry no timestamp field. User turns start with a
    // `<timestamp>` tag; the records after one inherit it.
    let mut current_time = String::new();

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

        let mut content_text = value
            .get("message")
            .and_then(|m| m.get("content"))
            .map(extract_text_content)
            .unwrap_or_default();
        if role == "user" {
            let (stamp, body) = split_user_query(&content_text);
            if let Some(stamp) = stamp {
                current_time = stamp;
            }
            content_text = body;
        }
        if content_text.trim().is_empty() {
            continue;
        }

        let model = value
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let timestamp = match value.get("timestamp").and_then(Value::as_str) {
            Some(stamp) if !stamp.is_empty() => stamp.to_string(),
            _ => current_time.clone(),
        };

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

/// Cursor names a project directory after its path with each non-alphanumeric
/// character replaced by "-" and no leading "-" (`/Users/me/src/ledger-svc` ->
/// `Users-me-src-ledger-svc`). The mapping is lossy, so look for the directory
/// on disk that encodes to the same name. Returns None when none exists.
pub(crate) fn decode_cursor_project_dir(slug: &str) -> Option<String> {
    if slug.is_empty() || !slug.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let parts = slug.split('-').collect::<Vec<_>>();
    let found = walk_cursor_slug(Path::new("/"), &parts, 0)?;
    // Some callers build names replacing only "/" (keeping "."), so compare
    // both sides in Cursor's fully normalised form.
    (cursor_project_slug(&found) == cursor_project_slug(slug)).then_some(found)
}

/// The directory behind a provider's directory name for it: Claude
/// "-Users-me-proj", Cursor "Users-me-proj" or Pi "--Users-me-proj--".
pub(crate) fn decode_project_dir_name(name: &str) -> Option<String> {
    let slug = name
        .strip_suffix("--")
        .unwrap_or(name)
        .trim_start_matches('-');
    decode_cursor_project_dir(slug)
}

pub(crate) fn cursor_project_slug(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_start_matches('-')
        .to_string()
}

fn walk_cursor_slug(dir: &Path, parts: &[&str], depth: usize) -> Option<String> {
    if parts.is_empty() {
        return Some(dir.to_string_lossy().into_owned());
    }
    if depth > 64 {
        return None;
    }
    // Match the directory's real entries, so names that mix separators
    // ("mmr-check.KRB", ".tmp", "my_app v2") decode as well as plain ones.
    let mut matches = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let take = slug_parts_matched(&name, parts)?;
            entry.path().is_dir().then_some((take, name))
        })
        .collect::<Vec<_>>();
    // Longest match first, then by name, so the walk is deterministic.
    matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    matches
        .into_iter()
        .find_map(|(take, name)| walk_cursor_slug(&dir.join(name), &parts[take..], depth + 1))
}

/// How many of the slug's "-"-separated `parts` the directory `name` accounts
/// for, when its slug is a prefix of them. Cursor and Claude replace every
/// character but letters and digits (".claude" is the parts `["", "claude"]`);
/// some callers replace only "/" (".claude" stays one part).
fn slug_parts_matched(name: &str, parts: &[&str]) -> Option<usize> {
    let normalised = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    [normalised.as_str(), name]
        .into_iter()
        .map(|slug| slug.split('-').collect::<Vec<_>>())
        .filter(|own| own.len() <= parts.len() && own[..] == parts[..own.len()])
        .map(|own| own.len())
        .max()
}

/// Split Cursor's `<timestamp>…</timestamp>\n<user_query>\n…\n</user_query>`
/// wrapper into an RFC 3339 UTC timestamp and the user's text.
fn split_user_query(text: &str) -> (Option<String>, String) {
    let Some(rest) = text.trim_start().strip_prefix("<timestamp>") else {
        return (None, text.to_string());
    };
    let Some((stamp, rest)) = rest.split_once("</timestamp>") else {
        return (None, text.to_string());
    };
    let rest = rest.trim();
    let body = rest
        .strip_prefix("<user_query>")
        .and_then(|inner| inner.strip_suffix("</user_query>"))
        .map(str::trim)
        .unwrap_or(rest);
    (parse_cursor_timestamp(stamp), body.to_string())
}

/// Parse "Friday, Oct 16, 2026, 11:00 AM (UTC+1)".
fn parse_cursor_timestamp(stamp: &str) -> Option<String> {
    let (local, zone) = stamp.trim().split_once(" (UTC")?;
    let zone = zone.strip_suffix(')')?;
    let offset = if zone.is_empty() {
        UtcOffset::UTC
    } else {
        let sign: i8 = if zone.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = zone[1..].split_once(':').unwrap_or((&zone[1..], "0"));
        UtcOffset::from_hms(
            sign * hours.parse::<i8>().ok()?,
            sign * minutes.parse::<i8>().ok()?,
            0,
        )
        .ok()?
    };
    let format = format_description!(
        "[weekday repr:long], [month repr:short] [day padding:none], [year], [hour repr:12 padding:none]:[minute] [period]"
    );
    let local = PrimitiveDateTime::parse(local.trim(), &format).ok()?;
    Some(format_utc(local.assume_offset(offset)))
}

fn format_utc(value: OffsetDateTime) -> String {
    let format =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    value
        .to_offset(UtcOffset::UTC)
        .format(&format)
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_paths_that_mix_separators_and_dot_folders() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let base = tmp
            .path()
            .canonicalize()
            .expect("canonical")
            .join("mmr-check.KRB/.tmp1");
        let project = base.join("home/src/ledger-svc");
        let worktree = project.join(".claude/worktrees/dedupe-redis");
        fs::create_dir_all(&worktree).expect("worktree");
        fs::create_dir_all(base.join("home/src/ledger-svc-old")).expect("sibling");
        let project = project.display().to_string();
        let worktree = worktree.display().to_string();

        // Cursor and Claude replace every character but letters and digits.
        assert_eq!(
            decode_cursor_project_dir(&cursor_project_slug(&project)),
            Some(project.clone())
        );
        assert_eq!(
            decode_project_dir_name(&format!("-{}", cursor_project_slug(&worktree))),
            Some(worktree)
        );
        // Some callers replace only "/" and keep the dots.
        let slash_only = project.trim_start_matches('/').replace('/', "-");
        assert_eq!(decode_cursor_project_dir(&slash_only), Some(project));
    }
}
