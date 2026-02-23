use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiProject {
    pub name: String,
    pub source: String,
    pub original_path: String,
    pub session_count: i32,
    pub message_count: i32,
    pub last_activity: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiProjectsResponse {
    pub projects: Vec<ApiProject>,
    pub total_messages: i64,
    pub total_sessions: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiSession {
    pub session_id: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub message_count: i32,
    pub user_messages: i32,
    pub assistant_messages: i32,
    pub preview: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiSessionsResponse {
    pub project_name: String,
    pub project_path: String,
    pub source: String,
    pub sessions: Vec<ApiSession>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiMessage {
    pub role: String,
    pub content: String,
    pub model: String,
    pub timestamp: String,
    pub is_subagent: bool,
    pub msg_type: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiMessagesResponse {
    pub session_id: String,
    pub project_name: String,
    pub project_path: String,
    pub source: String,
    pub messages: Vec<ApiMessage>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiSearchResult {
    pub id: i64,
    pub project: String,
    pub project_path: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub model: String,
    pub timestamp: String,
    pub is_subagent: bool,
    pub source: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiSearchResponse {
    pub query: String,
    pub total_count: usize,
    pub page: usize,
    pub per_page: usize,
    pub results: Vec<ApiSearchResult>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiSourceStats {
    pub source: String,
    pub message_count: i64,
    pub session_count: i64,
    pub project_count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiModelStats {
    pub source: String,
    pub model: String,
    pub message_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiProjectStats {
    pub source: String,
    pub project_path: String,
    pub total_messages: i64,
    pub user_messages: i64,
    pub assistant_messages: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApiAnalyticsResponse {
    pub source_stats: Vec<ApiSourceStats>,
    pub model_stats: Vec<ApiModelStats>,
    pub project_stats: Vec<ApiProjectStats>,
}
