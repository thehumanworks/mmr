use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::{NewEvent, Store, content_hash};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventBoundary {
    SessionStart,
    UserTurn,
    AssistantTurn,
    ToolCall,
    ToolResult,
    Compaction,
    SessionEnd,
    UnknownRawEvent,
}

impl EventBoundary {
    pub fn as_event_type(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::UserTurn => "user_turn",
            Self::AssistantTurn => "assistant_turn",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Compaction => "compaction",
            Self::SessionEnd => "session_end",
            Self::UnknownRawEvent => "unknown_raw_event",
        }
    }

    pub fn default_role(self) -> &'static str {
        match self {
            Self::SessionStart | Self::SessionEnd | Self::Compaction | Self::UnknownRawEvent => {
                "system"
            }
            Self::UserTurn => "user",
            Self::AssistantTurn => "assistant",
            Self::ToolCall => "assistant",
            Self::ToolResult => "tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDiscoveryRoot {
    pub project_path: PathBuf,
    pub source_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSessionRef {
    pub source: String,
    pub session_id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEvent {
    pub source: String,
    pub source_session_id: String,
    pub source_event_id: Option<String>,
    pub boundary: EventBoundary,
    pub role: Option<String>,
    pub timestamp: String,
    pub content_text: String,
    pub parser_version: String,
    pub raw_local_ref: String,
    pub parent_hash: Option<String>,
}

impl NormalizedEvent {
    pub fn into_store_event(self) -> NewEvent {
        let mut event = NewEvent::new(
            self.source,
            self.source_session_id,
            self.boundary.as_event_type(),
            self.role
                .unwrap_or_else(|| self.boundary.default_role().to_string()),
            self.timestamp,
            self.content_text,
            self.parser_version,
        )
        .with_raw_local_ref(self.raw_local_ref);

        if let Some(source_event_id) = self.source_event_id {
            event = event.with_source_event_id(source_event_id);
        }
        if let Some(parent_hash) = self.parent_hash {
            event = event.with_parent_hash(parent_hash);
        }
        event
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceWarning {
    pub raw_local_ref: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCursorUpdate {
    pub cursor_key: String,
    pub cursor_value: String,
    pub parser_version: String,
    pub last_event_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceImportBatch {
    pub source: String,
    pub parser_version: String,
    pub events: Vec<NormalizedEvent>,
    pub cursor_updates: Vec<SourceCursorUpdate>,
    pub warnings: Vec<SourceWarning>,
}

pub trait SourceAdapter {
    fn source_name(&self) -> &'static str;
    fn parser_version(&self) -> &'static str;
    fn discover(&self, root: &SourceDiscoveryRoot) -> Result<Vec<SourceSessionRef>>;
    fn import_session(&self, session: &SourceSessionRef) -> Result<SourceImportBatch>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchState {
    pub path: PathBuf,
    pub offset: u64,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchDelta {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub new_offset: u64,
    pub new_fingerprint: String,
    pub rotated: bool,
    pub partial_tail: bool,
}

pub struct FileWatcher;

impl FileWatcher {
    pub fn read_delta(state: &WatchState) -> Result<WatchDelta> {
        let bytes = fs::read(&state.path)
            .with_context(|| format!("read watched file {}", state.path.display()))?;
        let current_len = bytes.len() as u64;
        let compare_len = state.offset.min(current_len) as usize;
        let current_state_fingerprint = file_fingerprint(&bytes[..compare_len]);
        let fingerprint_changed = state
            .fingerprint
            .as_deref()
            .map(|fingerprint| fingerprint != current_state_fingerprint)
            .unwrap_or(false);
        let rotated = current_len < state.offset || fingerprint_changed;
        let start = if rotated { 0 } else { state.offset as usize };
        let mut delta = bytes.get(start..).unwrap_or_default().to_vec();
        let partial_tail = !delta.is_empty() && !delta.ends_with(b"\n");
        if partial_tail {
            if let Some(pos) = delta.iter().rposition(|byte| *byte == b'\n') {
                delta.truncate(pos + 1);
            } else {
                delta.clear();
            }
        }
        let emitted_end = start as u64 + delta.len() as u64;

        Ok(WatchDelta {
            path: state.path.clone(),
            bytes: delta,
            new_offset: if partial_tail {
                emitted_end
            } else {
                current_len
            },
            new_fingerprint: file_fingerprint(&bytes[..emitted_end as usize]),
            rotated,
            partial_tail,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileReport {
    pub source: String,
    pub discovered_sessions: usize,
    pub imported_events: usize,
    pub warnings: Vec<String>,
    pub event_ids: Vec<String>,
}

pub struct Reconciler<'a, A: SourceAdapter> {
    adapter: &'a A,
}

impl<'a, A: SourceAdapter> Reconciler<'a, A> {
    pub fn new(adapter: &'a A) -> Self {
        Self { adapter }
    }

    pub fn reconcile(
        &self,
        store: &mut Store,
        project_id: &str,
        root: &SourceDiscoveryRoot,
    ) -> Result<ReconcileReport> {
        let sessions = self.adapter.discover(root)?;
        let mut imported_events = 0usize;
        let mut warnings = Vec::new();
        let mut event_ids = Vec::new();

        for session in &sessions {
            let batch = self.adapter.import_session(session)?;
            store.upsert_source(&batch.source, &batch.parser_version)?;
            for warning in batch.warnings {
                warnings.push(match warning.raw_local_ref {
                    Some(raw_ref) => format!("{raw_ref}: {}", warning.message),
                    None => warning.message,
                });
            }

            for event in batch.events {
                let store_event = event.into_store_event();
                let event_id = store_event.event_id();
                let already_exists = store.event_exists(&event_id)?;
                let inserted = store.insert_event(project_id, &store_event)?;
                event_ids.push(inserted.id);
                if !already_exists {
                    imported_events += 1;
                }
            }

            for cursor in batch.cursor_updates {
                store.set_source_cursor(
                    project_id,
                    &batch.source,
                    &cursor.cursor_key,
                    &cursor.cursor_value,
                    &cursor.parser_version,
                    cursor.last_event_hash.as_deref(),
                )?;
            }
        }

        event_ids.sort();
        event_ids.dedup();

        Ok(ReconcileReport {
            source: self.adapter.source_name().to_string(),
            discovered_sessions: sessions.len(),
            imported_events,
            warnings,
            event_ids,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FixtureAdapter {
    source_name: &'static str,
    parser_version: &'static str,
}

impl FixtureAdapter {
    pub fn new() -> Self {
        Self {
            source_name: "fixture",
            parser_version: "fixture-jsonl-v1",
        }
    }
}

impl Default for FixtureAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceAdapter for FixtureAdapter {
    fn source_name(&self) -> &'static str {
        self.source_name
    }

    fn parser_version(&self) -> &'static str {
        self.parser_version
    }

    fn discover(&self, root: &SourceDiscoveryRoot) -> Result<Vec<SourceSessionRef>> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&root.source_root)
            .with_context(|| format!("read source root {}", root.source_root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let session_id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| anyhow!("fixture session file has no valid stem"))?
                .to_string();
            sessions.push(SourceSessionRef {
                source: self.source_name.to_string(),
                session_id,
                path,
            });
        }
        sessions.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(sessions)
    }

    fn import_session(&self, session: &SourceSessionRef) -> Result<SourceImportBatch> {
        let content = fs::read_to_string(&session.path)
            .with_context(|| format!("read fixture session {}", session.path.display()))?;
        parse_fixture_jsonl(
            self.source_name,
            self.parser_version,
            &session.session_id,
            &session.path,
            &content,
        )
    }
}

pub fn parse_fixture_jsonl(
    source: &str,
    parser_version: &str,
    fallback_session_id: &str,
    path: &Path,
    content: &str,
) -> Result<SourceImportBatch> {
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    let mut last_hash = None;
    let mut current_session_id = fallback_session_id.to_string();

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let raw_local_ref = format!("{}:{}", path.display(), line_index + 1);
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(err) => {
                warnings.push(SourceWarning {
                    raw_local_ref: Some(raw_local_ref),
                    message: format!("skipped malformed JSONL row: {err}"),
                });
                continue;
            }
        };

        if let Some(discovered_session_id) = string_field(&value, "session_id")
            .or_else(|| string_field(&value, "sessionId"))
            .or_else(|| {
                value
                    .get("payload")
                    .and_then(|payload| string_field(payload, "id"))
            })
        {
            current_session_id = discovered_session_id;
        }
        let session_id = current_session_id.clone();
        let timestamp =
            string_field(&value, "timestamp").unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
        let boundary = boundary_from_value(&value);
        let role = string_field(&value, "role")
            .or_else(|| {
                value
                    .get("message")
                    .and_then(|msg| string_field(msg, "role"))
            })
            .or_else(|| Some(boundary.default_role().to_string()));
        let content_text = content_from_value(&value).unwrap_or_else(|| value.to_string());
        let source_event_id = string_field(&value, "uuid")
            .or_else(|| string_field(&value, "id"))
            .or_else(|| Some(format!("row:{}", content_hash(line))));
        let parent_hash = last_hash.clone();

        let normalized = NormalizedEvent {
            source: source.to_string(),
            source_session_id: session_id,
            source_event_id,
            boundary,
            role,
            timestamp,
            content_text,
            parser_version: parser_version.to_string(),
            raw_local_ref: raw_local_ref.clone(),
            parent_hash,
        };
        last_hash = Some(content_hash(&normalized.content_text));
        events.push(normalized);
    }

    Ok(SourceImportBatch {
        source: source.to_string(),
        parser_version: parser_version.to_string(),
        cursor_updates: vec![SourceCursorUpdate {
            cursor_key: path.display().to_string(),
            cursor_value: format!("offset:{}", content.len()),
            parser_version: parser_version.to_string(),
            last_event_hash: last_hash,
        }],
        events,
        warnings,
    })
}

fn boundary_from_value(value: &Value) -> EventBoundary {
    match string_field(value, "event_type")
        .or_else(|| string_field(value, "type"))
        .as_deref()
    {
        Some("session_meta" | "session_start" | "session") => EventBoundary::SessionStart,
        Some("session_end") => EventBoundary::SessionEnd,
        Some("compaction") => EventBoundary::Compaction,
        Some("user") => EventBoundary::UserTurn,
        Some("assistant") => EventBoundary::AssistantTurn,
        Some("tool_output" | "tool_result") => EventBoundary::ToolResult,
        Some("tool_call") => EventBoundary::ToolCall,
        Some("note") => EventBoundary::UserTurn,
        Some("event_msg") => EventBoundary::UserTurn,
        Some("response_item") => EventBoundary::AssistantTurn,
        Some("message") => match string_field(value, "role").as_deref() {
            Some("assistant") => EventBoundary::AssistantTurn,
            Some("tool" | "toolResult") => EventBoundary::ToolResult,
            _ => EventBoundary::UserTurn,
        },
        _ => EventBoundary::UnknownRawEvent,
    }
}

fn content_from_value(value: &Value) -> Option<String> {
    string_field(value, "content")
        .or_else(|| text_from_field(value.get("content")))
        .or_else(|| text_from_field(value.get("message")))
        .or_else(|| {
            value
                .get("payload")
                .and_then(|payload| string_field(payload, "message"))
        })
        .or_else(|| {
            value
                .get("payload")
                .and_then(|payload| text_from_field(payload.get("content")))
        })
        .or_else(|| {
            value
                .get("message")
                .and_then(|message| string_field(message, "content"))
        })
        .or_else(|| {
            value
                .get("message")
                .and_then(|message| text_from_field(message.get("content")))
        })
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn text_from_field(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| {
                    item.as_str().map(ToString::to_string).or_else(|| {
                        item.get("text")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() { None } else { Some(text) }
        }
        Value::Object(_) => value
            .and_then(|object| object.get("text"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        _ => None,
    }
}

fn file_fingerprint(bytes: &[u8]) -> String {
    let prefix_len = bytes.len().min(4096);
    content_hash(String::from_utf8_lossy(&bytes[..prefix_len]).as_ref())
}

pub fn event_hash_set(events: &[NormalizedEvent]) -> HashSet<String> {
    events
        .iter()
        .cloned()
        .map(NormalizedEvent::into_store_event)
        .map(|event| event.event_id())
        .collect()
}

pub fn event_hash_multiset(events: &[NormalizedEvent]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for event in events
        .iter()
        .cloned()
        .map(NormalizedEvent::into_store_event)
    {
        *counts.entry(event.event_id()).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(path, contents).expect("write");
    }

    #[test]
    fn fixture_adapter_normalizes_events_and_warnings() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let session = tmp.path().join("mixed.jsonl");
        write(
            &session,
            r#"{"type":"session_meta","timestamp":"2026-05-24T09:00:00Z","payload":{"id":"sess-1"}}
not-json
{"type":"event_msg","timestamp":"2026-05-24T09:00:01Z","payload":{"message":"hello"}}
{"type":"response_item","timestamp":"2026-05-24T09:00:02Z","role":"assistant","content":"hi"}"#,
        );

        let adapter = FixtureAdapter::new();
        let session_ref = SourceSessionRef {
            source: "fixture".to_string(),
            session_id: "mixed".to_string(),
            path: session.clone(),
        };
        let batch = adapter.import_session(&session_ref).expect("batch");

        assert_eq!(batch.events.len(), 3);
        assert_eq!(batch.warnings.len(), 1);
        assert_eq!(batch.events[0].boundary, EventBoundary::SessionStart);
        assert_eq!(batch.events[1].boundary, EventBoundary::UserTurn);
        assert_eq!(batch.events[2].boundary, EventBoundary::AssistantTurn);
        assert!(batch.events.iter().all(|event| {
            event.parser_version == "fixture-jsonl-v1"
                && event.raw_local_ref.contains("mixed.jsonl")
        }));
    }

    #[test]
    fn watcher_skips_partial_tail_until_complete_newline() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("active.jsonl");
        write(&path, "{\"ok\":1}\n{\"partial\":");

        let delta = FileWatcher::read_delta(&WatchState {
            path: path.clone(),
            offset: 0,
            fingerprint: None,
        })
        .expect("delta");

        assert_eq!(
            String::from_utf8(delta.bytes).expect("utf8"),
            "{\"ok\":1}\n"
        );
        assert!(delta.partial_tail);
        assert_eq!(delta.new_offset, 9);

        fs::write(&path, "{\"ok\":1}\n{\"partial\":true}\n").expect("complete");
        let complete = FileWatcher::read_delta(&WatchState {
            path,
            offset: delta.new_offset,
            fingerprint: Some(delta.new_fingerprint),
        })
        .expect("complete delta");
        assert_eq!(
            String::from_utf8(complete.bytes).expect("utf8"),
            "{\"partial\":true}\n"
        );
        assert_eq!(complete.new_offset, 26);
        assert!(!complete.partial_tail);
    }

    #[test]
    fn watcher_detects_truncation_rotation() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("active.jsonl");
        write(&path, "{\"after\":\"rotation\"}\n");

        let delta = FileWatcher::read_delta(&WatchState {
            path: path.clone(),
            offset: 200,
            fingerprint: None,
        })
        .expect("delta");

        assert!(delta.rotated);
        assert_eq!(
            String::from_utf8(delta.bytes).expect("utf8"),
            "{\"after\":\"rotation\"}\n"
        );
    }

    #[test]
    fn watcher_detects_same_size_rotation_with_fingerprint() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("active.jsonl");
        write(&path, "{\"old\":true}\n");
        let initial = FileWatcher::read_delta(&WatchState {
            path: path.clone(),
            offset: 0,
            fingerprint: None,
        })
        .expect("initial");

        write(&path, "{\"new\":true}\n");
        let rotated = FileWatcher::read_delta(&WatchState {
            path,
            offset: initial.new_offset,
            fingerprint: Some(initial.new_fingerprint),
        })
        .expect("rotated");

        assert!(rotated.rotated);
        assert_eq!(
            String::from_utf8(rotated.bytes).expect("utf8"),
            "{\"new\":true}\n"
        );
    }

    #[test]
    fn reconciler_replays_fixture_idempotently() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let project_dir = tmp.path().join("project");
        let source_root = tmp.path().join("fixtures");
        fs::create_dir_all(&project_dir).expect("project");
        fs::create_dir_all(&source_root).expect("source");
        write(
            &source_root.join("sess.jsonl"),
            r#"{"type":"event_msg","timestamp":"2026-05-24T09:00:01Z","payload":{"message":"hello"}}
{"type":"response_item","timestamp":"2026-05-24T09:00:02Z","role":"assistant","content":"hi"}"#,
        );

        let mut store = Store::open_in_memory().expect("store");
        let project = store.ensure_project_link(&project_dir).expect("project");
        let root = SourceDiscoveryRoot {
            project_path: project_dir,
            source_root,
        };
        let adapter = FixtureAdapter::new();
        let reconciler = Reconciler::new(&adapter);

        let first = reconciler
            .reconcile(&mut store, &project.id, &root)
            .expect("first");
        let second = reconciler
            .reconcile(&mut store, &project.id, &root)
            .expect("second");

        assert_eq!(first.event_ids, second.event_ids);
        assert_eq!(second.imported_events, 0);
        assert_eq!(store.event_count().expect("count"), 2);
    }
}
