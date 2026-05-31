use super::api::ApiMessage;
use super::domain::SourceFilter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RememberSelection {
    Latest,
    All,
    Session { session_id: String },
}

pub struct RememberRequest<'a> {
    pub project: &'a str,
    pub selection: RememberSelection,
    pub source: Option<SourceFilter>,
    pub instructions: Option<&'a str>,
    pub model: &'a str,
}

#[derive(Debug)]
pub(crate) struct SessionTranscript {
    pub session_id: String,
    pub messages: Vec<ApiMessage>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionSelection {
    pub session_id: String,
    pub project_name: String,
    pub source: SourceFilter,
}
