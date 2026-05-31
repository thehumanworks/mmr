use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const LATEST_SCHEMA_VERSION: i64 = 2;
pub const DEFAULT_REDACTION_POLICY_ID: &str = "redaction-policy:v1:default";

#[derive(Debug, Clone, Serialize)]
pub struct StoreInfo {
    pub db_path: String,
    pub schema_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub canonical_path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub project_id: String,
    pub source: String,
    pub source_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub id: String,
    pub project_id: String,
    pub session_id: String,
    pub source: String,
    pub source_session_id: String,
    pub source_event_id: Option<String>,
    pub event_type: String,
    pub role: String,
    pub timestamp: String,
    pub content_text: String,
    pub content_hash: String,
    pub parent_hash: Option<String>,
    pub parser_version: String,
    pub raw_local_ref: Option<String>,
    pub sync_status: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SourceEventIdentity<'a> {
    pub project_id: &'a str,
    pub source: &'a str,
    pub source_session_id: &'a str,
    pub source_event_id: Option<&'a str>,
    pub event_type: &'a str,
    pub role: &'a str,
    pub timestamp: &'a str,
    pub parent_hash: Option<&'a str>,
    pub parser_version: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocumentRecord {
    pub id: String,
    pub event_id: String,
    pub project_id: String,
    pub session_id: String,
    pub source: String,
    pub document_text: String,
    pub citation: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RedactionRunRecord {
    pub id: String,
    pub policy_id: String,
    pub event_id: String,
    pub status: String,
    pub blocking_findings: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RedactionSpanRecord {
    pub id: String,
    pub run_id: String,
    pub event_id: String,
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub replacement: String,
    pub confidence: f64,
    pub blocks_sync: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewRedactionSpan {
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub replacement: String,
    pub confidence: f64,
    pub blocks_sync: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRecord {
    pub id: String,
    pub kind: String,
    pub media_type: String,
    pub content_hash: String,
    pub storage_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCursorRecord {
    pub source: String,
    pub project_id: String,
    pub cursor_key: String,
    pub cursor_value: String,
    pub parser_version: String,
    pub last_event_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncManifestRecord {
    pub id: String,
    pub remote: String,
    pub project_id: String,
    pub manifest_version: i64,
    pub root_hash: String,
    pub redaction_policy_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncManifestEntryRecord {
    pub id: String,
    pub manifest_id: String,
    pub entry_kind: String,
    pub entry_ref: String,
    pub content_hash: String,
    pub sync_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSyncManifestEntry {
    pub entry_kind: String,
    pub entry_ref: String,
    pub content_hash: String,
    pub sync_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DreamRunRecord {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub model: String,
    pub status: String,
    pub input_evidence_hash: String,
    pub output_hash: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DreamCandidateRecord {
    pub id: String,
    pub dream_run_id: String,
    pub project_id: String,
    pub kind: String,
    pub claim: String,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub counterevidence_refs: Vec<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LearnedMemoryRecord {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub claim: String,
    pub confidence: f64,
    pub status: String,
    pub evidence_refs: Vec<String>,
    pub counterevidence_refs: Vec<String>,
    pub dream_run_id: Option<String>,
    pub created_at: String,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewDreamCandidate {
    pub kind: String,
    pub claim: String,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub counterevidence_refs: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewLearnedMemory {
    pub kind: String,
    pub claim: String,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub counterevidence_refs: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DreamAssimilationRecord {
    pub run: DreamRunRecord,
    pub candidates: Vec<DreamCandidateRecord>,
    pub learned_memory: Vec<LearnedMemoryRecord>,
}

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub source: String,
    pub source_session_id: String,
    pub source_event_id: Option<String>,
    pub event_type: String,
    pub role: String,
    pub timestamp: String,
    pub content_text: String,
    pub parent_hash: Option<String>,
    pub parser_version: String,
    pub raw_local_ref: Option<String>,
    pub blob_id: Option<String>,
    pub redaction_policy_id: String,
    pub sync_status: String,
}

impl NewEvent {
    pub fn new(
        source: impl Into<String>,
        source_session_id: impl Into<String>,
        event_type: impl Into<String>,
        role: impl Into<String>,
        timestamp: impl Into<String>,
        content_text: impl Into<String>,
        parser_version: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            source_session_id: source_session_id.into(),
            source_event_id: None,
            event_type: event_type.into(),
            role: role.into(),
            timestamp: timestamp.into(),
            content_text: content_text.into(),
            parent_hash: None,
            parser_version: parser_version.into(),
            raw_local_ref: None,
            blob_id: None,
            redaction_policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
            sync_status: "local_only".to_string(),
        }
    }

    pub fn with_source_event_id(mut self, source_event_id: impl Into<String>) -> Self {
        self.source_event_id = Some(source_event_id.into());
        self
    }

    pub fn with_parent_hash(mut self, parent_hash: impl Into<String>) -> Self {
        self.parent_hash = Some(parent_hash.into());
        self
    }

    pub fn with_raw_local_ref(mut self, raw_local_ref: impl Into<String>) -> Self {
        self.raw_local_ref = Some(raw_local_ref.into());
        self
    }

    pub fn with_blob_id(mut self, blob_id: impl Into<String>) -> Self {
        self.blob_id = Some(blob_id.into());
        self
    }

    pub fn content_hash(&self) -> String {
        content_hash(&self.content_text)
    }

    pub fn event_id(&self) -> String {
        event_id(self)
    }
}

#[derive(Debug)]
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_memory_fabric_schema",
        sql: INITIAL_SCHEMA,
    },
    Migration {
        version: 2,
        name: "dream_counterevidence_refs",
        sql: DREAM_COUNTEREVIDENCE_REFS_SCHEMA,
    },
];

const INITIAL_SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE project_aliases (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    source TEXT NOT NULL,
    alias TEXT NOT NULL,
    alias_kind TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    UNIQUE(source, alias)
);

CREATE TABLE project_links (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE sources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    adapter_version TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(enabled IN (0, 1))
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    source TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    title TEXT,
    raw_local_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, source, source_session_id)
);

CREATE TABLE blobs (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    media_type TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    storage_ref TEXT NOT NULL,
    byte_len INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(content_hash, storage_ref)
);

CREATE TABLE events (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    source TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    source_event_id TEXT,
    event_type TEXT NOT NULL,
    role TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    content_text TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    parent_hash TEXT,
    parser_version TEXT NOT NULL,
    raw_local_ref TEXT,
    blob_id TEXT,
    redaction_policy_id TEXT NOT NULL,
    sync_status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY(blob_id) REFERENCES blobs(id) ON DELETE SET NULL,
    FOREIGN KEY(redaction_policy_id) REFERENCES redaction_policies(id),
    UNIQUE(id),
    CHECK(sync_status IN ('local_only', 'pending_redaction', 'redacted', 'synced', 'blocked'))
);

CREATE TABLE source_cursors (
    id TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL,
    project_id TEXT NOT NULL,
    cursor_key TEXT NOT NULL,
    cursor_value TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    last_event_hash TEXT,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    UNIQUE(source, project_id, cursor_key)
);

CREATE TABLE redaction_policies (
    id TEXT PRIMARY KEY NOT NULL,
    version TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(version)
);

CREATE TABLE redaction_runs (
    id TEXT PRIMARY KEY NOT NULL,
    policy_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    status TEXT NOT NULL,
    blocking_findings INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    FOREIGN KEY(policy_id) REFERENCES redaction_policies(id),
    FOREIGN KEY(event_id) REFERENCES events(id) ON DELETE CASCADE,
    CHECK(status IN ('pending', 'passed', 'blocked', 'failed'))
);

CREATE TABLE redaction_spans (
    id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    replacement TEXT NOT NULL,
    confidence REAL NOT NULL,
    blocks_sync INTEGER NOT NULL,
    FOREIGN KEY(run_id) REFERENCES redaction_runs(id) ON DELETE CASCADE,
    FOREIGN KEY(event_id) REFERENCES events(id) ON DELETE CASCADE,
    CHECK(start_byte <= end_byte),
    CHECK(blocks_sync IN (0, 1))
);

CREATE TABLE search_documents (
    id TEXT PRIMARY KEY NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    source TEXT NOT NULL,
    document_text TEXT NOT NULL,
    citation TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(event_id) REFERENCES events(id) ON DELETE CASCADE,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE summaries (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    selection_kind TEXT NOT NULL,
    selection_ref TEXT,
    agent TEXT NOT NULL,
    model TEXT,
    instructions_hash TEXT NOT NULL,
    output_text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    CHECK(selection_kind IN ('latest', 'all', 'session'))
);

CREATE TABLE dream_runs (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL,
    input_evidence_hash TEXT NOT NULL,
    output_hash TEXT,
    created_at TEXT NOT NULL,
    completed_at TEXT,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    CHECK(status IN ('running', 'completed', 'failed'))
);

CREATE TABLE dream_candidates (
    id TEXT PRIMARY KEY NOT NULL,
    dream_run_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    claim TEXT NOT NULL,
    confidence REAL NOT NULL,
    evidence_refs_json TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(dream_run_id) REFERENCES dream_runs(id) ON DELETE CASCADE,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    CHECK(status IN ('pending', 'accepted', 'rejected'))
);

CREATE TABLE learned_memory (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    claim TEXT NOT NULL,
    confidence REAL NOT NULL,
    status TEXT NOT NULL,
    evidence_refs_json TEXT NOT NULL,
    dream_run_id TEXT,
    created_at TEXT NOT NULL,
    superseded_by TEXT,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY(dream_run_id) REFERENCES dream_runs(id) ON DELETE SET NULL,
    FOREIGN KEY(superseded_by) REFERENCES learned_memory(id) ON DELETE SET NULL,
    CHECK(status IN ('active', 'pending', 'superseded', 'rejected'))
);

CREATE TABLE sync_manifests (
    id TEXT PRIMARY KEY NOT NULL,
    remote TEXT NOT NULL,
    project_id TEXT NOT NULL,
    manifest_version INTEGER NOT NULL,
    root_hash TEXT NOT NULL,
    redaction_policy_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY(redaction_policy_id) REFERENCES redaction_policies(id)
);

CREATE TABLE sync_manifest_entries (
    id TEXT PRIMARY KEY NOT NULL,
    manifest_id TEXT NOT NULL,
    entry_kind TEXT NOT NULL,
    entry_ref TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    sync_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(manifest_id) REFERENCES sync_manifests(id) ON DELETE CASCADE,
    UNIQUE(manifest_id, entry_kind, entry_ref)
);

CREATE INDEX idx_project_aliases_project_id ON project_aliases(project_id);
CREATE INDEX idx_sessions_project_source ON sessions(project_id, source, started_at);
CREATE INDEX idx_events_project_session_time ON events(project_id, session_id, timestamp);
CREATE INDEX idx_events_source_session ON events(source, source_session_id, timestamp);
CREATE INDEX idx_events_content_hash ON events(content_hash);
CREATE INDEX idx_source_cursors_project ON source_cursors(project_id, source);
CREATE INDEX idx_redaction_runs_blocking ON redaction_runs(status, blocking_findings);
CREATE INDEX idx_search_documents_project_source ON search_documents(project_id, source);
CREATE INDEX idx_learned_memory_project_status ON learned_memory(project_id, status);
CREATE INDEX idx_sync_manifest_entries_manifest ON sync_manifest_entries(manifest_id);

INSERT INTO redaction_policies (id, version, description, created_at)
VALUES ('redaction-policy:v1:default', 'v1', 'Default deterministic redaction policy placeholder', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

PRAGMA user_version = 1;
"#;

const DREAM_COUNTEREVIDENCE_REFS_SCHEMA: &str = r#"
ALTER TABLE dream_candidates
ADD COLUMN counterevidence_refs_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE learned_memory
ADD COLUMN counterevidence_refs_json TEXT NOT NULL DEFAULT '[]';

PRAGMA user_version = 2;
"#;

#[derive(Debug)]
pub struct Store {
    conn: Connection,
    db_path: PathBuf,
}

impl Store {
    pub fn open_default() -> Result<Self> {
        Self::open(default_db_path()?)
    }

    pub fn open(db_path: impl Into<PathBuf>) -> Result<Self> {
        let db_path = db_path.into();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create store directory {}", parent.display()))?;
        }
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("open mmr store {}", db_path.display()))?;
        run_migrations(&mut conn)?;
        Ok(Self { conn, db_path })
    }

    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory().context("open in-memory mmr store")?;
        run_migrations(&mut conn)?;
        Ok(Self {
            conn,
            db_path: PathBuf::from(":memory:"),
        })
    }

    pub fn info(&self) -> Result<StoreInfo> {
        Ok(StoreInfo {
            db_path: self.db_path.display().to_string(),
            schema_version: self.schema_version()?,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .context("read schema version")
    }

    pub fn ensure_project_link(&self, path: &Path) -> Result<ProjectRecord> {
        let canonical_path = canonical_project_path(path)?;
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(canonical_path.as_str())
            .to_string();
        let project_id = project_id(&canonical_path);
        let now = now_rfc3339()?;

        self.conn.execute(
            "INSERT INTO projects (id, canonical_path, display_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(canonical_path) DO UPDATE SET
                display_name = excluded.display_name,
                updated_at = excluded.updated_at",
            params![project_id, canonical_path, display_name, now],
        )?;

        let link_id = format!("link:v1:{}", hash_hex(canonical_path.as_bytes()));
        self.conn.execute(
            "INSERT INTO project_links (id, project_id, canonical_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(canonical_path) DO UPDATE SET
                project_id = excluded.project_id,
                updated_at = excluded.updated_at",
            params![link_id, project_id, canonical_path, now],
        )?;

        self.conn.execute(
            "INSERT INTO project_aliases (id, project_id, source, alias, alias_kind, created_at)
             VALUES (?1, ?2, 'local', ?3, 'canonical_path', ?4)
             ON CONFLICT(source, alias) DO NOTHING",
            params![
                format!(
                    "alias:v1:{}",
                    hash_hex(format!("local:{canonical_path}").as_bytes())
                ),
                project_id,
                canonical_path,
                now
            ],
        )?;

        self.project_by_id(&project_id)
    }

    pub fn project_by_id(&self, project_id: &str) -> Result<ProjectRecord> {
        self.conn
            .query_row(
                "SELECT id, canonical_path, display_name FROM projects WHERE id = ?1",
                params![project_id],
                |row| {
                    Ok(ProjectRecord {
                        id: row.get(0)?,
                        canonical_path: row.get(1)?,
                        display_name: row.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("project not found: {project_id}"))
    }

    pub fn project_by_path(&self, path: &Path) -> Result<Option<ProjectRecord>> {
        let canonical_path = canonical_project_path(path)?;
        self.conn
            .query_row(
                "SELECT id, canonical_path, display_name FROM projects WHERE canonical_path = ?1",
                params![canonical_path],
                |row| {
                    Ok(ProjectRecord {
                        id: row.get(0)?,
                        canonical_path: row.get(1)?,
                        display_name: row.get(2)?,
                    })
                },
            )
            .optional()
            .context("lookup project by path")
    }

    pub fn upsert_source(&self, name: &str, adapter_version: &str) -> Result<String> {
        let id = format!("source:v1:{}", hash_hex(name.as_bytes()));
        let now = now_rfc3339()?;
        self.conn.execute(
            "INSERT INTO sources (id, name, adapter_version, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?4)
             ON CONFLICT(name) DO UPDATE SET
                adapter_version = excluded.adapter_version,
                enabled = 1,
                updated_at = excluded.updated_at",
            params![id, name, adapter_version, now],
        )?;
        Ok(id)
    }

    pub fn upsert_session(
        &self,
        project_id: &str,
        source: &str,
        source_session_id: &str,
        started_at: &str,
        raw_local_ref: Option<&str>,
    ) -> Result<SessionRecord> {
        self.upsert_source(source, "manual")?;
        let id = session_id(project_id, source, source_session_id);
        let now = now_rfc3339()?;
        self.conn.execute(
            "INSERT INTO sessions
                (id, project_id, source, source_session_id, started_at, raw_local_ref, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(project_id, source, source_session_id) DO UPDATE SET
                started_at = excluded.started_at,
                raw_local_ref = COALESCE(excluded.raw_local_ref, sessions.raw_local_ref),
                updated_at = excluded.updated_at",
            params![id, project_id, source, source_session_id, started_at, raw_local_ref, now],
        )?;
        self.session_by_id(&id)
    }

    pub fn session_by_id(&self, session_id: &str) -> Result<SessionRecord> {
        self.conn
            .query_row(
                "SELECT id, project_id, source, source_session_id FROM sessions WHERE id = ?1",
                params![session_id],
                |row| {
                    Ok(SessionRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        source: row.get(2)?,
                        source_session_id: row.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("session not found: {session_id}"))
    }

    pub fn insert_blob_ref(
        &self,
        kind: &str,
        media_type: &str,
        storage_ref: &str,
        content_hash: &str,
        byte_len: i64,
    ) -> Result<BlobRecord> {
        let id = format!(
            "blob:v1:{}",
            hash_hex(format!("{content_hash}:{storage_ref}").as_bytes())
        );
        let now = now_rfc3339()?;
        self.conn.execute(
            "INSERT INTO blobs (id, kind, media_type, content_hash, storage_ref, byte_len, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(content_hash, storage_ref) DO NOTHING",
            params![id, kind, media_type, content_hash, storage_ref, byte_len, now],
        )?;
        self.blob_by_id(&id)
    }

    pub fn blob_by_id(&self, blob_id: &str) -> Result<BlobRecord> {
        self.conn
            .query_row(
                "SELECT id, kind, media_type, content_hash, storage_ref FROM blobs WHERE id = ?1",
                params![blob_id],
                |row| {
                    Ok(BlobRecord {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        media_type: row.get(2)?,
                        content_hash: row.get(3)?,
                        storage_ref: row.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("blob not found: {blob_id}"))
    }

    pub fn insert_event(&mut self, project_id: &str, event: &NewEvent) -> Result<EventRecord> {
        let id = event.event_id();
        self.insert_event_in_transaction(project_id, event, None)?;

        self.event_by_id(&id)
    }

    pub fn insert_event_with_search_document(
        &mut self,
        project_id: &str,
        event: &NewEvent,
    ) -> Result<(EventRecord, SearchDocumentRecord)> {
        let id = event.event_id();
        let tx = self.conn.transaction()?;
        Self::insert_event_on_transaction(&tx, project_id, event, None)?;
        let event_record = Self::event_by_id_on_conn(&tx, &id)?;
        let search_document = Self::upsert_search_document_on_conn(&tx, &event_record)?;
        tx.commit()?;

        Ok((event_record, search_document))
    }

    fn insert_event_in_transaction(
        &mut self,
        project_id: &str,
        event: &NewEvent,
        fail_after_session: Option<&str>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        Self::insert_event_on_transaction(&tx, project_id, event, fail_after_session)?;
        tx.commit()?;
        Ok(())
    }

    fn insert_event_on_transaction(
        tx: &rusqlite::Transaction<'_>,
        project_id: &str,
        event: &NewEvent,
        fail_after_session: Option<&str>,
    ) -> Result<()> {
        let now = now_rfc3339()?;
        let source_id = format!("source:v1:{}", hash_hex(event.source.as_bytes()));
        tx.execute(
            "INSERT INTO sources (id, name, adapter_version, enabled, created_at, updated_at)
             VALUES (?1, ?2, 'manual', 1, ?3, ?3)
             ON CONFLICT(name) DO UPDATE SET
                enabled = 1,
                updated_at = excluded.updated_at",
            params![source_id, event.source, now],
        )?;

        let session_id = session_id(project_id, &event.source, &event.source_session_id);
        tx.execute(
            "INSERT INTO sessions
                (id, project_id, source, source_session_id, started_at, raw_local_ref, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(project_id, source, source_session_id) DO UPDATE SET
                started_at = excluded.started_at,
                raw_local_ref = COALESCE(excluded.raw_local_ref, sessions.raw_local_ref),
                updated_at = excluded.updated_at",
            params![
                session_id,
                project_id,
                event.source,
                event.source_session_id,
                event.timestamp,
                event.raw_local_ref,
                now
            ],
        )?;

        if let Some(message) = fail_after_session {
            bail!("{message}");
        }

        tx.execute(
            "INSERT INTO events
                (id, project_id, session_id, source, source_session_id, source_event_id,
                 event_type, role, timestamp, content_text, content_hash, parent_hash,
                 parser_version, raw_local_ref, blob_id, redaction_policy_id, sync_status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
             ON CONFLICT(id) DO NOTHING",
            params![
                event.event_id(),
                project_id,
                session_id,
                event.source,
                event.source_session_id,
                event.source_event_id,
                event.event_type,
                event.role,
                event.timestamp,
                event.content_text,
                event.content_hash(),
                event.parent_hash,
                event.parser_version,
                event.raw_local_ref,
                event.blob_id,
                event.redaction_policy_id,
                event.sync_status,
                now
            ],
        )?;
        Ok(())
    }

    pub fn event_by_id(&self, event_id: &str) -> Result<EventRecord> {
        Self::event_by_id_on_conn(&self.conn, event_id)
    }

    fn event_by_id_on_conn(conn: &Connection, event_id: &str) -> Result<EventRecord> {
        conn.query_row(
            "SELECT id, project_id, session_id, source, source_session_id, source_event_id, event_type, role,
                    timestamp, content_text, content_hash, parent_hash, parser_version,
                    raw_local_ref, sync_status
             FROM events WHERE id = ?1",
            params![event_id],
            event_record_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("event not found: {event_id}"))
    }

    pub fn event_exists(&self, event_id: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE id = ?1)",
                params![event_id],
                |row| row.get(0),
            )
            .context("check event existence")
    }

    pub fn event_by_source_identity(
        &self,
        identity: SourceEventIdentity<'_>,
    ) -> Result<Option<EventRecord>> {
        self.conn
            .query_row(
                "SELECT id, project_id, session_id, source, source_session_id, source_event_id, event_type, role,
                    timestamp, content_text, content_hash, parent_hash, parser_version,
                    raw_local_ref, sync_status
                 FROM events
                 WHERE project_id = ?1
                    AND source = ?2
                    AND source_session_id = ?3
                    AND ((source_event_id IS NULL AND ?4 IS NULL) OR source_event_id = ?4)
                    AND event_type = ?5
                    AND role = ?6
                    AND timestamp = ?7
                    AND ((parent_hash IS NULL AND ?8 IS NULL) OR parent_hash = ?8)
                    AND parser_version = ?9
                 ORDER BY id ASC
                 LIMIT 1",
                params![
                    identity.project_id,
                    identity.source,
                    identity.source_session_id,
                    identity.source_event_id,
                    identity.event_type,
                    identity.role,
                    identity.timestamp,
                    identity.parent_hash,
                    identity.parser_version
                ],
                event_record_from_row,
            )
            .optional()
            .context("lookup event by source identity")
    }

    pub fn upsert_search_document(&self, event: &EventRecord) -> Result<SearchDocumentRecord> {
        Self::upsert_search_document_on_conn(&self.conn, event)
    }

    fn upsert_search_document_on_conn(
        conn: &Connection,
        event: &EventRecord,
    ) -> Result<SearchDocumentRecord> {
        let id = format!("search-doc:v1:{}", hash_hex(event.id.as_bytes()));
        let citation = format!("mmr://event/{}", event.id);
        let now = now_rfc3339()?;
        conn.execute(
            "INSERT INTO search_documents
                (id, event_id, project_id, session_id, source, document_text, citation, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(event_id) DO UPDATE SET
                document_text = excluded.document_text,
                citation = excluded.citation,
                updated_at = excluded.updated_at",
            params![
                id,
                event.id,
                event.project_id,
                event.session_id,
                event.source,
                event.content_text,
                citation,
                now
            ],
        )?;
        Self::search_document_by_event_on_conn(conn, &event.id)
    }

    pub fn search_document_by_event(&self, event_id: &str) -> Result<SearchDocumentRecord> {
        Self::search_document_by_event_on_conn(&self.conn, event_id)
    }

    fn search_document_by_event_on_conn(
        conn: &Connection,
        event_id: &str,
    ) -> Result<SearchDocumentRecord> {
        conn.query_row(
            "SELECT id, event_id, project_id, session_id, source, document_text, citation
             FROM search_documents WHERE event_id = ?1",
            params![event_id],
            search_document_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("search document not found for event: {event_id}"))
    }

    pub fn record_redaction_result(
        &mut self,
        event_id: &str,
        policy_id: &str,
        status: &str,
        spans: &[NewRedactionSpan],
    ) -> Result<RedactionRunRecord> {
        let tx = self.conn.transaction()?;
        let run_id = redaction_run_id(event_id, policy_id);
        let now = now_rfc3339()?;
        let blocking_findings = spans.iter().filter(|span| span.blocks_sync).count() as i64;

        tx.execute(
            "INSERT INTO redaction_runs
                (id, policy_id, event_id, status, blocking_findings, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                blocking_findings = excluded.blocking_findings,
                created_at = excluded.created_at",
            params![run_id, policy_id, event_id, status, blocking_findings, now],
        )?;
        tx.execute(
            "DELETE FROM redaction_spans WHERE run_id = ?1",
            params![run_id],
        )?;

        for span in spans {
            let id = redaction_span_id(&run_id, event_id, span);
            tx.execute(
                "INSERT INTO redaction_spans
                    (id, run_id, event_id, kind, start_byte, end_byte, replacement, confidence, blocks_sync)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    run_id,
                    event_id,
                    span.kind,
                    i64::try_from(span.start_byte).context("redaction start_byte overflows i64")?,
                    i64::try_from(span.end_byte).context("redaction end_byte overflows i64")?,
                    span.replacement,
                    span.confidence,
                    if span.blocks_sync { 1 } else { 0 },
                ],
            )?;
        }

        let sync_status = match status {
            "blocked" => "blocked",
            "passed" => "redacted",
            "failed" => "blocked",
            _ => "pending_redaction",
        };
        tx.execute(
            "UPDATE events SET sync_status = ?1 WHERE id = ?2",
            params![sync_status, event_id],
        )?;
        tx.commit()?;

        self.latest_redaction_run_for_event(event_id)?
            .ok_or_else(|| anyhow!("redaction run not found after recording: {event_id}"))
    }

    pub fn latest_redaction_run_for_event(
        &self,
        event_id: &str,
    ) -> Result<Option<RedactionRunRecord>> {
        self.conn
            .query_row(
                "SELECT id, policy_id, event_id, status, blocking_findings, created_at
                 FROM redaction_runs
                 WHERE event_id = ?1
                 ORDER BY created_at DESC, id DESC
                 LIMIT 1",
                params![event_id],
                redaction_run_from_row,
            )
            .optional()
            .context("read latest redaction run")
    }

    pub fn redaction_spans_for_run(&self, run_id: &str) -> Result<Vec<RedactionSpanRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, event_id, kind, start_byte, end_byte, replacement, confidence, blocks_sync
             FROM redaction_spans
             WHERE run_id = ?1
             ORDER BY start_byte ASC, end_byte ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![run_id], redaction_span_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read redaction spans")
    }

    pub fn events_for_project(
        &self,
        project_id: &str,
        source: Option<&str>,
        source_session_id: Option<&str>,
    ) -> Result<Vec<EventRecord>> {
        let mut sql = String::from(
            "SELECT id, project_id, session_id, source, source_session_id, source_event_id, event_type, role,
                    timestamp, content_text, content_hash, parent_hash, parser_version,
                    raw_local_ref, sync_status
             FROM events
             WHERE project_id = ?1",
        );
        if source.is_some() {
            sql.push_str(" AND source = ?2");
        }
        if source_session_id.is_some() {
            sql.push_str(if source.is_some() {
                " AND source_session_id = ?3"
            } else {
                " AND source_session_id = ?2"
            });
        }
        sql.push_str(" ORDER BY timestamp ASC, id ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = match (source, source_session_id) {
            (Some(source), Some(source_session_id)) => {
                stmt.query(params![project_id, source, source_session_id])?
            }
            (Some(source), None) => stmt.query(params![project_id, source])?,
            (None, Some(source_session_id)) => {
                stmt.query(params![project_id, source_session_id])?
            }
            (None, None) => stmt.query(params![project_id])?,
        };

        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            events.push(EventRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                session_id: row.get(2)?,
                source: row.get(3)?,
                source_session_id: row.get(4)?,
                source_event_id: row.get(5)?,
                event_type: row.get(6)?,
                role: row.get(7)?,
                timestamp: row.get(8)?,
                content_text: row.get(9)?,
                content_hash: row.get(10)?,
                parent_hash: row.get(11)?,
                parser_version: row.get(12)?,
                raw_local_ref: row.get(13)?,
                sync_status: row.get(14)?,
            });
        }
        Ok(events)
    }

    pub fn events_for_source(
        &self,
        source: &str,
        since: Option<&str>,
        limit_per_project: Option<usize>,
    ) -> Result<Vec<EventRecord>> {
        let mut sql = String::from(
            "SELECT id, project_id, session_id, source, source_session_id, source_event_id, event_type, role,
                    timestamp, content_text, content_hash, parent_hash, parser_version,
                    raw_local_ref, sync_status
             FROM events
             WHERE source = ?1",
        );
        if since.is_some() {
            sql.push_str(" AND timestamp >= ?2");
        }
        sql.push_str(" ORDER BY project_id ASC, timestamp ASC, id ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match since {
            Some(since) => stmt.query_map(params![source, since], event_record_from_row)?,
            None => stmt.query_map(params![source], event_record_from_row)?,
        };
        let mut events = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("read events for source")?;

        if let Some(limit) = limit_per_project {
            if limit == 0 {
                return Ok(Vec::new());
            }
            let mut by_project: Vec<EventRecord> = Vec::new();
            let mut current_project = String::new();
            let mut current_events = Vec::new();
            for event in events {
                if current_project.is_empty() {
                    current_project = event.project_id.clone();
                }
                if event.project_id != current_project {
                    append_project_window(&mut by_project, current_events, limit);
                    current_events = Vec::new();
                    current_project = event.project_id.clone();
                }
                current_events.push(event);
            }
            append_project_window(&mut by_project, current_events, limit);
            events = by_project;
        }

        Ok(events)
    }

    pub fn events_for_source_session(
        &self,
        source_session_id: &str,
        source: Option<&str>,
    ) -> Result<Vec<EventRecord>> {
        let mut sql = String::from(
            "SELECT id, project_id, session_id, source, source_session_id, source_event_id, event_type, role,
                    timestamp, content_text, content_hash, parent_hash, parser_version,
                    raw_local_ref, sync_status
             FROM events
             WHERE source_session_id = ?1",
        );
        if source.is_some() {
            sql.push_str(" AND source = ?2");
        }
        sql.push_str(" ORDER BY project_id ASC, timestamp ASC, id ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match source {
            Some(source) => {
                stmt.query_map(params![source_session_id, source], event_record_from_row)?
            }
            None => stmt.query_map(params![source_session_id], event_record_from_row)?,
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read events for source session")
    }

    pub fn set_source_cursor(
        &self,
        project_id: &str,
        source: &str,
        cursor_key: &str,
        cursor_value: &str,
        parser_version: &str,
        last_event_hash: Option<&str>,
    ) -> Result<()> {
        let id = format!(
            "cursor:v1:{}",
            hash_hex(format!("{source}:{project_id}:{cursor_key}").as_bytes())
        );
        let now = now_rfc3339()?;
        self.conn.execute(
            "INSERT INTO source_cursors
                (id, source, project_id, cursor_key, cursor_value, parser_version, last_event_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(source, project_id, cursor_key) DO UPDATE SET
                cursor_value = excluded.cursor_value,
                parser_version = excluded.parser_version,
                last_event_hash = excluded.last_event_hash,
                updated_at = excluded.updated_at",
            params![id, source, project_id, cursor_key, cursor_value, parser_version, last_event_hash, now],
        )?;
        Ok(())
    }

    pub fn source_cursor(
        &self,
        project_id: &str,
        source: &str,
        cursor_key: &str,
    ) -> Result<Option<SourceCursorRecord>> {
        self.conn
            .query_row(
                "SELECT source, project_id, cursor_key, cursor_value, parser_version, last_event_hash
                 FROM source_cursors
                 WHERE project_id = ?1 AND source = ?2 AND cursor_key = ?3",
                params![project_id, source, cursor_key],
                |row| {
                    Ok(SourceCursorRecord {
                        source: row.get(0)?,
                        project_id: row.get(1)?,
                        cursor_key: row.get(2)?,
                        cursor_value: row.get(3)?,
                        parser_version: row.get(4)?,
                        last_event_hash: row.get(5)?,
                    })
                },
            )
            .optional()
            .context("read source cursor")
    }

    pub fn mark_events_synced(&self, event_ids: &[String]) -> Result<()> {
        for event_id in event_ids {
            self.conn.execute(
                "UPDATE events SET sync_status = 'synced' WHERE id = ?1",
                params![event_id],
            )?;
        }
        Ok(())
    }

    pub fn record_sync_manifest(
        &self,
        remote: &str,
        project_id: &str,
        manifest_version: i64,
        root_hash: &str,
        redaction_policy_id: &str,
        entries: &[NewSyncManifestEntry],
    ) -> Result<SyncManifestRecord> {
        let manifest_id = sync_manifest_id(remote, project_id, root_hash, redaction_policy_id);
        let now = now_rfc3339()?;
        self.conn.execute(
            "INSERT INTO sync_manifests
                (id, remote, project_id, manifest_version, root_hash, redaction_policy_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO NOTHING",
            params![
                manifest_id,
                remote,
                project_id,
                manifest_version,
                root_hash,
                redaction_policy_id,
                now
            ],
        )?;

        for entry in entries {
            let entry_id = sync_manifest_entry_id(&manifest_id, entry);
            self.conn.execute(
                "INSERT INTO sync_manifest_entries
                    (id, manifest_id, entry_kind, entry_ref, content_hash, sync_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(manifest_id, entry_kind, entry_ref) DO NOTHING",
                params![
                    entry_id,
                    manifest_id,
                    entry.entry_kind,
                    entry.entry_ref,
                    entry.content_hash,
                    entry.sync_path,
                    now
                ],
            )?;
        }

        self.sync_manifest_by_id(&manifest_id)
    }

    pub fn sync_manifests_for_project(&self, project_id: &str) -> Result<Vec<SyncManifestRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, remote, project_id, manifest_version, root_hash, redaction_policy_id, created_at
             FROM sync_manifests
             WHERE project_id = ?1
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![project_id], sync_manifest_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read sync manifests")
    }

    pub fn sync_manifest_entries(&self, manifest_id: &str) -> Result<Vec<SyncManifestEntryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, manifest_id, entry_kind, entry_ref, content_hash, sync_path, created_at
             FROM sync_manifest_entries
             WHERE manifest_id = ?1
             ORDER BY entry_kind ASC, entry_ref ASC, sync_path ASC",
        )?;
        let rows = stmt.query_map(params![manifest_id], sync_manifest_entry_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read sync manifest entries")
    }

    fn sync_manifest_by_id(&self, manifest_id: &str) -> Result<SyncManifestRecord> {
        self.conn
            .query_row(
                "SELECT id, remote, project_id, manifest_version, root_hash, redaction_policy_id, created_at
                 FROM sync_manifests WHERE id = ?1",
                params![manifest_id],
                sync_manifest_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("sync manifest not found: {manifest_id}"))
    }

    pub fn start_dream_run(
        &mut self,
        project_id: &str,
        provider: &str,
        model: &str,
        input_evidence_hash: &str,
    ) -> Result<DreamRunRecord> {
        let now = now_rfc3339()?;
        let id = dream_run_id(project_id, provider, model, input_evidence_hash, &now);
        self.conn.execute(
            "INSERT INTO dream_runs
                (id, project_id, provider, model, status, input_evidence_hash, output_hash, created_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, 'running', ?5, NULL, ?6, NULL)",
            params![id, project_id, provider, model, input_evidence_hash, now],
        )?;
        self.dream_run_by_id(&id)
    }

    pub fn fail_dream_run(
        &mut self,
        dream_run_id: &str,
        output_hash: Option<&str>,
    ) -> Result<DreamRunRecord> {
        let now = now_rfc3339()?;
        let updated = self.conn.execute(
            "UPDATE dream_runs
             SET status = 'failed', output_hash = ?2, completed_at = ?3
             WHERE id = ?1 AND status = 'running'",
            params![dream_run_id, output_hash, now],
        )?;
        if updated == 0 {
            bail!("dream run is not running or does not exist: {dream_run_id}");
        }
        self.dream_run_by_id(dream_run_id)
    }

    pub fn complete_dream_run(
        &mut self,
        dream_run_id: &str,
        output_hash: &str,
        candidates: &[NewDreamCandidate],
        learned_memory: &[NewLearnedMemory],
    ) -> Result<DreamAssimilationRecord> {
        let now = now_rfc3339()?;
        let tx = self.conn.transaction()?;
        let updated = tx.execute(
            "UPDATE dream_runs
             SET status = 'completed', output_hash = ?2, completed_at = ?3
             WHERE id = ?1 AND status = 'running'",
            params![dream_run_id, output_hash, now],
        )?;
        if updated == 0 {
            bail!("dream run is not running or does not exist: {dream_run_id}");
        }

        let project_id: String = tx.query_row(
            "SELECT project_id FROM dream_runs WHERE id = ?1",
            params![dream_run_id],
            |row| row.get(0),
        )?;

        for candidate in candidates {
            validate_dream_candidate_status(&candidate.status)?;
            validate_required_evidence_refs_on_conn(
                &tx,
                "dream candidate",
                &project_id,
                &candidate.evidence_refs,
            )?;
            validate_optional_evidence_refs_on_conn(
                &tx,
                &project_id,
                &candidate.counterevidence_refs,
            )?;
            let candidate_id = dream_candidate_id(dream_run_id, candidate);
            let evidence_refs_json = refs_json(&candidate.evidence_refs)?;
            let counterevidence_refs_json = refs_json(&candidate.counterevidence_refs)?;
            tx.execute(
                "INSERT INTO dream_candidates
                    (id, dream_run_id, project_id, kind, claim, confidence, evidence_refs_json,
                     status, created_at, counterevidence_refs_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    confidence = excluded.confidence,
                    evidence_refs_json = excluded.evidence_refs_json,
                    counterevidence_refs_json = excluded.counterevidence_refs_json,
                    status = excluded.status",
                params![
                    candidate_id,
                    dream_run_id,
                    project_id,
                    candidate.kind,
                    candidate.claim,
                    candidate.confidence,
                    evidence_refs_json,
                    candidate.status,
                    now,
                    counterevidence_refs_json,
                ],
            )?;
        }

        for memory in learned_memory {
            validate_learned_memory_status(&memory.status)?;
            let memory_id = learned_memory_id(&project_id, memory);
            validate_required_evidence_refs_on_conn(
                &tx,
                "learned memory",
                &project_id,
                &memory.evidence_refs,
            )?;
            validate_optional_evidence_refs_on_conn(
                &tx,
                &project_id,
                &memory.counterevidence_refs,
            )?;
            let evidence_refs_json = refs_json(&memory.evidence_refs)?;
            let counterevidence_refs_json = refs_json(&memory.counterevidence_refs)?;
            tx.execute(
                "INSERT INTO learned_memory
                    (id, project_id, kind, claim, confidence, status, evidence_refs_json,
                     dream_run_id, created_at, superseded_by, counterevidence_refs_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    memory_id,
                    project_id,
                    memory.kind,
                    memory.claim,
                    memory.confidence,
                    memory.status,
                    evidence_refs_json,
                    dream_run_id,
                    now,
                    counterevidence_refs_json,
                ],
            )?;
        }

        tx.commit()?;
        Ok(DreamAssimilationRecord {
            run: self.dream_run_by_id(dream_run_id)?,
            candidates: self.dream_candidates_for_run(dream_run_id)?,
            learned_memory: self.learned_memory_for_dream_run(dream_run_id)?,
        })
    }

    pub fn upsert_learned_memory_from_sync(
        &self,
        memory_id: &str,
        project_id: &str,
        memory: &NewLearnedMemory,
        created_at: &str,
    ) -> Result<LearnedMemoryRecord> {
        validate_learned_memory_status(&memory.status)?;
        validate_required_evidence_refs_on_conn(
            &self.conn,
            "learned memory",
            project_id,
            &memory.evidence_refs,
        )?;
        validate_optional_evidence_refs_on_conn(
            &self.conn,
            project_id,
            &memory.counterevidence_refs,
        )?;
        let evidence_refs_json = refs_json(&memory.evidence_refs)?;
        let counterevidence_refs_json = refs_json(&memory.counterevidence_refs)?;
        self.conn.execute(
            "INSERT INTO learned_memory
                (id, project_id, kind, claim, confidence, status, evidence_refs_json,
                 dream_run_id, created_at, superseded_by, counterevidence_refs_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, NULL, ?9)
             ON CONFLICT(id) DO UPDATE SET
                confidence = excluded.confidence,
                status = excluded.status,
                evidence_refs_json = excluded.evidence_refs_json,
                counterevidence_refs_json = excluded.counterevidence_refs_json",
            params![
                memory_id,
                project_id,
                memory.kind,
                memory.claim,
                memory.confidence,
                memory.status,
                evidence_refs_json,
                created_at,
                counterevidence_refs_json,
            ],
        )?;
        self.learned_memory_by_id(memory_id)
    }

    pub fn dream_run_by_id(&self, dream_run_id: &str) -> Result<DreamRunRecord> {
        self.conn
            .query_row(
                "SELECT id, project_id, provider, model, status, input_evidence_hash,
                        output_hash, created_at, completed_at
                 FROM dream_runs WHERE id = ?1",
                params![dream_run_id],
                dream_run_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("dream run not found: {dream_run_id}"))
    }

    pub fn dream_runs_for_project(&self, project_id: &str) -> Result<Vec<DreamRunRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, provider, model, status, input_evidence_hash,
                    output_hash, created_at, completed_at
             FROM dream_runs
             WHERE project_id = ?1
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![project_id], dream_run_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read dream runs")
    }

    pub fn dream_candidates_for_run(
        &self,
        dream_run_id: &str,
    ) -> Result<Vec<DreamCandidateRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, dream_run_id, project_id, kind, claim, confidence,
                    evidence_refs_json, status, created_at, counterevidence_refs_json
             FROM dream_candidates
             WHERE dream_run_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![dream_run_id], dream_candidate_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read dream candidates")
    }

    pub fn learned_memory_for_project(&self, project_id: &str) -> Result<Vec<LearnedMemoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, kind, claim, confidence, status, evidence_refs_json,
                    dream_run_id, created_at, superseded_by, counterevidence_refs_json
             FROM learned_memory
             WHERE project_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![project_id], learned_memory_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read learned memory")
    }

    pub fn learned_memory_by_id(&self, memory_id: &str) -> Result<LearnedMemoryRecord> {
        self.conn
            .query_row(
                "SELECT id, project_id, kind, claim, confidence, status, evidence_refs_json,
                        dream_run_id, created_at, superseded_by, counterevidence_refs_json
                 FROM learned_memory WHERE id = ?1",
                params![memory_id],
                learned_memory_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("learned memory not found: {memory_id}"))
    }

    pub fn learned_memory_for_dream_run(
        &self,
        dream_run_id: &str,
    ) -> Result<Vec<LearnedMemoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, kind, claim, confidence, status, evidence_refs_json,
                    dream_run_id, created_at, superseded_by, counterevidence_refs_json
             FROM learned_memory
             WHERE dream_run_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![dream_run_id], learned_memory_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read learned memory for dream run")
    }

    pub fn event_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .context("count events")
    }

    pub fn schema_table_names(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()
            .context("list table names")
    }

    #[cfg(test)]
    fn insert_event_then_fail(&mut self, project_id: &str, event: &NewEvent) -> Result<()> {
        self.insert_event_in_transaction(project_id, event, Some("intentional rollback"))
    }

    #[cfg(test)]
    fn insert_event_and_search_document_then_fail(
        &mut self,
        project_id: &str,
        event: &NewEvent,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        Self::insert_event_on_transaction(&tx, project_id, event, None)?;
        let event_record = Self::event_by_id_on_conn(&tx, &event.event_id())?;
        Self::upsert_search_document_on_conn(&tx, &event_record)?;
        bail!("intentional rollback after search document")
    }
}

pub fn default_db_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(dirs::data_dir)
        .ok_or_else(|| anyhow!("could not resolve data directory for mmr store"))?;
    Ok(base.join("mmr").join("mmr.db"))
}

fn event_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRecord> {
    Ok(EventRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        source: row.get(3)?,
        source_session_id: row.get(4)?,
        source_event_id: row.get(5)?,
        event_type: row.get(6)?,
        role: row.get(7)?,
        timestamp: row.get(8)?,
        content_text: row.get(9)?,
        content_hash: row.get(10)?,
        parent_hash: row.get(11)?,
        parser_version: row.get(12)?,
        raw_local_ref: row.get(13)?,
        sync_status: row.get(14)?,
    })
}

fn append_project_window(
    target: &mut Vec<EventRecord>,
    mut events: Vec<EventRecord>,
    limit: usize,
) {
    if events.len() > limit {
        let start = events.len() - limit;
        events = events.split_off(start);
    }
    target.extend(events);
}

fn search_document_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchDocumentRecord> {
    Ok(SearchDocumentRecord {
        id: row.get(0)?,
        event_id: row.get(1)?,
        project_id: row.get(2)?,
        session_id: row.get(3)?,
        source: row.get(4)?,
        document_text: row.get(5)?,
        citation: row.get(6)?,
    })
}

fn redaction_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RedactionRunRecord> {
    Ok(RedactionRunRecord {
        id: row.get(0)?,
        policy_id: row.get(1)?,
        event_id: row.get(2)?,
        status: row.get(3)?,
        blocking_findings: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn redaction_span_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RedactionSpanRecord> {
    let start_byte: i64 = row.get(4)?;
    let end_byte: i64 = row.get(5)?;
    let blocks_sync: i64 = row.get(8)?;
    Ok(RedactionSpanRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        event_id: row.get(2)?,
        kind: row.get(3)?,
        start_byte: usize::try_from(start_byte)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, start_byte))?,
        end_byte: usize::try_from(end_byte)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, end_byte))?,
        replacement: row.get(6)?,
        confidence: row.get(7)?,
        blocks_sync: blocks_sync == 1,
    })
}

fn sync_manifest_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SyncManifestRecord> {
    Ok(SyncManifestRecord {
        id: row.get(0)?,
        remote: row.get(1)?,
        project_id: row.get(2)?,
        manifest_version: row.get(3)?,
        root_hash: row.get(4)?,
        redaction_policy_id: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn sync_manifest_entry_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SyncManifestEntryRecord> {
    Ok(SyncManifestEntryRecord {
        id: row.get(0)?,
        manifest_id: row.get(1)?,
        entry_kind: row.get(2)?,
        entry_ref: row.get(3)?,
        content_hash: row.get(4)?,
        sync_path: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn dream_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DreamRunRecord> {
    Ok(DreamRunRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        provider: row.get(2)?,
        model: row.get(3)?,
        status: row.get(4)?,
        input_evidence_hash: row.get(5)?,
        output_hash: row.get(6)?,
        created_at: row.get(7)?,
        completed_at: row.get(8)?,
    })
}

fn dream_candidate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DreamCandidateRecord> {
    Ok(DreamCandidateRecord {
        id: row.get(0)?,
        dream_run_id: row.get(1)?,
        project_id: row.get(2)?,
        kind: row.get(3)?,
        claim: row.get(4)?,
        confidence: row.get(5)?,
        evidence_refs: refs_from_row(row, 6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        counterevidence_refs: refs_from_row(row, 9)?,
    })
}

fn learned_memory_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LearnedMemoryRecord> {
    Ok(LearnedMemoryRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        kind: row.get(2)?,
        claim: row.get(3)?,
        confidence: row.get(4)?,
        status: row.get(5)?,
        evidence_refs: refs_from_row(row, 6)?,
        dream_run_id: row.get(7)?,
        created_at: row.get(8)?,
        superseded_by: row.get(9)?,
        counterevidence_refs: refs_from_row(row, 10)?,
    })
}

fn refs_from_row(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Vec<String>> {
    let json: String = row.get(index)?;
    serde_json::from_str(&json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

pub fn content_hash(content: &str) -> String {
    format!("sha256:{}", hash_hex(content.as_bytes()))
}

pub fn event_id(event: &NewEvent) -> String {
    let source_event_id = event.source_event_id.as_deref().unwrap_or("");
    let parent_hash = event.parent_hash.as_deref().unwrap_or("");
    let identity = format!(
        "{{\"source\":\"{}\",\"source_session_id\":\"{}\",\"source_event_id\":\"{}\",\"event_type\":\"{}\",\"role\":\"{}\",\"timestamp\":\"{}\",\"content_hash\":\"{}\",\"parent_hash\":\"{}\",\"parser_version\":\"{}\"}}",
        escape_json_fragment(&event.source),
        escape_json_fragment(&event.source_session_id),
        escape_json_fragment(source_event_id),
        escape_json_fragment(&event.event_type),
        escape_json_fragment(&event.role),
        escape_json_fragment(&event.timestamp),
        escape_json_fragment(&event.content_hash()),
        escape_json_fragment(parent_hash),
        escape_json_fragment(&event.parser_version),
    );
    format!("evt:v1:{}", hash_hex(identity.as_bytes()))
}

fn run_migrations(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL,
            checksum TEXT NOT NULL
         );",
    )?;

    for migration in MIGRATIONS {
        let checksum = content_hash(migration.sql);
        let existing_checksum = conn
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![migration.version],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        if let Some(existing_checksum) = existing_checksum {
            if existing_checksum != checksum {
                bail!(
                    "migration checksum drift for version {}: expected {}, found {}",
                    migration.version,
                    existing_checksum,
                    checksum
                );
            }
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at, checksum)
             VALUES (?1, ?2, ?3, ?4)",
            params![migration.version, migration.name, now_rfc3339()?, checksum],
        )?;
        tx.commit()?;
    }

    Ok(())
}

fn canonical_project_path(path: &Path) -> Result<String> {
    Ok(path
        .canonicalize()
        .with_context(|| format!("canonicalize project path {}", path.display()))?
        .to_string_lossy()
        .into_owned())
}

fn project_id(canonical_path: &str) -> String {
    format!("proj:v1:{}", hash_hex(canonical_path.as_bytes()))
}

fn session_id(project_id: &str, source: &str, source_session_id: &str) -> String {
    format!(
        "session:v1:{}",
        hash_hex(format!("{project_id}:{source}:{source_session_id}").as_bytes())
    )
}

fn redaction_run_id(event_id: &str, policy_id: &str) -> String {
    format!(
        "redaction-run:v1:{}",
        hash_hex(format!("{event_id}:{policy_id}").as_bytes())
    )
}

fn redaction_span_id(run_id: &str, event_id: &str, span: &NewRedactionSpan) -> String {
    format!(
        "redaction-span:v1:{}",
        hash_hex(
            format!(
                "{}:{}:{}:{}:{}:{}",
                run_id, event_id, span.kind, span.start_byte, span.end_byte, span.replacement
            )
            .as_bytes()
        )
    )
}

fn sync_manifest_id(
    remote: &str,
    project_id: &str,
    root_hash: &str,
    redaction_policy_id: &str,
) -> String {
    format!(
        "sync-manifest:v1:{}",
        hash_hex(format!("{remote}:{project_id}:{root_hash}:{redaction_policy_id}").as_bytes())
    )
}

fn sync_manifest_entry_id(manifest_id: &str, entry: &NewSyncManifestEntry) -> String {
    format!(
        "sync-manifest-entry:v1:{}",
        hash_hex(
            format!(
                "{}:{}:{}:{}",
                manifest_id, entry.entry_kind, entry.entry_ref, entry.sync_path
            )
            .as_bytes()
        )
    )
}

fn dream_run_id(
    project_id: &str,
    provider: &str,
    model: &str,
    input_evidence_hash: &str,
    created_at: &str,
) -> String {
    format!(
        "dream-run:v1:{}",
        hash_hex(
            format!("{project_id}:{provider}:{model}:{input_evidence_hash}:{created_at}")
                .as_bytes()
        )
    )
}

fn dream_candidate_id(dream_run_id: &str, candidate: &NewDreamCandidate) -> String {
    format!(
        "dream-candidate:v1:{}",
        hash_hex(
            format!(
                "{}:{}:{}:{}:{}",
                dream_run_id,
                candidate.kind,
                candidate.claim,
                refs_material(&candidate.evidence_refs),
                refs_material(&candidate.counterevidence_refs)
            )
            .as_bytes()
        )
    )
}

fn learned_memory_id(project_id: &str, memory: &NewLearnedMemory) -> String {
    format!(
        "learned-memory:v1:{}",
        hash_hex(
            format!(
                "{}:{}:{}:{}:{}",
                project_id,
                memory.kind,
                memory.claim,
                refs_material(&memory.evidence_refs),
                refs_material(&memory.counterevidence_refs)
            )
            .as_bytes()
        )
    )
}

fn refs_material(refs: &[String]) -> String {
    sorted_refs(refs).join("\n")
}

fn sorted_refs(refs: &[String]) -> Vec<String> {
    let mut refs = refs.to_vec();
    refs.sort();
    refs.dedup();
    refs
}

fn refs_json(refs: &[String]) -> Result<String> {
    serde_json::to_string(&sorted_refs(refs)).context("serialize evidence refs")
}

fn validate_dream_candidate_status(status: &str) -> Result<()> {
    if matches!(status, "pending" | "accepted" | "rejected") {
        Ok(())
    } else {
        bail!("invalid dream candidate status: {status}")
    }
}

fn validate_learned_memory_status(status: &str) -> Result<()> {
    if matches!(status, "active" | "pending" | "superseded" | "rejected") {
        Ok(())
    } else {
        bail!("invalid learned memory status: {status}")
    }
}

fn validate_required_evidence_refs_on_conn(
    conn: &Connection,
    label: &str,
    project_id: &str,
    refs: &[String],
) -> Result<()> {
    if refs.is_empty() {
        bail!("{label} requires at least one evidence ref");
    }
    validate_optional_evidence_refs_on_conn(conn, project_id, refs)
}

fn validate_optional_evidence_refs_on_conn(
    conn: &Connection,
    project_id: &str,
    refs: &[String],
) -> Result<()> {
    for evidence_ref in refs {
        let event_id = evidence_ref
            .strip_prefix("mmr://event/")
            .ok_or_else(|| anyhow!("invalid evidence ref scheme: {evidence_ref}"))?;
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE id = ?1 AND project_id = ?2)",
                params![event_id, project_id],
                |row| row.get(0),
            )
            .context("validate evidence ref")?;
        if !exists {
            bail!("references missing evidence: {evidence_ref}");
        }
    }
    Ok(())
}

fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("format timestamp")
}

fn hash_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn escape_json_fragment(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUIRED_TABLES: &[&str] = &[
        "blobs",
        "dream_candidates",
        "dream_runs",
        "events",
        "learned_memory",
        "project_aliases",
        "project_links",
        "projects",
        "redaction_policies",
        "redaction_runs",
        "redaction_spans",
        "schema_migrations",
        "search_documents",
        "sessions",
        "source_cursors",
        "sources",
        "summaries",
        "sync_manifest_entries",
        "sync_manifests",
    ];

    #[test]
    fn migrations_replay_from_empty_database() {
        let store = Store::open_in_memory().expect("store");

        assert_eq!(
            store.schema_version().expect("schema"),
            LATEST_SCHEMA_VERSION
        );
        let tables = store.schema_table_names().expect("tables");
        for table in REQUIRED_TABLES {
            assert!(
                tables.iter().any(|name| name == table),
                "missing table {table}"
            );
        }
    }

    #[test]
    fn migrations_are_idempotent_and_detect_checksum_drift() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let db_path = tmp.path().join("mmr.db");
        let store = Store::open(&db_path).expect("first open");
        assert_eq!(
            store.schema_version().expect("schema"),
            LATEST_SCHEMA_VERSION
        );
        drop(store);

        let reopened = Store::open(&db_path).expect("second open");
        assert_eq!(
            reopened.schema_version().expect("schema"),
            LATEST_SCHEMA_VERSION
        );
        reopened
            .conn
            .execute(
                "UPDATE schema_migrations SET checksum = 'sha256:bad' WHERE version = 1",
                [],
            )
            .expect("drift checksum");
        drop(reopened);

        let err = Store::open(&db_path).expect_err("checksum drift should fail");
        assert!(err.to_string().contains("migration checksum drift"));
    }

    #[test]
    fn project_links_work_for_non_git_directories_and_lookup_by_id_or_path() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let project_dir = tmp.path().join("plain-project");
        fs::create_dir_all(&project_dir).expect("project dir");
        let store = Store::open_in_memory().expect("store");

        let linked = store.ensure_project_link(&project_dir).expect("link");
        assert!(linked.id.starts_with("proj:v1:"));
        assert_eq!(store.project_by_id(&linked.id).expect("by id"), linked);
        assert_eq!(
            store
                .project_by_path(&project_dir)
                .expect("by path")
                .expect("project"),
            linked
        );
    }

    #[test]
    fn event_ids_use_content_parent_and_parser_version() {
        let base = NewEvent::new(
            "codex",
            "sess-1",
            "message",
            "assistant",
            "2026-05-24T09:00:00Z",
            "same content",
            "codex-v1",
        )
        .with_source_event_id("event-1");
        let same = base.clone();
        let different_parent = base.clone().with_parent_hash("sha256:parent");
        let different_parser = NewEvent {
            parser_version: "codex-v2".to_string(),
            ..base.clone()
        };

        assert_eq!(base.content_hash(), same.content_hash());
        assert_eq!(base.event_id(), same.event_id());
        assert_ne!(base.event_id(), different_parent.event_id());
        assert_ne!(base.event_id(), different_parser.event_id());
    }

    #[test]
    fn replaying_same_events_is_idempotent() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let project_dir = tmp.path().join("memory-fabric");
        fs::create_dir_all(&project_dir).expect("project dir");
        let mut store = Store::open_in_memory().expect("store");
        let project = store.ensure_project_link(&project_dir).expect("project");
        let event = NewEvent::new(
            "codex",
            "codex-mvp-1",
            "message",
            "user",
            "2026-05-24T09:00:05Z",
            "Add a source-neutral memory store.",
            "codex-jsonl-v1",
        )
        .with_source_event_id("line-2")
        .with_raw_local_ref("tests/fixtures/memory_fabric/codex_session.jsonl:2");

        let first = store
            .insert_event(&project.id, &event)
            .expect("first insert");
        let second = store
            .insert_event(&project.id, &event)
            .expect("second insert");
        assert_eq!(first.id, second.id);
        assert_eq!(store.event_count().expect("count"), 1);

        let events = store
            .events_for_project(&project.id, Some("codex"), Some("codex-mvp-1"))
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].raw_local_ref.as_deref(),
            event.raw_local_ref.as_deref()
        );
    }

    #[test]
    fn store_api_covers_query_cursor_blob_and_rollback() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let project_dir = tmp.path().join("memory-fabric");
        fs::create_dir_all(&project_dir).expect("project dir");
        let mut store = Store::open_in_memory().expect("store");
        let project = store.ensure_project_link(&project_dir).expect("project");
        let blob = store
            .insert_blob_ref(
                "raw_local_ref",
                "application/jsonl",
                "/private/raw/codex_session.jsonl",
                &content_hash("redacted fixture bytes"),
                1024,
            )
            .expect("blob");
        let event = NewEvent::new(
            "codex",
            "codex-mvp-1",
            "message",
            "assistant",
            "2026-05-24T09:01:00Z",
            "I will define the store contract before wiring importers.",
            "codex-jsonl-v1",
        )
        .with_source_event_id("line-3")
        .with_blob_id(blob.id.clone());

        let inserted = store.insert_event(&project.id, &event).expect("event");
        assert_eq!(inserted.source, "codex");
        assert_eq!(inserted.content_hash, event.content_hash());

        store
            .set_source_cursor(
                &project.id,
                "codex",
                "tests/fixtures/memory_fabric/codex_session.jsonl",
                "offset:3",
                "codex-jsonl-v1",
                Some(inserted.content_hash.as_str()),
            )
            .expect("cursor");
        let cursor = store
            .source_cursor(
                &project.id,
                "codex",
                "tests/fixtures/memory_fabric/codex_session.jsonl",
            )
            .expect("cursor")
            .expect("cursor record");
        assert_eq!(cursor.cursor_value, "offset:3");
        assert_eq!(cursor.parser_version, "codex-jsonl-v1");
        assert_eq!(
            cursor.last_event_hash.as_deref(),
            Some(inserted.content_hash.as_str())
        );

        let failing_event = NewEvent::new(
            "codex",
            "codex-mvp-rollback",
            "message",
            "user",
            "2026-05-24T09:02:00Z",
            "rollback me",
            "codex-jsonl-v1",
        )
        .with_source_event_id("line-4");
        assert!(
            store
                .insert_event_then_fail(&project.id, &failing_event)
                .expect_err("rollback")
                .to_string()
                .contains("intentional rollback")
        );
        assert!(store.event_by_id(&failing_event.event_id()).is_err());

        let note_event = NewEvent::new(
            "note",
            "notes",
            "note",
            "user",
            "2026-05-24T09:03:00Z",
            "transactional note",
            "note-v1",
        )
        .with_source_event_id("note:tx-success");
        let (note, search_document) = store
            .insert_event_with_search_document(&project.id, &note_event)
            .expect("event and search document");
        assert_eq!(search_document.event_id, note.id);
        assert_eq!(search_document.document_text, "transactional note");

        let failing_note = NewEvent::new(
            "note",
            "notes",
            "note",
            "user",
            "2026-05-24T09:04:00Z",
            "rollback note search doc",
            "note-v1",
        )
        .with_source_event_id("note:tx-fail");
        assert!(
            store
                .insert_event_and_search_document_then_fail(&project.id, &failing_note)
                .expect_err("rollback event/search document")
                .to_string()
                .contains("intentional rollback after search document")
        );
        assert!(store.event_by_id(&failing_note.event_id()).is_err());
        assert!(
            store
                .search_document_by_event(&failing_note.event_id())
                .is_err()
        );
    }

    #[test]
    fn default_db_path_respects_xdg_data_home() {
        let tmp = tempfile::tempdir().expect("temp dir");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }
        let path = default_db_path().expect("path");
        assert_eq!(path, tmp.path().join("mmr").join("mmr.db"));
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
    }
}
