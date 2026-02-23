use anyhow::Result;
use duckdb::Connection;

use super::source::claude::ingest_claude;
use super::source::codex::ingest_codex;
use super::stats::IngestStats;

pub fn ingest_all(conn: &Connection) -> Result<IngestStats> {
    let mut id_counter: i64 = 0;

    let (cp, cs, cm) = ingest_claude(conn, &mut id_counter)?;
    let (xp, xs, xm) = ingest_codex(conn, &mut id_counter)?;

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
