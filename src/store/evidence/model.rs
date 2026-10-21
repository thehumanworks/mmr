use anyhow::{Result, bail};
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, marker::PhantomData};

fn bounded_vec<'de, D, T, const MAX: usize>(
    deserializer: D,
) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T, const MAX: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for Bounded<T, MAX> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "at most {MAX} entries")
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() == MAX {
                    return Err(serde::de::Error::custom(
                        "evidence collection exceeds count bound",
                    ));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Bounded::<T, MAX>(PhantomData))
}
fn snapshot_sessions<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<SnapshotSession>, D::Error> {
    bounded_vec::<'de, D, SnapshotSession, MAX_SESSIONS>(d)
}
fn event_refs<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<EvidenceEventRef>, D::Error> {
    bounded_vec::<'de, D, EvidenceEventRef, MAX_EVENTS>(d)
}

pub const MAX_SESSIONS: usize = 64;
pub const MAX_EVENTS: usize = 4096;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PROPOSAL_BYTES: usize = 1024 * 1024;
pub(super) const MAX_LABEL_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy)]
pub struct EvidenceBudget {
    pub max_sessions: usize,
    pub max_events: usize,
    pub max_bytes: usize,
}

impl Default for EvidenceBudget {
    fn default() -> Self {
        Self {
            max_sessions: MAX_SESSIONS,
            max_events: MAX_EVENTS,
            max_bytes: MAX_BYTES,
        }
    }
}

impl EvidenceBudget {
    pub(super) fn validate(self) -> Result<Self> {
        if self.max_sessions == 0
            || self.max_sessions > MAX_SESSIONS
            || self.max_events == 0
            || self.max_events > MAX_EVENTS
            || self.max_bytes == 0
            || self.max_bytes > MAX_BYTES
        {
            bail!("evidence budget exceeds supported bounds");
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceWatermark {
    pub store_epoch: String,
    pub sequence: i64,
}

/// None session_id starts strictly after the sequence (a fully acknowledged window).
/// Some session_id pages within a sequence that affected several sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceCursor {
    pub watermark: EvidenceWatermark,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionSeal {
    pub session_id: String,
    pub revision: i64,
    pub control_sequence: i64,
    pub digest: String,
    pub attestation: String,
    pub completed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceSessionChange {
    pub session_id: String,
    pub revision: i64,
    pub change_sequence: i64,
    pub present: bool,
    pub completed: bool,
    pub source_rows: usize,
    pub source_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceChangePage {
    pub through: EvidenceWatermark,
    pub next: Option<EvidenceCursor>,
    pub changes: Vec<EvidenceSessionChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectEvidenceMetadata {
    pub id: String,
    pub canonical_path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEvidenceMetadata {
    pub id: String,
    pub project_id: String,
    pub source: String,
    pub source_session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub title: Option<String>,
    pub raw_local_ref: Option<String>,
    pub adapter_version: String,
    pub source_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBlob {
    pub id: String,
    pub kind: String,
    pub media_type: String,
    pub content_hash: String,
    pub storage_ref: String,
    pub byte_len: i64,
}

/// Local raw source data. This type grants no provider/projection authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEvidenceEvent {
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
    pub blob_id: Option<String>,
    pub blob: Option<EvidenceBlob>,
    pub redaction_policy_id: String,
    pub redaction_policy_version: String,
    pub redaction_policy_description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceEventRef {
    pub event_id: String,
    pub semantic_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSession {
    pub seal: CompletionSeal,
    pub metadata: SessionEvidenceMetadata,
    pub source_rows: usize,
    #[serde(deserialize_with = "event_refs")]
    pub events: Vec<EvidenceEventRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSnapshotMetadata {
    pub format_version: u32,
    pub store_epoch: String,
    pub project: ProjectEvidenceMetadata,
    pub projection_policy_version: String,
    #[serde(deserialize_with = "snapshot_sessions")]
    pub sessions: Vec<SnapshotSession>,
    pub event_count: usize,
    pub source_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceSnapshot {
    pub id: String,
    pub digest: String,
    pub metadata: EvidenceSnapshotMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurrentEvidenceSnapshot {
    pub snapshot: EvidenceSnapshot,
    pub events: Vec<LocalEvidenceEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceRevocation {
    pub event_id: String,
    pub attesting_project_id: String,
    pub reason: String,
    pub control_sequence: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalReceipt {
    pub id: String,
    pub snapshot_id: String,
    pub output_hash: String,
    pub proposal_json: String,
    pub created_at: String,
}
