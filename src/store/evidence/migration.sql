-- Local evidence fence only. No jobs, providers or learned-memory mutation.
CREATE TABLE evidence_clock (
 singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
 store_epoch TEXT NOT NULL CHECK(length(CAST(store_epoch AS BLOB)) = 32),
 sequence INTEGER NOT NULL CHECK(typeof(sequence) = 'integer' AND sequence >= 0)
);
INSERT INTO evidence_clock VALUES(1, lower(hex(randomblob(16))), 1);

CREATE TABLE evidence_sessions (
 session_id TEXT PRIMARY KEY NOT NULL,
 project_id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(typeof(revision) = 'integer' AND revision > 0),
 changed_seq INTEGER NOT NULL,
 event_count INTEGER NOT NULL CHECK(event_count >= 0),
 event_bytes INTEGER NOT NULL CHECK(event_bytes >= 0),
 present INTEGER NOT NULL CHECK(present IN (0,1)),
 sealed_revision INTEGER,
 seal_digest TEXT,
 attestation TEXT CHECK(attestation IS NULL OR length(CAST(attestation AS BLOB)) <= 1024),
 sealed_at TEXT,
 seal_seq INTEGER,
 CHECK((sealed_revision IS NULL AND seal_digest IS NULL AND attestation IS NULL AND sealed_at IS NULL AND seal_seq IS NULL)
    OR (sealed_revision = revision AND seal_digest IS NOT NULL AND attestation IS NOT NULL AND sealed_at IS NOT NULL AND seal_seq IS NOT NULL))
);
CREATE INDEX idx_evidence_session_changes ON evidence_sessions(project_id, changed_seq, session_id);
CREATE INDEX idx_evidence_events_session ON events(session_id);
CREATE INDEX idx_evidence_events_blob ON events(blob_id);
CREATE INDEX idx_evidence_events_policy ON events(redaction_policy_id);
CREATE INDEX idx_evidence_sessions_source ON sessions(source);

-- No FK cascade: logical exclusions outlive removal/reimport of raw rows.
CREATE TABLE evidence_revocations (
 project_id TEXT NOT NULL,
 event_id TEXT PRIMARY KEY NOT NULL,
 reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB)) BETWEEN 1 AND 1024),
 control_seq INTEGER NOT NULL,
 created_at TEXT NOT NULL,
 CHECK(length(CAST(event_id AS BLOB)) <= 1024)
);

CREATE TABLE evidence_snapshots (
 id TEXT PRIMARY KEY NOT NULL CHECK(length(CAST(id AS BLOB)) <= 128),
 project_id TEXT NOT NULL,
 store_epoch TEXT NOT NULL,
 digest TEXT NOT NULL,
 metadata_json TEXT NOT NULL CHECK(length(CAST(metadata_json AS BLOB)) <= 4194304),
 created_at TEXT NOT NULL
);
CREATE TABLE evidence_proposal_receipts (
 id TEXT PRIMARY KEY NOT NULL CHECK(length(CAST(id AS BLOB)) <= 128),
 snapshot_id TEXT NOT NULL,
 output_hash TEXT NOT NULL,
 proposal_json TEXT NOT NULL CHECK(length(CAST(proposal_json AS BLOB)) <= 1048576),
 created_at TEXT NOT NULL,
 FOREIGN KEY(snapshot_id) REFERENCES evidence_snapshots(id),
 UNIQUE(snapshot_id, output_hash)
);

