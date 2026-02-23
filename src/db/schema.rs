use anyhow::Result;
use duckdb::Connection;

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

pub fn init_db(conn: &Connection) -> Result<()> {
    load_fts(conn)?;
    ensure_schema(conn)?;
    Ok(())
}
