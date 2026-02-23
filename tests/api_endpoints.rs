use axum::body::Body;
use axum::http::{Request, StatusCode};
use duckdb::{params, Connection};
use http_body_util::BodyExt;
use mmr::api::{build_router, AppState};
use mmr::db::{create_fts_index, init_db};
use mmr::query::QueryService;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (name, source, original_path, session_count, message_count, last_activity) VALUES (?, 'claude', ?, 1, 2, '2025-01-01T00:01:00')",
        params!["-Users-test-proj", "/Users/test/proj"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (name, source, original_path, session_count, message_count, last_activity) VALUES (?, 'codex', ?, 1, 2, '2025-01-02T00:01:00')",
        params!["/Users/test/codex-proj", "/Users/test/codex-proj"],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sessions (session_id, project, source, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages) VALUES ('sess-claude-1', '-Users-test-proj', 'claude', '2025-01-01T00:00:00', '2025-01-01T00:01:00', 2, 1, 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sessions (session_id, project, source, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages) VALUES ('sess-codex-1', '/Users/test/codex-proj', 'codex', '2025-01-02T00:00:00', '2025-01-02T00:01:00', 2, 1, 1)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (1, 'claude', '-Users-test-proj', '/Users/test/proj', 'sess-claude-1', FALSE, 'u1', '', 'user', 'user', 'hello world from claude', '', '2025-01-01T00:00:00', '', '', '', '', 0, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (2, 'claude', '-Users-test-proj', '/Users/test/proj', 'sess-claude-1', FALSE, 'a1', 'u1', 'assistant', 'assistant', 'hi there from assistant', 'claude-3-opus', '2025-01-01T00:01:00', '', '', '', '', 100, 50)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (3, 'codex', '/Users/test/codex-proj', '/Users/test/codex-proj', 'sess-codex-1', FALSE, '', '', 'user', 'user', 'hello world from codex', 'gpt-4', '2025-01-02T00:00:00', '', '', '', '', 0, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO messages (id, source, project, project_path, session_id, is_subagent, message_uuid, parent_uuid, msg_type, role, content_text, model, timestamp, cwd, git_branch, slug, version, input_tokens, output_tokens) VALUES (4, 'codex', '/Users/test/codex-proj', '/Users/test/codex-proj', 'sess-codex-1', FALSE, '', '', 'assistant', 'assistant', 'hi there from codex assistant', 'gpt-4', '2025-01-02T00:01:00', '', '', '', '', 200, 100)",
        [],
    )
    .unwrap();

    create_fts_index(&conn).unwrap();
    conn
}

fn build_test_app(conn: Connection) -> axum::Router {
    let state: AppState = Arc::new(Mutex::new(conn));
    build_router(state)
}

async fn get_json(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

#[tokio::test]
async fn test_projects_returns_all() {
    let app = build_test_app(setup_test_db());
    let (status, json) = get_json(app, "/api/projects").await;
    assert_eq!(status, StatusCode::OK);

    let projects = json["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 4);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 2);
}

#[tokio::test]
async fn test_sessions_for_project() {
    let app = build_test_app(setup_test_db());
    let (status, json) = get_json(app, "/api/sessions?name=-Users-test-proj&source=claude").await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(json["project_name"].as_str().unwrap(), "-Users-test-proj");
    assert_eq!(json["source"].as_str().unwrap(), "claude");
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"].as_str().unwrap(), "sess-claude-1");
}

#[tokio::test]
async fn test_messages_by_session_id() {
    let app = build_test_app(setup_test_db());
    let (status, json) = get_json(app, "/api/messages?session=sess-claude-1").await;
    assert_eq!(status, StatusCode::OK);

    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"].as_str().unwrap(), "assistant");
    assert_eq!(messages[1]["role"].as_str().unwrap(), "user");
}

#[tokio::test]
async fn test_search_and_analytics() {
    let app = build_test_app(setup_test_db());
    let (search_status, search_json) = get_json(app.clone(), "/api/search?q=hello").await;
    assert_eq!(search_status, StatusCode::OK);
    assert_eq!(search_json["query"].as_str().unwrap(), "hello");
    assert!(search_json["results"].is_array());
    assert!(search_json["total_count"].is_number());

    let (analytics_status, analytics_json) = get_json(app, "/api/analytics").await;
    assert_eq!(analytics_status, StatusCode::OK);
    assert!(!analytics_json["source_stats"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_openapi_spec() {
    let app = build_test_app(setup_test_db());
    let (status, json) = get_json(app, "/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["openapi"].as_str().unwrap(), "3.1.0");
    assert!(json["paths"]["/api/projects"].is_object());
}

#[test]
fn test_query_service_sessions_normalizes_codex_project_without_leading_slash() {
    let conn = setup_test_db();
    let service = QueryService::new(&conn);
    let out = service
        .sessions("Users/test/codex-proj", Some("codex"), None, 0)
        .unwrap();
    assert_eq!(out.project_name, "/Users/test/codex-proj");
    assert_eq!(out.project_path, "/Users/test/codex-proj");
    assert_eq!(out.sessions.len(), 1);
}

#[test]
fn test_query_service_messages_are_chronological() {
    let conn = setup_test_db();
    let service = QueryService::new(&conn);
    let out = service.messages("sess-claude-1", None, 0).unwrap();
    assert_eq!(out.messages.len(), 2);
    assert_eq!(out.messages[0].role, "user");
    assert_eq!(out.messages[1].role, "assistant");
}
