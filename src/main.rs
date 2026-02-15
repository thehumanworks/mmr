use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    response::Html,
    routing::get,
    Json, Router,
};
use clap::{Parser, Subcommand};
use duckdb::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

// --- Claude JSONL Schema Types ---

#[derive(Deserialize, Debug)]
struct ClaudeJsonlLine {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    message: Option<ClaudeMessagePayload>,
    timestamp: Option<String>,
    uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    parent_uuid: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    slug: Option<String>,
    version: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ClaudeMessagePayload {
    role: Option<String>,
    content: Option<serde_json::Value>,
    model: Option<String>,
    usage: Option<serde_json::Value>,
}

// --- DuckDB Setup ---

fn load_fts(conn: &Connection) -> Result<()> {
    // Prefer LOAD (fast, typically no network). Fall back to INSTALL+LOAD if needed.
    if conn.execute_batch("LOAD fts;").is_ok() {
        return Ok(());
    }
    conn.execute_batch("INSTALL fts; LOAD fts;")?;
    Ok(())
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS messages (
            id              INTEGER PRIMARY KEY,
            source          VARCHAR NOT NULL DEFAULT 'claude',
            project         VARCHAR NOT NULL,
            project_path    VARCHAR NOT NULL,
            session_id      VARCHAR NOT NULL,
            is_subagent     BOOLEAN DEFAULT FALSE,
            message_uuid    VARCHAR,
            parent_uuid     VARCHAR,
            msg_type        VARCHAR,
            role            VARCHAR,
            content_text    VARCHAR,
            model           VARCHAR,
            timestamp       VARCHAR,
            cwd             VARCHAR,
            git_branch      VARCHAR,
            slug            VARCHAR,
            version         VARCHAR,
            input_tokens    BIGINT,
            output_tokens   BIGINT,
            source_file     VARCHAR DEFAULT '',
            source_offset   BIGINT DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS projects (
            name            VARCHAR NOT NULL,
            source          VARCHAR NOT NULL DEFAULT 'claude',
            original_path   VARCHAR NOT NULL,
            session_count   INTEGER DEFAULT 0,
            message_count   INTEGER DEFAULT 0,
            last_activity   VARCHAR DEFAULT '',
            PRIMARY KEY (name, source)
        );

        CREATE TABLE IF NOT EXISTS sessions (
            session_id      VARCHAR NOT NULL,
            project         VARCHAR NOT NULL,
            source          VARCHAR NOT NULL DEFAULT 'claude',
            first_timestamp VARCHAR,
            last_timestamp  VARCHAR,
            message_count   INTEGER DEFAULT 0,
            user_messages   INTEGER DEFAULT 0,
            assistant_messages INTEGER DEFAULT 0,
            PRIMARY KEY (session_id, project, source)
        );

        -- Key/value metadata for on-disk CLI cache (safe to exist in server/in-memory DB too).
        CREATE TABLE IF NOT EXISTS cache_meta (
            key   VARCHAR PRIMARY KEY,
            value VARCHAR NOT NULL
        );

        -- Per-file incremental checkpoints. These rows are used to ingest only
        -- appended data on each refresh.
        CREATE TABLE IF NOT EXISTS ingest_files (
            source                  VARCHAR NOT NULL,
            file_path               VARCHAR NOT NULL,
            project                 VARCHAR NOT NULL,
            project_path            VARCHAR NOT NULL,
            session_id              VARCHAR NOT NULL,
            is_subagent             BOOLEAN DEFAULT FALSE,
            last_offset             BIGINT DEFAULT 0,
            file_size               BIGINT DEFAULT 0,
            file_mtime_unix         BIGINT DEFAULT 0,
            last_ingested_unix      BIGINT DEFAULT 0,
            last_message_timestamp  VARCHAR DEFAULT '',
            last_message_key        VARCHAR DEFAULT '',
            meta_model              VARCHAR DEFAULT '',
            meta_git_branch         VARCHAR DEFAULT '',
            meta_version            VARCHAR DEFAULT '',
            PRIMARY KEY (source, file_path)
        );

        -- Per-project watermark metadata across sources.
        CREATE TABLE IF NOT EXISTS ingest_projects (
            source              VARCHAR NOT NULL,
            project             VARCHAR NOT NULL,
            project_path        VARCHAR NOT NULL,
            first_seen_unix     BIGINT DEFAULT 0,
            last_seen_unix      BIGINT DEFAULT 0,
            last_ingested_unix  BIGINT DEFAULT 0,
            PRIMARY KEY (source, project)
        );

        -- Per-session latest message watermark.
        CREATE TABLE IF NOT EXISTS ingest_sessions (
            source                  VARCHAR NOT NULL,
            project                 VARCHAR NOT NULL,
            project_path            VARCHAR NOT NULL,
            session_id              VARCHAR NOT NULL,
            last_message_timestamp  VARCHAR DEFAULT '',
            last_message_key        VARCHAR DEFAULT '',
            last_ingested_unix      BIGINT DEFAULT 0,
            PRIMARY KEY (source, session_id)
        );
        ",
    )?;

    // Lightweight schema migrations for caches created by older versions.
    conn.execute_batch(
        "
        ALTER TABLE messages ADD COLUMN IF NOT EXISTS source_file VARCHAR DEFAULT '';
        ALTER TABLE messages ADD COLUMN IF NOT EXISTS source_offset BIGINT DEFAULT 0;
        ",
    )?;

    Ok(())
}

fn init_db(conn: &Connection) -> Result<()> {
    load_fts(conn)?;
    ensure_schema(conn)?;
    Ok(())
}

fn create_fts_index(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("DROP INDEX IF EXISTS fts_idx;");
    conn.execute_batch(
        "PRAGMA create_fts_index('messages', 'id', 'content_text', 'role', 'project', 'msg_type', 'source', overwrite=1);"
    )?;
    Ok(())
}

// --- Claude JSONL Parsing ---

fn extract_text_content(content: &serde_json::Value) -> String {
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

fn extract_usage(usage: &serde_json::Value) -> (i64, i64) {
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

fn decode_project_name(dir_name: &str) -> String {
    dir_name.to_string()
}

/// Extract the actual project path from JSONL session files by reading the `cwd`
/// field from the first parseable line that has one.
///
/// Claude Code's encoding (`replace(/[^a-zA-Z0-9]/g, "-")`) is lossy: `/`, `.`,
/// `-`, `_`, and spaces all map to `-`, making decoding from the dir name alone
/// impossible. Instead we read the ground-truth `cwd` from session data.
fn extract_project_path_from_sessions(project_dir: &Path) -> Option<String> {
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

fn ingest_claude_jsonl_file(
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

fn ingest_claude(conn: &Connection, id_counter: &mut i64) -> Result<(usize, usize, usize)> {
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
            .unwrap_or_else(|| decode_project_name(&project_dir_name));
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

                // Check for subagents directory
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

// --- Codex JSONL Parsing ---

/// Parse a single Codex session JSONL file.
/// Returns (session_id, cwd, message_count).
fn ingest_codex_jsonl_file(
    conn: &Connection,
    path: &Path,
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

                        // Use cwd as both project name and path for codex
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
                        // Extract output_text from content array
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
    let _ = session_timestamp; // used via individual message timestamps
    Ok(Some((session_id, cwd, count)))
}

fn ingest_codex(conn: &Connection, id_counter: &mut i64) -> Result<(usize, usize, usize)> {
    let home = dirs::home_dir().context("No home directory")?;
    let codex_dir = home.join(".codex");

    if !codex_dir.exists() {
        tracing::warn!("Codex directory not found at {}", codex_dir.display());
        return Ok((0, 0, 0));
    }

    // Collect all JSONL files from sessions/ (recursive) and archived_sessions/
    let mut jsonl_files = Vec::new();

    let sessions_dir = codex_dir.join("sessions");
    if sessions_dir.exists() {
        collect_jsonl_recursive(&sessions_dir, &mut jsonl_files)?;
    }

    let archived_dir = codex_dir.join("archived_sessions");
    if archived_dir.exists() {
        collect_jsonl_recursive(&archived_dir, &mut jsonl_files)?;
    }

    // Track per-project stats: cwd -> (sessions, messages)
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

fn collect_jsonl_recursive(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
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

#[derive(Default)]
struct IncrementalRefreshStats {
    inserted_messages: usize,
    removed_messages: usize,
    changed_files: usize,
    unchanged_files: usize,
}

#[derive(Clone, Debug)]
struct IngestFileState {
    project: String,
    project_path: String,
    session_id: String,
    is_subagent: bool,
    last_offset: i64,
    file_size: i64,
    file_mtime_unix: i64,
    last_message_timestamp: String,
    last_message_key: String,
    meta_model: String,
    meta_git_branch: String,
    meta_version: String,
}

#[derive(Clone, Debug, Default)]
struct CodexSessionMeta {
    session_id: String,
    cwd: String,
    model_provider: String,
    git_branch: String,
    cli_version: String,
}

#[derive(Default)]
struct FileIngestOutcome {
    inserted_messages: usize,
    final_offset: u64,
    last_message_timestamp: String,
    last_message_key: String,
}

#[derive(Default)]
struct ClaudeFileIngestOutcome {
    base: FileIngestOutcome,
    session_id: String,
}

#[derive(Default)]
struct CodexFileIngestOutcome {
    base: FileIngestOutcome,
    meta: CodexSessionMeta,
}

enum FileRefreshMode {
    Skip,
    Append(u64),
    Rewrite,
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn metadata_mtime_unix(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn file_set_key(source: &str, file_path: &str) -> String {
    format!("{source}\t{file_path}")
}

fn load_ingest_file_state(
    conn: &Connection,
    source: &str,
    file_path: &str,
) -> Result<Option<IngestFileState>> {
    let mut stmt = conn.prepare(
        "SELECT project, project_path, session_id, is_subagent, last_offset, file_size, file_mtime_unix,
                last_message_timestamp, last_message_key, meta_model, meta_git_branch, meta_version
         FROM ingest_files
         WHERE source = ? AND file_path = ?",
    )?;
    let mut rows = stmt.query(params![source, file_path])?;
    if let Some(row) = rows.next()? {
        Ok(Some(IngestFileState {
            project: row.get(0)?,
            project_path: row.get(1)?,
            session_id: row.get(2)?,
            is_subagent: row.get(3)?,
            last_offset: row.get(4)?,
            file_size: row.get(5)?,
            file_mtime_unix: row.get(6)?,
            last_message_timestamp: row.get(7)?,
            last_message_key: row.get(8)?,
            meta_model: row.get(9)?,
            meta_git_branch: row.get(10)?,
            meta_version: row.get(11)?,
        }))
    } else {
        Ok(None)
    }
}

fn upsert_ingest_file_state(
    conn: &Connection,
    source: &str,
    file_path: &str,
    state: &IngestFileState,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ingest_files (
            source, file_path, project, project_path, session_id, is_subagent,
            last_offset, file_size, file_mtime_unix, last_ingested_unix,
            last_message_timestamp, last_message_key, meta_model, meta_git_branch, meta_version
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (source, file_path) DO UPDATE SET
            project = excluded.project,
            project_path = excluded.project_path,
            session_id = excluded.session_id,
            is_subagent = excluded.is_subagent,
            last_offset = excluded.last_offset,
            file_size = excluded.file_size,
            file_mtime_unix = excluded.file_mtime_unix,
            last_ingested_unix = excluded.last_ingested_unix,
            last_message_timestamp = excluded.last_message_timestamp,
            last_message_key = excluded.last_message_key,
            meta_model = excluded.meta_model,
            meta_git_branch = excluded.meta_git_branch,
            meta_version = excluded.meta_version",
        params![
            source,
            file_path,
            &state.project,
            &state.project_path,
            &state.session_id,
            state.is_subagent,
            state.last_offset,
            state.file_size,
            state.file_mtime_unix,
            now_unix_secs(),
            &state.last_message_timestamp,
            &state.last_message_key,
            &state.meta_model,
            &state.meta_git_branch,
            &state.meta_version,
        ],
    )?;
    Ok(())
}

fn upsert_ingest_project(
    conn: &Connection,
    source: &str,
    project: &str,
    project_path: &str,
) -> Result<()> {
    let now = now_unix_secs();
    conn.execute(
        "INSERT INTO ingest_projects (
            source, project, project_path, first_seen_unix, last_seen_unix, last_ingested_unix
         ) VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (source, project) DO UPDATE SET
            project_path = excluded.project_path,
            last_seen_unix = excluded.last_seen_unix,
            last_ingested_unix = excluded.last_ingested_unix",
        params![source, project, project_path, now, now, now],
    )?;
    Ok(())
}

fn decide_file_refresh_mode(
    existing: Option<&IngestFileState>,
    file_size: u64,
    file_mtime: i64,
) -> FileRefreshMode {
    let Some(state) = existing else {
        return FileRefreshMode::Append(0);
    };

    if file_size as i64 == state.file_size && file_mtime == state.file_mtime_unix {
        return FileRefreshMode::Skip;
    }

    if file_size > state.last_offset as u64
        && file_size >= state.file_size as u64
        && file_mtime >= state.file_mtime_unix
    {
        return FileRefreshMode::Append(state.last_offset as u64);
    }

    FileRefreshMode::Rewrite
}

fn delete_messages_for_file(conn: &Connection, source: &str, file_path: &str) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE source = ? AND source_file = ?",
        params![source, file_path],
        |row| row.get(0),
    )?;
    if count > 0 {
        conn.execute(
            "DELETE FROM messages WHERE source = ? AND source_file = ?",
            params![source, file_path],
        )?;
    }
    Ok(count as usize)
}

fn ingest_claude_jsonl_file_from_offset(
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

fn probe_codex_session_meta(path: &Path) -> Result<CodexSessionMeta> {
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

fn ingest_codex_jsonl_file_from_offset(
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

fn process_claude_file_incremental(
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

fn process_codex_file_incremental(
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

fn cleanup_removed_files(
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

fn rebuild_derived_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        DELETE FROM sessions;
        DELETE FROM projects;
        DELETE FROM ingest_sessions;

        INSERT INTO sessions (
            session_id, project, source, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages
        )
        SELECT
            session_id,
            project,
            source,
            MIN(timestamp) as first_timestamp,
            MAX(timestamp) as last_timestamp,
            COUNT(*) as message_count,
            SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END) as user_messages,
            SUM(CASE WHEN role = 'assistant' THEN 1 ELSE 0 END) as assistant_messages
        FROM messages
        WHERE is_subagent = FALSE
        GROUP BY session_id, project, source;

        INSERT INTO projects (
            name, source, original_path, session_count, message_count, last_activity
        )
        SELECT
            m.project as name,
            m.source as source,
            MAX(m.project_path) as original_path,
            (
                SELECT COUNT(*) FROM sessions s
                WHERE s.project = m.project AND s.source = m.source
            ) as session_count,
            COUNT(*) as message_count,
            COALESCE((
                SELECT MAX(s.last_timestamp) FROM sessions s
                WHERE s.project = m.project AND s.source = m.source
            ), '') as last_activity
        FROM messages m
        GROUP BY m.project, m.source;
        ",
    )?;

    conn.execute(
        "INSERT INTO ingest_sessions (
            source, project, project_path, session_id, last_message_timestamp, last_message_key, last_ingested_unix
         )
         SELECT
            source,
            project,
            MAX(project_path),
            session_id,
            MAX(timestamp) as last_message_timestamp,
            COALESCE(MAX(NULLIF(message_uuid, '')), CAST(MAX(source_offset) AS VARCHAR)) as last_message_key,
            ?
         FROM messages
         GROUP BY source, project, session_id",
        params![now_unix_secs()],
    )?;

    Ok(())
}

fn count_for_source(conn: &Connection, table: &str, source: &str) -> Result<usize> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE source = ?");
    let n: i64 = conn.query_row(&sql, params![source], |row| row.get(0))?;
    Ok(n as usize)
}

fn compute_ingest_stats(conn: &Connection) -> Result<IngestStats> {
    Ok(IngestStats {
        claude_projects: count_for_source(conn, "projects", "claude")?,
        claude_sessions: count_for_source(conn, "sessions", "claude")?,
        claude_messages: count_for_source(conn, "messages", "claude")?,
        codex_projects: count_for_source(conn, "projects", "codex")?,
        codex_sessions: count_for_source(conn, "sessions", "codex")?,
        codex_messages: count_for_source(conn, "messages", "codex")?,
    })
}

fn refresh_incremental_cache(conn: &Connection, quiet: bool) -> Result<IngestStats> {
    let mut id_counter: i64 =
        conn.query_row("SELECT COALESCE(MAX(id), 0) FROM messages", [], |row| {
            row.get(0)
        })?;

    let mut refresh_stats = IncrementalRefreshStats::default();
    let mut seen_files = HashSet::new();

    let claude_dir = dirs::home_dir()
        .context("No home directory")?
        .join(".claude")
        .join("projects");
    if claude_dir.exists() {
        let mut project_entries: Vec<_> = std::fs::read_dir(&claude_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|f| f.is_dir()).unwrap_or(false))
            .collect();
        project_entries.sort_by_key(|e| e.file_name());

        for entry in project_entries {
            let project_dir_name = entry.file_name().to_string_lossy().to_string();
            let project_path = extract_project_path_from_sessions(&entry.path())
                .unwrap_or_else(|| decode_project_name(&project_dir_name));

            let mut session_entries: Vec<_> = std::fs::read_dir(entry.path())?
                .filter_map(|e| e.ok())
                .collect();
            session_entries.sort_by_key(|e| e.file_name());

            for session_entry in session_entries {
                let session_path = session_entry.path();
                if session_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                process_claude_file_incremental(
                    conn,
                    &session_path,
                    &project_dir_name,
                    &project_path,
                    false,
                    &mut id_counter,
                    &mut seen_files,
                    &mut refresh_stats,
                )?;

                let session_id = session_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let subagents_dir = entry.path().join(&session_id).join("subagents");
                if subagents_dir.exists() {
                    let mut sub_entries: Vec<_> = std::fs::read_dir(&subagents_dir)?
                        .filter_map(|e| e.ok())
                        .collect();
                    sub_entries.sort_by_key(|e| e.file_name());
                    for sub_entry in sub_entries {
                        let sub_path = sub_entry.path();
                        if sub_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                            continue;
                        }
                        process_claude_file_incremental(
                            conn,
                            &sub_path,
                            &project_dir_name,
                            &project_path,
                            true,
                            &mut id_counter,
                            &mut seen_files,
                            &mut refresh_stats,
                        )?;
                    }
                }
            }
        }
    }

    let codex_dir = dirs::home_dir()
        .context("No home directory")?
        .join(".codex");
    if codex_dir.exists() {
        let mut codex_files = Vec::new();
        let sessions_dir = codex_dir.join("sessions");
        if sessions_dir.exists() {
            collect_jsonl_recursive(&sessions_dir, &mut codex_files)?;
        }
        let archived_dir = codex_dir.join("archived_sessions");
        if archived_dir.exists() {
            collect_jsonl_recursive(&archived_dir, &mut codex_files)?;
        }
        codex_files.sort();
        for file_path in codex_files {
            process_codex_file_incremental(
                conn,
                &file_path,
                &mut id_counter,
                &mut seen_files,
                &mut refresh_stats,
            )?;
        }
    }

    cleanup_removed_files(conn, &seen_files, &mut refresh_stats)?;

    let has_changes = refresh_stats.changed_files > 0;
    if has_changes {
        rebuild_derived_tables(conn)?;
        let message_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        if message_count > 0 {
            create_fts_index(conn)?;
        }
    }

    let projects_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;
    let sessions_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
    if projects_count == 0 && sessions_count == 0 {
        rebuild_derived_tables(conn)?;
        let message_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        if message_count > 0 {
            create_fts_index(conn)?;
        }
    }

    let stats = compute_ingest_stats(conn)?;
    write_cache_meta(conn, &stats)?;

    if !quiet {
        eprintln!(
            "Incremental refresh: +{} messages, -{} messages, {} changed files ({} unchanged)",
            refresh_stats.inserted_messages,
            refresh_stats.removed_messages,
            refresh_stats.changed_files,
            refresh_stats.unchanged_files
        );
    }

    Ok(stats)
}

// --- Combined Ingestion ---

struct IngestStats {
    claude_projects: usize,
    claude_sessions: usize,
    claude_messages: usize,
    codex_projects: usize,
    codex_sessions: usize,
    codex_messages: usize,
}

fn ingest_all(conn: &Connection) -> Result<IngestStats> {
    let mut id_counter: i64 = 0;

    let (cp, cs, cm) = ingest_claude(conn, &mut id_counter)?;
    let (xp, xs, xm) = ingest_codex(conn, &mut id_counter)?;

    // Populate sessions table from messages
    conn.execute_batch(
        "
        INSERT INTO sessions (session_id, project, source, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages)
        SELECT
            session_id,
            project,
            source,
            MIN(timestamp) as first_timestamp,
            MAX(timestamp) as last_timestamp,
            COUNT(*) as message_count,
            SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END) as user_messages,
            SUM(CASE WHEN role = 'assistant' THEN 1 ELSE 0 END) as assistant_messages
        FROM messages
        WHERE is_subagent = FALSE
        GROUP BY session_id, project, source;
        ",
    )?;

    // Update projects.last_activity from the most recent session timestamp
    conn.execute_batch(
        "
        UPDATE projects SET last_activity = (
            SELECT MAX(s.last_timestamp)
            FROM sessions s
            WHERE s.project = projects.name AND s.source = projects.source
        );
        ",
    )?;

    Ok(IngestStats {
        claude_projects: cp,
        claude_sessions: cs,
        claude_messages: cm,
        codex_projects: xp,
        codex_sessions: xs,
        codex_messages: xm,
    })
}

const CACHE_SCHEMA_VERSION: &str = "2";

fn write_cache_meta(conn: &Connection, stats: &IngestStats) -> Result<()> {
    let refreshed_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    conn.execute("DELETE FROM cache_meta", [])?;
    conn.execute(
        "INSERT INTO cache_meta (key, value) VALUES ('schema_version', ?)",
        params![CACHE_SCHEMA_VERSION],
    )?;
    conn.execute(
        "INSERT INTO cache_meta (key, value) VALUES ('refreshed_at_unix', ?)",
        params![refreshed_at_unix],
    )?;
    conn.execute(
        "INSERT INTO cache_meta (key, value) VALUES ('claude_messages', ?)",
        params![stats.claude_messages.to_string()],
    )?;
    conn.execute(
        "INSERT INTO cache_meta (key, value) VALUES ('codex_messages', ?)",
        params![stats.codex_messages.to_string()],
    )?;
    Ok(())
}

// --- Web Server ---

type AppState = Arc<Mutex<Connection>>;

#[derive(Deserialize, utoipa::IntoParams)]
struct ProjectQuery {
    name: Option<String>,
    source: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
struct MessageQuery {
    session: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
struct SearchParams {
    q: Option<String>,
    project: Option<String>,
    source: Option<String>,
    page: Option<usize>,
}

// --- API Response Types ---

#[derive(Serialize, utoipa::ToSchema)]
struct ApiProject {
    name: String,
    source: String,
    original_path: String,
    session_count: i32,
    message_count: i32,
    last_activity: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiProjectsResponse {
    projects: Vec<ApiProject>,
    total_messages: i64,
    total_sessions: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiSession {
    session_id: String,
    first_timestamp: String,
    last_timestamp: String,
    message_count: i32,
    user_messages: i32,
    assistant_messages: i32,
    preview: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiSessionsResponse {
    project_name: String,
    project_path: String,
    source: String,
    sessions: Vec<ApiSession>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiMessage {
    role: String,
    content: String,
    model: String,
    timestamp: String,
    is_subagent: bool,
    msg_type: String,
    input_tokens: i64,
    output_tokens: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiMessagesResponse {
    session_id: String,
    project_name: String,
    project_path: String,
    source: String,
    messages: Vec<ApiMessage>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiSearchResult {
    id: i64,
    project: String,
    project_path: String,
    session_id: String,
    role: String,
    content: String,
    model: String,
    timestamp: String,
    is_subagent: bool,
    source: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiSearchResponse {
    query: String,
    total_count: usize,
    page: usize,
    per_page: usize,
    results: Vec<ApiSearchResult>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiSourceStats {
    source: String,
    message_count: i64,
    session_count: i64,
    project_count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiModelStats {
    source: String,
    model: String,
    message_count: i64,
    input_tokens: i64,
    output_tokens: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiProjectStats {
    source: String,
    project_path: String,
    total_messages: i64,
    user_messages: i64,
    assistant_messages: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ApiAnalyticsResponse {
    source_stats: Vec<ApiSourceStats>,
    model_stats: Vec<ApiModelStats>,
    project_stats: Vec<ApiProjectStats>,
}

// --- SPA Handler ---

async fn spa_handler() -> Html<String> {
    Html(SPA_HTML.to_string())
}

// Search row type now includes source as the 10th field
type SearchRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    String,
);

#[allow(clippy::type_complexity)]
fn run_search(
    conn: &Connection,
    query: &str,
    project_filter: &str,
    source_filter: &str,
    per_page: usize,
    offset: usize,
) -> (usize, Vec<SearchRow>) {
    if let Ok(result) = try_fts_search(conn, query, project_filter, source_filter, per_page, offset)
    {
        return result;
    }
    like_search(conn, query, project_filter, source_filter, per_page, offset)
}

#[allow(clippy::type_complexity)]
fn try_fts_search(
    conn: &Connection,
    query: &str,
    project_filter: &str,
    source_filter: &str,
    per_page: usize,
    offset: usize,
) -> Result<(usize, Vec<SearchRow>)> {
    // Build WHERE clauses dynamically
    let mut where_clauses = vec!["score IS NOT NULL".to_string()];
    let mut bind_values: Vec<String> = vec![query.to_string()]; // first bind is always the FTS query

    if !project_filter.is_empty() {
        where_clauses.push("project = ?".to_string());
        bind_values.push(project_filter.to_string());
    }
    if source_filter == "claude" || source_filter == "codex" {
        where_clauses.push("source = ?".to_string());
        bind_values.push(source_filter.to_string());
    }

    let where_str = where_clauses.join(" AND ");

    let count_sql = format!(
        "SELECT COUNT(*) FROM (SELECT *, fts_main_messages.match_bm25(id, ?) AS score FROM messages) t WHERE {}",
        where_str
    );
    let search_sql = format!(
        "SELECT id, project, project_path, session_id, role, content_text, model, timestamp, is_subagent, source
         FROM (SELECT *, fts_main_messages.match_bm25(id, ?) AS score FROM messages) t
         WHERE {}
         ORDER BY score DESC
         LIMIT {} OFFSET {}",
        where_str, per_page, offset
    );

    let count: i64 = match bind_values.len() {
        1 => conn.query_row(&count_sql, params![bind_values[0]], |row| row.get(0))?,
        2 => conn.query_row(&count_sql, params![bind_values[0], bind_values[1]], |row| {
            row.get(0)
        })?,
        3 => conn.query_row(
            &count_sql,
            params![bind_values[0], bind_values[1], bind_values[2]],
            |row| row.get(0),
        )?,
        _ => unreachable!(),
    };

    let mut stmt = conn.prepare(&search_sql)?;
    let rows: Vec<SearchRow> = match bind_values.len() {
        1 => stmt
            .query_map(params![bind_values[0]], map_search_row)?
            .filter_map(|r| r.ok())
            .collect(),
        2 => stmt
            .query_map(params![bind_values[0], bind_values[1]], map_search_row)?
            .filter_map(|r| r.ok())
            .collect(),
        3 => stmt
            .query_map(
                params![bind_values[0], bind_values[1], bind_values[2]],
                map_search_row,
            )?
            .filter_map(|r| r.ok())
            .collect(),
        _ => unreachable!(),
    };

    Ok((count as usize, rows))
}

#[allow(clippy::type_complexity)]
fn like_search(
    conn: &Connection,
    query: &str,
    project_filter: &str,
    source_filter: &str,
    per_page: usize,
    offset: usize,
) -> (usize, Vec<SearchRow>) {
    let mut where_clauses = vec!["content_text LIKE '%' || ? || '%'".to_string()];
    let mut bind_values: Vec<String> = vec![query.to_string()];

    if !project_filter.is_empty() {
        where_clauses.push("project = ?".to_string());
        bind_values.push(project_filter.to_string());
    }
    if source_filter == "claude" || source_filter == "codex" {
        where_clauses.push("source = ?".to_string());
        bind_values.push(source_filter.to_string());
    }

    let where_str = where_clauses.join(" AND ");

    let count_sql = format!("SELECT COUNT(*) FROM messages WHERE {}", where_str);
    let search_sql = format!(
        "SELECT id, project, project_path, session_id, role, content_text, model, timestamp, is_subagent, source
         FROM messages WHERE {}
         ORDER BY timestamp DESC
         LIMIT {} OFFSET {}",
        where_str, per_page, offset
    );

    let count: i64 = match bind_values.len() {
        1 => conn
            .query_row(&count_sql, params![bind_values[0]], |row| row.get(0))
            .unwrap_or(0),
        2 => conn
            .query_row(&count_sql, params![bind_values[0], bind_values[1]], |row| {
                row.get(0)
            })
            .unwrap_or(0),
        3 => conn
            .query_row(
                &count_sql,
                params![bind_values[0], bind_values[1], bind_values[2]],
                |row| row.get(0),
            )
            .unwrap_or(0),
        _ => unreachable!(),
    };

    let mut stmt = conn.prepare(&search_sql).unwrap();
    let rows: Vec<SearchRow> = match bind_values.len() {
        1 => stmt
            .query_map(params![bind_values[0]], map_search_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect(),
        2 => stmt
            .query_map(params![bind_values[0], bind_values[1]], map_search_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect(),
        3 => stmt
            .query_map(
                params![bind_values[0], bind_values[1], bind_values[2]],
                map_search_row,
            )
            .unwrap()
            .filter_map(|r| r.ok())
            .collect(),
        _ => unreachable!(),
    };

    (count as usize, rows)
}

fn map_search_row(row: &duckdb::Row) -> duckdb::Result<SearchRow> {
    Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, String>(6)?,
        row.get::<_, String>(7)?,
        row.get::<_, bool>(8)?,
        row.get::<_, String>(9)?,
    ))
}

// --- JSON API Handlers ---

#[utoipa::path(
    get,
    path = "/api/projects",
    responses((status = 200, body = ApiProjectsResponse)),
    tag = "projects"
)]
async fn api_projects(State(db): State<AppState>) -> Json<ApiProjectsResponse> {
    let conn = db.lock().unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT name, source, original_path, session_count, message_count, last_activity FROM projects ORDER BY last_activity DESC",
        )
        .unwrap();
    let projects: Vec<ApiProject> = stmt
        .query_map([], |row| {
            Ok(ApiProject {
                name: row.get::<_, String>(0)?,
                source: row.get::<_, String>(1)?,
                original_path: row.get::<_, String>(2)?,
                session_count: row.get::<_, i32>(3)?,
                message_count: row.get::<_, i32>(4)?,
                last_activity: row.get::<_, String>(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    let total_messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .unwrap_or(0);
    let total_sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap_or(0);

    Json(ApiProjectsResponse {
        projects,
        total_messages,
        total_sessions,
    })
}

#[utoipa::path(
    get,
    path = "/api/sessions",
    params(ProjectQuery),
    responses((status = 200, body = ApiSessionsResponse)),
    tag = "sessions"
)]
async fn api_sessions(
    State(db): State<AppState>,
    Query(params): Query<ProjectQuery>,
) -> Json<ApiSessionsResponse> {
    let conn = db.lock().unwrap();
    let project_name = params.name.as_deref().unwrap_or("");
    let source = params.source.as_deref().unwrap_or("codex");

    let project_path: String = conn
        .query_row(
            "SELECT original_path FROM projects WHERE name = ? AND source = ?",
            params![project_name, source],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| project_name.to_string());

    let mut stmt = conn
        .prepare(
            "SELECT session_id, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages
             FROM sessions WHERE project = ? AND source = ? ORDER BY last_timestamp DESC",
        )
        .unwrap();

    let sessions: Vec<ApiSession> = stmt
        .query_map(params![project_name, source], |row| {
            let sid: String = row.get(0)?;
            Ok((sid, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i32>(3)?, row.get::<_, i32>(4)?, row.get::<_, i32>(5)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .map(|(sid, first_ts, last_ts, msg_count, user_msgs, asst_msgs)| {
            let preview: String = conn
                .query_row(
                    "SELECT content_text FROM messages WHERE session_id = ? AND project = ? AND source = ? AND role = 'user' ORDER BY id ASC LIMIT 1",
                    params![&sid, project_name, source],
                    |row| row.get(0),
                )
                .unwrap_or_default();
            let preview_short = if preview.len() > 120 {
                let end = preview.ceil_char_boundary(120);
                format!("{}...", &preview[..end])
            } else {
                preview
            };
            ApiSession {
                session_id: sid,
                first_timestamp: first_ts,
                last_timestamp: last_ts,
                message_count: msg_count,
                user_messages: user_msgs,
                assistant_messages: asst_msgs,
                preview: preview_short,
            }
        })
        .collect();

    Json(ApiSessionsResponse {
        project_name: project_name.to_string(),
        project_path,
        source: source.to_string(),
        sessions,
    })
}

#[utoipa::path(
    get,
    path = "/api/messages",
    params(MessageQuery),
    responses((status = 200, body = ApiMessagesResponse)),
    tag = "messages"
)]
async fn api_messages(
    State(db): State<AppState>,
    Query(params): Query<MessageQuery>,
) -> Json<ApiMessagesResponse> {
    let conn = db.lock().unwrap();
    let session_id = params.session.as_deref().unwrap_or("");

    // Derive project and source from the session_id (which is globally unique)
    let (project_name, source): (String, String) = conn
        .query_row(
            "SELECT project, source FROM sessions WHERE session_id = ?",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|_| (String::new(), String::new()));

    let project_path: String = conn
        .query_row(
            "SELECT original_path FROM projects WHERE name = ? AND source = ?",
            params![&project_name, &source],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| project_name.clone());

    let mut stmt = conn
        .prepare(
            "SELECT role, content_text, model, timestamp, is_subagent, msg_type, input_tokens, output_tokens
             FROM messages
             WHERE session_id = ?
             ORDER BY id DESC",
        )
        .unwrap();

    let messages: Vec<ApiMessage> = stmt
        .query_map(params![session_id], |row| {
            Ok(ApiMessage {
                role: row.get::<_, String>(0)?,
                content: row.get::<_, String>(1)?,
                model: row.get::<_, String>(2)?,
                timestamp: row.get::<_, String>(3)?,
                is_subagent: row.get::<_, bool>(4)?,
                msg_type: row.get::<_, String>(5)?,
                input_tokens: row.get::<_, i64>(6)?,
                output_tokens: row.get::<_, i64>(7)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(ApiMessagesResponse {
        session_id: session_id.to_string(),
        project_name,
        project_path,
        source,
        messages,
    })
}

#[utoipa::path(
    get,
    path = "/api/search",
    params(SearchParams),
    responses((status = 200, body = ApiSearchResponse)),
    tag = "search"
)]
async fn api_search(
    State(db): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<ApiSearchResponse> {
    let conn = db.lock().unwrap();
    let query = params.q.as_deref().unwrap_or("");
    let project_filter = params.project.as_deref().unwrap_or("");
    let source_filter = params.source.as_deref().unwrap_or("");
    let page = params.page.unwrap_or(0);
    let per_page = 50;
    let offset = page * per_page;

    if query.is_empty() {
        return Json(ApiSearchResponse {
            query: String::new(),
            total_count: 0,
            page,
            per_page,
            results: Vec::new(),
        });
    }

    let (total_count, rows) = run_search(
        &conn,
        query,
        project_filter,
        source_filter,
        per_page,
        offset,
    );

    let results: Vec<ApiSearchResult> = rows
        .into_iter()
        .map(
            |(
                id,
                project,
                project_path,
                session_id,
                role,
                content,
                model,
                timestamp,
                is_subagent,
                source,
            )| {
                ApiSearchResult {
                    id,
                    project,
                    project_path,
                    session_id,
                    role,
                    content,
                    model,
                    timestamp,
                    is_subagent,
                    source,
                }
            },
        )
        .collect();

    Json(ApiSearchResponse {
        query: query.to_string(),
        total_count,
        page,
        per_page,
        results,
    })
}

#[utoipa::path(
    get,
    path = "/api/analytics",
    responses((status = 200, body = ApiAnalyticsResponse)),
    tag = "analytics"
)]
async fn api_analytics(State(db): State<AppState>) -> Json<ApiAnalyticsResponse> {
    let conn = db.lock().unwrap();

    // Overview by source
    let mut stmt = conn
        .prepare(
            "SELECT source, COUNT(*) as msg_count, COUNT(DISTINCT session_id) as sess_count, COUNT(DISTINCT project) as proj_count
             FROM messages GROUP BY source ORDER BY msg_count DESC",
        )
        .unwrap();
    let source_stats: Vec<ApiSourceStats> = stmt
        .query_map([], |row| {
            Ok(ApiSourceStats {
                source: row.get::<_, String>(0)?,
                message_count: row.get::<_, i64>(1)?,
                session_count: row.get::<_, i64>(2)?,
                project_count: row.get::<_, i64>(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    // Token usage by model
    let mut stmt = conn
        .prepare(
            "SELECT source, model, COUNT(*) as msg_count, SUM(input_tokens) as total_input, SUM(output_tokens) as total_output
             FROM messages
             WHERE model != '' AND role = 'assistant'
             GROUP BY source, model
             ORDER BY msg_count DESC",
        )
        .unwrap();
    let model_stats: Vec<ApiModelStats> = stmt
        .query_map([], |row| {
            Ok(ApiModelStats {
                source: row.get::<_, String>(0)?,
                model: row.get::<_, String>(1)?,
                message_count: row.get::<_, i64>(2)?,
                input_tokens: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    // Messages by project
    let mut stmt = conn
        .prepare(
            "SELECT source, project_path, COUNT(*) as cnt,
                    SUM(CASE WHEN role='user' THEN 1 ELSE 0 END) as user_cnt,
                    SUM(CASE WHEN role='assistant' THEN 1 ELSE 0 END) as asst_cnt
             FROM messages
             GROUP BY source, project_path
             ORDER BY cnt DESC",
        )
        .unwrap();
    let project_stats: Vec<ApiProjectStats> = stmt
        .query_map([], |row| {
            Ok(ApiProjectStats {
                source: row.get::<_, String>(0)?,
                project_path: row.get::<_, String>(1)?,
                total_messages: row.get::<_, i64>(2)?,
                user_messages: row.get::<_, i64>(3)?,
                assistant_messages: row.get::<_, i64>(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(ApiAnalyticsResponse {
        source_stats,
        model_stats,
        project_stats,
    })
}

const SPA_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
    <title>AI Chat History</title>
    <style>
    @view-transition { navigation: auto; }
    * { margin: 0; padding: 0; box-sizing: border-box; }
    ::view-transition-old(root), ::view-transition-new(root) { animation-duration: 0.3s; }
    body { font-family: 'OpenAI Sans', -apple-system, BlinkMacSystemFont, sans-serif; background: #000; color: #fafafa; line-height: 1.6; -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; }
    header { background: #000; padding: 1.5rem 2rem; }
    header h1 { font-family: 'Bitstream Charter', 'Charter', Georgia, serif; color: #fafafa; font-size: 1.5rem; font-weight: 600; letter-spacing: -0.02em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; display: flex; align-items: center; gap: 0.5rem; }
    header .stats { color: #71717a; font-size: 0.85rem; margin-top: 0.5rem; font-weight: 400; }
    header .back { color: #a1a1aa; text-decoration: none; font-size: 0.85rem; display: inline-block; margin-bottom: 0.75rem; font-weight: 500; transition: color 0.2s; cursor: pointer; }
    header .back:hover { color: #fafafa; }
    nav.tabs { display: flex; gap: 0.5rem; padding: 0 2rem 0.5rem; background: #000; }
    nav.tabs a { padding: 0.5rem 1rem; color: #71717a; text-decoration: none; font-size: 0.85rem; font-weight: 500; border-radius: 6px; transition: all 0.2s; cursor: pointer; }
    nav.tabs a:hover { color: #fafafa; background: #18181b; }
    nav.tabs a.active { color: #fafafa; background: #18181b; }
    nav.source-tabs { display: flex; gap: 0.375rem; padding: 0.25rem 2rem 1rem; background: #000; }
    nav.source-tabs a { padding: 0.35rem 0.75rem; color: #52525b; text-decoration: none; font-size: 0.75rem; font-weight: 600; border-radius: 20px; border: 1px solid #27272a; transition: all 0.2s; text-transform: uppercase; letter-spacing: 0.04em; cursor: pointer; }
    nav.source-tabs a:hover { color: #a1a1aa; border-color: #3f3f46; }
    nav.source-tabs a.active { color: #fafafa; background: #27272a; border-color: #3f3f46; }
    .search-bar { padding: 0 2rem 1.5rem; background: #000; }
    .search-bar form { display: flex; gap: 0.5rem; max-width: 800px; margin: 0 auto; }
    .search-bar input[type="text"] { flex: 1; padding: 0.65rem 0.875rem; background: #18181b; border: none; border-radius: 8px; color: #fafafa; font-size: 0.9rem; font-weight: 400; transition: background 0.2s; }
    .search-bar input:focus { outline: none; background: #27272a; }
    .search-bar button { padding: 0.65rem 1.25rem; background: #fafafa; color: #000; border: none; border-radius: 8px; cursor: pointer; font-size: 0.9rem; font-weight: 600; transition: all 0.2s; }
    .search-bar button:hover { background: #e4e4e7; transform: translateY(-1px); }
    main { max-width: 1200px; margin: 0 auto; padding: 0 2rem 2rem; }
    main h2 { font-family: 'Bitstream Charter', 'Charter', Georgia, serif; color: #fafafa; font-size: 1.1rem; font-weight: 600; margin-bottom: 1rem; padding-bottom: 0.75rem; letter-spacing: -0.01em; text-align: center; }
    .projects-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(350px, 1fr)); gap: 0.875rem; }
    .project-card { display: block; padding: 1.25rem; background: #0a0a0a; border-radius: 10px; text-decoration: none; color: inherit; transition: all 0.2s; cursor: pointer; }
    .project-card:hover { background: #18181b; transform: translateY(-2px); }
    .project-card-header { display: flex; align-items: center; gap: 0.5rem; }
    .project-path { color: #fafafa; font-weight: 600; font-size: 0.95rem; letter-spacing: -0.01em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; min-width: 0; }
    .project-stats { color: #71717a; font-size: 0.8rem; margin-top: 0.5rem; font-weight: 400; }
    .sessions-list { display: flex; flex-direction: column; gap: 0.75rem; }
    .session-card { display: block; padding: 1rem 1.25rem; background: #0a0a0a; border-radius: 10px; text-decoration: none; color: inherit; transition: all 0.2s; cursor: pointer; }
    .session-card:hover { background: #18181b; transform: translateY(-1px); }
    .session-header-row { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.5rem; }
    .session-id { color: #a1a1aa; font-family: 'SF Mono', 'Fira Code', monospace; font-weight: 600; font-size: 0.8rem; }
    .session-time { color: #52525b; font-size: 0.75rem; font-weight: 400; }
    .session-preview { color: #71717a; font-size: 0.85rem; line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
    .session-stats { color: #52525b; font-size: 0.75rem; margin-top: 0.5rem; font-weight: 400; }
    .conversation { display: flex; flex-direction: column; gap: 1.25rem; }
    .message { padding: 1.25rem; border-radius: 12px; }
    .message.user { background: #0a0a0a; }
    .message.assistant { background: #18181b; }
    .message-header { display: flex; align-items: center; gap: 0.5rem; margin-bottom: 0.75rem; font-size: 0.8rem; }
    .role { font-weight: 700; text-transform: uppercase; font-size: 0.65rem; padding: 0.2rem 0.5rem; border-radius: 4px; letter-spacing: 0.05em; }
    .message.user .role { background: #27272a; color: #a1a1aa; }
    .message.assistant .role { background: #3f3f46; color: #d4d4d8; }
    .badge { font-size: 0.6rem; padding: 0.15rem 0.4rem; border-radius: 4px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; flex-shrink: 0; }
    .badge.subagent { background: #3f3f46; color: #d4d4d8; }
    .badge.model { background: #27272a; color: #a1a1aa; }
    .badge.source-claude { background: #1e3a5f; color: #93c5fd; }
    .badge.source-codex { background: #14532d; color: #86efac; }
    .timestamp { color: #52525b; margin-left: auto; font-weight: 400; font-size: 0.75rem; }
    .message-content { font-size: 0.9rem; line-height: 1.7; overflow-x: auto; font-weight: 400; }
    .message-content pre { white-space: pre-wrap; word-wrap: break-word; font-family: 'SF Mono', 'Fira Code', monospace; font-size: 0.82rem; max-height: 600px; overflow-y: auto; padding: 0.75rem; background: #000; border-radius: 8px; line-height: 1.5; }
    .message-content p { margin: 0; }
    .search-result { padding: 1rem 1.25rem; background: #0a0a0a; border-radius: 10px; margin-bottom: 0.75rem; transition: all 0.2s; }
    .search-result:hover { background: #18181b; }
    .result-header { display: flex; align-items: center; gap: 0.5rem; font-size: 0.8rem; flex-wrap: wrap; }
    .result-header a { color: #a1a1aa; text-decoration: none; font-weight: 500; transition: color 0.2s; cursor: pointer; }
    .result-header a:hover { color: #fafafa; }
    .result-header .model { color: #52525b; font-weight: 400; }
    .result-snippet { margin-top: 0.75rem; font-size: 0.85rem; color: #71717a; line-height: 1.6; word-break: break-word; }
    .result-snippet mark { background: #713f12; color: #fafafa; padding: 0.1rem 0.3rem; border-radius: 3px; font-weight: 500; }
    .pagination { display: flex; align-items: center; justify-content: center; gap: 1rem; padding: 1.5rem; margin-top: 1rem; }
    .pagination a { color: #a1a1aa; text-decoration: none; padding: 0.5rem 0.875rem; background: #0a0a0a; border-radius: 6px; font-weight: 500; transition: all 0.2s; cursor: pointer; }
    .pagination a:hover { background: #18181b; color: #fafafa; }
    .pagination span { color: #71717a; font-size: 0.85rem; font-weight: 400; }
    table { width: 100%; border-collapse: collapse; margin-bottom: 2rem; }
    th, td { padding: 0.75rem; text-align: left; }
    th { color: #71717a; font-size: 0.8rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; }
    td { font-size: 0.85rem; color: #fafafa; font-weight: 400; }
    tr:not(:last-child) td { border-bottom: 1px solid #18181b; }
    .loading { text-align: center; padding: 3rem; color: #71717a; font-size: 0.9rem; }
    .error { text-align: center; padding: 3rem; color: #ef4444; font-size: 0.9rem; }
    </style>
</head>
<body>
    <div id="app"><div class="loading">Loading...</div></div>
    <script>
    (function() {
        const $ = document.getElementById.bind(document);
        const E = encodeURIComponent;

        function esc(s) {
            if (!s) return '';
            return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
        }

        function formatTokens(n) {
            if (n >= 1000000) return (n/1000000).toFixed(1) + 'M';
            if (n >= 1000) return (n/1000).toFixed(1) + 'K';
            return String(n);
        }

        function sourceBadge(source) {
            if (source === 'claude') return '<span class="badge source-claude">claude</span>';
            if (source === 'codex') return '<span class="badge source-codex">codex</span>';
            return '';
        }

        function sourceTabs(current, makeHref) {
            function cls(v) { return v === current ? 'active' : ''; }
            return '<nav class="source-tabs">'
                + '<a class="'+cls('')+'" onclick="navigate(\''+makeHref('')+'\')">All</a>'
                + '<a class="'+cls('claude')+'" onclick="navigate(\''+makeHref('claude')+'\')">Claude</a>'
                + '<a class="'+cls('codex')+'" onclick="navigate(\''+makeHref('codex')+'\')">Codex</a>'
                + '</nav>';
        }

        function makeSnippet(content, query, ctxChars) {
            if (!query) return esc(content.substring(0, ctxChars * 2));
            var lower = content.toLowerCase();
            var qLower = query.toLowerCase();
            var pos = lower.indexOf(qLower);
            if (pos === -1) {
                var t = content.substring(0, ctxChars * 2);
                return esc(t) + (content.length > ctxChars * 2 ? '...' : '');
            }
            var start = Math.max(0, pos - ctxChars);
            var end = Math.min(content.length, pos + query.length + ctxChars);
            var slice = content.substring(start, end);
            var escaped = esc(slice);
            var qEsc = esc(query);
            // Case-insensitive highlight
            var re = new RegExp(qEsc.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
            var highlighted = escaped.replace(re, function(m) { return '<mark>' + m + '</mark>'; });
            var prefix = start > 0 ? '...' : '';
            var suffix = end < content.length ? '...' : '';
            return prefix + highlighted + suffix;
        }

        function renderContent(content) {
            var escaped = esc(content);
            if (content.length > 200 || content.indexOf('\n') !== -1) {
                return '<pre>' + escaped + '</pre>';
            }
            return '<p>' + escaped + '</p>';
        }

        function shortTs(ts) {
            return ts ? ts.substring(0, 19) : '';
        }

        async function fetchJSON(url) {
            var resp = await fetch(url);
            if (!resp.ok) throw new Error('HTTP ' + resp.status);
            return resp.json();
        }

        function getParams() {
            return new URLSearchParams(window.location.search);
        }

        function navigate(url) {
            history.pushState(null, '', url);
            route();
        }
        window.navigate = navigate;

        window.addEventListener('popstate', route);

        // Intercept form submits for search
        document.addEventListener('submit', function(e) {
            var form = e.target;
            if (form.tagName === 'FORM' && form.getAttribute('data-spa') === '1') {
                e.preventDefault();
                var fd = new FormData(form);
                var params = new URLSearchParams();
                for (var pair of fd.entries()) {
                    if (pair[1]) params.set(pair[0], pair[1]);
                }
                navigate(form.action.replace(window.location.origin, '') + '?' + params.toString());
            }
        });

        async function route() {
            var path = window.location.pathname;
            var params = getParams();
            var app = $('app');
            app.innerHTML = '<div class="loading">Loading...</div>';
            try {
                if (path === '/analytics') {
                    await renderAnalytics(app);
                } else if (path === '/session') {
                    await renderSession(app, params);
                } else if (path === '/project') {
                    await renderProject(app, params);
                } else if (path === '/search') {
                    await renderSearch(app, params);
                } else {
                    await renderIndex(app, params);
                }
            } catch(err) {
                app.innerHTML = '<div class="error">Error: ' + esc(err.message) + '</div>';
            }
        }

        async function renderIndex(app, params) {
            var source = params.get('source') || '';
            var data = await fetchJSON('/api/projects');
            var filtered = source ? data.projects.filter(function(p) { return p.source === source; }) : data.projects;
            var html = '<header><h1>AI Chat History</h1>'
                + '<div class="stats">' + data.total_messages + ' messages across ' + data.total_sessions + ' sessions in ' + data.projects.length + ' projects</div></header>';
            html += '<nav class="tabs"><a class="active" onclick="navigate(\'/\')">Projects</a><a onclick="navigate(\'/analytics\')">Analytics</a></nav>';
            html += sourceTabs(source, function(s) { return s ? '/?source=' + s : '/'; });
            html += '<div class="search-bar"><form action="/search" data-spa="1">';
            if (source) html += '<input type="hidden" name="source" value="' + esc(source) + '" />';
            html += '<input type="text" name="q" placeholder="Search all conversations..." autofocus /><button type="submit">Search</button></form></div>';
            html += '<main><h2>Projects</h2><div class="projects-grid">';
            for (var i = 0; i < filtered.length; i++) {
                var p = filtered[i];
                var href = '/project?name=' + E(p.name) + '&source=' + E(p.source);
                html += '<div class="project-card" onclick="navigate(\'' + href + '\')">'
                    + '<div class="project-card-header"><div class="project-path">' + esc(p.original_path) + '</div>' + sourceBadge(p.source) + '</div>'
                    + '<div class="project-stats">' + p.session_count + ' sessions &middot; ' + p.message_count + ' messages</div></div>';
            }
            html += '</div></main>';
            app.innerHTML = html;
        }

        async function renderProject(app, params) {
            var name = params.get('name') || '';
            var source = params.get('source') || 'codex';
            var apiUrl = '/api/sessions?name=' + E(name) + '&source=' + E(source);
            var data = await fetchJSON(apiUrl);
            var dirname = data.project_path.split('/').pop() || data.project_path;
            var backUrl = source ? '/?source=' + E(source) : '/';
            var html = '<header><a class="back" onclick="navigate(\'' + backUrl + '\')">&larr; All Projects</a>'
                + '<h1>' + sourceBadge(data.source) + ' ' + esc(dirname) + '</h1>'
                + '<div class="stats">' + esc(data.project_path) + ' &middot; ' + data.sessions.length + ' sessions</div></header>';
            html += '<div class="search-bar"><form action="/search" data-spa="1">'
                + '<input type="hidden" name="project" value="' + esc(name) + '" />'
                + '<input type="hidden" name="source" value="' + esc(source) + '" />'
                + '<input type="text" name="q" placeholder="Search within this project..." autofocus /><button type="submit">Search</button></form></div>';
            html += '<main><div class="sessions-list">';
            for (var i = 0; i < data.sessions.length; i++) {
                var s = data.sessions[i];
                var href = '/session?session=' + E(s.session_id);
                var shortSid = s.session_id.substring(0, 8);
                html += '<div class="session-card" onclick="navigate(\'' + href + '\')">'
                    + '<div class="session-header-row"><span class="session-id">' + esc(shortSid) + '</span><span class="session-time">' + shortTs(s.first_timestamp) + '</span></div>'
                    + '<div class="session-preview">' + esc(s.preview) + '</div>'
                    + '<div class="session-stats">' + s.message_count + ' messages (' + s.user_messages + ' user, ' + s.assistant_messages + ' assistant)</div></div>';
            }
            html += '</div></main>';
            app.innerHTML = html;
        }

        async function renderSession(app, params) {
            var session = params.get('session') || '';
            var apiUrl = '/api/messages?session=' + E(session);
            var data = await fetchJSON(apiUrl);
            var shortSid = data.session_id.substring(0, 8);
            var backUrl = '/project?name=' + E(data.project_name) + '&source=' + E(data.source);
            var html = '<header><a class="back" onclick="navigate(\'' + backUrl + '\')">&larr; ' + esc(data.project_path) + '</a>'
                + '<h1>' + sourceBadge(data.source) + ' Session ' + esc(shortSid) + '</h1>'
                + '<div class="stats">' + data.messages.length + ' messages</div></header>';
            html += '<main class="conversation">';
            for (var i = 0; i < data.messages.length; i++) {
                var m = data.messages[i];
                var roleClass = m.role === 'user' ? 'user' : 'assistant';
                var subBadge = m.is_subagent ? '<span class="badge subagent">subagent</span>' : '';
                var modelBadge = m.model ? '<span class="badge model">' + esc(m.model) + '</span>' : '';
                html += '<div class="message ' + roleClass + '">'
                    + '<div class="message-header"><span class="role">' + esc(m.role) + '</span>' + subBadge + modelBadge
                    + '<span class="timestamp">' + shortTs(m.timestamp) + '</span></div>'
                    + '<div class="message-content">' + renderContent(m.content) + '</div></div>';
            }
            html += '</main>';
            app.innerHTML = html;
        }

        async function renderSearch(app, params) {
            var query = params.get('q') || '';
            var projectFilter = params.get('project') || '';
            var sourceFilter = params.get('source') || '';
            var page = parseInt(params.get('page') || '0', 10);
            var apiUrl = '/api/search?q=' + E(query);
            if (projectFilter) apiUrl += '&project=' + E(projectFilter);
            if (sourceFilter) apiUrl += '&source=' + E(sourceFilter);
            apiUrl += '&page=' + page;
            var data = await fetchJSON(apiUrl);

            var scopeParts = [];
            if (projectFilter) scopeParts.push('in <strong>' + esc(projectFilter) + '</strong>');
            if (sourceFilter) scopeParts.push('(' + esc(sourceFilter) + ')');
            var scopeText = scopeParts.length ? ' ' + scopeParts.join(' ') : '';

            var html = '<header><a class="back" onclick="navigate(\'/\')">&larr; All Projects</a>'
                + '<h1>Search Results</h1>'
                + '<div class="stats">' + data.total_count + ' results' + scopeText + '</div></header>';

            html += sourceTabs(sourceFilter, function(s) {
                var u = '/search';
                var ps = [];
                if (query) ps.push('q=' + E(query));
                if (projectFilter) ps.push('project=' + E(projectFilter));
                if (s) ps.push('source=' + s);
                return ps.length ? u + '?' + ps.join('&') : u;
            });

            html += '<div class="search-bar"><form action="/search" data-spa="1">';
            if (projectFilter) html += '<input type="hidden" name="project" value="' + esc(projectFilter) + '" />';
            if (sourceFilter) html += '<input type="hidden" name="source" value="' + esc(sourceFilter) + '" />';
            html += '<input type="text" name="q" value="' + esc(query) + '" placeholder="Search conversations..." autofocus /><button type="submit">Search</button></form></div>';

            html += '<main>';
            if (!query) {
                html += '<p>Enter a search query above.</p>';
            } else {
                for (var i = 0; i < data.results.length; i++) {
                    var r = data.results[i];
                    var shortSid = r.session_id.substring(0, 8);
                    var snippet = makeSnippet(r.content, query, 200);
                    var subBadge = r.is_subagent ? '<span class="badge subagent">subagent</span>' : '';
                    var projHref = '/project?name=' + E(r.project) + '&source=' + E(r.source);
                    var sessHref = '/session?session=' + E(r.session_id);
                    html += '<div class="search-result"><div class="result-header">'
                        + '<span class="role ' + esc(r.role) + '">' + esc(r.role) + '</span>'
                        + sourceBadge(r.source) + subBadge
                        + ' <a onclick="navigate(\'' + projHref + '\')">' + esc(r.project_path) + '</a> &middot; '
                        + '<a onclick="navigate(\'' + sessHref + '\')">Session ' + esc(shortSid) + '</a>'
                        + '<span class="model">' + esc(r.model) + '</span>'
                        + '<span class="timestamp">' + shortTs(r.timestamp) + '</span>'
                        + '</div><div class="result-snippet">' + snippet + '</div></div>';
                }

                if (data.total_count > data.per_page) {
                    html += '<div class="pagination">';
                    var sp = sourceFilter ? '&source=' + E(sourceFilter) : '';
                    var pp = projectFilter ? '&project=' + E(projectFilter) : '';
                    if (page > 0) {
                        html += '<a onclick="navigate(\'/search?q=' + E(query) + pp + sp + '&page=' + (page - 1) + '\')">&#8592; Previous</a>';
                    }
                    var totalPages = Math.ceil(data.total_count / data.per_page);
                    html += '<span>Page ' + (page + 1) + ' of ' + totalPages + '</span>';
                    if (page + 1 < totalPages) {
                        html += '<a onclick="navigate(\'/search?q=' + E(query) + pp + sp + '&page=' + (page + 1) + '\')">Next &#8594;</a>';
                    }
                    html += '</div>';
                }
            }
            html += '</main>';
            app.innerHTML = html;
        }

        async function renderAnalytics(app) {
            var data = await fetchJSON('/api/analytics');
            var html = '<header><a class="back" onclick="navigate(\'/\')">&larr; All Projects</a><h1>Analytics</h1></header>';
            html += '<nav class="tabs"><a onclick="navigate(\'/\')">Projects</a><a class="active" onclick="navigate(\'/analytics\')">Analytics</a></nav>';
            html += '<main>';

            // Source stats
            html += '<h2>Overview by Source</h2><table><thead><tr><th>Source</th><th>Messages</th><th>Sessions</th><th>Projects</th></tr></thead><tbody>';
            for (var i = 0; i < data.source_stats.length; i++) {
                var s = data.source_stats[i];
                html += '<tr><td>' + sourceBadge(s.source) + esc(s.source) + '</td><td>' + s.message_count + '</td><td>' + s.session_count + '</td><td>' + s.project_count + '</td></tr>';
            }
            html += '</tbody></table>';

            // Model stats
            html += '<h2>Token Usage by Model</h2><table><thead><tr><th>Model</th><th>Messages</th><th>Input Tokens</th><th>Output Tokens</th></tr></thead><tbody>';
            for (var i = 0; i < data.model_stats.length; i++) {
                var m = data.model_stats[i];
                html += '<tr><td>' + sourceBadge(m.source) + esc(m.model) + '</td><td>' + m.message_count + '</td><td>' + formatTokens(m.input_tokens) + '</td><td>' + formatTokens(m.output_tokens) + '</td></tr>';
            }
            html += '</tbody></table>';

            // Project stats
            html += '<h2>Messages by Project</h2><table><thead><tr><th>Project</th><th>Total</th><th>User</th><th>Assistant</th></tr></thead><tbody>';
            for (var i = 0; i < data.project_stats.length; i++) {
                var p = data.project_stats[i];
                html += '<tr><td>' + sourceBadge(p.source) + esc(p.project_path) + '</td><td>' + p.total_messages + '</td><td>' + p.user_messages + '</td><td>' + p.assistant_messages + '</td></tr>';
            }
            html += '</tbody></table>';

            html += '</main>';
            app.innerHTML = html;
        }

        // Initial route
        route();
    })();
    </script>
</body>
</html>"##;

// --- CLI Cache (On-Disk DuckDB) ---

fn cache_db_path() -> Result<PathBuf> {
    // New name: MMR_DB_PATH. Keep MEMORY_DB_PATH for backwards compatibility.
    if let Ok(p) = std::env::var("MMR_DB_PATH") {
        return Ok(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("MEMORY_DB_PATH") {
        return Ok(PathBuf::from(p));
    }

    let base = dirs::cache_dir()
        .or_else(dirs::data_local_dir)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .context("Could not determine cache directory")?;

    let new_path = base.join("mmr").join("mmr.duckdb");
    let legacy_path = base.join("memory").join("memory.duckdb");
    if new_path.exists() {
        return Ok(new_path);
    }
    if legacy_path.exists() {
        return Ok(legacy_path);
    }
    Ok(new_path)
}

fn open_cache_db_for_cli(quiet: bool) -> Result<Connection> {
    let cache_path = cache_db_path()?;
    let cache_dir = cache_path
        .parent()
        .context("Cache path has no parent directory")?;
    std::fs::create_dir_all(cache_dir)?;

    let mut conn = Connection::open(&cache_path)?;
    init_db(&conn)?;

    let schema_version: Option<String> = conn
        .query_row(
            "SELECT value FROM cache_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();
    if let Some(version) = schema_version {
        if version != CACHE_SCHEMA_VERSION {
            if !quiet {
                eprintln!(
                    "Cache schema changed ({} -> {}). Rebuilding cache at {}.",
                    version,
                    CACHE_SCHEMA_VERSION,
                    cache_path.display()
                );
            }
            drop(conn);
            let _ = std::fs::remove_file(&cache_path);
            conn = Connection::open(&cache_path)?;
            init_db(&conn)?;
        }
    }

    refresh_incremental_cache(&conn, quiet)?;
    Ok(conn)
}

fn rebuild_cli_cache(quiet: bool) -> Result<()> {
    let cache_path = cache_db_path()?;
    let cache_dir = cache_path
        .parent()
        .context("Cache path has no parent directory")?;
    std::fs::create_dir_all(cache_dir)?;

    let tmp_path = {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        cache_dir.join(format!(".mmr-cache-{}-{}.duckdb", std::process::id(), ts))
    };

    if !quiet {
        eprintln!("Building CLI cache at {}", cache_path.display());
        eprintln!("Ingesting conversation history...");
    }

    let ingest_result: Result<IngestStats> = (|| {
        let conn = Connection::open(&tmp_path)?;
        init_db(&conn)?;
        let stats = refresh_incremental_cache(&conn, quiet)?;
        Ok(stats)
    })();

    let stats = match ingest_result {
        Ok(stats) => stats,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    };

    // Swap the new cache into place. Prefer atomic rename. If the destination exists and
    // rename fails (e.g. on Windows), remove then rename.
    if let Err(e) = std::fs::rename(&tmp_path, &cache_path) {
        if cache_path.exists() {
            std::fs::remove_file(&cache_path)?;
            std::fs::rename(&tmp_path, &cache_path)?;
        } else {
            return Err(e.into());
        }
    }

    if !quiet {
        eprintln!(
            "  Claude: {} messages from {} sessions across {} projects",
            stats.claude_messages, stats.claude_sessions, stats.claude_projects
        );
        eprintln!(
            "  Codex:  {} messages from {} sessions across {} projects",
            stats.codex_messages, stats.codex_sessions, stats.codex_projects
        );
        let total_messages = stats.claude_messages + stats.codex_messages;
        let total_sessions = stats.claude_sessions + stats.codex_sessions;
        eprintln!(
            "  Total:  {} messages, {} sessions",
            total_messages, total_sessions
        );
        eprintln!("Cache ready.");
    }

    Ok(())
}

// --- CLI Definition ---

#[derive(Parser)]
#[command(name = "mmr", about = "Search and browse AI conversation history")]
struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pretty: bool,

    /// Filter by source: claude, codex
    #[arg(long, global = true)]
    source: Option<String>,

    /// Suppress ingestion progress (stderr)
    #[arg(long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all projects
    Projects {
        /// Maximum number of projects to return
        #[arg(long)]
        limit: Option<usize>,
        /// Number of projects to skip
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// List sessions for a project
    Sessions {
        /// Project name
        #[arg(long)]
        project: String,
        /// Maximum number of sessions to return
        #[arg(long)]
        limit: Option<usize>,
        /// Number of sessions to skip
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Get messages for a session
    Messages {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Return only the last N messages
        #[arg(long)]
        limit: Option<usize>,
        /// Number of messages to skip
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Search across all conversations
    Search {
        /// Search query
        query: String,
        /// Filter by project name
        #[arg(long)]
        project: Option<String>,
        /// Page number (0-indexed)
        #[arg(long, default_value = "0")]
        page: usize,
        /// Results per page
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Show usage statistics
    Stats,
    /// (Re)ingest conversation history and rebuild the CLI cache
    #[command(alias = "refresh")]
    Ingest,
    /// Start the web server (default when no subcommand given)
    Serve,
}

// --- CLI Command Implementations ---

fn pagination_clause(limit: Option<usize>, offset: usize) -> String {
    let mut clause = String::new();
    if let Some(limit) = limit {
        clause.push_str(&format!(" LIMIT {}", limit));
    }
    if offset > 0 {
        if limit.is_none() {
            clause.push_str(" LIMIT 9223372036854775807");
        }
        clause.push_str(&format!(" OFFSET {}", offset));
    }
    clause
}

fn cmd_projects(
    conn: &Connection,
    source: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiProjectsResponse> {
    let pagination = pagination_clause(limit, offset);
    let (query_sql, has_source) = match source {
        Some(s) if s == "claude" || s == "codex" => (
            format!(
                "SELECT name, source, original_path, session_count, message_count, last_activity FROM projects WHERE source = ? ORDER BY last_activity DESC{}",
                pagination
            ),
            true,
        ),
        _ => (
            format!(
                "SELECT name, source, original_path, session_count, message_count, last_activity FROM projects ORDER BY last_activity DESC{}",
                pagination
            ),
            false,
        ),
    };

    let mut stmt = conn.prepare(&query_sql)?;
    let projects: Vec<ApiProject> = if has_source {
        stmt.query_map(params![source.unwrap()], |row| {
            Ok(ApiProject {
                name: row.get::<_, String>(0)?,
                source: row.get::<_, String>(1)?,
                original_path: row.get::<_, String>(2)?,
                session_count: row.get::<_, i32>(3)?,
                message_count: row.get::<_, i32>(4)?,
                last_activity: row.get::<_, String>(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok(ApiProject {
                name: row.get::<_, String>(0)?,
                source: row.get::<_, String>(1)?,
                original_path: row.get::<_, String>(2)?,
                session_count: row.get::<_, i32>(3)?,
                message_count: row.get::<_, i32>(4)?,
                last_activity: row.get::<_, String>(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let (total_messages, total_sessions) = if has_source {
        let m: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE source = ?",
            params![source.unwrap()],
            |row| row.get(0),
        )?;
        let s: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE source = ?",
            params![source.unwrap()],
            |row| row.get(0),
        )?;
        (m, s)
    } else {
        let m: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        let s: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        (m, s)
    };

    Ok(ApiProjectsResponse {
        projects,
        total_messages,
        total_sessions,
    })
}

fn resolve_project_for_source(conn: &Connection, source: &str, project: &str) -> String {
    if source != "codex" {
        return project.to_string();
    }

    let mut candidates = Vec::new();
    let trimmed = project.trim();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
        if trimmed.starts_with('/') {
            let without = trimmed.trim_start_matches('/');
            if !without.is_empty() {
                candidates.push(without.to_string());
            }
        } else {
            candidates.push(format!("/{}", trimmed));
        }
    }

    candidates.sort();
    candidates.dedup();

    for candidate in candidates {
        let found: Result<String, _> = conn.query_row(
            "SELECT name FROM projects WHERE source = 'codex' AND (name = ? OR original_path = ?) LIMIT 1",
            params![&candidate, &candidate],
            |row| row.get(0),
        );
        if let Ok(name) = found {
            return name;
        }
    }

    project.to_string()
}

fn cmd_sessions(
    conn: &Connection,
    project: &str,
    source: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiSessionsResponse> {
    let source = source.unwrap_or("codex");
    let project = resolve_project_for_source(conn, source, project);

    let project_path: String = conn
        .query_row(
            "SELECT original_path FROM projects WHERE name = ? AND source = ?",
            params![&project, source],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| project.clone());

    let query_sql = format!(
        "SELECT session_id, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages
         FROM sessions WHERE project = ? AND source = ? ORDER BY last_timestamp DESC{}",
        pagination_clause(limit, offset)
    );
    let mut stmt = conn.prepare(&query_sql)?;

    let sessions: Vec<ApiSession> = stmt
        .query_map(params![&project, source], |row| {
            let sid: String = row.get(0)?;
            Ok((sid, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i32>(3)?, row.get::<_, i32>(4)?, row.get::<_, i32>(5)?))
        })?
        .filter_map(|r| r.ok())
        .map(|(sid, first_ts, last_ts, msg_count, user_msgs, asst_msgs)| {
            let preview: String = conn
                .query_row(
                    "SELECT content_text FROM messages WHERE session_id = ? AND project = ? AND source = ? AND role = 'user' ORDER BY id ASC LIMIT 1",
                    params![&sid, &project, source],
                    |row| row.get(0),
                )
                .unwrap_or_default();
            let preview_short = if preview.len() > 120 {
                let end = preview.ceil_char_boundary(120);
                format!("{}...", &preview[..end])
            } else {
                preview
            };
            ApiSession {
                session_id: sid,
                first_timestamp: first_ts,
                last_timestamp: last_ts,
                message_count: msg_count,
                user_messages: user_msgs,
                assistant_messages: asst_msgs,
                preview: preview_short,
            }
        })
        .collect();

    Ok(ApiSessionsResponse {
        project_name: project,
        project_path,
        source: source.to_string(),
        sessions,
    })
}

fn cmd_messages(
    conn: &Connection,
    session_id: &str,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiMessagesResponse> {
    let (project_name, source): (String, String) = conn
        .query_row(
            "SELECT project, source FROM sessions WHERE session_id = ?",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|_| (String::new(), String::new()));

    let project_path: String = conn
        .query_row(
            "SELECT original_path FROM projects WHERE name = ? AND source = ?",
            params![&project_name, &source],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| project_name.clone());

    let query_sql = format!(
        "SELECT role, content_text, model, timestamp, is_subagent, msg_type, input_tokens, output_tokens
             FROM messages
             WHERE session_id = ?
             ORDER BY id DESC{}",
        pagination_clause(limit, offset)
    );
    let mut stmt = conn.prepare(&query_sql)?;
    let messages: Vec<ApiMessage> = stmt
        .query_map(params![session_id], |row| {
            Ok(ApiMessage {
                role: row.get::<_, String>(0)?,
                content: row.get::<_, String>(1)?,
                model: row.get::<_, String>(2)?,
                timestamp: row.get::<_, String>(3)?,
                is_subagent: row.get::<_, bool>(4)?,
                msg_type: row.get::<_, String>(5)?,
                input_tokens: row.get::<_, i64>(6)?,
                output_tokens: row.get::<_, i64>(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(ApiMessagesResponse {
        session_id: session_id.to_string(),
        project_name,
        project_path,
        source,
        messages,
    })
}

fn cmd_search(
    conn: &Connection,
    query: &str,
    project: Option<&str>,
    source: Option<&str>,
    page: usize,
    per_page: usize,
) -> Result<ApiSearchResponse> {
    if query.is_empty() {
        return Ok(ApiSearchResponse {
            query: String::new(),
            total_count: 0,
            page,
            per_page,
            results: Vec::new(),
        });
    }

    let resolved_project = match (project, source) {
        (Some(p), Some("codex")) => resolve_project_for_source(conn, "codex", p),
        (Some(p), _) => p.to_string(),
        (None, _) => String::new(),
    };
    let project_filter = resolved_project.as_str();
    let source_filter = source.unwrap_or("");
    let offset = page * per_page;

    let (total_count, rows) =
        run_search(conn, query, project_filter, source_filter, per_page, offset);

    let results: Vec<ApiSearchResult> = rows
        .into_iter()
        .map(
            |(
                id,
                project,
                project_path,
                session_id,
                role,
                content,
                model,
                timestamp,
                is_subagent,
                source,
            )| {
                ApiSearchResult {
                    id,
                    project,
                    project_path,
                    session_id,
                    role,
                    content,
                    model,
                    timestamp,
                    is_subagent,
                    source,
                }
            },
        )
        .collect();

    Ok(ApiSearchResponse {
        query: query.to_string(),
        total_count,
        page,
        per_page,
        results,
    })
}

fn cmd_stats(conn: &Connection, source: Option<&str>) -> Result<ApiAnalyticsResponse> {
    let source_filter = source.filter(|s| *s == "claude" || *s == "codex");

    let source_stats: Vec<ApiSourceStats> = if let Some(sf) = source_filter {
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) as msg_count, COUNT(DISTINCT session_id) as sess_count, COUNT(DISTINCT project) as proj_count
             FROM messages WHERE source = ? GROUP BY source ORDER BY msg_count DESC",
        )?;
        stmt.query_map(params![sf], |row| {
            Ok(ApiSourceStats {
                source: row.get::<_, String>(0)?,
                message_count: row.get::<_, i64>(1)?,
                session_count: row.get::<_, i64>(2)?,
                project_count: row.get::<_, i64>(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) as msg_count, COUNT(DISTINCT session_id) as sess_count, COUNT(DISTINCT project) as proj_count
             FROM messages GROUP BY source ORDER BY msg_count DESC",
        )?;
        stmt.query_map([], |row| {
            Ok(ApiSourceStats {
                source: row.get::<_, String>(0)?,
                message_count: row.get::<_, i64>(1)?,
                session_count: row.get::<_, i64>(2)?,
                project_count: row.get::<_, i64>(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let model_stats: Vec<ApiModelStats> = if let Some(sf) = source_filter {
        let mut stmt = conn.prepare(
            "SELECT source, model, COUNT(*) as msg_count, SUM(input_tokens) as total_input, SUM(output_tokens) as total_output
             FROM messages
             WHERE model != '' AND role = 'assistant' AND source = ?
             GROUP BY source, model
             ORDER BY msg_count DESC",
        )?;
        stmt.query_map(params![sf], |row| {
            Ok(ApiModelStats {
                source: row.get::<_, String>(0)?,
                model: row.get::<_, String>(1)?,
                message_count: row.get::<_, i64>(2)?,
                input_tokens: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT source, model, COUNT(*) as msg_count, SUM(input_tokens) as total_input, SUM(output_tokens) as total_output
             FROM messages
             WHERE model != '' AND role = 'assistant'
             GROUP BY source, model
             ORDER BY msg_count DESC",
        )?;
        stmt.query_map([], |row| {
            Ok(ApiModelStats {
                source: row.get::<_, String>(0)?,
                model: row.get::<_, String>(1)?,
                message_count: row.get::<_, i64>(2)?,
                input_tokens: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let project_stats: Vec<ApiProjectStats> = if let Some(sf) = source_filter {
        let mut stmt = conn.prepare(
            "SELECT source, project_path, COUNT(*) as cnt,
                    SUM(CASE WHEN role='user' THEN 1 ELSE 0 END) as user_cnt,
                    SUM(CASE WHEN role='assistant' THEN 1 ELSE 0 END) as asst_cnt
             FROM messages WHERE source = ?
             GROUP BY source, project_path
             ORDER BY cnt DESC",
        )?;
        stmt.query_map(params![sf], |row| {
            Ok(ApiProjectStats {
                source: row.get::<_, String>(0)?,
                project_path: row.get::<_, String>(1)?,
                total_messages: row.get::<_, i64>(2)?,
                user_messages: row.get::<_, i64>(3)?,
                assistant_messages: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT source, project_path, COUNT(*) as cnt,
                    SUM(CASE WHEN role='user' THEN 1 ELSE 0 END) as user_cnt,
                    SUM(CASE WHEN role='assistant' THEN 1 ELSE 0 END) as asst_cnt
             FROM messages
             GROUP BY source, project_path
             ORDER BY cnt DESC",
        )?;
        stmt.query_map([], |row| {
            Ok(ApiProjectStats {
                source: row.get::<_, String>(0)?,
                project_path: row.get::<_, String>(1)?,
                total_messages: row.get::<_, i64>(2)?,
                user_messages: row.get::<_, i64>(3)?,
                assistant_messages: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(ApiAnalyticsResponse {
        source_stats,
        model_stats,
        project_stats,
    })
}

fn run_cli(cli: Cli) -> Result<()> {
    let Cli {
        pretty,
        source,
        quiet,
        command,
    } = cli;

    let command = command.expect("caller ensures this is Some");

    match command {
        Commands::Ingest => rebuild_cli_cache(quiet),
        Commands::Serve => unreachable!(), // handled before calling run_cli
        other => {
            let conn = open_cache_db_for_cli(quiet)?;
            let source = source.as_deref();

            let json_output = match other {
                Commands::Projects { limit, offset } => {
                    let result = cmd_projects(&conn, source, limit, offset)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Sessions {
                    project,
                    limit,
                    offset,
                } => {
                    let result = cmd_sessions(&conn, &project, source, limit, offset)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Messages {
                    session,
                    limit,
                    offset,
                } => {
                    let result = cmd_messages(&conn, &session, limit, offset)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Search {
                    query,
                    project,
                    page,
                    limit,
                } => {
                    let result =
                        cmd_search(&conn, &query, project.as_deref(), source, page, limit)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Stats => {
                    let result = cmd_stats(&conn, source)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Ingest | Commands::Serve => unreachable!("handled above"),
            };

            println!("{}", json_output);
            Ok(())
        }
    }
}

// --- OpenAPI ---

#[derive(OpenApi)]
#[openapi(
    info(
        title = "AI Chat History API",
        version = "0.1.0",
        description = "API for browsing Claude Code and OpenAI Codex conversation history"
    ),
    tags(
        (name = "projects", description = "Project listing and filtering"),
        (name = "sessions", description = "Session listing within projects"),
        (name = "messages", description = "Message retrieval within sessions"),
        (name = "search", description = "Full-text search across conversations"),
        (name = "analytics", description = "Usage analytics and statistics")
    )
)]
struct ApiDoc;

// --- Main ---

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // If a CLI subcommand is given (and it's not `serve`), run in CLI mode
    match &cli.command {
        Some(Commands::Serve) | None => {}
        Some(_) => {
            return run_cli(cli);
        }
    }

    // --- Server mode ---
    println!("Initializing DuckDB...");
    let conn = Connection::open_in_memory()?;
    init_db(&conn)?;

    println!("Ingesting conversation history...");
    let stats = ingest_all(&conn)?;
    println!(
        "  Claude: {} messages from {} sessions across {} projects",
        stats.claude_messages, stats.claude_sessions, stats.claude_projects
    );
    println!(
        "  Codex:  {} messages from {} sessions across {} projects",
        stats.codex_messages, stats.codex_sessions, stats.codex_projects
    );
    let total_messages = stats.claude_messages + stats.codex_messages;
    let total_sessions = stats.claude_sessions + stats.codex_sessions;
    println!(
        "  Total:  {} messages, {} sessions",
        total_messages, total_sessions
    );

    println!("Building FTS index...");
    create_fts_index(&conn)?;
    println!("FTS index ready.");

    let state: AppState = Arc::new(Mutex::new(conn));

    // Build API router with OpenAPI spec collection
    let (api_router, openapi) = OpenApiRouter::<AppState>::with_openapi(ApiDoc::openapi())
        .routes(routes!(api_projects))
        .routes(routes!(api_sessions))
        .routes(routes!(api_messages))
        .routes(routes!(api_search))
        .routes(routes!(api_analytics))
        .split_for_parts();

    let openapi_json = openapi.to_pretty_json().unwrap();

    let app = Router::new()
        .merge(api_router)
        .route(
            "/openapi.json",
            get({
                let json = openapi_json.clone();
                move || async move {
                    (
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        json,
                    )
                }
            }),
        )
        .fallback(get(spa_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3131").await?;
    println!("\nAI Chat History UI available at: http://0.0.0.0:3131");
    println!("Press Ctrl+C to stop.\n");
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Insert test projects
        conn.execute(
            "INSERT INTO projects (name, source, original_path, session_count, message_count, last_activity) VALUES (?, 'claude', ?, 1, 2, '2025-01-01T00:01:00')",
            params!["-Users-test-proj", "/Users/test/proj"],
        ).unwrap();
        conn.execute(
            "INSERT INTO projects (name, source, original_path, session_count, message_count, last_activity) VALUES (?, 'codex', ?, 1, 2, '2025-01-02T00:01:00')",
            params!["/Users/test/codex-proj", "/Users/test/codex-proj"],
        ).unwrap();

        // Insert test sessions
        conn.execute(
            "INSERT INTO sessions (session_id, project, source, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages) VALUES ('sess-claude-1', '-Users-test-proj', 'claude', '2025-01-01T00:00:00', '2025-01-01T00:01:00', 2, 1, 1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (session_id, project, source, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages) VALUES ('sess-codex-1', '/Users/test/codex-proj', 'codex', '2025-01-02T00:00:00', '2025-01-02T00:01:00', 2, 1, 1)",
            [],
        ).unwrap();

        // Insert test messages
        conn.execute(
            "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (1, 'claude', '-Users-test-proj', '/Users/test/proj', 'sess-claude-1', FALSE, 'u1', '', 'user', 'user', 'hello world from claude', '', '2025-01-01T00:00:00', '', '', '', '', 0, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (2, 'claude', '-Users-test-proj', '/Users/test/proj', 'sess-claude-1', FALSE, 'a1', 'u1', 'assistant', 'assistant', 'hi there from assistant', 'claude-3-opus', '2025-01-01T00:01:00', '', '', '', '', 100, 50)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (3, 'codex', '/Users/test/codex-proj', '/Users/test/codex-proj', 'sess-codex-1', FALSE, '', '', 'user', 'user', 'hello world from codex', 'gpt-4', '2025-01-02T00:00:00', '', '', '', '', 0, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (4, 'codex', '/Users/test/codex-proj', '/Users/test/codex-proj', 'sess-codex-1', FALSE, '', '', 'assistant', 'assistant', 'hi there from codex assistant', 'gpt-4', '2025-01-02T00:01:00', '', '', '', '', 200, 100)",
            [],
        ).unwrap();

        create_fts_index(&conn).unwrap();
        conn
    }

    fn build_test_app(conn: Connection) -> Router {
        let state: AppState = Arc::new(Mutex::new(conn));

        let (api_router, openapi) = OpenApiRouter::<AppState>::with_openapi(ApiDoc::openapi())
            .routes(routes!(api_projects))
            .routes(routes!(api_sessions))
            .routes(routes!(api_messages))
            .routes(routes!(api_search))
            .routes(routes!(api_analytics))
            .split_for_parts();

        let openapi_json = openapi.to_pretty_json().unwrap();

        Router::new()
            .merge(api_router)
            .route(
                "/openapi.json",
                get({
                    let json = openapi_json.clone();
                    move || async move {
                        (
                            [(axum::http::header::CONTENT_TYPE, "application/json")],
                            json,
                        )
                    }
                }),
            )
            .fallback(get(spa_handler))
            .with_state(state)
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn test_projects_returns_all() {
        let app = build_test_app(setup_test_db());
        let (status, json) = get_json(app, "/api/projects").await;
        assert_eq!(status, StatusCode::OK);

        let projects = json["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 2);

        let sources: Vec<&str> = projects
            .iter()
            .map(|p| p["source"].as_str().unwrap())
            .collect();
        assert!(sources.contains(&"claude"));
        assert!(sources.contains(&"codex"));
        assert_eq!(json["total_messages"].as_i64().unwrap(), 4);
        assert_eq!(json["total_sessions"].as_i64().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_sessions_for_project() {
        let app = build_test_app(setup_test_db());
        let (status, json) =
            get_json(app, "/api/sessions?name=-Users-test-proj&source=claude").await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(json["project_name"].as_str().unwrap(), "-Users-test-proj");
        assert_eq!(json["source"].as_str().unwrap(), "claude");

        let sessions = json["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["session_id"].as_str().unwrap(), "sess-claude-1");
        assert_eq!(sessions[0]["message_count"].as_i64().unwrap(), 2);
    }

    #[test]
    fn test_cmd_sessions_normalizes_codex_project_without_leading_slash() {
        let conn = setup_test_db();
        let out = cmd_sessions(&conn, "Users/test/codex-proj", Some("codex"), None, 0).unwrap();
        assert_eq!(out.project_name, "/Users/test/codex-proj");
        assert_eq!(out.project_path, "/Users/test/codex-proj");
        assert_eq!(out.sessions.len(), 1);
    }

    #[test]
    fn test_cmd_sessions_defaults_source_to_codex() {
        let conn = setup_test_db();
        let out = cmd_sessions(&conn, "Users/test/codex-proj", None, None, 0).unwrap();
        assert_eq!(out.source, "codex");
        assert_eq!(out.project_name, "/Users/test/codex-proj");
        assert_eq!(out.sessions.len(), 1);
    }

    #[test]
    fn test_resolve_project_for_source_normalizes_codex_path() {
        let conn = setup_test_db();
        let resolved = resolve_project_for_source(&conn, "codex", "Users/test/codex-proj");
        assert_eq!(resolved, "/Users/test/codex-proj");
    }

    #[test]
    fn test_cmd_projects_applies_limit_and_offset_without_reordering() {
        let conn = setup_test_db();
        let out = cmd_projects(&conn, None, Some(1), 1).unwrap();
        assert_eq!(out.projects.len(), 1);
        assert_eq!(out.projects[0].name, "-Users-test-proj");
        assert_eq!(out.projects[0].source, "claude");
        assert_eq!(out.total_messages, 4);
        assert_eq!(out.total_sessions, 2);
    }

    #[test]
    fn test_cmd_sessions_applies_limit_and_offset_without_reordering() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO sessions (session_id, project, source, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages) VALUES ('sess-claude-2', '-Users-test-proj', 'claude', '2025-01-01T00:02:00', '2025-01-01T00:03:00', 1, 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (5, 'claude', '-Users-test-proj', '/Users/test/proj', 'sess-claude-2', FALSE, 'u2', '', 'user', 'user', 'second session question', '', '2025-01-01T00:02:00', '', '', '', '', 0, 0)",
            [],
        )
        .unwrap();

        let out = cmd_sessions(&conn, "-Users-test-proj", Some("claude"), Some(1), 1).unwrap();
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(out.sessions[0].session_id, "sess-claude-1");
    }

    #[test]
    fn test_cmd_messages_applies_limit_and_offset_without_reordering() {
        let conn = setup_test_db();
        let out = cmd_messages(&conn, "sess-claude-1", Some(1), 1).unwrap();
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].role, "user");
    }

    #[tokio::test]
    async fn test_messages_by_session_id() {
        let app = build_test_app(setup_test_db());
        let (status, json) = get_json(app, "/api/messages?session=sess-claude-1").await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(json["session_id"].as_str().unwrap(), "sess-claude-1");
        assert_eq!(json["project_name"].as_str().unwrap(), "-Users-test-proj");
        assert_eq!(json["source"].as_str().unwrap(), "claude");

        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        // Messages are sorted most recent first (DESC by id)
        assert_eq!(messages[0]["role"].as_str().unwrap(), "assistant");
        assert_eq!(messages[1]["role"].as_str().unwrap(), "user");
    }

    #[tokio::test]
    async fn test_search_basic() {
        let app = build_test_app(setup_test_db());
        let (status, json) = get_json(app, "/api/search?q=hello").await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(json["query"].as_str().unwrap(), "hello");
        assert!(json["results"].is_array());
        assert!(json["total_count"].is_number());
        assert_eq!(json["page"].as_i64().unwrap(), 0);
        assert_eq!(json["per_page"].as_i64().unwrap(), 50);
    }

    #[tokio::test]
    async fn test_search_empty_query() {
        let app = build_test_app(setup_test_db());
        let (status, json) = get_json(app, "/api/search?q=").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["total_count"].as_i64().unwrap(), 0);
        assert_eq!(json["results"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_analytics() {
        let app = build_test_app(setup_test_db());
        let (status, json) = get_json(app, "/api/analytics").await;
        assert_eq!(status, StatusCode::OK);

        let source_stats = json["source_stats"].as_array().unwrap();
        assert!(!source_stats.is_empty());

        let model_stats = json["model_stats"].as_array().unwrap();
        assert!(!model_stats.is_empty());

        let project_stats = json["project_stats"].as_array().unwrap();
        assert!(!project_stats.is_empty());
    }

    #[tokio::test]
    async fn test_openapi_spec() {
        let app = build_test_app(setup_test_db());
        let (status, json) = get_json(app, "/openapi.json").await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(json["openapi"].as_str().unwrap(), "3.1.0");
        assert!(json["paths"]["/api/projects"].is_object());
        // IndexParams should no longer appear in the spec
        let spec_str = serde_json::to_string(&json).unwrap();
        assert!(
            !spec_str.contains("IndexParams"),
            "IndexParams should not appear in the OpenAPI spec"
        );
    }

    /// Simulates Claude Code's encoding: `path.replace(/[^a-zA-Z0-9]/g, "-")`
    /// Source: Claude Code binary, function CZT (minified)
    fn claude_code_encode(path: &str) -> String {
        path.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    }

    /// All known project dir names from ~/.claude/projects/ mapped to their
    /// actual cwd paths (extracted from the JSONL session files).
    /// This is ground-truth data from the user's machine.
    fn known_mappings() -> Vec<(&'static str, &'static str)> {
        vec![
            // Simple paths (no ambiguity: only `/` was replaced)
            ("-Users-mish", "/Users/mish"),
            ("-Users-mish-ClaudeOS", "/Users/mish/ClaudeOS"),
            ("-Users-mish-memory", "/Users/mish/memory"),
            (
                "-Users-mish-workspaces-experiments-agpy",
                "/Users/mish/workspaces/experiments/agpy",
            ),
            (
                "-Users-mish-workspaces-experiments-msi",
                "/Users/mish/workspaces/experiments/msi",
            ),
            (
                "-Users-mish-workspaces-games-goodboy",
                "/Users/mish/workspaces/games/goodboy",
            ),
            (
                "-Users-mish-workspaces-sandbox-agpy",
                "/Users/mish/workspaces/sandbox/agpy",
            ),
            (
                "-Users-mish-workspaces-tools-notebooklm",
                "/Users/mish/workspaces/tools/notebooklm",
            ),
            (
                "-Users-mish-workspaces-tools-wit",
                "/Users/mish/workspaces/tools/wit",
            ),
            // Dot-prefixed components: `.foo` -> `-foo`, producing `--` in encoded form
            (
                "-Users-mish--claude-skills-wit",
                "/Users/mish/.claude/skills/wit",
            ),
            ("-Users-mish--warp-themes", "/Users/mish/.warp/themes"),
            (
                "-Users-mish-workspaces-experiments-modal-rs--agents-tasks",
                "/Users/mish/workspaces/experiments/modal-rs/.agents/tasks",
            ),
            // Paths with literal dashes: `-` in original path also becomes `-` (LOSSY)
            (
                "-Users-mish-workspaces-experiments-codex-auth",
                "/Users/mish/workspaces/experiments/codex-auth",
            ),
            (
                "-Users-mish-workspaces-experiments-modal-rs",
                "/Users/mish/workspaces/experiments/modal-rs",
            ),
            (
                "-Users-mish-workspaces-experiments-modal-rs-main-fixed",
                "/Users/mish/workspaces/experiments/modal-rs-main-fixed",
            ),
            (
                "-Users-mish-workspaces-experiments-modal-rs-main-updated",
                "/Users/mish/workspaces/experiments/modal-rs-main-updated",
            ),
            (
                "-Users-mish-workspaces-experiments-modal-rs-main-updated-crates-asi",
                "/Users/mish/workspaces/experiments/modal-rs-main-updated/crates/asi",
            ),
            (
                "-Users-mish-workspaces-tools-perplexity-finance",
                "/Users/mish/workspaces/tools/perplexity-finance",
            ),
            // Underscore in original path: `_` also becomes `-` (LOSSY)
            (
                "-Users-mish-workspaces-experiments-modalrs-optimized",
                "/Users/mish/workspaces/experiments/modalrs_optimized",
            ),
        ]
    }

    #[test]
    fn test_claude_code_encoding_rule() {
        // Verify that our understanding of the encoding rule matches all
        // known real-world project directories.
        for (encoded, actual_path) in known_mappings() {
            let computed = claude_code_encode(actual_path);
            assert_eq!(
                computed, encoded,
                "Encoding mismatch for path {}: expected '{}', got '{}'",
                actual_path, encoded, computed
            );
        }
    }

    #[test]
    fn test_decode_project_name_is_identity_fallback() {
        // decode_project_name is now a no-op fallback: it returns the raw dir
        // name unchanged, since the encoding is lossy and cannot be reversed.
        // The real path comes from extract_project_path_from_sessions().
        assert_eq!(
            decode_project_name("-Users-mish--claude-skills-wit"),
            "-Users-mish--claude-skills-wit"
        );
        assert_eq!(
            decode_project_name("-Users-mish-workspaces-experiments-codex-auth"),
            "-Users-mish-workspaces-experiments-codex-auth"
        );
        assert_eq!(decode_project_name("some-plain-name"), "some-plain-name");
    }

    #[test]
    fn test_encoding_is_lossy() {
        // Prove that the encoding is fundamentally lossy: multiple distinct
        // paths can produce the same encoded directory name.
        let path_with_dash = "/Users/mish/my-project";
        let path_with_slash = "/Users/mish/my/project";
        let path_with_underscore = "/Users/mish/my_project";
        let path_with_space = "/Users/mish/my project";
        let path_with_dot = "/Users/mish/my.project";

        let encoded_dash = claude_code_encode(path_with_dash);
        let encoded_slash = claude_code_encode(path_with_slash);
        let encoded_underscore = claude_code_encode(path_with_underscore);
        let encoded_space = claude_code_encode(path_with_space);
        let encoded_dot = claude_code_encode(path_with_dot);

        // All five produce the exact same encoded string
        assert_eq!(encoded_dash, encoded_slash);
        assert_eq!(encoded_dash, encoded_underscore);
        assert_eq!(encoded_dash, encoded_space);
        assert_eq!(encoded_dash, encoded_dot);
        assert_eq!(encoded_dash, "-Users-mish-my-project");
    }
}
