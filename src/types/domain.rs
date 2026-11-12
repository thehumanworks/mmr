use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SourceFilter {
    Claude,
    Codex,
    Cursor,
    Grok,
    Pi,
}

impl SourceFilter {
    /// The lowercase name used in flags, JSON and the store.
    pub fn as_str(self) -> &'static str {
        self.kind().as_str()
    }

    pub fn kind(self) -> SourceKind {
        match self {
            Self::Claude => SourceKind::Claude,
            Self::Codex => SourceKind::Codex,
            Self::Cursor => SourceKind::Cursor,
            Self::Grok => SourceKind::Grok,
            Self::Pi => SourceKind::Pi,
        }
    }

    /// Parse a lowercase source name such as `codex`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "cursor" => Some(Self::Cursor),
            "grok" => Some(Self::Grok),
            "pi" => Some(Self::Pi),
            _ => None,
        }
    }
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
    Grok,
    Pi,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::Pi => "pi",
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
