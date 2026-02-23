use anyhow::{Context, Result};
use duckdb::Connection;
use std::collections::HashSet;

use crate::db::{create_fts_index, rebuild_derived_tables, write_cache_meta};
use crate::ingest::source::common::decode_project_name;
use crate::ingest::source::common::{collect_jsonl_recursive, extract_project_path_from_sessions};
use crate::ingest::stats::compute_ingest_stats;

use super::processors::{
    cleanup_removed_files, process_claude_file_incremental, process_codex_file_incremental,
};
use super::state::IncrementalRefreshStats;
use crate::ingest::IngestStats;

pub fn refresh_incremental_cache(conn: &Connection, quiet: bool) -> Result<IngestStats> {
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
