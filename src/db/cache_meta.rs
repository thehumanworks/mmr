use anyhow::Result;
use duckdb::{params, Connection};

use crate::ingest::IngestStats;

const CACHE_SCHEMA_VERSION: &str = "2";

pub fn cache_schema_version() -> &'static str {
    CACHE_SCHEMA_VERSION
}

pub fn write_cache_meta(conn: &Connection, stats: &IngestStats) -> Result<()> {
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