-- One-time scalar backfill, indexed by session; never infer old completion.
INSERT INTO evidence_sessions(session_id,project_id,revision,changed_seq,event_count,event_bytes,present)
SELECT s.id,s.project_id,1,1,(SELECT count(*) FROM events e WHERE e.session_id=s.id),
 coalesce((SELECT sum(coalesce(length(CAST(e.id AS BLOB)), 0) + coalesce(length(CAST(e.project_id AS BLOB)), 0) + coalesce(length(CAST(e.session_id AS BLOB)), 0) + coalesce(length(CAST(e.source AS BLOB)), 0) + coalesce(length(CAST(e.source_session_id AS BLOB)), 0) + coalesce(length(CAST(e.source_event_id AS BLOB)), 0) + coalesce(length(CAST(e.event_type AS BLOB)), 0) + coalesce(length(CAST(e.role AS BLOB)), 0) + coalesce(length(CAST(e.timestamp AS BLOB)), 0) + coalesce(length(CAST(e.content_text AS BLOB)), 0) + coalesce(length(CAST(e.content_hash AS BLOB)), 0) + coalesce(length(CAST(e.parent_hash AS BLOB)), 0) + coalesce(length(CAST(e.parser_version AS BLOB)), 0) + coalesce(length(CAST(e.raw_local_ref AS BLOB)), 0) + coalesce(length(CAST(e.blob_id AS BLOB)), 0) + coalesce(length(CAST(e.redaction_policy_id AS BLOB)), 0) + 256) FROM events e WHERE e.session_id=s.id),0),1 FROM sessions s;

CREATE TRIGGER evidence_session_insert AFTER INSERT ON sessions BEGIN
 UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
 INSERT INTO evidence_sessions(session_id,project_id,revision,changed_seq,event_count,event_bytes,present)
 VALUES(NEW.id,NEW.project_id,1,(SELECT sequence FROM evidence_clock WHERE singleton=1),0,0,1)
 ON CONFLICT(session_id) DO UPDATE SET project_id=excluded.project_id, revision=revision+1,
 changed_seq=excluded.changed_seq,present=1,sealed_revision=NULL,seal_digest=NULL,attestation=NULL,sealed_at=NULL,seal_seq=NULL;
END;
CREATE TRIGGER evidence_session_identity BEFORE UPDATE OF id,project_id,source,source_session_id ON sessions
 WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR OLD.source IS NOT NEW.source OR OLD.source_session_id IS NOT NEW.source_session_id
 BEGIN SELECT RAISE(ABORT,'session identity is immutable'); END;
CREATE TRIGGER evidence_session_update AFTER UPDATE OF started_at,ended_at,title,raw_local_ref ON sessions
 WHEN OLD.started_at IS NOT NEW.started_at OR OLD.ended_at IS NOT NEW.ended_at OR OLD.title IS NOT NEW.title OR OLD.raw_local_ref IS NOT NEW.raw_local_ref BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL WHERE session_id = NEW.id;
