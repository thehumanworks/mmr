-- Finite local queue only; identity/settings never contain executable or argv.
CREATE TABLE assimilation_jobs (
 id TEXT PRIMARY KEY NOT NULL CHECK(length(CAST(id AS BLOB)) BETWEEN 1 AND 128),
 project_id TEXT NOT NULL CHECK(length(CAST(project_id AS BLOB)) BETWEEN 1 AND 128),
 store_epoch TEXT NOT NULL CHECK(length(CAST(store_epoch AS BLOB))=32),
 snapshot_id TEXT NOT NULL REFERENCES evidence_snapshots(id),
 snapshot_digest TEXT NOT NULL CHECK(length(CAST(snapshot_digest AS BLOB))<=128),
 policy_fingerprint TEXT NOT NULL CHECK(length(CAST(policy_fingerprint AS BLOB))<=128),
 policy_json TEXT NOT NULL CHECK(length(CAST(policy_json AS BLOB))<=4096),
 limits_json TEXT NOT NULL CHECK(length(CAST(limits_json AS BLOB))<=4096),
 state TEXT NOT NULL CHECK(state IN ('queued','claimed','executing','proposed','failed','stale','cancelled','unknown')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(typeof(attempts)='integer' AND attempts BETWEEN 0 AND 8),
 fence INTEGER NOT NULL DEFAULT 0 CHECK(typeof(fence)='integer' AND fence>=0),
 owner_id TEXT CHECK(owner_id IS NULL OR length(CAST(owner_id AS BLOB)) BETWEEN 1 AND 128),
 lease_expires_at_ms INTEGER,
 cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
 execution_intent_at_ms INTEGER,
 cleanup_confirmed INTEGER NOT NULL DEFAULT 0 CHECK(cleanup_confirmed IN (0,1)),
 projection_json TEXT CHECK(projection_json IS NULL OR length(CAST(projection_json AS BLOB))<=262144),
 diagnostic_code TEXT CHECK(diagnostic_code IS NULL OR diagnostic_code IN ('claim_attempts_exhausted','lease_expired','owner_lost','cancelled','no_eligible_evidence','projection_failed','budget_exceeded','runner_failed','runner_timed_out','invalid_output','cleanup_unknown','lease_window_exhausted','evidence_stale')),
 receipt_id TEXT REFERENCES evidence_proposal_receipts(id),
 created_at_ms INTEGER NOT NULL CHECK(created_at_ms>=0),
 updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=0),
 CHECK((owner_id IS NULL AND lease_expires_at_ms IS NULL) OR (owner_id IS NOT NULL AND lease_expires_at_ms IS NOT NULL)),
 CHECK(state NOT IN ('claimed','executing') OR (owner_id IS NOT NULL AND attempts>0 AND fence>0)),
 CHECK(state NOT IN ('executing','proposed','unknown') OR (projection_json IS NOT NULL AND execution_intent_at_ms IS NOT NULL)),
 CHECK(state NOT IN ('queued','claimed','executing','unknown') OR cleanup_confirmed=0),
 CHECK(state NOT IN ('proposed','failed','stale','cancelled') OR cleanup_confirmed=1),
 CHECK((state='proposed' AND receipt_id IS NOT NULL) OR (state<>'proposed' AND receipt_id IS NULL)),
 CHECK(state<>'queued' OR (attempts=0 AND fence=0 AND owner_id IS NULL AND projection_json IS NULL)),
 CHECK(state<>'claimed' OR projection_json IS NULL)
);
CREATE INDEX idx_jobs_claim ON assimilation_jobs(project_id,policy_fingerprint,state,created_at_ms,id);
CREATE INDEX idx_jobs_expiry ON assimilation_jobs(project_id,state,lease_expires_at_ms);
CREATE INDEX idx_jobs_inspection ON assimilation_jobs(project_id,created_at_ms,id);
CREATE TRIGGER assimilation_job_identity BEFORE UPDATE OF id,project_id,store_epoch,snapshot_id,snapshot_digest,policy_fingerprint,policy_json,limits_json ON assimilation_jobs
 WHEN OLD.id IS NOT NEW.id OR OLD.project_id IS NOT NEW.project_id OR OLD.store_epoch IS NOT NEW.store_epoch OR OLD.snapshot_id IS NOT NEW.snapshot_id OR OLD.snapshot_digest IS NOT NEW.snapshot_digest OR OLD.policy_fingerprint IS NOT NEW.policy_fingerprint OR OLD.policy_json IS NOT NEW.policy_json OR OLD.limits_json IS NOT NEW.limits_json
 BEGIN SELECT RAISE(ABORT,'job identity is immutable'); END;
PRAGMA user_version=4;
