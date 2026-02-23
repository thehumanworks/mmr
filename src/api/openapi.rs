use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "AI Chat History API",
        version = "0.1.0",
        description = "API for browsing Claude Code and OpenAI Codex conversation history"
    ),
    paths(
        crate::api::handlers::api_projects,
        crate::api::handlers::api_sessions,
        crate::api::handlers::api_messages,
        crate::api::handlers::api_search,
        crate::api::handlers::api_analytics
    ),
    components(
        schemas(
            crate::api::dto::ApiProjectsResponse,
            crate::api::dto::ApiSessionsResponse,
            crate::api::dto::ApiMessagesResponse,
            crate::api::dto::ApiSearchResponse,
            crate::api::dto::ApiAnalyticsResponse
        )
    ),
    tags(
        (name = "projects", description = "Project listing and filtering"),
        (name = "sessions", description = "Session listing within projects"),
        (name = "messages", description = "Message retrieval within sessions"),
        (name = "search", description = "Full-text search across conversations"),
        (name = "analytics", description = "Usage analytics and statistics")
    )
)]
pub struct ApiDoc;