END;
CREATE TRIGGER evidence_session_delete AFTER DELETE ON sessions BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL, present = 0 WHERE session_id = OLD.id;
END;
CREATE TRIGGER evidence_event_insert AFTER INSERT ON events BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL, event_count = event_count + 1, event_bytes = event_bytes + (coalesce(length(CAST(NEW.id AS BLOB)), 0) + coalesce(length(CAST(NEW.project_id AS BLOB)), 0) + coalesce(length(CAST(NEW.session_id AS BLOB)), 0) + coalesce(length(CAST(NEW.source AS BLOB)), 0) + coalesce(length(CAST(NEW.source_session_id AS BLOB)), 0) + coalesce(length(CAST(NEW.source_event_id AS BLOB)), 0) + coalesce(length(CAST(NEW.event_type AS BLOB)), 0) + coalesce(length(CAST(NEW.role AS BLOB)), 0) + coalesce(length(CAST(NEW.timestamp AS BLOB)), 0) + coalesce(length(CAST(NEW.content_text AS BLOB)), 0) + coalesce(length(CAST(NEW.content_hash AS BLOB)), 0) + coalesce(length(CAST(NEW.parent_hash AS BLOB)), 0) + coalesce(length(CAST(NEW.parser_version AS BLOB)), 0) + coalesce(length(CAST(NEW.raw_local_ref AS BLOB)), 0) + coalesce(length(CAST(NEW.blob_id AS BLOB)), 0) + coalesce(length(CAST(NEW.redaction_policy_id AS BLOB)), 0) + 256) WHERE session_id = NEW.session_id;
UPDATE sessions SET ended_at=NULL WHERE id=NEW.session_id AND ended_at IS NOT NULL;
END;
CREATE TRIGGER evidence_event_delete AFTER DELETE ON events BEGIN
INSERT OR IGNORE INTO evidence_revocations(project_id,event_id,reason,control_seq,created_at) VALUES(OLD.project_id,OLD.id,'physical event deletion',(SELECT sequence+1 FROM evidence_clock WHERE singleton=1),strftime('%Y-%m-%dT%H:%M:%fZ','now'));
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL, event_count = event_count - 1, event_bytes = event_bytes - (coalesce(length(CAST(OLD.id AS BLOB)), 0) + coalesce(length(CAST(OLD.project_id AS BLOB)), 0) + coalesce(length(CAST(OLD.session_id AS BLOB)), 0) + coalesce(length(CAST(OLD.source AS BLOB)), 0) + coalesce(length(CAST(OLD.source_session_id AS BLOB)), 0) + coalesce(length(CAST(OLD.source_event_id AS BLOB)), 0) + coalesce(length(CAST(OLD.event_type AS BLOB)), 0) + coalesce(length(CAST(OLD.role AS BLOB)), 0) + coalesce(length(CAST(OLD.timestamp AS BLOB)), 0) + coalesce(length(CAST(OLD.content_text AS BLOB)), 0) + coalesce(length(CAST(OLD.content_hash AS BLOB)), 0) + coalesce(length(CAST(OLD.parent_hash AS BLOB)), 0) + coalesce(length(CAST(OLD.parser_version AS BLOB)), 0) + coalesce(length(CAST(OLD.raw_local_ref AS BLOB)), 0) + coalesce(length(CAST(OLD.blob_id AS BLOB)), 0) + coalesce(length(CAST(OLD.redaction_policy_id AS BLOB)), 0) + 256) WHERE session_id = OLD.session_id;
UPDATE sessions SET ended_at=NULL WHERE id=OLD.session_id AND ended_at IS NOT NULL;
END;
CREATE TRIGGER evidence_event_update AFTER UPDATE ON events
 WHEN OLD.rowid IS NOT NEW.rowid OR OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR OLD.session_id IS NOT NEW.session_id OR OLD.source IS NOT NEW.source OR OLD.source_session_id IS NOT NEW.source_session_id OR OLD.source_event_id IS NOT NEW.source_event_id OR OLD.event_type IS NOT NEW.event_type OR OLD.role IS NOT NEW.role OR OLD.timestamp IS NOT NEW.timestamp OR OLD.content_text IS NOT NEW.content_text OR OLD.content_hash IS NOT NEW.content_hash OR OLD.parent_hash IS NOT NEW.parent_hash OR OLD.parser_version IS NOT NEW.parser_version OR OLD.raw_local_ref IS NOT NEW.raw_local_ref OR OLD.blob_id IS NOT NEW.blob_id OR OLD.redaction_policy_id IS NOT NEW.redaction_policy_id BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL, event_count = event_count - 1, event_bytes = event_bytes - (coalesce(length(CAST(OLD.id AS BLOB)), 0) + coalesce(length(CAST(OLD.project_id AS BLOB)), 0) + coalesce(length(CAST(OLD.session_id AS BLOB)), 0) + coalesce(length(CAST(OLD.source AS BLOB)), 0) + coalesce(length(CAST(OLD.source_session_id AS BLOB)), 0) + coalesce(length(CAST(OLD.source_event_id AS BLOB)), 0) + coalesce(length(CAST(OLD.event_type AS BLOB)), 0) + coalesce(length(CAST(OLD.role AS BLOB)), 0) + coalesce(length(CAST(OLD.timestamp AS BLOB)), 0) + coalesce(length(CAST(OLD.content_text AS BLOB)), 0) + coalesce(length(CAST(OLD.content_hash AS BLOB)), 0) + coalesce(length(CAST(OLD.parent_hash AS BLOB)), 0) + coalesce(length(CAST(OLD.parser_version AS BLOB)), 0) + coalesce(length(CAST(OLD.raw_local_ref AS BLOB)), 0) + coalesce(length(CAST(OLD.blob_id AS BLOB)), 0) + coalesce(length(CAST(OLD.redaction_policy_id AS BLOB)), 0) + 256) WHERE session_id = OLD.session_id;
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL, event_count = event_count + 1, event_bytes = event_bytes + (coalesce(length(CAST(NEW.id AS BLOB)), 0) + coalesce(length(CAST(NEW.project_id AS BLOB)), 0) + coalesce(length(CAST(NEW.session_id AS BLOB)), 0) + coalesce(length(CAST(NEW.source AS BLOB)), 0) + coalesce(length(CAST(NEW.source_session_id AS BLOB)), 0) + coalesce(length(CAST(NEW.source_event_id AS BLOB)), 0) + coalesce(length(CAST(NEW.event_type AS BLOB)), 0) + coalesce(length(CAST(NEW.role AS BLOB)), 0) + coalesce(length(CAST(NEW.timestamp AS BLOB)), 0) + coalesce(length(CAST(NEW.content_text AS BLOB)), 0) + coalesce(length(CAST(NEW.content_hash AS BLOB)), 0) + coalesce(length(CAST(NEW.parent_hash AS BLOB)), 0) + coalesce(length(CAST(NEW.parser_version AS BLOB)), 0) + coalesce(length(CAST(NEW.raw_local_ref AS BLOB)), 0) + coalesce(length(CAST(NEW.blob_id AS BLOB)), 0) + coalesce(length(CAST(NEW.redaction_policy_id AS BLOB)), 0) + 256) WHERE session_id = NEW.session_id;
UPDATE sessions SET ended_at=NULL WHERE id IN (OLD.session_id,NEW.session_id) AND ended_at IS NOT NULL;
END;
CREATE TRIGGER evidence_revocation_insert AFTER INSERT ON evidence_revocations BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL WHERE session_id IN (SELECT session_id FROM events WHERE id=NEW.event_id);
UPDATE sessions SET ended_at=NULL WHERE ended_at IS NOT NULL AND id IN (SELECT session_id FROM events WHERE id=NEW.event_id);
END;
CREATE TRIGGER evidence_revocations_update_immutable BEFORE UPDATE ON evidence_revocations BEGIN SELECT RAISE(ABORT,'evidence records are immutable'); END;
CREATE TRIGGER evidence_revocations_delete_immutable BEFORE DELETE ON evidence_revocations BEGIN SELECT RAISE(ABORT,'evidence records are immutable'); END;
CREATE TRIGGER evidence_snapshots_update_immutable BEFORE UPDATE ON evidence_snapshots BEGIN SELECT RAISE(ABORT,'evidence records are immutable'); END;
CREATE TRIGGER evidence_snapshots_delete_immutable BEFORE DELETE ON evidence_snapshots BEGIN SELECT RAISE(ABORT,'evidence records are immutable'); END;
CREATE TRIGGER evidence_proposal_receipts_update_immutable BEFORE UPDATE ON evidence_proposal_receipts BEGIN SELECT RAISE(ABORT,'evidence records are immutable'); END;
CREATE TRIGGER evidence_proposal_receipts_delete_immutable BEFORE DELETE ON evidence_proposal_receipts BEGIN SELECT RAISE(ABORT,'evidence records are immutable'); END;
CREATE TRIGGER evidence_projects_update AFTER UPDATE OF canonical_path,display_name ON projects
 WHEN OLD.canonical_path IS NOT NEW.canonical_path OR OLD.display_name IS NOT NEW.display_name BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL WHERE session_id IN (SELECT id FROM sessions WHERE project_id=NEW.id);
