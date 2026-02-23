use axum::{routing::get, Router};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::handlers::{
    __path_api_analytics, __path_api_messages, __path_api_projects, __path_api_search,
    __path_api_sessions,
};
use crate::api::handlers::{api_analytics, api_messages, api_projects, api_search, api_sessions};
use crate::api::openapi::ApiDoc;
use crate::api::spa::spa_handler;
use crate::api::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let (api_router, openapi) = OpenApiRouter::<AppState>::with_openapi(ApiDoc::openapi())
        .routes(routes!(api_projects))
        .routes(routes!(api_sessions))
        .routes(routes!(api_messages))
        .routes(routes!(api_search))
        .routes(routes!(api_analytics))
        .split_for_parts();

    let openapi_json = openapi
        .to_pretty_json()
        .unwrap_or_else(|_| "{}".to_string());

    Router::new()
        .merge(api_router)
        .route(
            "/openapi.json",
            get({
                let json = openapi_json.clone();
                move || async move {
                    (
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        json,
                    )
                }
            }),
        )
        .fallback(get(spa_handler))
        .with_state(state)
}
