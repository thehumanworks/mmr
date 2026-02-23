use axum::{
    extract::{Query, State},
    Json,
};

use crate::api::dto::{
    ApiAnalyticsResponse, ApiMessagesResponse, ApiProjectsResponse, ApiSearchResponse,
    ApiSessionsResponse,
};
use crate::api::params::{MessageQuery, ProjectQuery, SearchParams};
use crate::api::state::AppState;
use crate::query::QueryService;

#[utoipa::path(
    get,
    path = "/api/projects",
    responses((status = 200, body = ApiProjectsResponse)),
    tag = "projects"
)]
pub async fn api_projects(State(db): State<AppState>) -> Json<ApiProjectsResponse> {
    let conn = db.lock().unwrap();
    let service = QueryService::new(&conn);
    Json(service.projects(None, None, 0).unwrap())
}

#[utoipa::path(
    get,
    path = "/api/sessions",
    params(ProjectQuery),
    responses((status = 200, body = ApiSessionsResponse)),
    tag = "sessions"
)]
pub async fn api_sessions(
    State(db): State<AppState>,
    Query(params): Query<ProjectQuery>,
) -> Json<ApiSessionsResponse> {
    let conn = db.lock().unwrap();
    let service = QueryService::new(&conn);
    let project_name = params.name.unwrap_or_default();
    Json(
        service
            .sessions(&project_name, params.source.as_deref(), None, 0)
            .unwrap(),
    )
}

#[utoipa::path(
    get,
    path = "/api/messages",
    params(MessageQuery),
    responses((status = 200, body = ApiMessagesResponse)),
    tag = "messages"
)]
pub async fn api_messages(
    State(db): State<AppState>,
    Query(params): Query<MessageQuery>,
) -> Json<ApiMessagesResponse> {
    let conn = db.lock().unwrap();
    let service = QueryService::new(&conn);
    let session_id = params.session.unwrap_or_default();
    let mut response = service.messages(&session_id, None, 0).unwrap();
    // Preserve legacy API behavior (newest-first) while CLI keeps chronological output.
    response.messages.reverse();
    Json(response)
}

#[utoipa::path(
    get,
    path = "/api/search",
    params(SearchParams),
    responses((status = 200, body = ApiSearchResponse)),
    tag = "search"
)]
pub async fn api_search(
    State(db): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<ApiSearchResponse> {
    let conn = db.lock().unwrap();
    let service = QueryService::new(&conn);
    let query = params.q.unwrap_or_default();
    let page = params.page.unwrap_or(0);
    Json(
        service
            .search(
                &query,
                params.project.as_deref(),
                params.source.as_deref(),
                page,
                50,
            )
            .unwrap(),
    )
}

#[utoipa::path(
    get,
    path = "/api/analytics",
    responses((status = 200, body = ApiAnalyticsResponse)),
    tag = "analytics"
)]
pub async fn api_analytics(State(db): State<AppState>) -> Json<ApiAnalyticsResponse> {
    let conn = db.lock().unwrap();
    let service = QueryService::new(&conn);
    Json(service.stats(None).unwrap())
}