UPDATE sessions SET ended_at = NULL WHERE ended_at IS NOT NULL AND project_id=NEW.id;
END;
CREATE TRIGGER evidence_blobs_update AFTER UPDATE OF kind,media_type,content_hash,storage_ref,byte_len ON blobs
 WHEN OLD.kind IS NOT NEW.kind OR OLD.media_type IS NOT NEW.media_type OR OLD.content_hash IS NOT NEW.content_hash OR OLD.storage_ref IS NOT NEW.storage_ref OR OLD.byte_len IS NOT NEW.byte_len BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL WHERE session_id IN (SELECT id FROM sessions WHERE id IN (SELECT session_id FROM events WHERE blob_id=NEW.id));
UPDATE sessions SET ended_at = NULL WHERE ended_at IS NOT NULL AND id IN (SELECT session_id FROM events WHERE blob_id=NEW.id);
END;
CREATE TRIGGER evidence_redaction_policies_update AFTER UPDATE OF version,description ON redaction_policies
 WHEN OLD.version IS NOT NEW.version OR OLD.description IS NOT NEW.description BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL WHERE session_id IN (SELECT id FROM sessions WHERE id IN (SELECT session_id FROM events WHERE redaction_policy_id=NEW.id));
UPDATE sessions SET ended_at = NULL WHERE ended_at IS NOT NULL AND id IN (SELECT session_id FROM events WHERE redaction_policy_id=NEW.id);
END;
-- REPLACE deletes then reinserts policies; default NO ACTION FKs do not mutate
-- events, so enabled recursive DELETE triggers must invalidate dependencies here.
CREATE TRIGGER evidence_redaction_policy_delete AFTER DELETE ON redaction_policies BEGIN
 UPDATE evidence_clock SET sequence=sequence+1 WHERE singleton=1;
 UPDATE evidence_sessions SET revision=revision+1,changed_seq=(SELECT sequence FROM evidence_clock WHERE singleton=1),sealed_revision=NULL,seal_digest=NULL,attestation=NULL,sealed_at=NULL,seal_seq=NULL
 WHERE session_id IN (SELECT session_id FROM events WHERE redaction_policy_id=OLD.id);
 UPDATE sessions SET ended_at=NULL WHERE ended_at IS NOT NULL AND id IN (SELECT session_id FROM events WHERE redaction_policy_id=OLD.id);
