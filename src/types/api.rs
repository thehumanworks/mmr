use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiProject {
    pub name: String,
    pub source: String,
    pub original_path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub session_count: i32,
    pub message_count: i32,
    pub last_activity: String,
}

#[derive(Debug, Serialize)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiSession {
    pub session_id: String,
    pub source: String,
    pub project_name: String,
    pub project_path: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub message_count: i32,
    pub user_messages: i32,
    pub assistant_messages: i32,
    pub preview: String,
}

#[derive(Debug, Serialize)]
pub struct ApiSessionsResponse {
    pub sessions: Vec<ApiSession>,
    pub total_sessions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub session_id: String,
    pub source: String,
    pub project_name: String,
    pub role: String,
    pub content: String,
    pub model: String,
    pub timestamp: String,
    pub is_subagent: bool,
    pub msg_type: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ApiMessageOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessageOrigin {
    pub host: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_mmr_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
    pub next_page: bool,
    pub next_offset: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
    /// Present only when a reverse session-axis selector (`--session-back`/`--session-range`)
    /// is used; omitted entirely otherwise so default `messages` output stays byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_selection: Option<SessionSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_results: Option<Vec<ApiPeerResult>>,
}

/// Self-describing record of which prior session(s) a reverse-axis query resolved to,
/// so a caller can read the answer without re-deriving the recency ranking.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSelection {
    pub scope: SessionSelectionScope,
    pub axis: String,
    pub total_sessions_in_scope: i64,
    pub selected: Vec<SelectedSession>,
    /// The newest (assumed-live) session that was held back when `--include-newest` is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_newest: Option<SkippedNewest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSelectionScope {
    pub project: Option<String>,
    pub all: bool,
    pub source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SelectedSession {
    pub age: u32,
    pub session_id: String,
    pub source: String,
    pub project_name: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub message_count: i32,
    pub equivalent_command: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkippedNewest {
    pub age: u32,
    pub session_id: String,
    pub last_timestamp: String,
    pub assumed_live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiPeerResult {
    pub host: String,
    pub transport: String,
    pub command: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_mmr_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_messages: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_sessions: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct RememberResponse {
    pub backend: String,
    pub model: String,
    pub text: String,
}

impl RememberResponse {
    pub fn new(
        backend: impl Into<String>,
        model: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            backend: backend.into(),
            model: model.into(),
            text: text.into(),
        }
    }
}
