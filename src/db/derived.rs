use anyhow::Result;
use duckdb::{params, Connection};

use crate::ingest::now_unix_secs;

pub fn rebuild_derived_tables(conn: &Connection) -> Result<()> {
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
