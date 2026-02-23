use serde::Deserialize;

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ProjectQuery {
    pub name: Option<String>,
    pub source: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct MessageQuery {
    pub session: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct SearchParams {
    pub q: Option<String>,
    pub project: Option<String>,
    pub source: Option<String>,
    pub page: Option<usize>,
}
