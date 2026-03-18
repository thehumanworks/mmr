use std::collections::HashSet;

use super::domain::SourceKind;

#[derive(Debug, Clone)]
pub(crate) struct ProjectAggregate {
    pub name: String,
    pub source: SourceKind,
    pub original_path: String,
    pub session_count: i32,
    pub message_count: i32,
    pub last_activity: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionAggregate {
    pub project_name: String,
    pub project_path: String,
    pub source: SourceKind,
    pub session_id: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub message_count: i32,
    pub user_messages: i32,
    pub assistant_messages: i32,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PreviewCandidate {
    pub timestamp: String,
    pub source_file: String,
    pub line_index: usize,
    pub content: String,
}

#[derive(Debug)]
pub(crate) struct ProjectAggregateState {
    pub name: String,
    pub source: SourceKind,
    pub original_path: String,
    pub session_ids: HashSet<String>,
    pub message_count: i32,
    pub last_activity: String,
}

#[derive(Debug)]
pub(crate) struct SessionAggregateState {
    pub project_name: String,
    pub project_path: String,
    pub source: SourceKind,
    pub session_id: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub message_count: i32,
    pub user_messages: i32,
    pub assistant_messages: i32,
    pub preview: Option<PreviewCandidate>,
}

/// Resolved project may match multiple names across sources (e.g. Codex uses paths,
/// Claude uses directory-encoded names for the same underlying project).
#[derive(Debug, Clone)]
pub(crate) struct ResolvedProject {
    pub names: Vec<String>,
}
