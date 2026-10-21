//! Bounded local completion/snapshot fences; no runner, job or activation state.
//!
//! Store handles enable `foreign_keys=ON` and `recursive_triggers=ON`. External
//! SQLite source writers must enable BOTH on every connection before mutation;
//! unconfigured handles (including default REPLACE behavior) are unsupported.

use std::collections::BTreeSet;
use std::io::{self, Write};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;

use super::{Store, content_hash, now_rfc3339};
use crate::dream::{DreamRunnerOutput, validate_output};

mod model;
pub use model::*;

pub(super) const SCHEMA: &str = include_str!("evidence/migration.sql");

fn label(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_LABEL_BYTES {
        bail!("evidence label is empty or exceeds byte limit");
    }
    Ok(())
}

struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(
                "evidence serialization exceeds byte budget",
            ));
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = required
                .max(self.bytes.capacity().saturating_mul(2))
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(io::Error::other)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn json<T: Serialize>(value: &T, limit: usize) -> Result<String> {
    let mut writer = BoundedJson {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value).context("bounded evidence serialization")?;
    String::from_utf8(writer.bytes).context("evidence JSON UTF-8")
}
fn digest<T: Serialize>(value: &T, limit: usize) -> Result<String> {
    // The buffer is bounded before every extension; no unrestricted to_value/clone.
    Ok(content_hash(&json(value, limit)?))
}
fn watermark(conn: &Connection) -> Result<EvidenceWatermark> {
    Ok(conn.query_row(
        "SELECT store_epoch,sequence FROM evidence_clock WHERE singleton=1",
        [],
        |r| {
            Ok(EvidenceWatermark {
                store_epoch: r.get(0)?,
                sequence: r.get(1)?,
            })
        },
    )?)
}
fn change_predicate(with_session: bool) -> &'static str {
    if with_session {
        "project_id=?1 AND changed_seq<=?2 AND (changed_seq,session_id)>(?3,?4)"
    } else {
        "project_id=?1 AND changed_seq<=?2 AND changed_seq>?3 AND ?4 IS NULL"
    }
}
fn advance(conn: &Connection) -> Result<i64> {
    conn.execute(
        "UPDATE evidence_clock SET sequence=sequence+1 WHERE singleton=1",
        [],
    )?;
    Ok(watermark(conn)?.sequence)
}
fn seal(conn: &Connection, id: &str) -> Result<Option<CompletionSeal>> {
    conn.query_row("SELECT session_id,sealed_revision,seal_seq,seal_digest,attestation,sealed_at FROM evidence_sessions WHERE session_id=?1 AND present=1 AND sealed_revision=revision", [id], |r| {
        Ok(CompletionSeal { session_id:r.get(0)?,revision:r.get(1)?,control_sequence:r.get(2)?,digest:r.get(3)?,attestation:r.get(4)?,completed_at:r.get(5)? })
    }).optional().context("read completion seal")
}

struct CurrentSession {
    metadata: SessionEvidenceMetadata,
    revision: i64,
    source_rows: usize,
    events: Vec<LocalEvidenceEvent>,
}

// Byte preflight runs only for explicit sessions and at most max_events+1 rows.
// Operational created_at/updated_at/sync_status are deliberately not semantics.
const EVENT_COLUMNS: &str = "e.id,e.project_id,e.session_id,e.source,e.source_session_id,e.source_event_id,e.event_type,e.role,e.timestamp,e.content_text,e.content_hash,e.parent_hash,e.parser_version,e.raw_local_ref,e.blob_id,e.redaction_policy_id,p.version,p.description,b.id,b.kind,b.media_type,b.content_hash,b.storage_ref,b.byte_len";
const EVENT_JOIN: &str = " FROM events e JOIN redaction_policies p ON p.id=e.redaction_policy_id LEFT JOIN blobs b ON b.id=e.blob_id";

