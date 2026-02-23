use axum::response::Html;

const SPA_HTML: &str = include_str!("index.html");

pub async fn spa_handler() -> Html<String> {
    Html(SPA_HTML.to_string())
}
