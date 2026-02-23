use anyhow::Result;
use duckdb::Connection;

pub fn create_fts_index(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("DROP INDEX IF EXISTS fts_idx;");
    conn.execute_batch(
        "PRAGMA create_fts_index('messages', 'id', 'content_text', 'role', 'project', 'msg_type', 'source', overwrite=1);",
    )?;
    Ok(())
}
