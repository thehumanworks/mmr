use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::redaction::{
    DeterministicPrivacyDetector, PiiCoverage, PiiCoverageStatus, PrivacyDetector,
    RedactionFinding, scan_text_with_detector,
};
use crate::store::{
    DEFAULT_REDACTION_POLICY_ID, EventRecord, LearnedMemoryRecord, NewEvent, NewLearnedMemory,
    NewRedactionSpan, NewSyncManifestEntry, ProjectRecord, SourceEventIdentity, Store,
    content_hash, default_db_path,
};

const ENV_FAKE_REMOTE_DIR: &str = "MMR_FAKE_REMOTE_DIR";
const ENV_FAKE_REMOTE_AUTH: &str = "MMR_FAKE_REMOTE_AUTH";
const ENV_GITHUB_USER: &str = "MMR_GITHUB_USER";
const ENV_GITHUB_USER_FALLBACK: &str = "GITHUB_USER";
const REMOTE_REPO_NAME: &str = "mmr-store";
const MANIFEST_VERSION: u32 = 1;
const BACKEND_NAME: &str = "file-github";

#[derive(Debug, Clone)]
pub struct FakeGithubRemote {
    descriptor: String,
    root: PathBuf,
    auth_ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteSummary {
    pub descriptor: String,
    pub backend: String,
    pub available: bool,
    pub auth_status: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HydrationReport {
    pub remote_events: usize,
    pub inserted_events: usize,
    pub existing_events: usize,
    pub remote_learned_memory: usize,
    pub inserted_learned_memory: usize,
    pub existing_learned_memory: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub status: String,
    pub remote: RemoteSummary,
    pub policy_id: String,
    pub manifest_id: String,
    pub root_hash: String,
    pub total_events: usize,
    pub synced_events: usize,
    pub uploaded_events: usize,
    pub uploaded_search_documents: usize,
    pub synced_learned_memory: usize,
    pub uploaded_learned_memory: usize,
    pub blocked_events: usize,
    pub blocked_learned_memory: usize,
    pub remote_events: usize,
    pub remote_learned_memory: usize,
    pub append_only: bool,
    pub pii_coverage: PiiCoverage,
    pub blocked: Vec<SyncBlockedEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncBlockedEvent {
    pub event_id: String,
    pub source: String,
    pub event_type: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteEventPayload {
    manifest_version: u32,
    event_id: String,
    source: String,
    source_session_id: String,
    source_event_id: Option<String>,
    event_type: String,
    role: String,
    timestamp: String,
    content_text: String,
    content_hash: String,
    parent_hash: Option<String>,
    parser_version: String,
    redaction_policy_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteSearchPayload {
    manifest_version: u32,
    event_id: String,
    source: String,
    document_text: String,
    citation: String,
    content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteProjectPayload {
    manifest_version: u32,
    project_id: String,
    display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteLearnedMemoryPayload {
    manifest_version: u32,
    id: String,
    kind: String,
    claim: String,
    confidence: f64,
    status: String,
    evidence_refs: Vec<String>,
    counterevidence_refs: Vec<String>,
    content_hash: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteManifestPayload {
    manifest_version: u32,
    remote: String,
    project_id: String,
    root_hash: String,
    redaction_policy_id: String,
    entries: Vec<RemoteManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteManifestEntry {
    kind: String,
    ref_id: String,
    content_hash: String,
    sync_path: String,
}

#[derive(Debug, Clone)]
struct SyncProjection {
    event_id: String,
    original_event_id: String,
    event: RemoteEventPayload,
    search: RemoteSearchPayload,
    event_path: String,
    search_path: String,
}

#[derive(Debug, Clone)]
struct LearnedMemoryProjection {
    memory_id: String,
    memory: RemoteLearnedMemoryPayload,
    path: String,
}

pub fn remote_for_operations() -> Result<FakeGithubRemote> {
    FakeGithubRemote::from_env(true)
}

pub fn remote_for_status() -> Result<FakeGithubRemote> {
    FakeGithubRemote::from_env(false)
}

pub fn safe_projection_blocker(event: &EventRecord) -> Option<&'static str> {
    match event.event_type.as_str() {
        "tool_call" => Some("tool_call events require a dedicated safe sync projection"),
        "tool_result" => Some("tool_result events require a dedicated safe sync projection"),
        "unknown_raw_event" => {
            Some("unknown_raw_event events require a dedicated safe sync projection")
        }
        _ => None,
    }
}

pub fn hydrate_project(
    store: &mut Store,
    project: &ProjectRecord,
    remote: &FakeGithubRemote,
) -> Result<HydrationReport> {
    let events = remote.read_events(project)?;
    let learned_memory = remote.read_learned_memory(project)?;
    let mut inserted_events = 0;
    let mut existing_events = 0;
    let mut inserted_learned_memory = 0;
    let mut existing_learned_memory = 0;
    let mut synced_event_ids = Vec::new();
    let mut remote_event_ref_map = BTreeMap::new();
    let mut remote_evidence_refs = BTreeSet::new();

    for payload in &events {
        let event = payload.to_new_event(remote)?;
        let event_record = if let Some(event_record) =
            store.event_by_source_identity(SourceEventIdentity {
                project_id: &project.id,
                source: &payload.source,
                source_session_id: &payload.source_session_id,
                source_event_id: payload.source_event_id.as_deref(),
                event_type: &payload.event_type,
                role: &payload.role,
                timestamp: &payload.timestamp,
                parent_hash: payload.parent_hash.as_deref(),
                parser_version: &payload.parser_version,
            })? {
            store.upsert_search_document(&event_record)?;
            existing_events += 1;
            event_record
        } else if store.event_exists(&event.event_id())? {
            let event_record = store.event_by_id(&event.event_id())?;
            store.upsert_search_document(&event_record)?;
            existing_events += 1;
            event_record
        } else {
            let (event_record, _) = store.insert_event_with_search_document(&project.id, &event)?;
            inserted_events += 1;
            event_record
        };
        synced_event_ids.push(event_record.id.clone());
        remote_evidence_refs.insert(format!("mmr://event/{}", payload.event_id));
        remote_event_ref_map.insert(
            format!("mmr://event/{}", payload.event_id),
            format!("mmr://event/{}", event_record.id),
        );
    }
    store.mark_events_synced(&synced_event_ids)?;

    for payload in &learned_memory {
        payload
            .validate(&remote_evidence_refs)
            .map_err(|err| anyhow::anyhow!("invalid remote learned memory payload: {err}"))?;
        let evidence_refs =
            remap_remote_learned_memory_refs(&payload.evidence_refs, &remote_event_ref_map)?;
        let counterevidence_refs =
            remap_remote_learned_memory_refs(&payload.counterevidence_refs, &remote_event_ref_map)?;
        let existed = store.learned_memory_by_id(&payload.id).is_ok();
        let memory = NewLearnedMemory {
            kind: payload.kind.clone(),
            claim: payload.claim.clone(),
            confidence: payload.confidence,
            evidence_refs,
            counterevidence_refs,
            status: payload.status.clone(),
        };
        store.upsert_learned_memory_from_sync(
            &payload.id,
            &project.id,
            &memory,
            &payload.created_at,
        )?;
        if existed {
            existing_learned_memory += 1;
        } else {
            inserted_learned_memory += 1;
        }
    }

    Ok(HydrationReport {
        remote_events: events.len(),
        inserted_events,
        existing_events,
        remote_learned_memory: learned_memory.len(),
        inserted_learned_memory,
        existing_learned_memory,
    })
}

pub fn sync_project(
    store: &mut Store,
    project: &ProjectRecord,
    remote: &FakeGithubRemote,
    source: Option<&str>,
) -> Result<SyncReport> {
    let created = remote.ensure()?;
    let project_prefix = remote.project_prefix_for_write(project)?;
    let remote_project_id = remote
        .project_id_for_prefix(&project_prefix)?
        .unwrap_or_else(|| project.id.clone());
    remote.write_project(project, &project_prefix, &remote_project_id)?;

    let events = store.events_for_project(&project.id, source, None)?;
    let mut projections = Vec::new();
    let mut blocked = Vec::new();
    let mut synced_original_ids = Vec::new();
    let detector = DeterministicPrivacyDetector;
    let mut pii_coverage = detector.coverage();

    for event in &events {
        let outcome = scan_text_with_detector(&event.content_text, &detector);
        pii_coverage = outcome.pii_coverage.clone();
        let spans = outcome
            .findings
            .iter()
            .map(new_redaction_span_from_finding)
            .collect::<Vec<_>>();
        let mut reasons = Vec::new();
        if let Some(reason) = safe_projection_blocker(event) {
            reasons.push(reason.to_string());
        }
        reasons.extend(blocking_reasons(&outcome.findings));

        if reasons.is_empty() && outcome.pii_coverage.status != PiiCoverageStatus::Available {
            reasons.push(outcome.pii_coverage.reason.clone());
        }

        let status = if reasons.is_empty() {
            "passed"
        } else {
            "blocked"
        };
        store.record_redaction_result(&event.id, DEFAULT_REDACTION_POLICY_ID, status, &spans)?;

        if !reasons.is_empty() {
            blocked.push(SyncBlockedEvent {
                event_id: event.id.clone(),
                source: event.source.clone(),
                event_type: event.event_type.clone(),
                reasons,
            });
            continue;
        }

        let projection = SyncProjection::from_event(&project_prefix, event, outcome.redacted_text);
        synced_original_ids.push(event.id.clone());
        projections.push(projection);
    }

    let mut evidence_ref_map = BTreeMap::new();
    for projection in &projections {
        evidence_ref_map.insert(
            format!("mmr://event/{}", projection.original_event_id),
            format!("mmr://event/{}", projection.event_id),
        );
    }
    let mut learned_projections = Vec::new();
    let mut blocked_learned_memory = 0;
    if source.is_none() {
        for memory in store.learned_memory_for_project(&project.id)? {
            if memory.status != "active" {
                continue;
            }
            match LearnedMemoryProjection::from_memory(&project_prefix, &memory, &evidence_ref_map)
            {
                Ok(Some(projection)) => learned_projections.push(projection),
                Ok(None) => blocked_learned_memory += 1,
                Err(err) => return Err(err),
            }
        }
    }

    let mut uploaded_events = 0;
    let mut uploaded_search_documents = 0;
    let mut uploaded_learned_memory = 0;
    let mut manifest_entries = Vec::new();
    for projection in &projections {
        if remote.write_json_if_absent(&projection.event_path, &projection.event)? {
            uploaded_events += 1;
        }
        if remote.write_json_if_absent(&projection.search_path, &projection.search)? {
            uploaded_search_documents += 1;
        }
        manifest_entries.push(RemoteManifestEntry {
            kind: "event".to_string(),
            ref_id: projection.event_id.clone(),
            content_hash: projection.event.content_hash.clone(),
            sync_path: projection.event_path.clone(),
        });
        manifest_entries.push(RemoteManifestEntry {
            kind: "search_document".to_string(),
            ref_id: projection.event_id.clone(),
            content_hash: projection.search.content_hash.clone(),
            sync_path: projection.search_path.clone(),
        });
    }
    for projection in &learned_projections {
        if remote.write_json_if_absent(&projection.path, &projection.memory)? {
            uploaded_learned_memory += 1;
        }
        manifest_entries.push(RemoteManifestEntry {
            kind: "learned_memory".to_string(),
            ref_id: projection.memory_id.clone(),
            content_hash: projection.memory.content_hash.clone(),
            sync_path: projection.path.clone(),
        });
    }
    manifest_entries.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.ref_id.cmp(&right.ref_id))
            .then_with(|| left.sync_path.cmp(&right.sync_path))
    });

    let root_hash = manifest_root_hash(&manifest_entries);
    let manifest = RemoteManifestPayload {
        manifest_version: MANIFEST_VERSION,
        remote: remote.descriptor.clone(),
        project_id: remote_project_id,
        root_hash: root_hash.clone(),
        redaction_policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
        entries: manifest_entries,
    };
    let manifest_path = remote.manifest_path(&project_prefix, &root_hash);
    remote.write_json_if_absent(&manifest_path, &manifest)?;

    let store_entries = manifest
        .entries
        .iter()
        .map(|entry| NewSyncManifestEntry {
            entry_kind: entry.kind.clone(),
            entry_ref: entry.ref_id.clone(),
            content_hash: entry.content_hash.clone(),
            sync_path: entry.sync_path.clone(),
        })
        .collect::<Vec<_>>();
    let manifest_record = store.record_sync_manifest(
        &remote.descriptor,
        &project.id,
        i64::from(MANIFEST_VERSION),
        &root_hash,
        DEFAULT_REDACTION_POLICY_ID,
        &store_entries,
    )?;
    store.mark_events_synced(&synced_original_ids)?;

    let remote_events = remote.count_events(project)?;
    let remote_learned_memory = remote.count_learned_memory(project)?;
    let status = if blocked.is_empty() && blocked_learned_memory == 0 {
        "synced"
    } else if projections.is_empty() && learned_projections.is_empty() {
        "blocked"
    } else {
        "partial"
    };

    Ok(SyncReport {
        status: status.to_string(),
        remote: RemoteSummary {
            created,
            ..remote.summary()
        },
        policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
        manifest_id: manifest_record.id,
        root_hash,
        total_events: events.len(),
        synced_events: projections.len(),
        uploaded_events,
        uploaded_search_documents,
        synced_learned_memory: learned_projections.len(),
        uploaded_learned_memory,
        blocked_events: blocked.len(),
        blocked_learned_memory,
        remote_events,
        remote_learned_memory,
        append_only: true,
        pii_coverage,
        blocked,
    })
}

impl FakeGithubRemote {
    fn from_env(check_auth: bool) -> Result<Self> {
        let user = github_user();
        let descriptor = format!("github:{user}/{REMOTE_REPO_NAME}");
        let root = std::env::var_os(ENV_FAKE_REMOTE_DIR)
            .map(PathBuf::from)
            .unwrap_or(default_remote_root(&descriptor)?);
        let auth_ok = fake_auth_ok();
        if check_auth && !auth_ok {
            bail!("remote auth failed for {descriptor}");
        }
        Ok(Self {
            descriptor,
            root,
            auth_ok,
        })
    }

    pub fn summary(&self) -> RemoteSummary {
        RemoteSummary {
            descriptor: self.descriptor.clone(),
            backend: BACKEND_NAME.to_string(),
            available: self.auth_ok && self.root.exists(),
            auth_status: if self.auth_ok { "ok" } else { "failed" }.to_string(),
            created: false,
        }
    }

    pub fn descriptor(&self) -> &str {
        &self.descriptor
    }

    pub fn ensure(&self) -> Result<bool> {
        if !self.auth_ok {
            bail!("remote auth failed for {}", self.descriptor);
        }
        let created = !self.root.exists();
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create remote root {}", self.root.display()))?;
        let meta = serde_json::json!({
            "backend": BACKEND_NAME,
            "descriptor": self.descriptor,
            "repo": REMOTE_REPO_NAME,
        });
        self.write_json_if_absent("remote.json", &meta)?;
        Ok(created)
    }

    pub fn count_events(&self, project: &ProjectRecord) -> Result<usize> {
        let Some(project_prefix) = self.project_prefix_for_read(project)? else {
            return Ok(0);
        };
        let event_dir = self.root.join(project_prefix).join("sessions");
        if !event_dir.exists() {
            return Ok(0);
        }
        let count = WalkDir::new(event_dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter(|entry| {
                entry
                    .path()
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "events")
            })
            .count();
        Ok(count)
    }

    pub fn count_learned_memory(&self, project: &ProjectRecord) -> Result<usize> {
        let Some(project_prefix) = self.project_prefix_for_read(project)? else {
            return Ok(0);
        };
        let memory_dir = self.root.join(project_prefix).join("learned-memory");
        if !memory_dir.exists() {
            return Ok(0);
        }
        let count = WalkDir::new(memory_dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .count();
        Ok(count)
    }

    fn write_project(
        &self,
        project: &ProjectRecord,
        project_prefix: &str,
        project_id: &str,
    ) -> Result<()> {
        let project_path = format!("{project_prefix}/project.json");
        let absolute_project_path = self.root.join(&project_path);
        if absolute_project_path.exists() {
            let bytes = fs::read(&absolute_project_path).with_context(|| {
                format!("read remote project {}", absolute_project_path.display())
            })?;
            let payload =
                serde_json::from_slice::<RemoteProjectPayload>(&bytes).with_context(|| {
                    format!("parse remote project {}", absolute_project_path.display())
                })?;
            if payload.project_id != project_id {
                bail!("remote project id mismatch at {project_path}");
            }
            return Ok(());
        }
        let payload = RemoteProjectPayload {
            manifest_version: MANIFEST_VERSION,
            project_id: project_id.to_string(),
            display_name: project.display_name.clone(),
        };
        self.write_json_if_absent(&project_path, &payload)?;
        Ok(())
    }

    fn read_events(&self, project: &ProjectRecord) -> Result<Vec<RemoteEventPayload>> {
        let Some(project_prefix) = self.project_prefix_for_read(project)? else {
            return Ok(Vec::new());
        };
        let event_dir = self.root.join(project_prefix).join("sessions");
        if !event_dir.exists() {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        for entry in WalkDir::new(event_dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter(|entry| {
                entry
                    .path()
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "events")
            })
        {
            let bytes = fs::read(entry.path())
                .with_context(|| format!("read remote event {}", entry.path().display()))?;
            let payload = serde_json::from_slice::<RemoteEventPayload>(&bytes)
                .with_context(|| format!("parse remote event {}", entry.path().display()))?;
            payload
                .validate()
                .map_err(|err| anyhow::anyhow!("invalid remote event payload: {err}"))?;
            events.push(payload);
        }
        events.sort_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    fn read_learned_memory(
        &self,
        project: &ProjectRecord,
    ) -> Result<Vec<RemoteLearnedMemoryPayload>> {
        let Some(project_prefix) = self.project_prefix_for_read(project)? else {
            return Ok(Vec::new());
        };
        let memory_dir = self.root.join(project_prefix).join("learned-memory");
        if !memory_dir.exists() {
            return Ok(Vec::new());
        }
        let mut memories = Vec::new();
        for entry in WalkDir::new(memory_dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        {
            let bytes = fs::read(entry.path()).with_context(|| {
                format!("read remote learned memory {}", entry.path().display())
            })?;
            let payload = serde_json::from_slice::<RemoteLearnedMemoryPayload>(&bytes)
                .with_context(|| {
                    format!("parse remote learned memory {}", entry.path().display())
                })?;
            payload
                .validate_shape()
                .map_err(|err| anyhow::anyhow!("invalid remote learned memory payload: {err}"))?;
            memories.push(payload);
        }
        memories.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(memories)
    }

    fn write_json_if_absent<T: Serialize>(&self, relative_path: &str, value: &T) -> Result<bool> {
        let path = self.root.join(relative_path);
        let value = serde_json::to_value(value).context("serialize remote payload")?;
        if path.exists() {
            let existing = fs::read(&path)
                .with_context(|| format!("read existing remote payload {}", path.display()))?;
            let existing = serde_json::from_slice::<serde_json::Value>(&existing)
                .with_context(|| format!("parse existing remote payload {}", path.display()))?;
            if existing != value {
                bail!("remote payload conflict at {relative_path}");
            }
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create remote directory {}", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(&value).context("serialize remote payload")?;
        fs::write(&path, bytes)
            .with_context(|| format!("write remote payload {}", path.display()))?;
        Ok(true)
    }

    fn project_prefix(&self, project: &ProjectRecord) -> String {
        format!("projects/{}", safe_path_component(&project.id))
    }

    fn project_prefix_for_read(&self, project: &ProjectRecord) -> Result<Option<String>> {
        let current = self.project_prefix(project);
        if self.root.join(&current).exists() {
            return Ok(Some(current));
        }
        self.single_remote_project_prefix()
    }

    fn project_prefix_for_write(&self, project: &ProjectRecord) -> Result<String> {
        let current = self.project_prefix(project);
        if self.root.join(&current).exists() {
            return Ok(current);
        }
        Ok(self.single_remote_project_prefix()?.unwrap_or(current))
    }

    fn project_id_for_prefix(&self, project_prefix: &str) -> Result<Option<String>> {
        let project_path = self.root.join(project_prefix).join("project.json");
        if !project_path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&project_path)
            .with_context(|| format!("read remote project {}", project_path.display()))?;
        let payload = serde_json::from_slice::<RemoteProjectPayload>(&bytes)
            .with_context(|| format!("parse remote project {}", project_path.display()))?;
        Ok(Some(payload.project_id))
    }

    fn single_remote_project_prefix(&self) -> Result<Option<String>> {
        let projects_dir = self.root.join("projects");
        if !projects_dir.exists() {
            return Ok(None);
        }
        let mut projects = fs::read_dir(&projects_dir)
            .with_context(|| format!("read remote projects {}", projects_dir.display()))?
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        projects.sort();
        if projects.len() == 1 {
            Ok(Some(format!("projects/{}", projects.remove(0))))
        } else {
            Ok(None)
        }
    }

    fn manifest_path(&self, project_prefix: &str, root_hash: &str) -> String {
        format!(
            "{project_prefix}/manifests/{}.json",
            safe_path_component(root_hash)
        )
    }
}

impl SyncProjection {
    fn from_event(project_prefix: &str, event: &EventRecord, content_text: String) -> Self {
        let projected_event = NewEvent::new(
            &event.source,
            &event.source_session_id,
            &event.event_type,
            &event.role,
            &event.timestamp,
            &content_text,
            &event.parser_version,
        );
        let projected_event = match &event.source_event_id {
            Some(source_event_id) => projected_event.with_source_event_id(source_event_id),
            None => projected_event,
        };
        let projected_event = match &event.parent_hash {
            Some(parent_hash) => projected_event.with_parent_hash(parent_hash),
            None => projected_event,
        };
        let event_id = projected_event.event_id();
        let event_content_hash = projected_event.content_hash();
        let event_payload = RemoteEventPayload {
            manifest_version: MANIFEST_VERSION,
            event_id: event_id.clone(),
            source: event.source.clone(),
            source_session_id: event.source_session_id.clone(),
            source_event_id: event.source_event_id.clone(),
            event_type: event.event_type.clone(),
            role: event.role.clone(),
            timestamp: event.timestamp.clone(),
            content_text,
            content_hash: event_content_hash.clone(),
            parent_hash: event.parent_hash.clone(),
            parser_version: event.parser_version.clone(),
            redaction_policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
        };
        let search = RemoteSearchPayload {
            manifest_version: MANIFEST_VERSION,
            event_id: event_id.clone(),
            source: event.source.clone(),
            document_text: event_payload.content_text.clone(),
            citation: format!("mmr://event/{event_id}"),
            content_hash: content_hash(&format!(
                "search:{event_id}:{}",
                event_payload.content_text
            )),
        };
        let session_component = safe_path_component(&event.source_session_id);
        let event_component = safe_path_component(&event_id);
        let event_path =
            format!("{project_prefix}/sessions/{session_component}/events/{event_component}.json");
        let search_path = format!("{project_prefix}/search/{event_component}.json");

        Self {
            event_id,
            original_event_id: event.id.clone(),
            event: event_payload,
            search,
            event_path,
            search_path,
        }
    }
}

impl LearnedMemoryProjection {
    fn from_memory(
        project_prefix: &str,
        memory: &LearnedMemoryRecord,
        evidence_ref_map: &BTreeMap<String, String>,
    ) -> Result<Option<Self>> {
        if !remote_memory_kind_is_safe(&memory.kind) {
            return Ok(None);
        }
        let outcome = scan_text_with_detector(&memory.claim, &DeterministicPrivacyDetector);
        if outcome.blocks_sync || outcome.pii_coverage.status != PiiCoverageStatus::Available {
            return Ok(None);
        }
        let kind_outcome = scan_text_with_detector(&memory.kind, &DeterministicPrivacyDetector);
        if kind_outcome.blocks_sync
            || kind_outcome.pii_coverage.status != PiiCoverageStatus::Available
            || !kind_outcome.findings.is_empty()
        {
            return Ok(None);
        }
        let Some(evidence_refs) = remap_evidence_refs(&memory.evidence_refs, evidence_ref_map)
        else {
            return Ok(None);
        };
        let Some(counterevidence_refs) =
            remap_evidence_refs(&memory.counterevidence_refs, evidence_ref_map)
        else {
            return Ok(None);
        };
        if evidence_refs.is_empty() {
            return Ok(None);
        }
        let remote_kind = memory.kind.trim().to_ascii_lowercase();
        let memory_id = remote_learned_memory_id(
            &remote_kind,
            &outcome.redacted_text,
            memory.confidence,
            &memory.status,
            &evidence_refs,
            &counterevidence_refs,
        )?;
        let content_hash = remote_learned_memory_content_hash(RemoteLearnedMemoryMaterial {
            id: Some(&memory_id),
            kind: &remote_kind,
            claim: &outcome.redacted_text,
            confidence: memory.confidence,
            status: &memory.status,
            evidence_refs: &evidence_refs,
            counterevidence_refs: &counterevidence_refs,
            created_at: Some(&memory.created_at),
        })?;
        let payload = RemoteLearnedMemoryPayload {
            manifest_version: MANIFEST_VERSION,
            id: memory_id.clone(),
            kind: remote_kind,
            claim: outcome.redacted_text,
            confidence: memory.confidence,
            status: memory.status.clone(),
            evidence_refs,
            counterevidence_refs,
            content_hash,
            created_at: memory.created_at.clone(),
        };
        let path = format!(
            "{project_prefix}/learned-memory/{}.json",
            safe_path_component(&memory_id)
        );
        Ok(Some(Self {
            memory_id,
            memory: payload,
            path,
        }))
    }
}

impl RemoteEventPayload {
    fn validate(&self) -> Result<()> {
        if self.manifest_version != MANIFEST_VERSION {
            bail!(
                "remote event manifest version mismatch: expected {MANIFEST_VERSION}, found {}",
                self.manifest_version
            );
        }
        if self.redaction_policy_id != DEFAULT_REDACTION_POLICY_ID {
            bail!(
                "remote event redaction policy mismatch: expected {DEFAULT_REDACTION_POLICY_ID}, found {}",
                self.redaction_policy_id
            );
        }
        let expected_content_hash = content_hash(&self.content_text);
        if self.content_hash != expected_content_hash {
            bail!("remote event content_hash mismatch for {}", self.event_id);
        }
        let expected_event_id = self.as_new_event().event_id();
        if self.event_id != expected_event_id {
            bail!("remote event id mismatch for {}", self.event_id);
        }
        Ok(())
    }

    fn as_new_event(&self) -> NewEvent {
        let mut event = NewEvent::new(
            &self.source,
            &self.source_session_id,
            &self.event_type,
            &self.role,
            &self.timestamp,
            &self.content_text,
            &self.parser_version,
        );
        if let Some(source_event_id) = &self.source_event_id {
            event = event.with_source_event_id(source_event_id);
        }
        if let Some(parent_hash) = &self.parent_hash {
            event = event.with_parent_hash(parent_hash);
        }
        event
    }

    fn to_new_event(&self, remote: &FakeGithubRemote) -> Result<NewEvent> {
        self.validate()?;
        Ok(self.as_new_event().with_raw_local_ref(format!(
            "remote://{}/{}",
            remote.descriptor(),
            safe_path_component(&self.event_id)
        )))
    }
}

impl RemoteLearnedMemoryPayload {
    fn validate_shape(&self) -> Result<()> {
        if self.manifest_version != MANIFEST_VERSION {
            bail!(
                "remote learned memory manifest version mismatch: expected {MANIFEST_VERSION}, found {}",
                self.manifest_version
            );
        }
        if !matches!(
            self.status.as_str(),
            "active" | "pending" | "superseded" | "rejected"
        ) {
            bail!("invalid remote learned memory status: {}", self.status);
        }
        if self.evidence_refs.is_empty() {
            bail!("remote learned memory requires evidence refs");
        }
        for evidence_ref in self
            .evidence_refs
            .iter()
            .chain(self.counterevidence_refs.iter())
        {
            if !evidence_ref.starts_with("mmr://event/") {
                bail!("invalid remote learned memory evidence ref: {evidence_ref}");
            }
        }
        if !remote_memory_kind_is_safe(&self.kind) {
            bail!("remote learned memory kind is unsafe");
        }
        let expected_id = remote_learned_memory_id(
            &self.kind,
            &self.claim,
            self.confidence,
            &self.status,
            &self.evidence_refs,
            &self.counterevidence_refs,
        )?;
        if self.id != expected_id {
            bail!("remote learned memory id mismatch for {}", self.id);
        }
        let expected_hash = remote_learned_memory_content_hash(RemoteLearnedMemoryMaterial {
            id: Some(&self.id),
            kind: &self.kind,
            claim: &self.claim,
            confidence: self.confidence,
            status: &self.status,
            evidence_refs: &self.evidence_refs,
            counterevidence_refs: &self.counterevidence_refs,
            created_at: Some(&self.created_at),
        })?;
        if self.content_hash != expected_hash {
            bail!(
                "remote learned memory content_hash mismatch for {}",
                self.id
            );
        }
        Ok(())
    }

    fn validate(&self, valid_evidence_refs: &BTreeSet<String>) -> Result<()> {
        self.validate_shape()?;
        for evidence_ref in self
            .evidence_refs
            .iter()
            .chain(self.counterevidence_refs.iter())
        {
            if !valid_evidence_refs.contains(evidence_ref) {
                bail!("remote learned memory references missing evidence: {evidence_ref}");
            }
        }
        Ok(())
    }
}

fn new_redaction_span_from_finding(finding: &RedactionFinding) -> NewRedactionSpan {
    NewRedactionSpan {
        kind: finding.kind.clone(),
        start_byte: finding.start_byte,
        end_byte: finding.end_byte,
        replacement: finding.replacement.clone(),
        confidence: finding.confidence,
        blocks_sync: finding.blocks_sync,
    }
}

fn blocking_reasons(findings: &[RedactionFinding]) -> Vec<String> {
    let mut counts = BTreeMap::new();
    for finding in findings.iter().filter(|finding| finding.blocks_sync) {
        *counts.entry(finding.kind.as_str()).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .map(|(kind, count)| {
            format!(
                "{count} deterministic secret finding(s) of kind {kind} under policy {DEFAULT_REDACTION_POLICY_ID}"
            )
        })
        .collect()
}

fn remap_evidence_refs(
    refs: &[String],
    evidence_ref_map: &BTreeMap<String, String>,
) -> Option<Vec<String>> {
    let mut mapped = Vec::with_capacity(refs.len());
    for evidence_ref in refs {
        mapped.push(evidence_ref_map.get(evidence_ref)?.clone());
    }
    mapped.sort();
    mapped.dedup();
    Some(mapped)
}

fn remap_remote_learned_memory_refs(
    refs: &[String],
    evidence_ref_map: &BTreeMap<String, String>,
) -> Result<Vec<String>> {
    let mut mapped = Vec::with_capacity(refs.len());
    for evidence_ref in refs {
        let mapped_ref = evidence_ref_map.get(evidence_ref).ok_or_else(|| {
            anyhow::anyhow!("remote learned memory references missing evidence: {evidence_ref}")
        })?;
        mapped.push(mapped_ref.clone());
    }
    mapped.sort();
    mapped.dedup();
    Ok(mapped)
}

fn remote_memory_kind_is_safe(kind: &str) -> bool {
    let kind = kind.trim().to_ascii_lowercase();
    if kind.is_empty()
        || kind.len() > 64
        || !kind
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return false;
    }
    !["identity", "personal", "secret", "credential", "sensitive"]
        .iter()
        .any(|needle| kind.contains(needle))
}

fn remote_learned_memory_id(
    kind: &str,
    claim: &str,
    confidence: f64,
    status: &str,
    evidence_refs: &[String],
    counterevidence_refs: &[String],
) -> Result<String> {
    let material = remote_learned_memory_material(RemoteLearnedMemoryMaterial {
        id: None,
        kind,
        claim,
        confidence,
        status,
        evidence_refs,
        counterevidence_refs,
        created_at: None,
    })?;
    Ok(format!("learned-memory:v1:{}", content_hash(&material)))
}

fn remote_learned_memory_content_hash(input: RemoteLearnedMemoryMaterial<'_>) -> Result<String> {
    let material = remote_learned_memory_material(input)?;
    Ok(content_hash(&material))
}

struct RemoteLearnedMemoryMaterial<'a> {
    id: Option<&'a str>,
    kind: &'a str,
    claim: &'a str,
    confidence: f64,
    status: &'a str,
    evidence_refs: &'a [String],
    counterevidence_refs: &'a [String],
    created_at: Option<&'a str>,
}

fn remote_learned_memory_material(input: RemoteLearnedMemoryMaterial<'_>) -> Result<String> {
    let mut evidence_refs = input.evidence_refs.to_vec();
    evidence_refs.sort();
    evidence_refs.dedup();
    let mut counterevidence_refs = input.counterevidence_refs.to_vec();
    counterevidence_refs.sort();
    counterevidence_refs.dedup();
    serde_json::to_string(&serde_json::json!({
        "id": input.id,
        "kind": input.kind,
        "claim": input.claim,
        "confidence": input.confidence,
        "status": input.status,
        "evidence_refs": evidence_refs,
        "counterevidence_refs": counterevidence_refs,
        "created_at": input.created_at,
    }))
    .context("serialize remote learned memory material")
}

fn manifest_root_hash(entries: &[RemoteManifestEntry]) -> String {
    let mut material = String::new();
    for entry in entries {
        material.push_str(&entry.kind);
        material.push('\t');
        material.push_str(&entry.ref_id);
        material.push('\t');
        material.push_str(&entry.content_hash);
        material.push('\t');
        material.push_str(&entry.sync_path);
        material.push('\n');
    }
    content_hash(&material)
}

fn default_remote_root(descriptor: &str) -> Result<PathBuf> {
    let db_path = default_db_path()?;
    let base = db_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("default database path has no parent"))?;
    Ok(base.join("remotes").join(safe_path_component(descriptor)))
}

fn github_user() -> String {
    for key in [
        ENV_GITHUB_USER,
        ENV_GITHUB_USER_FALLBACK,
        "USER",
        "USERNAME",
    ] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    "authenticated-user".to_string()
}

fn fake_auth_ok() -> bool {
    std::env::var(ENV_FAKE_REMOTE_AUTH)
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            !matches!(value.as_str(), "0" | "false" | "fail" | "failed")
        })
        .unwrap_or(true)
}

fn safe_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
