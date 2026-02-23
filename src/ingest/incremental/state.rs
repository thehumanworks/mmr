use anyhow::Result;
use duckdb::{params, Connection};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub(crate) struct IncrementalRefreshStats {
    pub(crate) inserted_messages: usize,
    pub(crate) removed_messages: usize,
    pub(crate) changed_files: usize,
    pub(crate) unchanged_files: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct IngestFileState {
    pub(crate) project: String,
    pub(crate) project_path: String,
    pub(crate) session_id: String,
    pub(crate) is_subagent: bool,
    pub(crate) last_offset: i64,
    pub(crate) file_size: i64,
    pub(crate) file_mtime_unix: i64,
    pub(crate) last_message_timestamp: String,
    pub(crate) last_message_key: String,
    pub(crate) meta_model: String,
    pub(crate) meta_git_branch: String,
    pub(crate) meta_version: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CodexSessionMeta {
    pub(crate) session_id: String,
    pub(crate) cwd: String,
    pub(crate) model_provider: String,
    pub(crate) git_branch: String,
    pub(crate) cli_version: String,
}

#[derive(Default)]
pub(crate) struct FileIngestOutcome {
    pub(crate) inserted_messages: usize,
    pub(crate) final_offset: u64,
    pub(crate) last_message_timestamp: String,
    pub(crate) last_message_key: String,
}

#[derive(Default)]
pub(crate) struct ClaudeFileIngestOutcome {
    pub(crate) base: FileIngestOutcome,
    pub(crate) session_id: String,
}

#[derive(Default)]
pub(crate) struct CodexFileIngestOutcome {
    pub(crate) base: FileIngestOutcome,
    pub(crate) meta: CodexSessionMeta,
}

pub(crate) enum FileRefreshMode {
    Skip,
    Append(u64),
    Rewrite,
}

pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn metadata_mtime_unix(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub(crate) fn file_set_key(source: &str, file_path: &str) -> String {
    format!("{source}\t{file_path}")
}

pub(crate) fn load_ingest_file_state(
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

pub(crate) fn upsert_ingest_file_state(
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

pub(crate) fn upsert_ingest_project(
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

pub(crate) fn decide_file_refresh_mode(
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

pub(crate) fn delete_messages_for_file(
    conn: &Connection,
    source: &str,
    file_path: &str,
) -> Result<usize> {
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
