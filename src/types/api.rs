use serde::Serialize;

use super::domain::Agent;

#[derive(Debug, Serialize)]
pub struct ApiProject {
    pub name: String,
    pub source: String,
    pub original_path: String,
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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
}

#[derive(Debug, Serialize)]
pub struct ApiMessagesResponse {
    pub messages: Vec<ApiMessage>,
    pub total_messages: i64,
}

#[derive(Debug, Serialize)]
pub struct ApiMergeSession {
    pub from_session_id: String,
    pub to_session_id: String,
    pub from_source: String,
    pub to_source: String,
    pub from_project_name: String,
    pub to_project_name: String,
    pub created_target_session: bool,
    pub merged_messages: i32,
    pub timestamp_strategy: String,
    pub model_strategy: String,
    pub action: String,
    pub source_files: Vec<String>,
    pub target_file: String,
}

#[derive(Debug, Serialize)]
pub struct ApiMergeResponse {
    pub mode: String,
    pub from_agent: String,
    pub to_agent: String,
    pub dry_run: bool,
    pub zip_output: Option<String>,
    pub resolved_history_files: Vec<String>,
    pub session_merges: Vec<ApiMergeSession>,
    pub total_sessions_merged: i64,
    pub total_messages_merged: i64,
    pub schema_considerations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RememberResponse {
    pub agent: Agent,
    pub text: String,
}

impl RememberResponse {
    pub fn new(agent: Agent, text: impl Into<String>) -> Self {
        Self {
            agent,
            text: text.into(),
        }
    }
}