fn count_from_row(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<usize> {
    usize::try_from(row.get::<_, i64>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn event_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LocalEvidenceEvent> {
    let blob_id: Option<String> = r.get(18)?;
    let blob = blob_id
        .map(|id| -> rusqlite::Result<_> {
            Ok(EvidenceBlob {
                id,
                kind: r.get(19)?,
                media_type: r.get(20)?,
                content_hash: r.get(21)?,
                storage_ref: r.get(22)?,
                byte_len: r.get(23)?,
            })
        })
        .transpose()?;
    Ok(LocalEvidenceEvent {
        id: r.get(0)?,
        project_id: r.get(1)?,
        session_id: r.get(2)?,
        source: r.get(3)?,
        source_session_id: r.get(4)?,
        source_event_id: r.get(5)?,
        event_type: r.get(6)?,
        role: r.get(7)?,
        timestamp: r.get(8)?,
        content_text: r.get(9)?,
        content_hash: r.get(10)?,
        parent_hash: r.get(11)?,
        parser_version: r.get(12)?,
        raw_local_ref: r.get(13)?,
        blob_id: r.get(14)?,
        redaction_policy_id: r.get(15)?,
        redaction_policy_version: r.get(16)?,
        redaction_policy_description: r.get(17)?,
        blob,
    })
}

fn current_sessions(
    conn: &Connection,
    project_id: &str,
    ids: &[String],
    budget: EvidenceBudget,
) -> Result<(ProjectEvidenceMetadata, Vec<CurrentSession>)> {
    budget.validate()?;
    label(project_id)?;
    if ids.is_empty() || ids.len() > budget.max_sessions {
        bail!("evidence session budget exceeded");
    }
    let mut unique = BTreeSet::new();
    for id in ids {
        label(id)?;
        if !unique.insert(id) {
            bail!("duplicate evidence session");
        }
    }
    let project_bytes:i64=conn.query_row("SELECT length(CAST(id AS BLOB))+length(CAST(canonical_path AS BLOB))+length(CAST(display_name AS BLOB))+128 FROM projects WHERE id=?1", [project_id],|r|r.get(0)).optional()?.ok_or_else(||anyhow!("evidence project is missing"))?;
    let mut bytes = usize::try_from(project_bytes)?;
    let mut count = 0usize;
    let byte_columns = EVENT_COLUMNS
        .split(',')
        .filter(|c| *c != "b.byte_len")
        .map(|c| format!("coalesce(length(CAST({c} AS BLOB)),0)"))
        .collect::<Vec<_>>()
        .join("+");
    for id in ids {
        let (present,rows,source_bytes):(bool,i64,i64)=conn.query_row("SELECT present,event_count,event_bytes FROM evidence_sessions WHERE session_id=?1 AND project_id=?2",params![id,project_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or_else(||anyhow!("evidence session is missing"))?;
        if !present {
            bail!("evidence session was deleted");
        }
        count = count
            .checked_add(usize::try_from(rows)?)
            .ok_or_else(|| anyhow!("evidence count overflow"))?;
        if count > budget.max_events || usize::try_from(source_bytes)? > budget.max_bytes {
            bail!("evidence source budget exceeded");
        }
        let meta_bytes:i64=conn.query_row("SELECT length(CAST(s.id AS BLOB))+length(CAST(s.project_id AS BLOB))+length(CAST(s.source AS BLOB))+length(CAST(s.source_session_id AS BLOB))+length(CAST(s.started_at AS BLOB))+coalesce(length(CAST(s.ended_at AS BLOB)),0)+coalesce(length(CAST(s.title AS BLOB)),0)+coalesce(length(CAST(s.raw_local_ref AS BLOB)),0)+length(CAST(o.adapter_version AS BLOB))+256 FROM sessions s JOIN sources o ON o.name=s.source WHERE s.id=?1",[id],|r|r.get(0)).optional()?.ok_or_else(||anyhow!("evidence session metadata is missing"))?;
        bytes = bytes
            .checked_add(usize::try_from(meta_bytes)?)
            .ok_or_else(|| anyhow!("evidence byte overflow"))?;
        let sql = format!(
            "SELECT count(*),coalesce(sum(n),0) FROM (SELECT {byte_columns}+512 n{EVENT_JOIN} WHERE e.session_id=?1 ORDER BY e.rowid LIMIT ?2)"
        );
        let (actual, body_bytes): (i64, i64) = conn.query_row(
            &sql,
            params![id, i64::try_from(budget.max_events + 1)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if actual != rows {
            bail!("evidence source count fence failed");
        }
        bytes = bytes
            .checked_add(usize::try_from(body_bytes)?)
            .ok_or_else(|| anyhow!("evidence byte overflow"))?;
        if bytes > budget.max_bytes {
            bail!("evidence semantic byte budget exceeded");
        }
    }
    let project = conn.query_row(
        "SELECT id,canonical_path,display_name FROM projects WHERE id=?1",
        [project_id],
        |r| {
            Ok(ProjectEvidenceMetadata {
                id: r.get(0)?,
                canonical_path: r.get(1)?,
                display_name: r.get(2)?,
            })
        },
    )?;
    let mut sessions = Vec::with_capacity(ids.len());
    for id in ids {
        let (revision, rows): (i64, i64) = conn.query_row(
            "SELECT revision,event_count FROM evidence_sessions WHERE session_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let metadata=conn.query_row("SELECT s.id,s.project_id,s.source,s.source_session_id,s.started_at,s.ended_at,s.title,s.raw_local_ref,o.adapter_version,o.enabled FROM sessions s JOIN sources o ON o.name=s.source WHERE s.id=?1",[id],|r|Ok(SessionEvidenceMetadata{id:r.get(0)?,project_id:r.get(1)?,source:r.get(2)?,source_session_id:r.get(3)?,started_at:r.get(4)?,ended_at:r.get(5)?,title:r.get(6)?,raw_local_ref:r.get(7)?,adapter_version:r.get(8)?,source_enabled:r.get(9)?}))?;
        let sql = format!(
            "SELECT {EVENT_COLUMNS}{EVENT_JOIN} WHERE e.session_id=?1 AND NOT EXISTS(SELECT 1 FROM evidence_revocations v WHERE v.event_id=e.id) ORDER BY e.rowid LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let events = stmt
            .query_map(
                params![id, i64::try_from(budget.max_events + 1)?],
                event_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if events
            .iter()
            .any(|event| event.blob_id.is_some() && event.blob.is_none())
        {
            bail!("evidence blob metadata is missing");
        }
        if events.len() > budget.max_events {
            bail!("evidence event budget exceeded");
        }
        sessions.push(CurrentSession {
            metadata,
            revision,
            source_rows: usize::try_from(rows)?,
            events,
        });
    }
    Ok((project, sessions))
}
fn session_digest(
    epoch: &str,
    project: &ProjectEvidenceMetadata,
    session: &CurrentSession,
    budget: EvidenceBudget,
) -> Result<String> {
    digest(
        &(
            "session-seal:v1",
            epoch,
            project,
            &session.metadata,
            session.revision,
            &session.events,
        ),
        budget.max_bytes,
    )
}
fn snapshot(
    conn: &Connection,
    project_id: &str,
    ids: &[String],
    policy: &str,
    budget: EvidenceBudget,
) -> Result<(EvidenceSnapshot, Vec<LocalEvidenceEvent>)> {
    label(policy)?;
    let epoch = watermark(conn)?.store_epoch;
    let (project, sessions) = current_sessions(conn, project_id, ids, budget)?;
    let mut members = Vec::with_capacity(ids.len());
    let mut events = Vec::new();
    let mut rows = 0usize;
    for current in sessions {
        let completion = seal(conn, &current.metadata.id)?
            .ok_or_else(|| anyhow!("evidence session is not explicitly completed"))?;
        if completion.revision != current.revision
            || completion.digest != session_digest(&epoch, &project, &current, budget)?
        {
            bail!("evidence completion seal is stale");
        }
        let refs = current
            .events
            .iter()
            .map(|event| {
                Ok(EvidenceEventRef {
                    event_id: event.id.clone(),
                    semantic_hash: digest(event, budget.max_bytes)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        rows += current.source_rows;
        members.push(SnapshotSession {
            seal: completion,
            metadata: current.metadata,
            source_rows: current.source_rows,
            events: refs,
        });
        events.extend(current.events);
    }
    if events.is_empty() {
        bail!("evidence snapshot has no non-revoked events");
    }
    let metadata = EvidenceSnapshotMetadata {
        format_version: 1,
        store_epoch: epoch,
        project,
        projection_policy_version: policy.to_string(),
        sessions: members,
        event_count: events.len(),
        source_rows: rows,
    };
    let hash = digest(&metadata, budget.max_bytes)?;
    let id = format!(
        "evidence-snapshot:v1:{}",
        hash.trim_start_matches("sha256:")
    );
    Ok((
        EvidenceSnapshot {
            id,
            digest: hash,
            metadata,
        },
        events,
    ))
}

fn load_snapshot(conn: &Connection, id: &str, budget: EvidenceBudget) -> Result<EvidenceSnapshot> {
    budget.validate()?;
    label(id)?;
    let lengths:Option<(i64,i64,i64,i64,i64)>=conn.query_row("SELECT length(CAST(id AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(store_epoch AS BLOB)),length(CAST(digest AS BLOB)),length(CAST(metadata_json AS BLOB)) FROM evidence_snapshots WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let (a, b, c, d, n) = lengths.ok_or_else(|| anyhow!("evidence snapshot is missing"))?;
    if [a, b, c, d]
        .iter()
        .any(|n| *n < 0 || *n > MAX_LABEL_BYTES as i64)
        || n < 0
        || n > budget.max_bytes as i64
    {
        bail!("stored evidence metadata exceeds byte budget");
    }
    let (hash, epoch, project_id, body): (String, String, String, String) = conn.query_row(
        "SELECT digest,store_epoch,project_id,metadata_json FROM evidence_snapshots WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let metadata: EvidenceSnapshotMetadata =
        serde_json::from_str(&body).map_err(|_| anyhow!("invalid evidence snapshot metadata"))?;
    if metadata.format_version != 1
        || metadata.sessions.is_empty()
        || metadata.sessions.len() > budget.max_sessions
        || metadata.source_rows > budget.max_events
        || metadata.event_count > budget.max_events
        || metadata
            .sessions
            .iter()
            .map(|s| s.events.len())
            .sum::<usize>()
            != metadata.event_count
    {
        bail!("stored evidence snapshot exceeds version/count bounds");
    }
    if metadata.store_epoch != epoch
        || metadata.project.id != project_id
        || watermark(conn)?.store_epoch != epoch
        || digest(&metadata, budget.max_bytes)? != hash
        || id
            != format!(
                "evidence-snapshot:v1:{}",
                hash.trim_start_matches("sha256:")
            )
    {
        bail!("evidence snapshot integrity fence failed");
    }
    Ok(EvidenceSnapshot {
        id: id.to_string(),
        digest: hash,
        metadata,
    })
}

/// For a future worker's SAME transaction as lease/cancel/cursor acknowledgement.
/// This helper validates evidence only; it does not implement any worker lease.
pub(crate) fn validate_snapshot_on_tx(
    tx: &Transaction<'_>,
    id: &str,
    budget: EvidenceBudget,
) -> Result<CurrentEvidenceSnapshot> {
    let stored = load_snapshot(tx, id, budget)?;
    for member in &stored.metadata.sessions {
        if seal(tx, &member.seal.session_id)?.as_ref() != Some(&member.seal) {
            bail!("evidence snapshot is stale");
        }
    }
    let ids = stored
        .metadata
        .sessions
        .iter()
        .map(|s| s.seal.session_id.clone())
        .collect::<Vec<_>>();
    let (current, events) = snapshot(
        tx,
        &stored.metadata.project.id,
        &ids,
        &stored.metadata.projection_policy_version,
        budget,
    )?;
    if current != stored {
        bail!("evidence snapshot is stale");
    }
    Ok(CurrentEvidenceSnapshot {
        snapshot: stored,
        events,
    })
}

impl Store {
    pub fn evidence_high_watermark(&self) -> Result<EvidenceWatermark> {
        watermark(&self.conn)
    }

    /// Latest session state, not an unbounded audit log. Changed open sessions
    /// receive newer sequence positions when explicitly completed later.
    pub fn evidence_changes(
        &mut self,
        project_id: &str,
        after: Option<&EvidenceCursor>,
        through: Option<&EvidenceWatermark>,
        budget: EvidenceBudget,
    ) -> Result<EvidenceChangePage> {
        budget.validate()?;
        label(project_id)?;
        let tx = self.conn.transaction()?;
        let high = watermark(&tx)?;
        if through.is_some_and(|w| w.store_epoch.len() != 32)
            || after.is_some_and(|c| c.watermark.store_epoch.len() != 32)
        {
            bail!("evidence watermark epoch exceeds byte bounds");
        }
        let through = through.cloned().unwrap_or_else(|| high.clone());
        let start = after.map(|c| c.watermark.sequence).unwrap_or(0);
        let after_id = after.and_then(|c| c.session_id.as_deref());
        if let Some(id) = after_id {
            label(id)?;
        }
        if through.store_epoch != high.store_epoch
            || through.sequence > high.sequence
            || through.sequence < 0
            || start < 0
            || start > through.sequence
            || after.is_some_and(|c| c.watermark.store_epoch != high.store_epoch)
        {
            bail!("evidence watermark scope/range mismatch");
        }
        // Row-value comparison seeks into a shared sequence tie using the
        // composite index, avoiding rescanning earlier pages of that tie.
        let predicate = change_predicate(after_id.is_some());
        let sql = format!(
            "SELECT count(*),coalesce(sum(n),0) FROM (SELECT length(CAST(session_id AS BLOB))+256 n FROM evidence_sessions WHERE {predicate} ORDER BY changed_seq,session_id LIMIT ?5)"
        );
        let (count, bytes): (i64, i64) = tx.query_row(
            &sql,
            params![
                project_id,
                through.sequence,
                start,
                after_id,
                i64::try_from(budget.max_sessions + 1)?
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if bytes > budget.max_bytes as i64 {
            bail!("evidence change metadata byte budget exceeded");
        }
        let sql = format!(
            "SELECT session_id,revision,changed_seq,present,sealed_revision IS NOT NULL,event_count,event_bytes FROM evidence_sessions WHERE {predicate} ORDER BY changed_seq,session_id LIMIT ?5"
        );
        let mut stmt = tx.prepare(&sql)?;
        let changes = stmt
            .query_map(
                params![
                    project_id,
                    through.sequence,
                    start,
                    after_id,
                    i64::try_from(budget.max_sessions)?
                ],
                |r| {
                    Ok(EvidenceSessionChange {
                        session_id: r.get(0)?,
                        revision: r.get(1)?,
                        change_sequence: r.get(2)?,
                        present: r.get(3)?,
                        completed: r.get(4)?,
                        source_rows: count_from_row(r, 5)?,
                        source_bytes: count_from_row(r, 6)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next = if count > budget.max_sessions as i64 {
            changes.last().map(|last| EvidenceCursor {
                watermark: EvidenceWatermark {
                    store_epoch: high.store_epoch,
                    sequence: last.change_sequence,
                },
                session_id: Some(last.session_id.clone()),
            })
        } else {
            None
        };
        drop(stmt);
        tx.commit()?;
        Ok(EvidenceChangePage {
            through,
            next,
            changes,
        })
    }

    /// Trusted caller attests current LOCAL evidence completion; no idle/model inference.
    pub fn attest_session_completion(
        &mut self,
        session_id: &str,
        attestation: &str,
        budget: EvidenceBudget,
    ) -> Result<CompletionSeal> {
        budget.validate()?;
        label(session_id)?;
        label(attestation)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let completed = attest_completion_on_tx(&tx, session_id, attestation, budget)?;
        tx.commit()?;
        Ok(completed)
    }

    /// Store-wide event-ID exclusion, retaining raw rows and attesting-project provenance.
    pub fn revoke_evidence_event(
        &mut self,
        project_id: &str,
        event_id: &str,
        reason: &str,
    ) -> Result<EvidenceRevocation> {
        label(project_id)?;
        label(event_id)?;
        label(reason)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = revocation(&tx, event_id)?;
        if let Some(existing) = existing {
            tx.commit()?;
            return Ok(existing);
        }
        let owner_mismatch: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE id=?1 AND project_id<>?2)",
            params![event_id, project_id],
            |r| r.get(0),
        )?;
        if owner_mismatch {
            bail!("evidence revocation owner mismatch");
        }
        let sequence = watermark(&tx)?
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow!("evidence sequence overflow"))?;
        tx.execute("INSERT INTO evidence_revocations(project_id,event_id,reason,control_seq,created_at) VALUES(?1,?2,?3,?4,?5)",params![project_id,event_id,reason,sequence,now_rfc3339()?])?;
        let result =
            revocation(&tx, event_id)?.ok_or_else(|| anyhow!("revocation was not stored"))?;
        tx.commit()?;
        Ok(result)
    }

    pub fn create_evidence_snapshot(
        &mut self,
        project_id: &str,
        ordered_session_ids: &[String],
        projection_policy_version: &str,
        budget: EvidenceBudget,
    ) -> Result<EvidenceSnapshot> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (snapshot, _) = snapshot(
            &tx,
            project_id,
            ordered_session_ids,
            projection_policy_version,
            budget,
        )?;
        let body = json(&snapshot.metadata, budget.max_bytes)?;
        tx.execute("INSERT INTO evidence_snapshots(id,project_id,store_epoch,digest,metadata_json,created_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING",params![snapshot.id,project_id,snapshot.metadata.store_epoch,snapshot.digest,body,now_rfc3339()?])?;
        let persisted = load_snapshot(&tx, &snapshot.id, budget)?;
        if persisted != snapshot {
            bail!("evidence snapshot identity conflict");
        }
        tx.commit()?;
        Ok(snapshot)
    }

    /// Bounded LOCAL RAW material. Does not authorize projection, provider use or execution.
    pub fn read_current_evidence_snapshot(
        &mut self,
        snapshot_id: &str,
        budget: EvidenceBudget,
    ) -> Result<CurrentEvidenceSnapshot> {
        let tx = self.conn.transaction()?;
        let result = validate_snapshot_on_tx(&tx, snapshot_id, budget)?;
        tx.commit()?;
        Ok(result)
    }

    /// Only an evidence-fenced, pending/rejected receipt, NOT a worker/job lease commit.
    /// Typed input is already caller-owned; limits bound additional serialization/clones,
    /// not allocations the caller performed before entering this function.
    pub fn commit_snapshot_proposals(
        &mut self,
        snapshot_id: &str,
        output: DreamRunnerOutput,
        budget: EvidenceBudget,
    ) -> Result<ProposalReceipt> {
        budget.validate()?;
        label(snapshot_id)?;
        json(&output, MAX_PROPOSAL_BYTES)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result =
            commit_proposals_on_tx(&tx, snapshot_id, output, budget, None, MAX_PROPOSAL_BYTES)?;
        tx.commit()?;
        Ok(result)
    }

    /// Historical receipt inspection stays available after its evidence becomes stale.
    pub fn evidence_proposal_receipt(&self, id: &str) -> Result<ProposalReceipt> {
        label(id)?;
        receipt(&self.conn, id)
    }
}
/// Caller holds one immediate transaction, including any scope/batch checks.
pub(super) fn attest_completion_on_tx(
    tx: &Transaction<'_>,
    session_id: &str,
    attestation: &str,
    budget: EvidenceBudget,
) -> Result<CompletionSeal> {
    budget.validate()?;
    label(session_id)?;
    label(attestation)?;
    let project_bytes: i64 = tx.query_row("SELECT length(CAST(project_id AS BLOB)) FROM evidence_sessions WHERE session_id=?1 AND present=1", [session_id], |r|r.get(0)).optional()?.ok_or_else(||anyhow!("evidence session is missing"))?;
    if project_bytes > MAX_LABEL_BYTES as i64 {
        bail!("evidence project identity exceeds byte bound");
    }
    let project_id: String = tx
        .query_row(
            "SELECT project_id FROM evidence_sessions WHERE session_id=?1 AND present=1",
            [session_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("evidence session is missing"))?;
    // Preflight even for a repeated seal; callers cannot bypass smaller budgets.
    let ids = [session_id.to_string()];
    let (project, sessions) = current_sessions(tx, &project_id, &ids, budget)?;
    if let Some(existing) = seal(tx, session_id)? {
        if existing.attestation == attestation
            && existing.digest
                == session_digest(&watermark(tx)?.store_epoch, &project, &sessions[0], budget)?
        {
            return Ok(existing);
        }
    }
    let now = now_rfc3339()?;
    tx.execute(
        "UPDATE sessions SET ended_at=?2 WHERE id=?1",
        params![session_id, now],
    )?;
    let epoch = watermark(tx)?.store_epoch;
    let (project, mut sessions) = current_sessions(tx, &project_id, &ids, budget)?;
    let current = sessions
        .pop()
        .ok_or_else(|| anyhow!("evidence session is missing"))?;
    let hash = session_digest(&epoch, &project, &current, budget)?;
    let sequence = advance(tx)?;
    tx.execute("UPDATE evidence_sessions SET sealed_revision=revision,seal_digest=?2,attestation=?3,sealed_at=?4,seal_seq=?5,changed_seq=?5 WHERE session_id=?1",params![session_id,hash,attestation,now,sequence])?;
    let completed =
        seal(tx, session_id)?.ok_or_else(|| anyhow!("completion seal was not stored"))?;

    Ok(completed)
}

pub(super) fn preflight_completion_sessions(
    tx: &Transaction<'_>,
    project: &str,
    ids: &[String],
    budget: EvidenceBudget,
) -> Result<()> {
    current_sessions(tx, project, ids, budget).map(|_| ())
}

/// Dedicated receipt integration for a caller already holding the evidence/job fence.
/// Restricted citations are selected by the trusted shared-safe projection worker.
pub(super) fn commit_proposals_on_tx(
    tx: &Transaction<'_>,
    snapshot_id: &str,
    mut output: DreamRunnerOutput,
    budget: EvidenceBudget,
    restricted_refs: Option<&BTreeSet<String>>,
    max_output_bytes: usize,
) -> Result<ProposalReceipt> {
    if max_output_bytes == 0 || max_output_bytes > MAX_PROPOSAL_BYTES {
        bail!("invalid proposal byte bound");
    }
    json(&output, max_output_bytes)?;
    // Validate BEFORE duplicate lookup: stale evidence cannot get fresh success
    // by replaying an otherwise identical old receipt.
    let current = validate_snapshot_on_tx(tx, snapshot_id, budget)?;
    let refs = current
        .events
        .iter()
        .map(|e| format!("mmr://event/{}", e.id))
        .collect::<BTreeSet<_>>();
    let refs = match restricted_refs {
        Some(restricted) if restricted.is_subset(&refs) => restricted,
        Some(_) => bail!("proposal citations are outside current snapshot"),
        None => &refs,
    };
    validate_output(&mut output, refs)
        .map_err(|_| anyhow!("invalid snapshot proposal schema or evidence reference"))?;
    let body = json(&output, max_output_bytes)?;
    let hash = content_hash(&body);
    let id = format!(
        "evidence-receipt:v1:{}",
        content_hash(&format!("{snapshot_id}:{hash}")).trim_start_matches("sha256:")
    );
    tx.execute("INSERT INTO evidence_proposal_receipts(id,snapshot_id,output_hash,proposal_json,created_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(snapshot_id,output_hash) DO NOTHING",params![id,snapshot_id,hash,body,now_rfc3339()?])?;
    let result = receipt(tx, &id)?;

    Ok(result)
}

fn revocation(conn: &Connection, id: &str) -> Result<Option<EvidenceRevocation>> {
    let oversized: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM evidence_revocations WHERE event_id=?1 AND (length(CAST(project_id AS BLOB))>1024 OR length(CAST(reason AS BLOB))>1024))", [id], |r|r.get(0))?;
    if oversized {
        bail!("evidence revocation exceeds byte bounds");
    }
    conn.query_row(
        "SELECT event_id,project_id,reason,control_seq FROM evidence_revocations WHERE event_id=?1",
        [id],
        |r| {
            Ok(EvidenceRevocation {
                event_id: r.get(0)?,
                attesting_project_id: r.get(1)?,
                reason: r.get(2)?,
                control_sequence: r.get(3)?,
            })
        },
    )
    .optional()
    .context("read evidence revocation")
}
fn receipt(conn: &Connection, id: &str) -> Result<ProposalReceipt> {
    let lengths:Option<(i64,i64,i64,i64,i64)>=conn.query_row("SELECT length(CAST(id AS BLOB)),length(CAST(snapshot_id AS BLOB)),length(CAST(output_hash AS BLOB)),length(CAST(proposal_json AS BLOB)),length(CAST(created_at AS BLOB)) FROM evidence_proposal_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let (a, b, c, n, d) = lengths.ok_or_else(|| anyhow!("evidence receipt is missing"))?;
    if [a, b, c, d]
        .iter()
        .any(|n| *n < 0 || *n > MAX_LABEL_BYTES as i64)
        || n < 0
        || n > MAX_PROPOSAL_BYTES as i64
    {
        bail!("evidence receipt exceeds byte bounds");
    }
    conn.query_row("SELECT id,snapshot_id,output_hash,proposal_json,created_at FROM evidence_proposal_receipts WHERE id=?1",[id],|r|Ok(ProposalReceipt{id:r.get(0)?,snapshot_id:r.get(1)?,output_hash:r.get(2)?,proposal_json:r.get(3)?,created_at:r.get(4)?})).context("read evidence proposal receipt")
}

#[cfg(test)]
mod tests;
