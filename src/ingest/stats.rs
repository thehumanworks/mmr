use anyhow::Result;
use duckdb::{params, Connection};

#[derive(Clone, Debug)]
pub struct IngestStats {
    pub claude_projects: usize,
    pub claude_sessions: usize,
    pub claude_messages: usize,
    pub codex_projects: usize,
    pub codex_sessions: usize,
    pub codex_messages: usize,
}

fn count_for_source(conn: &Connection, table: &str, source: &str) -> Result<usize> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE source = ?");
    let n: i64 = conn.query_row(&sql, params![source], |row| row.get(0))?;
    Ok(n as usize)
}

pub fn compute_ingest_stats(conn: &Connection) -> Result<IngestStats> {
    Ok(IngestStats {
        claude_projects: count_for_source(conn, "projects", "claude")?,
        claude_sessions: count_for_source(conn, "sessions", "claude")?,
        claude_messages: count_for_source(conn, "messages", "claude")?,
        codex_projects: count_for_source(conn, "projects", "codex")?,
        codex_sessions: count_for_source(conn, "sessions", "codex")?,
        codex_messages: count_for_source(conn, "messages", "codex")?,
    })
}
