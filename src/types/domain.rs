use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SourceFilter {
    Claude,
    Codex,
    Cursor,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SortBy {
    #[default]
    Timestamp,
    #[value(name = "message-count")]
    MessageCount,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortOptions {
    pub by: SortBy,
    pub order: SortOrder,
}

impl SortOptions {
    pub const fn new(by: SortBy, order: SortOrder) -> Self {
        Self { by, order }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SourceKind {
    Claude,
    Codex,
    Cursor,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub source: SourceKind,
    pub project_name: String,
    pub project_path: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub model: String,
    pub timestamp: String,
    pub is_subagent: bool,
    pub msg_type: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub source_file: String,
    pub line_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum Agent {
    #[clap(name = "gemini")]
    Gemini,
    #[clap(name = "codex")]
    Codex,
    #[clap(name = "cursor")]
    Cursor,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum TargetAgent {
    Claude,
    Codex,
    Cursor,
}
