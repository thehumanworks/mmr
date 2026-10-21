use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeSource {
    Codex,
    Claude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputFormat {
    Codex,
    Claude,
    Canonical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetFormat {
    Canonical,
    CodexReadable,
    ClaudeReadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeArchive {
    pub sha256: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRecordRef {
    pub start: usize,
    pub end: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentRef {
    pub content_hash: Option<String>,
    pub media_type: Option<String>,
    pub locator: Option<String>,
    /// `reference_only` does not claim the referenced bytes are available.
    pub availability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRelationship {
    pub status: String,
    pub matches: Vec<String>,
    pub call_count: usize,
    pub result_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalBlock {
    pub kind: String,
    /// Preserve the source block, including opaque fields and nested tool results.
    pub source: Value,
    pub text: Option<String>,
    pub call_id: Option<String>,
    pub name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_value"
    )]
    pub arguments: Option<Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_value"
    )]
    pub result: Option<Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_value"
    )]
    pub status: Option<Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_value"
    )]
    pub is_error: Option<Value>,
    pub attachment: Option<AttachmentRef>,
    pub relationship: Option<ToolRelationship>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalEvent {
    pub id: String,
    pub sequence: usize,
    pub source_event_id: Option<String>,
    pub session_id: Option<String>,
    pub branch_id: Option<String>,
    pub parent_id: Option<String>,
    pub record_type: Option<String>,
    pub role: Option<String>,
    pub source_role: Option<String>,
    /// Exact source value, including a missing or non-string timestamp.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_value"
    )]
    pub timestamp: Option<Value>,
    pub raw: RawRecordRef,
    pub blocks: Vec<CanonicalBlock>,
}

impl CanonicalEvent {
    pub fn raw_text<'a>(&self, session: &'a CanonicalSession) -> Result<&'a str> {
        session
            .archive
            .text
            .get(self.raw.start..self.raw.end)
            .ok_or_else(|| anyhow::anyhow!("canonical record span is invalid"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionFinding {
    pub code: String,
    pub event_id: Option<String>,
    pub field: String,
    pub disposition: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalSession {
    pub schema_version: u32,
    pub parser_version: String,
    pub source: NativeSource,
    pub origin: String,
    pub archive: NativeArchive,
    pub events: Vec<CanonicalEvent>,
    pub findings: Vec<ConversionFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionCapabilities {
    pub native_archive: bool,
    pub readable_history: bool,
    pub native_resume: bool,
    pub api_replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionResponse {
    pub command: String,
    pub target: TargetFormat,
    pub capabilities: ConversionCapabilities,
    pub canonical: CanonicalSession,
    pub output_jsonl: Option<String>,
    pub findings: Vec<ConversionFinding>,
}

fn present_value<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