END;
CREATE TRIGGER evidence_sources_update AFTER UPDATE OF adapter_version,enabled ON sources
 WHEN OLD.adapter_version IS NOT NEW.adapter_version OR OLD.enabled IS NOT NEW.enabled BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL WHERE session_id IN (SELECT id FROM sessions WHERE source=NEW.name);
UPDATE sessions SET ended_at = NULL WHERE ended_at IS NOT NULL AND source=NEW.name;
END;
CREATE TRIGGER evidence_source_delete AFTER DELETE ON sources BEGIN
UPDATE evidence_clock SET sequence = sequence + 1 WHERE singleton = 1;
UPDATE evidence_sessions SET revision = revision + 1, changed_seq = (SELECT sequence FROM evidence_clock WHERE singleton = 1), sealed_revision = NULL, seal_digest = NULL, attestation = NULL, sealed_at = NULL, seal_seq = NULL WHERE session_id IN (SELECT id FROM sessions WHERE source=OLD.name);
UPDATE sessions SET ended_at = NULL WHERE ended_at IS NOT NULL AND source=OLD.name;
END;
CREATE TRIGGER evidence_source_identity BEFORE UPDATE OF id,name ON sources WHEN OLD.id IS NOT NEW.id OR OLD.name IS NOT NEW.name BEGIN SELECT RAISE(ABORT,'source identity is immutable'); END;

PRAGMA user_version = 3;
