use serde::Serialize;

use super::api::ApiMessage;
use super::domain::{SourceFilter, TargetAgent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RememberSelection {
    Latest,
    All,
    Session { session_id: String },
}

pub struct RememberRequest<'a> {
    pub agent: super::domain::Agent,
    pub project: &'a str,
    pub selection: RememberSelection,
    pub source: Option<SourceFilter>,
    pub instructions: Option<&'a str>,
    pub model: Option<&'a str>,
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

// Prompt command types
pub struct PromptRequest<'a> {
    pub agent: super::domain::Agent,
    pub target: TargetAgent,
    pub query: &'a str,
    pub project: &'a str,
    pub source: Option<SourceFilter>,
    pub model: Option<&'a str>,
}

#[derive(Debug, Serialize)]
pub struct PromptResponse {
    pub target: TargetAgent,
    pub prompt: String,
}

// Gemini API types
#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InteractionInputType {
    #[default]
    Text,
    Image,
}

impl std::fmt::Display for InteractionInputType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Image => write!(f, "image"),
        }
    }
}

#[derive(Debug, Serialize, Default)]
pub struct InteractionInput {
    #[serde(rename = "type")]
    pub type_: InteractionInputType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl InteractionInput {
    pub fn new(interaction_type: InteractionInputType, text: &str) -> Self {
        Self {
            type_: interaction_type,
            text: Some(text.into()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct InteractionCreateRequest<'a> {
    pub model: &'a str,
    pub input: Vec<InteractionInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<&'a str>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct InteractionCreateResponse {
    #[serde(default)]
    pub outputs: Vec<InteractionOutput>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct InteractionOutput {
    #[serde(default)]
    pub text: Option<String>,
}

pub struct GeminiGenerateRequest<'a> {
    pub input: Vec<InteractionInput>,
    pub system_instruction: Option<&'a str>,
}

pub struct GeminiGenerateResponse {
    pub text: String,
}

// Codex API types
pub struct CodexGenerateRequest<'a> {
    pub input: &'a str,
    pub developer_instructions: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CodexGenerateResponse {
    text: String,
}

impl CodexGenerateResponse {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn get_text(&self) -> &str {
        &self.text
    }
}
