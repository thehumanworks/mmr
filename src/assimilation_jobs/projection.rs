use std::io::{self, Write};

use serde::Serialize;

use super::{JobBudgets, JobWorkerError};
use crate::dream::{DreamEvidence, DreamRequest, EvidenceAccess};
use crate::redaction::{DeterministicPrivacyDetector, PiiCoverageStatus, scan_text_with_detector};
use crate::store::content_hash;
use crate::store::evidence::CurrentEvidenceSnapshot;
use crate::store::jobs::{JobOmission, JobOmissionReason, JobProjection};

pub const PROJECTION_POLICY: &str = "deterministic-shared-safe-narrative/v1";

pub struct ProjectedSnapshot {
    pub request: DreamRequest,
    pub projection: JobProjection,
}

pub fn bounded_serialized_len(
    value: &impl Serialize,
    limit: usize,
) -> Result<usize, JobWorkerError> {
    struct Counter {
        len: usize,
        limit: usize,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.len) {
                return Err(io::Error::other("bounded serialization"));
            }
            self.len += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { len: 0, limit };
    serde_json::to_writer(&mut counter, value).map_err(|_| JobWorkerError::Projection)?;
    Ok(counter.len)
}

fn safe_metadata(value: &str) -> bool {
    let scanned = scan_text_with_detector(value, &DeterministicPrivacyDetector);
    !scanned.blocks_sync
        && scanned.pii_coverage.status == PiiCoverageStatus::Available
        && scanned.redacted_text == value
}

fn opaque_identity(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

fn safe_project(value: &str) -> bool {
    // Store-generated digests are provenance, not entropy-secret candidates.
    opaque_identity(value, "proj:v1:") || safe_metadata(value)
}

pub fn project_snapshot(
    current: &CurrentEvidenceSnapshot,
    budgets: JobBudgets,
) -> Result<ProjectedSnapshot, JobWorkerError> {
    budgets.validate()?;
    if current.events.len() > budgets.max_events
        || current.snapshot.metadata.sessions.len() > budgets.max_sessions
        || !safe_project(&current.snapshot.metadata.project.id)
    {
        return Err(JobWorkerError::Projection);
    }
    let mut evidence = Vec::new();
    let mut included_event_ids = Vec::new();
    let mut omissions = Vec::new();
    for event in &current.events {
        let evidence_ref = format!("mmr://event/{}", event.id);
        // Tools, lifecycle payloads and unknown kinds require their own safe
        // projection. Narrative roles remain untrusted data, never authority.
        let eligible = matches!(
            event.event_type.as_str(),
            "user_turn" | "assistant_turn" | "compaction"
        ) && matches!(
            event.role.as_str(),
            "user" | "assistant" | "system" | "developer"
        ) && (opaque_identity(&event.id, "evt:v1:") || safe_metadata(&evidence_ref))
            && safe_project(&event.project_id)
            && [
                &event.source,
                &event.role,
                &event.event_type,
                &event.timestamp,
            ]
            .into_iter()
            .all(|v| safe_metadata(v));
        let scanned = scan_text_with_detector(&event.content_text, &DeterministicPrivacyDetector);
        if !eligible
            || scanned.blocks_sync
            || scanned.pii_coverage.status != PiiCoverageStatus::Available
        {
            omissions.push(JobOmission {
                event_id: event.id.clone(),
                reason: JobOmissionReason::PolicyBlocked,
            });
            continue;
        }
        included_event_ids.push(event.id.clone());
        evidence.push(DreamEvidence {
            evidence_ref,
            project_id: event.project_id.clone(),
            source: event.source.clone(),
            role: event.role.clone(),
            event_type: event.event_type.clone(),
            timestamp: event.timestamp.clone(),
            content_text: scanned.redacted_text,
        });
    }
    // Preflight before allocating serialized hash material. Bind omitted IDs,
    // the source snapshot and policy to the projection identity as well.
    let material = (
        &current.snapshot.digest,
        PROJECTION_POLICY,
        &evidence,
        &omissions,
    );
    bounded_serialized_len(&material, budgets.max_request_bytes)?;
    let hash =
        content_hash(&serde_json::to_string(&material).map_err(|_| JobWorkerError::Projection)?);
    let request = DreamRequest {
        project_id: current.snapshot.metadata.project.id.clone(),
        provider: "command".into(),
        model: None,
        evidence_access: EvidenceAccess::SharedSafe,
        evidence_hash: hash.clone(),
        evidence,
    };
    // The transport adds exactly one terminating newline.
    bounded_serialized_len(&request, budgets.max_request_bytes.saturating_sub(1))?;
    let projection = JobProjection {
        policy_version: PROJECTION_POLICY.into(),
        evidence_hash: hash,
        included_event_ids,
        omissions,
    };
    bounded_serialized_len(&projection, 256 * 1024)?;
    Ok(ProjectedSnapshot {
        request,
        projection,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn job_projection_opaque_identity_exemption_is_field_specific() {
        let hash = "0123456789abcdef".repeat(4);
        let project = format!("proj:v1:{hash}");
        assert!(safe_project(&project));
        assert!(
            !safe_metadata(&project),
            "freeform fields still reject entropy-like values"
        );
        for value in [
            format!("proj:v1:{}", hash.to_uppercase()),
            format!("proj:v1:{}", &hash[..63]),
            format!("proj:v1:{hash}0"),
            format!("proj:v1:{hash}\n"),
            format!("wrong:v1:{hash}"),
        ] {
            assert!(!opaque_identity(&value, "proj:v1:"));
            assert!(!safe_project(&value));
        }
        assert!(opaque_identity(&format!("evt:v1:{hash}"), "evt:v1:"));
        assert!(!opaque_identity(&project, "evt:v1:"));
    }
}
